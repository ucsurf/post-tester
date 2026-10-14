//! # post-tester plugin host
//!
//! Loads plugins and runs them, and request scripts, in sandboxed JavaScript.
//!
//! | Module        | Responsibility                                          |
//! |---------------|---------------------------------------------------------|
//! | [`manifest`]  | Reading and validating `plugin.json`                    |
//! | [`transpile`] | TypeScript → JavaScript                                 |
//! | [`sandbox`]   | The QuickJS runtime, its limits and host functions      |
//! | this file     | [`PluginHost`]: loading plugins and calling them        |
//!
//! The host implements [`Extensions`], so once attached to an [`Engine`] every
//! request automatically uses plugin template functions, auth methods and hooks.

pub mod manifest;
pub mod sandbox;
pub mod scaffold;
pub mod transpile;

use async_trait::async_trait;
use manifest::{Manifest, PLUGIN_TYPES, Permission, Problem};
use post_tester_core::events::{Level, Source, Span, SpanKind, Summary};
use post_tester_core::extensions::{Extensions, ScriptInput, ScriptOutput, ScriptPhase, ScriptRequest, ScriptResponse};
use post_tester_core::http::{PreparedRequest, Response};
use post_tester_core::model::{OrderedMap, Request};
use post_tester_core::{Engine, Error, Result, Workspace};
use sandbox::{HostState, Sandbox, TestOutcome};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, Weak};
use std::time::Duration;
use ts_rs::TS;

/// How long one plugin call or script may run.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The hooks a plugin can define in `hooks`.
pub const HOOK_NAMES: &[&str] = &["beforeRequest", "afterResponse"];

// ── What plugins provide ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Capabilities {
    pub template_functions: Vec<TemplateFunctionInfo>,
    pub auth: Vec<AuthInfo>,
    pub hooks: Vec<String>,
    pub importers: Vec<ImporterInfo>,
    pub exporters: Vec<ExporterInfo>,
    pub codegen: Vec<CodegenInfo>,
    pub viewers: Vec<ViewerInfo>,
    pub mcp_tools: Vec<McpToolInfo>,
    /// Top-level keys of the definition (used to check `provides`).
    #[serde(default)]
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ArgInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TemplateFunctionInfo {
    pub name: String,
    pub description: String,
    pub args: Vec<ArgInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AuthInfo {
    pub name: String,
    pub display_name: String,
    pub fields: Vec<FieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImporterInfo {
    pub name: String,
    pub display_name: String,
    pub file_extensions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExporterInfo {
    pub name: String,
    pub display_name: String,
    pub file_extension: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CodegenInfo {
    pub name: String,
    pub display_name: String,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ViewerInfo {
    pub name: String,
    pub display_name: String,
    pub content_types: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    #[ts(type = "Record<string, unknown>")]
    pub input_schema: serde_json::Value,
}

/// A loaded plugin.
pub struct Plugin {
    pub manifest: Manifest,
    pub dir: PathBuf,
    pub capabilities: Capabilities,
    /// Approved permissions.
    pub granted: Vec<String>,
    sandbox: Sandbox,
}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin").field("name", &self.manifest.name).field("dir", &self.dir).finish_non_exhaustive()
    }
}

/// A plugin as shown in lists.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub dir: String,
    pub provides: Vec<String>,
    pub permissions: Vec<String>,
    /// Declared permissions the user has not approved yet.
    pub pending_permissions: Vec<String>,
    pub capabilities: Capabilities,
}

impl Plugin {
    pub fn info(&self) -> PluginInfo {
        PluginInfo {
            name: self.manifest.name.clone(),
            display_name: self.manifest.display_name().to_string(),
            description: self.manifest.description.clone(),
            version: self.manifest.version.clone(),
            dir: self.dir.to_string_lossy().into_owned(),
            provides: self.manifest.provides.clone(),
            permissions: self.manifest.permissions.clone(),
            pending_permissions: self.manifest.permissions.iter().filter(|p| !self.granted.contains(p)).cloned().collect(),
            capabilities: self.capabilities.clone(),
        }
    }

    /// Calls a capability. See `__pt_dispatch` in `js/prelude.js` for the kinds.
    pub async fn call(&self, span: Option<&Span>, kind: &str, name: &str, payload: serde_json::Value) -> Result<serde_json::Value> {
        self.sandbox
            .call(span, kind, name, &payload)
            .await
            .map_err(|e| Error::Plugin(format!("{} ({kind} `{name}`): {e}", self.manifest.name)))
    }
}

/// The result of `pt plugin test`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct TestReport {
    pub plugin: String,
    pub files: Vec<String>,
    pub tests: Vec<TestOutcome>,
}

impl TestReport {
    pub fn passed(&self) -> bool {
        self.tests.iter().all(|t| t.passed)
    }
}

// ── The host ────────────────────────────────────────────────────────────────

/// Loads and runs plugins. Attach it to an engine with [`PluginHost::attach`].
pub struct PluginHost {
    engine: Weak<Engine>,
    plugins: RwLock<Vec<Arc<Plugin>>>,
    scripts: tokio::sync::OnceCell<Sandbox>,
    timeout: Duration,
}

impl std::fmt::Debug for PluginHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginHost").field("plugins", &self.plugins().len()).finish_non_exhaustive()
    }
}

