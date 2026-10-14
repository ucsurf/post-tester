//! The MCP client: test any MCP server like an API.
//!
//! Each connection is a session: one long-lived `McpCall` span titled
//! `MCP <name>`. Every JSON-RPC message appears live on it as a `Message`
//! event, and every call made through [`McpClients`] (a tool call, a resource
//! read, a prompt) is a child span holding its own request and response.

mod trace;

use post_tester_core::events::{Level, Outcome, Source, Span, SpanId, SpanKind, Summary};
use post_tester_core::{Engine, Error, Result};
use rmcp::model::{
    CallToolRequestParams, ClientCapabilities, ClientConfig, GetPromptRequestParams, Implementation, ProtocolVersion,
    ReadResourceRequestParams,
};
use rmcp::service::{ClientLifecycleMode, Peer, RunningService, serve_client_with_lifecycle_and_ct};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess, Transport as RmcpTransport};
use rmcp::{ClientCacheConfig, RoleClient};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use trace::{Traced, Tracer};
use ts_rs::TS;

/// How long connecting and initializing may take. Generous, because
/// `npx`/`uvx` servers often download packages on first start.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// How long `close` waits for a server to shut down cleanly.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// How many stderr lines of a stdio server are kept to explain failures.
const STDERR_LINES: usize = 20;

/// How to reach an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConnectOptions {
    /// Display name. Defaults to the server's own name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub transport: Transport,
}

/// The transport used to talk to an MCP server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, rename = "McpTransport")]
pub enum Transport {
    /// Start a local process and talk JSON-RPC over its stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        #[ts(as = "Option<BTreeMap<String, String>>", optional)]
        env: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        cwd: Option<String>,
    },
    /// Streamable HTTP.
    Http {
        url: String,
        #[serde(default)]
        #[ts(as = "Option<Vec<(String, String)>>", optional)]
        headers: Vec<(String, String)>,
    },
}

impl Transport {
    /// A short description such as `npx -y @mcp/server-everything` or the URL.
    fn describe(&self) -> String {
        match self {
            Transport::Stdio { command, args, .. } => {
                std::iter::once(command.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
            }
            Transport::Http { url, .. } => url.clone(),
        }
    }
}

/// An open session and what the server offers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub span_id: SpanId,
    pub transport: Transport,
    pub server_name: String,
    pub server_version: String,
    pub protocol_version: String,
    pub instructions: Option<String>,
    /// MCP `Tool` objects, exactly as the server sent them.
    #[ts(type = "Array<Record<string, unknown>>")]
    pub tools: Vec<Value>,
    /// MCP `Resource` objects.
    #[ts(type = "Array<Record<string, unknown>>")]
    pub resources: Vec<Value>,
    /// MCP `Prompt` objects.
    #[ts(type = "Array<Record<string, unknown>>")]
    pub prompts: Vec<Value>,
}

type Client = RunningService<RoleClient, ClientConfig>;
type Sessions = Arc<Mutex<HashMap<String, Arc<Session>>>>;

struct Session {
    info: Mutex<SessionInfo>,
    peer: Peer<RoleClient>,
    span: Span,
    tracer: Arc<Tracer>,
    /// Cancelling it closes the connection.
    cancel: CancellationToken,
    /// Watches the connection and finishes the span when it ends.
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Session {
    fn info(&self) -> SessionInfo {
        self.info.lock().expect("session lock poisoned").clone()
    }
}

/// Open MCP client sessions. Shared by the server's `/api/mcp/*` routes.
pub struct McpClients {
    engine: Arc<Engine>,
    sessions: Sessions,
}

impl std::fmt::Debug for McpClients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClients").field("sessions", &self.lock().len()).finish_non_exhaustive()
    }
}

impl McpClients {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self { engine, sessions: Arc::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Session>>> {
        self.sessions.lock().expect("sessions lock poisoned")
    }

    fn session(&self, id: &str) -> Result<Arc<Session>> {
        self.lock()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::Mcp(format!("no open MCP session `{id}`. It may have been closed; connect again")))
    }

