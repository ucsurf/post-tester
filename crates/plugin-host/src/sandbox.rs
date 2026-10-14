//! One isolated JavaScript runtime (QuickJS) with post-tester's host functions.
//!
//! Safety limits:
//! - **Memory:** each sandbox has its own heap with a fixed limit.
//! - **Time:** every call has a deadline; long-running JavaScript is interrupted.
//! - **No ambient access:** there is no `fetch`, file system or `process`. The only
//!   way out is through the host functions below, which check permissions.

use crate::manifest::Permission;
use crate::transpile::to_javascript;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use post_tester_core::Engine;
use post_tester_core::events::{EventKind, Level, Source, Span, SpanKind, Summary};
use post_tester_core::extensions::{ScriptInput, ScriptOutput, ScriptPhase};
use post_tester_core::http::{self, PreparedRequest};
use post_tester_core::model::RequestOptions;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::prelude::Async;
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Ctx, Function, Module, Object, Promise, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const PRELUDE: &str = include_str!("js/prelude.js");
const SDK_MODULE: &str = include_str!("js/sdk.js");
const TESTING_MODULE: &str = include_str!("js/testing.js");
pub const SDK_NAME: &str = "@post-tester/plugin-sdk";
pub const TESTING_NAME: &str = "@post-tester/plugin-sdk/testing";

const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const STACK_LIMIT: usize = 1024 * 1024;

/// What the code in a sandbox may do, and where its output goes.
#[derive(Debug)]
pub struct HostState {
    /// Shown in logs, e.g. the plugin name or "script".
    pub label: String,
    pub source: Source,
    /// Permissions declared in `plugin.json`.
    pub declared: Vec<Permission>,
    /// Permissions the user approved (a subset of `declared`).
    pub granted: Vec<Permission>,
    pub engine: Weak<Engine>,
    /// Root for `ctx.fs`; usually the workspace folder.
    pub fs_root: Option<PathBuf>,
    /// The span of the current call; logs and requests attach to it.
    current_span: Mutex<Option<Span>>,
}

impl HostState {
    pub fn new(
        label: impl Into<String>,
        source: Source,
        declared: Vec<Permission>,
        granted: Vec<Permission>,
        engine: Weak<Engine>,
        fs_root: Option<PathBuf>,
    ) -> Self {
        Self { label: label.into(), source, declared, granted, engine, fs_root, current_span: Mutex::new(None) }
    }

    fn span(&self) -> Option<Span> {
        self.current_span.lock().expect("span lock poisoned").clone()
    }

    fn engine(&self) -> Result<Arc<Engine>, String> {
        self.engine.upgrade().ok_or_else(|| "post-tester is shutting down".to_string())
    }

    /// Checks a permission and explains how to get it if missing.
    fn require(&self, wanted: &Permission) -> Result<(), String> {
        let covers = |p: &Permission| match (p, wanted) {
            (Permission::Network(allowed), Permission::Network(host)) => network_allows(allowed, host),
            (a, b) => a == b,
        };
        if self.granted.iter().any(covers) {
            return Ok(());
        }
        let name = permission_name(wanted);
        if self.declared.iter().any(covers) {
            Err(format!(
                "permission `{name}` was not approved for {}. Approve it with `pt plugin approve {}` or in the app's Plugins panel",
                self.label, self.label
            ))
        } else {
            Err(format!("permission denied: {} did not declare `{name}`. Add \"{name}\" to \"permissions\" in plugin.json", self.label))
        }
    }
}

fn permission_name(p: &Permission) -> String {
    match p {
        Permission::Network(host) => format!("network:{host}"),
        Permission::SecretsRead => "secrets:read".into(),
        Permission::FsRead => "fs:read".into(),
        Permission::FsWrite => "fs:write".into(),
    }
}

/// `*` allows every host; `*.example.com` allows sub-domains; otherwise an exact match.
fn network_allows(allowed: &str, host: &str) -> bool {
    allowed == "*"
        || allowed.eq_ignore_ascii_case(host)
        || allowed.strip_prefix("*.").is_some_and(|domain| host.to_ascii_lowercase().ends_with(&format!(".{domain}")))
}

