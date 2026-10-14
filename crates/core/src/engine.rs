//! The [`Engine`]: runs requests from start to finish and reports every step live.
//!
//! Lifecycle of one request:
//!
//! ```text
//! before-request script → prepare (variables, folders, body, auth)
//!   → plugin auth → beforeRequest hooks → send (DNS, TCP, TLS, …)
//!   → afterResponse hooks → tests → after-response script → extract variables
//! ```

use crate::error::{Error, Result};
use crate::events::{EventBus, EventKind, Level, Outcome, Span, SpanId, SpanKind, Source, Summary};
use crate::expr::{Assertion, AssertionResult, extract};
use crate::extensions::{Extensions, NoExtensions, ScriptInput, ScriptOutput, ScriptPhase, ScriptRequest, ScriptResponse};
use crate::http::{self, CookieJar, PreparedRequest, Response};
use crate::model::{FolderSettings, OrderedMap, Request};
use crate::prepare::prepare;
use crate::secrets::{EnvSecrets, SecretStore};
use crate::template::{TemplateContext, Variables};
use crate::workspace::Workspace;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// Options for one execution.
#[derive(Debug, Clone)]
pub struct SendOptions {
    pub source: Source,
    /// Environment name; `None` uses the workspace default.
    pub environment: Option<String>,
    /// Parent span, e.g. the collection run this request belongs to.
    pub parent: Option<Span>,
    /// Workspace id of the request, enabling folder inheritance.
    pub request_id: Option<String>,
}

impl SendOptions {
    pub fn new(source: Source) -> Self {
        Self { source, environment: None, parent: None, request_id: None }
    }

    pub fn environment(mut self, environment: Option<String>) -> Self {
        self.environment = environment;
        self
    }
}

/// The result of executing one request.
#[derive(Debug, Clone)]
pub struct Execution {
    pub span_id: SpanId,
    pub request: Option<PreparedRequest>,
    pub response: Option<Response>,
    pub tests: Vec<AssertionResult>,
    pub extracted: Vec<(String, String)>,
    pub outcome: Outcome,
    pub error: Option<String>,
}

impl Execution {
    pub fn passed(&self) -> bool {
        self.outcome == Outcome::Ok
    }
}

/// Runs requests. Shared by every front-end (server, CLI, MCP, desktop).
pub struct Engine {
    bus: EventBus,
    workspace: RwLock<Option<Workspace>>,
    secrets: Arc<dyn SecretStore>,
    extensions: RwLock<Arc<dyn Extensions>>,
    cookies: CookieJar,
    /// Variables set during this session by scripts and extraction rules.
    runtime: Mutex<HashMap<String, String>>,
    active: Mutex<HashMap<SpanId, CancellationToken>>,
    /// Open WebSocket sessions and the channel to send messages on them.
    ws_sessions: Mutex<HashMap<SpanId, WsSender>>,
}

pub(crate) type WsSender = tokio::sync::mpsc::UnboundedSender<tokio_tungstenite::tungstenite::Message>;

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").field("workspace", &self.workspace()).finish_non_exhaustive()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(EventBus::default(), None, Arc::new(EnvSecrets))
    }
}

impl Engine {
    pub fn new(bus: EventBus, workspace: Option<Workspace>, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            bus,
            workspace: RwLock::new(workspace),
            secrets,
            extensions: RwLock::new(Arc::new(NoExtensions)),
            cookies: CookieJar::new(),
            runtime: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            ws_sessions: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn ws_sessions(&self) -> std::sync::MutexGuard<'_, HashMap<SpanId, WsSender>> {
        self.ws_sessions.lock().expect("websocket lock poisoned")
    }

    /// A template context backed by this engine's secrets and plugins.
    pub(crate) fn template_context(&self, vars: Variables, span: &Span) -> impl TemplateContext + use<> {
        EngineContext { vars, secrets: self.secrets.clone(), extensions: self.extensions(), span: span.clone() }
    }

    pub fn bus(&self) -> &EventBus {
        &self.bus
    }

    pub fn workspace(&self) -> Option<Workspace> {
        self.workspace.read().expect("workspace lock poisoned").clone()
    }

    /// The workspace, or an error explaining how to get one.
    pub fn require_workspace(&self) -> Result<Workspace> {
        self.workspace().ok_or_else(|| Error::NotFound("workspace (open a folder or run `pt init`)".into()))
    }