    /// Connects, initializes and lists what the server offers.
    pub async fn connect(&self, options: ConnectOptions, source: Source) -> Result<SessionInfo> {
        let target = options.transport.describe();
        let title = options.name.clone().unwrap_or_else(|| target.clone());
        let url = matches!(options.transport, Transport::Http { .. }).then(|| target.clone());
        let summary = Summary { protocol: Some("mcp".into()), method: None, url, request_id: None };
        let span = Span::start(self.engine.bus(), source, None, SpanKind::McpCall, format!("MCP {title}"), summary);
        span.log(Level::Info, format!("Connecting to {target}"));

        let tracer = Tracer::new(span.clone());
        let cancel = CancellationToken::new();
        let client = match open(&options.transport, &span, &tracer, &cancel).await {
            Ok(client) => client,
            Err(error) => {
                span.finish_error(&error);
                return Err(error);
            }
        };
        // A testing tool must show what the server says now, never a cached answer.
        client.peer().set_response_cache_config(ClientCacheConfig::disabled()).await;

        let id = span.id().to_string();
        let peer = client.peer().clone();
        let info = describe(&id, &options, &span, &peer).await;
        span.log(
            Level::Info,
            format!(
                "Connected to {} {} (protocol {}): {} tool(s), {} resource(s), {} prompt(s)",
                info.server_name,
                info.server_version,
                info.protocol_version,
                info.tools.len(),
                info.resources.len(),
                info.prompts.len()
            ),
        );
        let session = Arc::new(Session {
            info: Mutex::new(info.clone()),
            peer,
            span: span.clone(),
            tracer,
            cancel: cancel.clone(),
            task: Mutex::new(None),
        });
        self.lock().insert(id.clone(), session.clone());
        let task = tokio::spawn(watch(self.engine.clone(), self.sessions.clone(), id, client, span, cancel));
        *session.task.lock().expect("session lock poisoned") = Some(task);
        Ok(info)
    }

    /// Open sessions, oldest first.
    pub fn sessions(&self) -> Vec<SessionInfo> {
        let mut sessions: Vec<SessionInfo> = self.lock().values().map(|s| s.info()).collect();
        // Span ids are time-ordered.
        sessions.sort_by_key(|s| s.span_id.0);
        sessions
    }

    /// Lists tools, resources and prompts again.
    pub async fn refresh(&self, id: &str) -> Result<SessionInfo> {
        let session = self.session(id)?;
        let options = {
            let info = session.info();
            ConnectOptions { name: Some(info.name), transport: info.transport }
        };
        let info = describe(id, &options, &session.span, &session.peer).await;
        *session.info.lock().expect("session lock poisoned") = info.clone();
        Ok(info)
    }

    /// Calls a tool and returns the MCP `CallToolResult` JSON.
    pub async fn call_tool(&self, id: &str, tool: &str, arguments: Value, source: Source) -> Result<Value> {
        let arguments = match arguments {
            Value::Object(map) => map,
            Value::Null => serde_json::Map::new(),
            other => return Err(Error::Mcp(format!("tool arguments must be a JSON object, got `{other}`"))),
        };
        let session = self.session(id)?;
        let span = session.call_span(source, "tools/call", tool);
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments);
        let result = session.run(&span, "tools/call", session.peer.call_tool(params)).await;
        finish_call(&span, result, tool_error)
    }

    /// Reads a resource and returns the MCP `ReadResourceResult` JSON.
    pub async fn read_resource(&self, id: &str, uri: &str, source: Source) -> Result<Value> {
        let session = self.session(id)?;
        let span = session.call_span(source, "resources/read", uri);
        let result = session.run(&span, "resources/read", session.peer.read_resource(ReadResourceRequestParams::new(uri))).await;
        finish_call(&span, result, |_| None)
    }

    /// Gets a prompt and returns the MCP `GetPromptResult` JSON.
    pub async fn get_prompt(&self, id: &str, name: &str, arguments: serde_json::Map<String, Value>, source: Source) -> Result<Value> {
        let session = self.session(id)?;
        let span = session.call_span(source, "prompts/get", name);
        let params = GetPromptRequestParams::new(name).with_arguments(arguments);
        let result = session.run(&span, "prompts/get", session.peer.get_prompt(params)).await;
        finish_call(&span, result, |_| None)
    }

    /// Closes a session and waits (briefly) for the server to shut down.
    pub async fn close(&self, id: &str) -> Result<()> {
        let session = self.lock().remove(id).ok_or_else(|| Error::Mcp(format!("no open MCP session `{id}`")))?;
        session.span.log(Level::Info, "Closed by you");
        session.cancel.cancel();
        let task = session.task.lock().expect("session lock poisoned").take();
        if let Some(task) = task
            && tokio::time::timeout(CLOSE_TIMEOUT, task).await.is_err()
        {
            session.span.log(Level::Warn, "The server did not shut down in time");
        }
        Ok(())
    }
}

impl Session {
    /// A child span for one call, e.g. `tools/call get_weather`.
    fn call_span(&self, source: Source, method: &str, target: &str) -> Span {
        let summary = Summary { protocol: Some("mcp".into()), method: Some(method.into()), url: None, request_id: None };
        Span::start(self.span.bus(), source, Some(self.span.id()), SpanKind::McpCall, format!("{method} {target}"), summary)
    }