/// A sandboxed JavaScript runtime.
pub struct Sandbox {
    // Kept alive for the lifetime of the context.
    _runtime: AsyncRuntime,
    context: AsyncContext,
    state: Arc<HostState>,
    /// Milliseconds after `epoch` when the current call must stop (0 = no call running).
    deadline: Arc<AtomicU64>,
    epoch: Instant,
    timeout: Duration,
    lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sandbox").field("label", &self.state.label).finish_non_exhaustive()
    }
}

impl Sandbox {
    /// Creates a sandbox. `module_root` is the folder relative imports may read from.
    pub async fn new(state: Arc<HostState>, module_root: Option<PathBuf>, timeout: Duration) -> Result<Self, String> {
        let runtime = AsyncRuntime::new().map_err(|e| e.to_string())?;
        runtime.set_memory_limit(MEMORY_LIMIT).await;
        runtime.set_max_stack_size(STACK_LIMIT).await;
        let deadline = Arc::new(AtomicU64::new(0));
        let epoch = Instant::now();
        {
            let deadline = deadline.clone();
            runtime
                .set_interrupt_handler(Some(Box::new(move || {
                    let d = deadline.load(Ordering::Relaxed);
                    d != 0 && epoch.elapsed().as_millis() as u64 > d
                })))
                .await;
        }
        let root = module_root.map(|r| r.canonicalize().unwrap_or(r));
        runtime.set_loader(ModuleLoader { root: root.clone() }, ModuleLoader { root }).await;
        let context = AsyncContext::full(&runtime).await.map_err(|e| e.to_string())?;

        let host_state = state.clone();
        context
            .async_with(async |ctx| {
                register_host_functions(&ctx, host_state).catch(&ctx).map_err(|e| e.to_string())?;
                ctx.eval::<(), _>(PRELUDE).catch(&ctx).map_err(|e| format!("prelude failed: {e}"))
            })
            .await?;

        Ok(Self { _runtime: runtime, context, state, deadline, epoch, timeout, lock: tokio::sync::Mutex::new(()) })
    }

    pub fn state(&self) -> &HostState {
        &self.state
    }

    /// Runs `f` with the call's span attached and the time limit armed.
    async fn guarded<T>(&self, span: Option<&Span>, f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
        let _guard = self.lock.lock().await;
        *self.state.current_span.lock().expect("span lock poisoned") = span.cloned();
        let limit_ms = self.epoch.elapsed().as_millis() as u64 + self.timeout.as_millis() as u64;
        self.deadline.store(limit_ms, Ordering::Relaxed);
        let result = tokio::time::timeout(self.timeout, f).await;
        self.deadline.store(0, Ordering::Relaxed);
        *self.state.current_span.lock().expect("span lock poisoned") = None;
        let timed_out = || format!("{} took longer than {} ms and was stopped", self.state.label, self.timeout.as_millis());
        match result {
            Ok(Err(e)) if e.contains("interrupted") => Err(timed_out()),
            Ok(result) => result,
            Err(_) => Err(timed_out()),
        }
    }

    /// Imports a module file and stores its default export as the plugin.
    pub async fn load_plugin(&self, main: &Path) -> Result<(), String> {
        let specifier = main.to_string_lossy().into_owned();
        self.guarded(None, async {
            self.context
                .async_with(async |ctx| {
                    let run = async {
                        let promise = Module::import(&ctx, specifier.clone())?;
                        let namespace: Object = promise.into_future().await?;
                        let default: Value = namespace.get("default")?;
                        ctx.globals().set("__pt_plugin", default.clone())?;
                        Ok::<_, rquickjs::Error>(default.is_undefined())
                    };
                    match run.await.catch(&ctx) {
                        Ok(true) => Err(format!("{specifier} must `export default definePlugin({{ ... }})`")),
                        Ok(false) => Ok(()),
                        Err(e) => Err(clean_error(&e.to_string())),
                    }
                })
                .await
        })
        .await
    }

    /// Calls a plugin capability (see `__pt_dispatch` in the prelude).
    pub async fn call(
        &self,
        span: Option<&Span>,
        kind: &str,
        name: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let payload = payload.to_string();
        let json = self.call_global(span, "__pt_call", (kind.to_string(), name.to_string(), payload)).await?;
        serde_json::from_str(&json).map_err(|e| format!("plugin returned invalid data: {e}"))
    }