    pub fn set_workspace(&self, workspace: Option<Workspace>) {
        *self.workspace.write().expect("workspace lock poisoned") = workspace;
    }

    pub fn secrets(&self) -> &Arc<dyn SecretStore> {
        &self.secrets
    }

    pub fn cookies(&self) -> &CookieJar {
        &self.cookies
    }

    pub fn set_extensions(&self, extensions: Arc<dyn Extensions>) {
        *self.extensions.write().expect("extensions lock poisoned") = extensions;
    }

    pub(crate) fn extensions(&self) -> Arc<dyn Extensions> {
        self.extensions.read().expect("extensions lock poisoned").clone()
    }

    /// Session variables set by scripts and extraction rules.
    pub fn runtime_variables(&self) -> HashMap<String, String> {
        self.runtime.lock().expect("runtime lock poisoned").clone()
    }

    pub fn set_runtime_variable(&self, name: &str, value: &str) {
        self.runtime.lock().expect("runtime lock poisoned").insert(name.into(), value.into());
    }

    pub fn clear_runtime_variables(&self) {
        self.runtime.lock().expect("runtime lock poisoned").clear();
    }

    /// Stops a running operation (request, collection run, stream). Returns
    /// `false` if nothing with this id is running.
    pub fn cancel(&self, span_id: SpanId) -> bool {
        match self.active.lock().expect("active lock poisoned").get(&span_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Registers a cancellable operation. Cancelling a parent cancels its children.
    pub fn track(&self, span: &Span, parent: Option<&CancellationToken>) -> ActiveGuard<'_> {
        let token = parent.map_or_else(CancellationToken::new, CancellationToken::child_token);
        self.active.lock().expect("active lock poisoned").insert(span.id(), token.clone());
        ActiveGuard { engine: self, id: span.id(), token }
    }

    /// Variables visible to a request: runtime → environment → folders (leaf first) → workspace.
    pub fn variables(&self, environment: Option<&str>, folders: &[FolderSettings]) -> Result<Variables> {
        let mut vars = Variables::new();
        vars.push_strings(self.runtime_variables());
        if let Some(workspace) = self.workspace() {
            let settings = workspace.settings()?;
            let env_name = environment.map(str::to_string).or(settings.default_environment.clone());
            if let Some(name) = env_name.filter(|n| !n.is_empty()) {
                vars.push_layer(&workspace.read_environment(&name)?.variables);
            }
            for folder in folders.iter().rev() {
                vars.push_layer(&folder.variables);
            }
            vars.push_layer(&settings.variables);
        } else {
            for folder in folders.iter().rev() {
                vars.push_layer(&folder.variables);
            }
        }
        Ok(vars)
    }

    /// The environment that will be used when none is given.
    pub fn resolve_environment(&self, environment: Option<String>) -> Option<String> {
        environment.or_else(|| self.workspace()?.settings().ok()?.default_environment).filter(|e| !e.is_empty())
    }

    /// Runs a saved request by its workspace id.
    pub async fn send_saved(&self, request_id: &str, mut options: SendOptions) -> Result<Execution> {
        let request = self.require_workspace()?.read_request(request_id)?;
        options.request_id = Some(request_id.to_string());
        Ok(self.send(&request, options).await)
    }

    /// Runs a request. Never panics or returns early: every outcome, including
    /// errors, is reported as events and in the returned [`Execution`].
    pub async fn send(&self, request: &Request, options: SendOptions) -> Execution {
        self.send_with_token(request, options, None).await
    }

    pub(crate) async fn send_with_token(
        &self,
        request: &Request,
        options: SendOptions,
        parent_token: Option<&CancellationToken>,
    ) -> Execution {
        let title = if request.name.is_empty() { format!("{} {}", request.method.as_str(), request.url) } else { request.name.clone() };
        let summary = Summary {
            protocol: Some(protocol_of(request).into()),
            method: Some(request.method.as_str().to_ascii_uppercase()),
            url: Some(request.url.clone()),
            request_id: options.request_id.clone(),
        };
        let span = match &options.parent {
            Some(parent) => parent.child(SpanKind::Request, title, summary),
            None => Span::start(&self.bus, options.source.clone(), None, SpanKind::Request, title, summary),
        };
        let guard = self.track(&span, parent_token);
        let mut execution = Execution {
            span_id: span.id(),
            request: None,
            response: None,
            tests: Vec::new(),
            extracted: Vec::new(),
            outcome: Outcome::Error,
            error: None,
        };
        let result = tokio::select! {
            result = self.execute(request, &options, &span, &guard.token, &mut execution) => result,
            () = guard.token.cancelled() => Err(Error::Cancelled),
        };
        match result {
            Ok(()) => {
                let failed = execution.tests.iter().any(|t| !t.passed);
                execution.outcome = if failed { Outcome::Failed } else { Outcome::Ok };
                let response = execution.response.as_ref();
                span.finish(execution.outcome, response.map(|r| r.status), response.map(|r| r.body.len() as u64), None);
            }
            Err(Error::Cancelled) => {
                execution.outcome = Outcome::Cancelled;
                execution.error = Some("cancelled".into());
                span.finish(Outcome::Cancelled, None, None, None);
            }
            Err(error) => {
                let message = error.to_string();
                span.log(Level::Error, &message);
                execution.error = Some(message);
                span.finish_error(error);
            }
        }
        execution
    }

    async fn execute(
        &self,
        request: &Request,
        options: &SendOptions,
        span: &Span,
        cancel: &CancellationToken,
        execution: &mut Execution,
    ) -> Result<()> {
        let extensions = self.extensions();
        let workspace = self.workspace();
        let folders = match (&workspace, &options.request_id) {
            (Some(ws), Some(id)) => ws.folder_chain(id)?,
            _ => Vec::new(),
        };
        let environment = self.resolve_environment(options.environment.clone());

        // 1. Before-request script: may set variables used below.
        if !request.scripts.before_request.trim().is_empty() {
            let vars = self.variables(environment.as_deref(), &folders)?;
            let input = ScriptInput {
                request: ScriptRequest {
                    method: request.method.as_str().to_string(),
                    url: request.url.clone(),
                    headers: request.headers.iter().filter(|h| h.enabled).map(|h| (h.key.clone(), h.value.clone())).collect(),
                    body: String::new(),
                },
                response: None,
                variables: vars.flatten(),
                environment: environment.clone(),
            };
            let output = self.run_script(&extensions, ScriptPhase::BeforeRequest, &request.scripts.before_request, input, span).await?;
            execution.tests.extend(output.tests);
        }

        // 2. Prepare: variables, folder inheritance, body, built-in auth.
        let vars = self.variables(environment.as_deref(), &folders)?;
        let ctx = self.template_context(vars, span);
        let prepared = prepare(request, &folders, &ctx).await?;
        if !prepared.unresolved.is_empty() {
            span.log(
                Level::Warn,
                format!("Unknown variable(s): {}. Check the spelling or the selected environment", prepared.unresolved.join(", ")),
            );
        }
        let mut outgoing = prepared.request;

        // 3. Plugin auth and hooks.
        if let Some((name, config)) = prepared.plugin_auth {
            outgoing = extensions.apply_auth(&name, &config, outgoing, span).await?;
        }
        outgoing = extensions.before_request(outgoing, span).await?;
        execution.request = Some(outgoing.clone());

        // 4. Send.
        let response = http::send(outgoing.clone(), span, Some(&self.cookies), cancel).await?;
        let response = extensions.after_response(&outgoing, response, span).await?;

        // 5. Tests.
        let data = response.data();
        for source in request.tests.iter().filter(|t| !t.trim().is_empty()) {
            let result = match Assertion::parse(source) {
                Ok(assertion) => assertion.evaluate(&data),
                Err(e) => AssertionResult { name: source.clone(), passed: false, detail: Some(e.to_string()) },
            };
            span.emit(EventKind::TestResult { name: result.name.clone(), passed: result.passed, detail: result.detail.clone() });
            execution.tests.push(result);
        }

        // 6. After-response script.
        if !request.scripts.after_response.trim().is_empty() {
            let input = ScriptInput {
                request: ScriptRequest::from_prepared(&outgoing),
                response: Some(ScriptResponse::from_response(&response)),
                variables: self.variables(environment.as_deref(), &folders)?.flatten(),
                environment: environment.clone(),
            };
            let output = self.run_script(&extensions, ScriptPhase::AfterResponse, &request.scripts.after_response, input, span).await?;
            execution.tests.extend(output.tests);
        }

        // 7. Extract variables for the next requests.
        for (name, expr) in &request.extract {
            let expr = crate::model::value_to_string(expr);
            match extract(&expr, &data) {
                Ok(Some(value)) => {
                    self.set_runtime_variable(name, &value);
                    span.emit(EventKind::VariableSet { name: name.clone(), value: value.clone() });
                    execution.extracted.push((name.clone(), value));
                }
                Ok(None) => span.log(Level::Warn, format!("Could not extract `{name}`: `{expr}` found nothing")),
                Err(e) => span.log(Level::Warn, format!("Could not extract `{name}`: {e}")),
            }
        }

        execution.response = Some(response);
        Ok(())
    }

    /// Shows exactly what would be sent, without sending it. Secrets are
    /// masked and scripts, plugin auth and hooks are not run.
    pub async fn preview(&self, request: &Request, request_id: Option<&str>, environment: Option<String>) -> Result<PreparedRequest> {
        let folders = match (self.workspace(), request_id) {
            (Some(ws), Some(id)) => ws.folder_chain(id)?,
            _ => Vec::new(),
        };
        let vars = self.variables(self.resolve_environment(environment).as_deref(), &folders)?;
        let span = Span::start(&EventBus::new(crate::events::BusConfig { history_len: 1, blob_capacity_bytes: 0 }), Source::System, None, SpanKind::System, "preview", Summary::default());
        let ctx = MaskedSecrets(self.template_context(vars, &span));
        let prepared = prepare(request, &folders, &ctx).await?;
        span.finish_ok(None, None);
        Ok(prepared.request)
    }

    async fn run_script(
        &self,
        extensions: &Arc<dyn Extensions>,
        phase: ScriptPhase,
        code: &str,
        input: ScriptInput,
        span: &Span,
    ) -> Result<ScriptOutput> {
        let output = extensions.run_script(phase, code, input, span).await?;
        for (name, value) in &output.set_variables {
            self.set_runtime_variable(name, value);
            span.emit(EventKind::VariableSet { name: name.clone(), value: value.clone() });
        }
        {
            let mut runtime = self.runtime.lock().expect("runtime lock poisoned");
            for name in &output.unset_variables {
                runtime.remove(name);
            }
        }
        for test in &output.tests {
            span.emit(EventKind::TestResult { name: test.name.clone(), passed: test.passed, detail: test.detail.clone() });
        }
        Ok(output)
    }
}

/// Removes an operation from the active list when it ends.
#[derive(Debug)]
pub struct ActiveGuard<'a> {
    engine: &'a Engine,
    id: SpanId,
    pub token: CancellationToken,
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        self.engine.active.lock().expect("active lock poisoned").remove(&self.id);
    }
}