    /// Runs a request, routing its messages to `span`, and converts the result to JSON.
    async fn run<T: Serialize>(
        &self,
        span: &Span,
        method: &str,
        call: impl Future<Output = Result<T, rmcp::ServiceError>>,
    ) -> Result<Value> {
        let _expectation = self.tracer.expect(method, span);
        let result = tokio::select! {
            result = call => result,
            () = self.cancel.cancelled() => return Err(Error::Mcp("the session was closed".into())),
        };
        let value = result.map_err(|e| Error::Mcp(format!("{method} failed: {e}")))?;
        serde_json::to_value(value).map_err(|e| Error::Mcp(format!("could not read the server's response: {e}")))
    }
}

/// Finishes a call span. `failure` explains results that report an error
/// (e.g. a tool result with `isError: true`).
fn finish_call(span: &Span, result: Result<Value>, failure: impl Fn(&Value) -> Option<String>) -> Result<Value> {
    match &result {
        Ok(value) => match failure(value) {
            Some(message) => span.finish(Outcome::Failed, None, None, Some(message)),
            None => span.finish_ok(None, None),
        },
        Err(error) => span.finish_error(error),
    }
    result
}

/// The text of a tool result with `isError: true`.
fn tool_error(result: &Value) -> Option<String> {
    if result.get("isError").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let text: Vec<&str> = result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("text").and_then(Value::as_str))
        .collect();
    Some(if text.is_empty() { "the tool reported an error".into() } else { text.join("\n") })
}

// ── Connecting ──────────────────────────────────────────────────────────────

fn client_config() -> ClientConfig {
    // Ask for the newest protocol that still has an `initialize` handshake:
    // it is the one virtually every server in the wild understands.
    ClientConfig::new(ClientCapabilities::default(), Implementation::new("post-tester", post_tester_core::VERSION))
        .with_protocol_version(ProtocolVersion::LATEST_WITH_INITIALIZE)
}

async fn open(transport: &Transport, span: &Span, tracer: &Arc<Tracer>, cancel: &CancellationToken) -> Result<Client> {
    match transport {
        Transport::Stdio { command, args, env, cwd } => {
            let stderr = Arc::new(Mutex::new(VecDeque::new()));
            let child = spawn(command, args, env, cwd.as_deref(), span, &stderr)?;
            initialize(child, tracer, cancel).await.map_err(|e| {
                let output = stderr.lock().expect("stderr lock poisoned").iter().cloned().collect::<Vec<_>>().join("\n");
                let hint = if output.is_empty() {
                    "Check that this command starts an MCP server that speaks JSON-RPC on stdout".to_string()
                } else {
                    format!("Server output:\n{output}")
                };
                Error::Mcp(format!("`{}` did not complete the MCP handshake: {e}. {hint}", transport.describe()))
            })
        }
        Transport::Http { url, headers } => {
            let config = http_config(url, headers)?;
            install_crypto_provider();
            let client = StreamableHttpClientTransport::from_config(config);
            initialize(client, tracer, cancel).await.map_err(|e| {
                Error::Mcp(format!(
                    "could not connect to the MCP server at {url}: {e}. Check the URL (it usually ends in `/mcp`), that the server is running, and any required headers such as Authorization"
                ))
            })
        }
    }
}

/// Starts a stdio server, forwarding its stderr to the session span.
fn spawn(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: Option<&str>,
    span: &Span,
    stderr: &Arc<Mutex<VecDeque<String>>>,
) -> Result<TokioChildProcess> {
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args).envs(env);
    if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
        cmd.current_dir(cwd);
    }
    let shown = std::iter::once(command).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
    let (child, output) = TokioChildProcess::builder(cmd).stderr(std::process::Stdio::piped()).spawn().map_err(|e| {
        let reason = match e.kind() {
            std::io::ErrorKind::NotFound => {
                "command not found — check the command and that it is on PATH (or use its full path)".to_string()
            }
            std::io::ErrorKind::PermissionDenied => "permission denied — check that the file is executable".to_string(),
            _ if cwd.is_some() => format!("{e} — check the command and the working directory"),
            _ => e.to_string(),
        };
        Error::Mcp(format!("could not start `{shown}`: {reason}"))
    })?;
    if let Some(output) = output {
        let span = span.clone();
        let stderr = stderr.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(output).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                span.log(Level::Info, format!("stderr: {line}"));
                let mut kept = stderr.lock().expect("stderr lock poisoned");
                if kept.len() == STDERR_LINES {
                    kept.pop_front();
                }
                kept.push_back(line);
            }
        });
    }
    Ok(child)
}