    /// Runs a request script.
    pub async fn run_script(&self, span: &Span, phase: ScriptPhase, code: &str, input: &ScriptInput) -> Result<ScriptOutput, String> {
        let phase = match phase {
            ScriptPhase::BeforeRequest => "before_request",
            ScriptPhase::AfterResponse => "after_response",
        };
        let input = serde_json::to_string(input).map_err(|e| e.to_string())?;
        let json = self.call_global(Some(span), "__pt_run_script", (phase.to_string(), code.to_string(), input)).await?;
        serde_json::from_str(&json).map_err(|e| format!("script returned invalid data: {e}"))
    }

    /// Imports a test file (which registers tests) and runs every test.
    pub async fn run_tests(&self, span: Option<&Span>, test_file: &Path) -> Result<Vec<TestOutcome>, String> {
        let specifier = test_file.to_string_lossy().into_owned();
        self.guarded(span, async {
            self.context
                .async_with(async |ctx| {
                    let run = async {
                        Module::import(&ctx, specifier.clone())?.into_future::<Value>().await?;
                        let runner: Function = ctx.globals().get("__pt_run_tests")?;
                        let promise: Promise = runner.call(())?;
                        promise.into_future::<String>().await
                    };
                    run.await.catch(&ctx).map_err(|e| clean_error(&e.to_string()))
                })
                .await
        })
        .await
        .and_then(|json| serde_json::from_str(&json).map_err(|e| e.to_string()))
    }

    async fn call_global(&self, span: Option<&Span>, function: &str, args: (String, String, String)) -> Result<String, String> {
        self.guarded(span, async {
            self.context
                .async_with(async |ctx| {
                    let run = async {
                        let f: Function = ctx.globals().get(function)?;
                        let promise: Promise = f.call(args.clone())?;
                        promise.into_future::<String>().await
                    };
                    run.await.catch(&ctx).map_err(|e| clean_error(&e.to_string()))
                })
                .await
        })
        .await
    }
}

/// The result of one plugin test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct TestOutcome {
    pub name: String,
    pub passed: bool,
    #[ts(optional)]
    pub detail: Option<String>,
}

/// Removes the "Error: " prefix and internal stack frames from messages, so
/// only frames in the plugin's own files remain.
fn clean_error(message: &str) -> String {
    // The prelude is evaluated as `eval_script`; `(native)` frames are engine internals.
    let internal = |line: &str| ["<input>", "eval_script", "(native)"].iter().any(|marker| line.contains(marker));
    let message = message.strip_prefix("Error: ").unwrap_or(message);
    message.lines().filter(|l| !internal(l)).collect::<Vec<_>>().join("\n").trim().to_string()
}

// ── Host functions ───────────────────────────────────────────────────────────

fn ok(value: impl Serialize) -> String {
    serde_json::json!({ "ok": value }).to_string()
}

fn err(message: impl std::fmt::Display) -> String {
    serde_json::json!({ "error": message.to_string() }).to_string()
}

fn envelope<T: Serialize>(result: Result<T, String>) -> String {
    match result {
        Ok(value) => ok(value),
        Err(message) => err(message),
    }
}