impl PluginHost {
    /// Creates a host and registers it as the engine's extensions.
    pub fn attach(engine: &Arc<Engine>) -> Arc<Self> {
        Self::attach_with_timeout(engine, DEFAULT_TIMEOUT)
    }

    /// Like [`PluginHost::attach`], with a custom time limit for each plugin call and script.
    pub fn attach_with_timeout(engine: &Arc<Engine>, timeout: Duration) -> Arc<Self> {
        let host = Arc::new(Self {
            engine: Arc::downgrade(engine),
            plugins: RwLock::new(Vec::new()),
            scripts: tokio::sync::OnceCell::new(),
            timeout,
        });
        engine.set_extensions(host.clone());
        host
    }

    fn engine(&self) -> Result<Arc<Engine>> {
        self.engine.upgrade().ok_or_else(|| Error::Plugin("the engine was shut down".into()))
    }

    pub fn plugins(&self) -> Vec<Arc<Plugin>> {
        self.plugins.read().expect("plugin lock poisoned").clone()
    }

    pub fn get(&self, name: &str) -> Option<Arc<Plugin>> {
        self.plugins().into_iter().find(|p| p.manifest.name == name)
    }

    pub fn unload(&self, name: &str) -> bool {
        let mut plugins = self.plugins.write().expect("plugin lock poisoned");
        let before = plugins.len();
        plugins.retain(|p| p.manifest.name != name);
        before != plugins.len()
    }

    /// Loads (or reloads) the plugin in `dir` with the given approved permissions.
    pub async fn load(&self, dir: &Path, granted: Vec<String>) -> std::result::Result<Arc<Plugin>, Vec<Problem>> {
        let engine = self.engine().map_err(|e| vec![problem("post-tester", e.to_string(), "restart post-tester")])?;
        let fs_root = engine.workspace().map(|w| w.root().to_path_buf());
        let plugin = load_plugin(dir, granted, Arc::downgrade(&engine), fs_root, self.timeout).await?;
        let plugin = Arc::new(plugin);
        let mut plugins = self.plugins.write().expect("plugin lock poisoned");
        plugins.retain(|p| p.manifest.name != plugin.manifest.name);
        plugins.push(plugin.clone());
        Ok(plugin)
    }

    /// Loads every plugin listed in the workspace settings. Problems are
    /// reported in Live Activity and returned.
    pub async fn load_workspace(&self, workspace: &Workspace) -> Vec<(String, Vec<Problem>)> {
        let Ok(settings) = workspace.settings() else { return Vec::new() };
        let Ok(engine) = self.engine() else { return Vec::new() };
        let mut failures = Vec::new();
        if settings.plugins.is_empty() {
            return failures;
        }
        // Only problems are reported in Live Activity; a normal start stays quiet.
        let mut notes: Vec<(Level, String)> = Vec::new();
        for path in &settings.plugins {
            let dir = workspace.root().join(path);
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
            let manifest_name = Manifest::load(&dir).map(|m| m.name).unwrap_or(name);
            let granted = settings.approved_permissions.get(&manifest_name).cloned().unwrap_or_default();
            match self.load(&dir, granted).await {
                Ok(plugin) => {
                    tracing::debug!(plugin = %plugin.manifest.name, "loaded");
                    let pending = plugin.info().pending_permissions;
                    if !pending.is_empty() {
                        let text = format!("{} is waiting for permission approval: {}", plugin.manifest.name, pending.join(", "));
                        notes.push((Level::Warn, text));
                    }
                }
                Err(problems) => {
                    notes.extend(problems.iter().map(|p| (Level::Error, format!("{path}: {p}"))));
                    failures.push((path.clone(), problems));
                }
            }
        }
        if !notes.is_empty() {
            let span = Span::start(engine.bus(), Source::System, None, SpanKind::System, "Loading plugins", Summary::default());
            for (level, text) in notes {
                span.log(level, text);
            }
            if failures.is_empty() {
                span.finish_ok(None, None);
            } else {
                span.finish_error(format!("{} plugin(s) failed to load", failures.len()));
            }
        }
        failures
    }