fn http_config(url: &str, headers: &[(String, String)]) -> Result<StreamableHttpClientTransportConfig> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(Error::Mcp(format!("`{url}` is not an HTTP URL. Use e.g. `http://localhost:3000/mcp`")));
    }
    let mut custom = HashMap::new();
    for (name, value) in headers.iter().filter(|(name, _)| !name.trim().is_empty()) {
        let header_name =
            http::HeaderName::from_bytes(name.trim().as_bytes()).map_err(|_| Error::Mcp(format!("`{name}` is not a valid header name")))?;
        let header_value = http::HeaderValue::from_str(value)
            .map_err(|_| Error::Mcp(format!("the value of header `{name}` contains invalid characters")))?;
        custom.insert(header_name, header_value);
    }
    Ok(StreamableHttpClientTransportConfig::with_uri(url.to_string()).custom_headers(custom))
}

/// HTTPS needs a process-wide rustls crypto provider. post-tester uses `ring`
/// everywhere, so install it unless something else already did.
fn install_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        // Fails only if another thread installed one first, which is fine.
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

async fn initialize<T>(transport: T, tracer: &Arc<Tracer>, cancel: &CancellationToken) -> std::result::Result<Client, String>
where
    T: RmcpTransport<RoleClient> + 'static,
{
    let traced = Traced::new(transport, tracer.clone());
    let start = serve_client_with_lifecycle_and_ct(client_config(), traced, ClientLifecycleMode::Initialize, cancel.clone());
    match tokio::time::timeout(CONNECT_TIMEOUT, start).await {
        Ok(Ok(client)) => Ok(client),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            cancel.cancel();
            Err(format!("no answer within {} seconds", CONNECT_TIMEOUT.as_secs()))
        }
    }
}

/// Reads the server's identity and lists what it offers. Lists the server
/// does not support are left empty (and logged), never fatal.
async fn describe(id: &str, options: &ConnectOptions, span: &Span, peer: &Peer<RoleClient>) -> SessionInfo {
    let peer_info = peer.peer_info();
    let server = peer_info.as_deref().and_then(|p| p.server_info.clone());
    let server_name = server.as_ref().map(|s| s.name.clone()).unwrap_or_default();
    let capabilities = peer_info.as_deref().map(|p| p.capabilities.clone()).unwrap_or_default();

    let tools = if capabilities.tools.is_some() { list(span, "tools", peer.list_all_tools()).await } else { Vec::new() };
    let resources = if capabilities.resources.is_some() { list(span, "resources", peer.list_all_resources()).await } else { Vec::new() };
    let prompts = if capabilities.prompts.is_some() { list(span, "prompts", peer.list_all_prompts()).await } else { Vec::new() };

    SessionInfo {
        id: id.to_string(),
        name: options
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| if server_name.is_empty() { options.transport.describe() } else { server_name.clone() }),
        span_id: span.id(),
        transport: options.transport.clone(),
        server_name,
        server_version: server.map(|s| s.version).unwrap_or_default(),
        protocol_version: peer_info.as_deref().map(|p| p.protocol_version.to_string()).unwrap_or_default(),
        instructions: peer_info.as_deref().and_then(|p| p.instructions.clone()),
        tools,
        resources,
        prompts,
    }
}

async fn list<T: Serialize>(span: &Span, what: &str, request: impl Future<Output = Result<Vec<T>, rmcp::ServiceError>>) -> Vec<Value> {
    match request.await {
        Ok(items) => items.iter().filter_map(|item| serde_json::to_value(item).ok()).collect(),
        Err(e) => {
            span.log(Level::Warn, format!("Could not list {what}: {e}"));
            Vec::new()
        }
    }
}

/// Owns the connection until it ends: closed by the user (`close` or the
/// engine's cancel button) or by the server. Then finishes the session span.
async fn watch(engine: Arc<Engine>, sessions: Sessions, id: String, client: Client, span: Span, cancel: CancellationToken) {
    let guard = engine.track(&span, None);
    let waiting = client.waiting();
    tokio::pin!(waiting);
    let ended = tokio::select! {
        ended = &mut waiting => ended,
        () = guard.token.cancelled() => {
            cancel.cancel();
            waiting.await
        }
    };
    drop(guard);
    sessions.lock().expect("sessions lock poisoned").remove(&id);
    match ended {
        _ if cancel.is_cancelled() => span.finish_ok(None, None),
        Ok(_) => span.finish_error("the MCP server closed the connection"),
        Err(e) => span.finish_error(format!("the MCP session stopped unexpectedly: {e}")),
    }
}