fn register_host_functions<'js>(ctx: &Ctx<'js>, state: Arc<HostState>) -> rquickjs::Result<()> {
    let host = Object::new(ctx.clone())?;

    let s = state.clone();
    host.set(
        "log",
        Function::new(ctx.clone(), move |level: String, text: String| {
            let level = match level.as_str() {
                "debug" => Level::Debug,
                "warn" => Level::Warn,
                "error" => Level::Error,
                _ => Level::Info,
            };
            match s.span() {
                Some(span) => span.log(level, format!("[{}] {text}", s.label)),
                None => tracing::info!(plugin = %s.label, "{text}"),
            }
        })?,
    )?;

    let s = state.clone();
    host.set(
        "progress",
        Function::new(ctx.clone(), move |done: f64, total: f64| {
            if let Some(span) = s.span() {
                span.progress(done.max(0.0) as u32, total.max(0.0) as u32);
            }
        })?,
    )?;

    host.set("uuid", Function::new(ctx.clone(), || uuid::Uuid::new_v4().to_string())?)?;
    host.set("sha256", Function::new(ctx.clone(), |data: String| hex::encode(Sha256::digest(data.as_bytes())))?)?;
    host.set(
        "hmacSha256",
        Function::new(ctx.clone(), |key: String, data: String| {
            let mut mac = Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
            mac.update(data.as_bytes());
            hex::encode(mac.finalize().into_bytes())
        })?,
    )?;
    host.set("base64Encode", Function::new(ctx.clone(), |text: String| base64::engine::general_purpose::STANDARD.encode(text))?)?;
    host.set(
        "base64Decode",
        Function::new(ctx.clone(), |text: String| {
            envelope(
                base64::engine::general_purpose::STANDARD
                    .decode(text.trim())
                    .map_err(|e| format!("invalid base64: {e}"))
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            )
        })?,
    )?;

    let s = state.clone();
    host.set(
        "varGet",
        Function::new(ctx.clone(), move |name: String| {
            envelope(s.engine().and_then(|engine| {
                let vars = engine.variables(None, &[]).map_err(|e| e.to_string())?;
                Ok(vars.get(&name).map(str::to_string))
            }))
        })?,
    )?;

    let s = state.clone();
    host.set(
        "varSet",
        Function::new(ctx.clone(), move |name: String, value: String| {
            envelope(s.engine().map(|engine| {
                engine.set_runtime_variable(&name, &value);
                if let Some(span) = s.span() {
                    span.emit(EventKind::VariableSet { name, value });
                }
            }))
        })?,
    )?;

    let s = state.clone();
    host.set(
        "http",
        Function::new(
            ctx.clone(),
            Async(move |json: String| {
                let s = s.clone();
                async move { envelope(host_http(&s, &json).await) }
            }),
        )?,
    )?;

    let s = state.clone();
    host.set(
        "secretGet",
        Function::new(
            ctx.clone(),
            Async(move |name: String| {
                let s = s.clone();
                async move {
                    let result = async {
                        s.require(&Permission::SecretsRead)?;
                        s.engine()?.secrets().get(&name).await.map_err(|e| e.to_string())
                    };
                    envelope(result.await)
                }
            }),
        )?,
    )?;

    let s = state.clone();
    host.set(
        "fsRead",
        Function::new(ctx.clone(), move |path: String| {
            envelope(s.require(&Permission::FsRead).and_then(|()| {
                let full = resolve_fs_path(&s, &path)?;
                std::fs::read_to_string(&full).map_err(|e| format!("cannot read `{path}`: {e}"))
            }))
        })?,
    )?;

    let s = state;
    host.set(
        "fsWrite",
        Function::new(ctx.clone(), move |path: String, content: String| {
            envelope(s.require(&Permission::FsWrite).and_then(|()| {
                let full = resolve_fs_path(&s, &path)?;
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&full, content).map_err(|e| format!("cannot write `{path}`: {e}"))
            }))
        })?,
    )?;

    ctx.globals().set("__host", host)
}

/// Resolves a path for `ctx.fs`, refusing anything outside the root.
fn resolve_fs_path(state: &HostState, path: &str) -> Result<PathBuf, String> {
    let root = state.fs_root.as_ref().ok_or("no workspace is open, so ctx.fs is not available")?;
    let relative = Path::new(path);
    let escapes = relative.is_absolute() || relative.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)));
    let outside = || format!("`{path}` is outside the workspace; use a path relative to the workspace folder");
    if escapes {
        return Err(outside());
    }
    // A symbolic link inside the workspace may point outside it, so compare real
    // paths: the file itself if it exists, otherwise its nearest existing folder.
    let full = root.join(relative);
    let real_root = root.canonicalize().map_err(|e| format!("cannot open the workspace folder: {e}"))?;
    let existing = full.ancestors().find(|p| p.exists()).unwrap_or(root);
    let real = existing.canonicalize().map_err(|e| format!("cannot resolve `{path}`: {e}"))?;
    if !real.starts_with(&real_root) {
        return Err(outside());
    }
    Ok(full)
}

#[derive(Deserialize)]
struct HttpCall {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
    timeout_ms: u64,
}