fn protocol_of(request: &Request) -> &'static str {
    match request.body {
        crate::model::Body::Graphql { .. } => "graphql",
        _ => "http",
    }
}

/// Shows secrets as `•••` instead of their values.
struct MaskedSecrets<T>(T);

#[async_trait]
impl<T: TemplateContext> TemplateContext for MaskedSecrets<T> {
    fn variable(&self, name: &str) -> Option<String> {
        self.0.variable(name)
    }

    async fn secret(&self, _name: &str) -> Result<String> {
        Ok("•••".into())
    }

    async fn function(&self, name: &str, args: &OrderedMap) -> Result<Option<String>> {
        self.0.function(name, args).await
    }
}

/// Resolves templates using the engine's variables, secrets and plugins.
struct EngineContext {
    vars: Variables,
    secrets: Arc<dyn SecretStore>,
    extensions: Arc<dyn Extensions>,
    span: Span,
}

#[async_trait]
impl TemplateContext for EngineContext {
    fn variable(&self, name: &str) -> Option<String> {
        self.vars.get(name).map(str::to_string)
    }

    async fn secret(&self, name: &str) -> Result<String> {
        self.secrets.get(name).await?.ok_or_else(|| Error::missing_secret(name))
    }

    async fn function(&self, name: &str, args: &OrderedMap) -> Result<Option<String>> {
        self.extensions.template_function(name, args, &self.span).await
    }
}