    /// Validates a plugin folder without keeping it loaded.
    pub async fn validate(&self, dir: &Path) -> Vec<Problem> {
        let Ok(engine) = self.engine() else { return vec![problem("post-tester", "engine unavailable".into(), "restart")] };
        match load_plugin(dir, Vec::new(), Arc::downgrade(&engine), None, self.timeout).await {
            Ok(_) => Vec::new(),
            Err(problems) => problems,
        }
    }

    /// Runs a plugin's `*.test.ts` / `*.test.js` files, with every declared permission granted.
    pub async fn test(&self, dir: &Path) -> std::result::Result<TestReport, Vec<Problem>> {
        let engine = self.engine().map_err(|e| vec![problem("post-tester", e.to_string(), "restart")])?;
        let dir = &plugin_dir(dir)?;
        let manifest = Manifest::load(dir).map_err(|p| vec![p])?;
        let plugin = load_plugin(dir, manifest.permissions.clone(), Arc::downgrade(&engine), Some(dir.to_path_buf()), self.timeout).await?;
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| vec![problem(&dir.display().to_string(), e.to_string(), "check the folder")])?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".test.ts") || n.ends_with(".test.js")))
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(vec![problem(
                "tests",
                "no test files found".into(),
                "add an index.test.ts file (see the plugin guide, section Testing)",
            )]);
        }
        let span = Span::start(
            engine.bus(),
            Source::Cli,
            None,
            SpanKind::PluginCall,
            format!("Test plugin {}", plugin.manifest.name),
            Summary::default(),
        );
        let mut tests = Vec::new();
        for file in &files {
            // Each test file gets a fresh sandbox so tests cannot affect each other.
            let state = Arc::new(HostState::new(
                plugin.manifest.name.clone(),
                Source::Plugin { name: plugin.manifest.name.clone() },
                plugin.manifest.parsed_permissions(),
                plugin.manifest.parsed_permissions(),
                Arc::downgrade(&engine),
                Some(dir.to_path_buf()),
            ));
            let sandbox = Sandbox::new(state, Some(dir.to_path_buf()), self.timeout)
                .await
                .map_err(|e| vec![problem("sandbox", e, "report this as a bug")])?;
            match sandbox.run_tests(Some(&span), file).await {
                Ok(results) => tests.extend(results),
                Err(e) => tests.push(TestOutcome { name: file_name(file), passed: false, detail: Some(e) }),
            }
        }
        let report = TestReport { plugin: plugin.manifest.name.clone(), files: files.iter().map(|f| file_name(f)).collect(), tests };
        if report.passed() {
            span.finish_ok(None, None)
        } else {
            span.finish(post_tester_core::events::Outcome::Failed, None, None, None)
        }
        Ok(report)
    }

    // ── Helpers for front-ends ──────────────────────────────────────────────

    /// Every plugin code generator as `plugin/name`.
    pub fn codegen_targets(&self) -> Vec<(String, CodegenInfo)> {
        self.plugins()
            .iter()
            .flat_map(|p| p.capabilities.codegen.iter().map(move |c| (format!("{}/{}", p.manifest.name, c.name), c.clone())))
            .collect()
    }

    /// Generates code with a plugin generator (`plugin/name`).
    pub async fn codegen(&self, id: &str, request: &PreparedRequest) -> Result<String> {
        let (plugin, name) = self.resolve(id, |c| c.codegen.iter().map(|g| g.name.clone()).collect())?;
        let value = plugin.call(None, "codegen", &name, serde_json::json!({ "request": ScriptRequest::from_prepared(request) })).await?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// Renders a response with a plugin viewer (`plugin/name`). Returns HTML.
    pub async fn render_viewer(&self, id: &str, response: &ScriptResponse) -> Result<String> {
        let (plugin, name) = self.resolve(id, |c| c.viewers.iter().map(|v| v.name.clone()).collect())?;
        let value = plugin.call(None, "viewer", &name, serde_json::json!({ "response": response })).await?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// Tries plugin importers. Returns `None` if no importer recognizes the text.
    pub async fn import(&self, text: &str) -> Option<Result<post_tester_core::import::Imported>> {
        for plugin in self.plugins() {
            for importer in &plugin.capabilities.importers {
                let detected = plugin.call(None, "importerDetect", &importer.name, serde_json::json!({ "text": text })).await;
                if detected.ok().and_then(|v| v.as_bool()) == Some(true) {
                    return Some(self.run_importer(&plugin, &importer.name, text).await);
                }
            }
        }
        None
    }

    async fn run_importer(&self, plugin: &Plugin, name: &str, text: &str) -> Result<post_tester_core::import::Imported> {
        let value = plugin.call(None, "importer", name, serde_json::json!({ "text": text })).await?;
        let mut imported =
            post_tester_core::import::Imported { name: value["name"].as_str().unwrap_or("Imported").to_string(), ..Default::default() };
        for raw in value["requests"].as_array().into_iter().flatten() {
            let folder = raw["folder"].as_str().unwrap_or_default().to_string();
            let mut raw = raw.clone();
            raw.as_object_mut().map(|o| o.remove("folder"));
            let request: Request = serde_json::from_value(raw)
                .map_err(|e| Error::Plugin(format!("{}: importer `{name}` returned an invalid request: {e}", plugin.manifest.name)))?;
            imported.add_request(&folder, request);
        }
        Ok(imported)
    }

    /// Every plugin exporter as `plugin/name`.
    pub fn export_targets(&self) -> Vec<(String, ExporterInfo)> {
        self.plugins()
            .iter()
            .flat_map(|p| p.capabilities.exporters.iter().map(move |e| (format!("{}/{}", p.manifest.name, e.name), e.clone())))
            .collect()
    }

    /// Exports requests with a plugin exporter (`plugin/name`). The plugin receives
    /// `{ name, requests }`, each request in the request-file format plus its `id`.
    pub async fn export(&self, id: &str, name: &str, requests: &[(String, Request)]) -> Result<String> {
        let (plugin, exporter) = self.resolve(id, |c| c.exporters.iter().map(|e| e.name.clone()).collect())?;
        let requests: Vec<serde_json::Value> = requests
            .iter()
            .map(|(id, request)| {
                let mut value = serde_json::to_value(request).unwrap_or_default();
                if let Some(object) = value.as_object_mut() {
                    object.insert("id".into(), id.clone().into());
                }
                value
            })
            .collect();
        let payload = serde_json::json!({ "collection": { "name": name, "requests": requests } });
        let value = plugin.call(None, "exporter", &exporter, payload).await?;
        Ok(value.as_str().unwrap_or_default().to_string())
    }

    /// Every plugin MCP tool as (`plugin_tool` name, plugin, info).
    pub fn mcp_tools(&self) -> Vec<(String, Arc<Plugin>, McpToolInfo)> {
        self.plugins()
            .into_iter()
            .flat_map(|p| {
                p.capabilities
                    .mcp_tools
                    .clone()
                    .into_iter()
                    .map(move |t| (format!("{}__{}", p.manifest.name.replace('-', "_"), t.name), p.clone(), t))
            })
            .collect()
    }

    /// Calls a plugin MCP tool.
    pub async fn call_mcp_tool(&self, plugin: &Plugin, tool: &str, input: serde_json::Value, span: &Span) -> Result<serde_json::Value> {
        plugin.call(Some(span), "mcpTool", tool, serde_json::json!({ "input": input })).await
    }

    /// Finds `plugin/name`, or a bare `name` that only one plugin provides.
    fn resolve(&self, id: &str, names: impl Fn(&Capabilities) -> Vec<String>) -> Result<(Arc<Plugin>, String)> {
        let plugins = self.plugins();
        if let Some((plugin_name, name)) = id.split_once('/')
            && let Some(plugin) = plugins.iter().find(|p| p.manifest.name == plugin_name)
            && names(&plugin.capabilities).iter().any(|n| n == name)
        {
            return Ok((plugin.clone(), name.to_string()));
        }
        let matches: Vec<_> = plugins.iter().filter(|p| names(&p.capabilities).iter().any(|n| n == id)).collect();
        match matches.as_slice() {
            [one] => Ok(((*one).clone(), id.to_string())),
            [] => Err(Error::Plugin(format!("no installed plugin provides `{id}`"))),
            many => Err(Error::Plugin(format!(
                "`{id}` is provided by several plugins ({}); use `plugin/{id}`",
                many.iter().map(|p| p.manifest.name.as_str()).collect::<Vec<_>>().join(", ")
            ))),
        }
    }

    async fn script_sandbox(&self) -> Result<&Sandbox> {
        let engine = self.engine()?;
        self.scripts
            .get_or_try_init(|| async {
                // Request scripts are written by the user, so they may call any host,
                // but they cannot read secrets or files.
                let state = Arc::new(HostState::new(
                    "script",
                    Source::User,
                    vec![Permission::Network("*".into())],
                    vec![Permission::Network("*".into())],
                    Arc::downgrade(&engine),
                    None,
                ));
                Sandbox::new(state, None, self.timeout).await.map_err(Error::Plugin)
            })
            .await
    }
}

#[async_trait]
impl Extensions for PluginHost {
    async fn template_function(&self, name: &str, args: &OrderedMap, span: &Span) -> Result<Option<String>> {
        let found = self.plugins().into_iter().find(|p| p.capabilities.template_functions.iter().any(|f| f.name == name));
        let Some(plugin) = found else { return Ok(None) };
        let value = plugin.call(Some(span), "templateFunction", name, serde_json::json!({ "args": args })).await?;
        Ok(Some(value.as_str().map_or_else(|| value.to_string(), str::to_string)))
    }

    async fn apply_auth(&self, name: &str, config: &OrderedMap, request: PreparedRequest, span: &Span) -> Result<PreparedRequest> {
        let (plugin, method) = self.resolve(name, |c| c.auth.iter().map(|a| a.name.clone()).collect())?;
        let payload = serde_json::json!({ "request": ScriptRequest::from_prepared(&request), "config": config });
        let value = plugin.call(Some(span), "auth", &method, payload).await?;
        apply_request_change(request, value, &plugin.manifest.name)
    }

    async fn before_request(&self, mut request: PreparedRequest, span: &Span) -> Result<PreparedRequest> {
        for plugin in self.plugins().iter().filter(|p| p.capabilities.hooks.iter().any(|h| h == "beforeRequest")) {
            let payload = serde_json::json!({ "request": ScriptRequest::from_prepared(&request) });
            let value = plugin.call(Some(span), "beforeRequest", "", payload).await?;
            request = apply_request_change(request, value, &plugin.manifest.name)?;
        }
        Ok(request)
    }

    async fn after_response(&self, _request: &PreparedRequest, mut response: Response, span: &Span) -> Result<Response> {
        for plugin in self.plugins().iter().filter(|p| p.capabilities.hooks.iter().any(|h| h == "afterResponse")) {
            let payload = serde_json::json!({ "response": ScriptResponse::from_response(&response) });
            let value = plugin.call(Some(span), "afterResponse", "", payload).await?;
            let changed: ScriptResponse = serde_json::from_value(value)
                .map_err(|e| Error::Plugin(format!("{}: afterResponse returned an invalid response: {e}", plugin.manifest.name)))?;
            response.status = changed.status;
            response.status_text = changed.status_text;
            response.headers = changed.headers;
            response.body = changed.body.into_bytes().into();
        }
        Ok(response)
    }

    async fn run_script(&self, phase: ScriptPhase, code: &str, input: ScriptInput, span: &Span) -> Result<ScriptOutput> {
        let sandbox = self.script_sandbox().await?;
        sandbox.run_script(span, phase, code, &input).await.map_err(|e| Error::Plugin(format!("script error: {e}")))
    }
}

fn apply_request_change(mut request: PreparedRequest, value: serde_json::Value, plugin: &str) -> Result<PreparedRequest> {
    let changed: ScriptRequest =
        serde_json::from_value(value).map_err(|e| Error::Plugin(format!("{plugin}: returned an invalid request: {e}")))?;
    changed.apply_to(&mut request)?;
    Ok(request)
}

/// Loads a plugin into a fresh sandbox and checks it thoroughly.
async fn load_plugin(
    dir: &Path,
    granted: Vec<String>,
    engine: Weak<Engine>,
    fs_root: Option<PathBuf>,
    timeout: Duration,
) -> std::result::Result<Plugin, Vec<Problem>> {
    let dir = &plugin_dir(dir)?;
    let manifest = Manifest::load(dir).map_err(|p| vec![p])?;
    let mut problems = manifest.validate();
    let main = dir.join(&manifest.main);
    if !main.is_file() {
        problems.push(problem("plugin.json: main", format!("`{}` does not exist", manifest.main), "create the file or fix the path"));
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    let declared = manifest.parsed_permissions();
    let granted_parsed: Vec<Permission> =
        granted.iter().filter_map(|p| Permission::parse(p).ok()).filter(|p| declared.contains(p)).collect();
    let state = Arc::new(HostState::new(
        manifest.name.clone(),
        Source::Plugin { name: manifest.name.clone() },
        declared,
        granted_parsed,
        engine,
        fs_root,
    ));
    let sandbox =
        Sandbox::new(state, Some(dir.to_path_buf()), timeout).await.map_err(|e| vec![problem("sandbox", e, "report this as a bug")])?;
    sandbox
        .load_plugin(&main)
        .await
        .map_err(|e| vec![problem(&manifest.main, e, "fix the error above; the plugin must `export default definePlugin({ ... })`")])?;

    let described = sandbox
        .call(None, "describe", "", &serde_json::Value::Null)
        .await
        .map_err(|e| vec![problem(&manifest.main, e, "check the plugin definition")])?;
    let capabilities: Capabilities =
        serde_json::from_value(described).map_err(|e| vec![problem(&manifest.main, e.to_string(), "check the plugin definition")])?;

    // `provides` and the exported keys must agree.
    let mut problems = Vec::new();
    for (kind, key) in PLUGIN_TYPES {
        let listed = manifest.provides.iter().any(|p| p == kind);
        let exported = capabilities.keys.iter().any(|k| k == key);
        if listed && !exported {
            problems.push(problem(
                &manifest.main,
                format!("plugin.json lists \"{kind}\" in provides, but definePlugin has no `{key}`"),
                &format!("add `{key}: {{ ... }}` or remove \"{kind}\" from provides"),
            ));
        }
        if exported && !listed {
            problems.push(problem(
                "plugin.json: provides",
                format!("definePlugin has `{key}`, but provides does not list \"{kind}\""),
                &format!("add \"{kind}\" to provides"),
            ));
        }
    }
    // Hooks are looked up by name, so a misspelled hook would silently never run.
    for hook in capabilities.hooks.iter().filter(|h| !HOOK_NAMES.contains(&h.as_str())) {
        problems.push(problem(&manifest.main, format!("unknown hook `{hook}`"), &format!("use one of: {}", HOOK_NAMES.join(", "))));
    }
    for f in &capabilities.template_functions {
        if f.description.trim().is_empty() {
            problems.push(problem(
                &manifest.main,
                format!("template function `{}` has no description", f.name),
                "add `description: \"...\"`; it is shown to users and AI agents",
            ));
        }
    }
    for t in &capabilities.mcp_tools {
        if t.description.trim().is_empty() {
            problems.push(problem(
                &manifest.main,
                format!("MCP tool `{}` has no description", t.name),
                "add `description: \"...\"`; AI agents use it to decide when to call the tool",
            ));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(Plugin { manifest, dir: dir.to_path_buf(), capabilities, granted, sandbox })
}

/// The plugin folder as an absolute path. Module specifiers are built from it,
/// and the module resolver only accepts absolute or `./` paths, so a relative
/// folder such as `plugins/my-plugin` would otherwise fail to load.
fn plugin_dir(dir: &Path) -> std::result::Result<PathBuf, Vec<Problem>> {
    match dir.canonicalize() {
        Ok(path) if path.is_dir() => Ok(path),
        Ok(_) => Err(vec![problem(
            &dir.display().to_string(),
            "is not a folder".into(),
            "pass the plugin folder (the one containing plugin.json)",
        )]),
        Err(e) => Err(vec![problem(
            &dir.display().to_string(),
            format!("cannot open the plugin folder: {e}"),
            "check the path; it must be a folder containing plugin.json (create one with `pt plugin new <type> <name>`)",
        )]),
    }
}

fn problem(location: &str, message: String, fix: &str) -> Problem {
    Problem { location: location.to_string(), message, fix: fix.to_string() }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}