async fn host_http(state: &HostState, json: &str) -> Result<serde_json::Value, String> {
    let call: HttpCall = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let url = url::Url::parse(&call.url).map_err(|e| format!("invalid URL `{}`: {e}", call.url))?;
    let host = url.host_str().unwrap_or_default().to_string();
    state.require(&Permission::Network(host.to_ascii_lowercase()))?;
    let engine = state.engine()?;

    let summary =
        Summary { protocol: Some("http".into()), method: Some(call.method.clone()), url: Some(call.url.clone()), request_id: None };
    let title = format!("{}: {} {}", state.label, call.method, call.url);
    let span = match state.span() {
        Some(parent) => Span::start(engine.bus(), state.source.clone(), Some(parent.id()), SpanKind::Request, title, summary),
        None => Span::start(engine.bus(), state.source.clone(), None, SpanKind::Request, title, summary),
    };
    let request = PreparedRequest {
        method: call.method,
        url,
        headers: call.headers,
        body: call.body.into_bytes().into(),
        options: RequestOptions { timeout_ms: call.timeout_ms, ..Default::default() },
    };
    match http::send(request, &span, Some(engine.cookies()), &CancellationToken::new()).await {
        Ok(response) => {
            span.finish_ok(Some(response.status), Some(response.body.len() as u64));
            Ok(serde_json::json!({
                "status": response.status,
                "status_text": response.status_text,
                "headers": response.headers,
                "body": String::from_utf8_lossy(&response.body),
                "time_ms": post_tester_core::events::millis(response.total),
            }))
        }
        Err(e) => {
            span.finish_error(&e);
            Err(e.to_string())
        }
    }
}

// ── Module loading ──────────────────────────────────────────────────────────

/// Resolves and loads `import`s: the SDK modules, and relative files inside the plugin folder.
struct ModuleLoader {
    root: Option<PathBuf>,
}

impl ModuleLoader {
    fn find_file(&self, base: &str, name: &str) -> Option<PathBuf> {
        let candidate =
            if Path::new(name).is_absolute() { PathBuf::from(name) } else { Path::new(base).parent().unwrap_or(Path::new("")).join(name) };
        let candidates = [
            candidate.clone(),
            candidate.with_extension("ts"),
            candidate.with_extension("js"),
            candidate.join("index.ts"),
            candidate.join("index.js"),
        ];
        let found = candidates.into_iter().find(|p| p.is_file())?.canonicalize().ok()?;
        match &self.root {
            Some(root) if !found.starts_with(root) => None,
            _ => Some(found),
        }
    }
}

impl Resolver for ModuleLoader {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        if name == SDK_NAME || name == TESTING_NAME {
            return Ok(name.to_string());
        }
        let relative_or_absolute = name.starts_with("./") || name.starts_with("../") || Path::new(name).is_absolute();
        if relative_or_absolute && let Some(path) = self.find_file(base, name) {
            return Ok(path.to_string_lossy().into_owned());
        }
        Err(rquickjs::Error::new_resolving_message(
            base,
            name,
            if relative_or_absolute {
                "file not found inside the plugin folder".to_string()
            } else {
                format!("only relative imports and `{SDK_NAME}` are supported; bundle npm packages into your plugin (e.g. with esbuild)")
            },
        ))
    }
}

impl Loader for ModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, rquickjs::module::Declared>> {
        let source = match name {
            SDK_NAME => SDK_MODULE.to_string(),
            TESTING_NAME => TESTING_MODULE.to_string(),
            path => {
                let text = std::fs::read_to_string(path).map_err(|e| rquickjs::Error::new_loading_message(name, e.to_string()))?;
                to_javascript(Path::new(path), &text).map_err(|e| rquickjs::Error::new_loading_message(name, e))?
            }
        };
        Module::declare(ctx.clone(), name, source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_patterns() {
        assert!(network_allows("*", "anything.dev"));
        assert!(network_allows("api.github.com", "API.github.com"));
        assert!(network_allows("*.example.com", "a.example.com"));
        assert!(!network_allows("*.example.com", "example.com.evil.dev"));
        assert!(!network_allows("api.github.com", "github.com"));
    }

    #[test]
    fn errors_are_cleaned() {
        assert_eq!(clean_error("Error: boom\n    at run (<input>:3)"), "boom");
        assert_eq!(
            clean_error("bad input\n    at run (/p/index.js:3:65)\n    at <anonymous> (eval_script:201:108)\n    at Function (native)"),
            "bad input\n    at run (/p/index.js:3:65)"
        );
    }
}
