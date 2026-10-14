//! The MCP server: AI agents (Claude Code, Cursor, …) use post-tester.
//!
//! Every tool call is a span with `Source::Agent { client }`, so the user sees
//! live what the agent does: the call, its arguments, its result, and the
//! requests it sends (as child spans).

mod activity;
mod tools;

use bytes::Bytes;
use post_tester_core::events::{Direction, EventKind, Source, Span, SpanKind, Summary};
use post_tester_core::{Engine, Error, Result};
use post_tester_plugin_host::PluginHost;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListResourcesResult, ListToolsResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, ResourceContents, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use std::sync::Arc;

/// Shown to agents when they connect: what post-tester is and how to use it.
const INSTRUCTIONS: &str = "\
post-tester is the user's local-first API client. It holds their saved HTTP and \
GraphQL requests (a workspace of YAML files), environments with variables, and tests such as `status == 200`. \
Everything you do here appears live in the user's post-tester window, marked as done by an AI agent.

Recommended workflow:
1. `get_workspace`: see the saved requests (ids such as `auth/login`), the environments and the default environment.
2. `get_request`: read one request: method, URL with {{variables}}, headers, body, auth, tests and extraction rules.
3. `send_request`: run it (by id, or an inline request) and inspect the status, body and test results. Values \
extracted from responses (for example a login token) are kept as variables for later requests.
4. When tests fail, read the failure details, fix the request or the API, and send again. Use `run_collection` \
to run a whole folder as a regression check, and `get_recent_activity` to see what the user ran recently.

Requests use `{{name}}` for variables and `{{secret:name}}` for secrets. Never ask for, print or store secret \
values: reference them. Use `save_request` only when the user wants a request created or changed.";

/// Resource URIs for saved requests: `post-tester://requests/{id}`.
const REQUEST_URI: &str = "post-tester://requests/";

/// Server settings.
#[derive(Debug, Clone, Default)]
pub struct ServerOptions {
    /// Agents may read and send, but not change the workspace: no
    /// `save_request` and no plugin tools that need `fs:write`.
    pub read_only: bool,
}

/// The MCP server. Cheap to clone: one clone serves each HTTP session.
#[derive(Debug, Clone)]
pub struct McpServer {
    engine: Arc<Engine>,
    plugins: Option<Arc<PluginHost>>,
    options: ServerOptions,
}

impl McpServer {
    pub fn new(engine: Arc<Engine>, plugins: Option<Arc<PluginHost>>, options: ServerOptions) -> Self {
        Self { engine, plugins, options }
    }

    /// Runs one tool call as an agent span: arguments in, result out.
    async fn call(&self, name: &str, arguments: Value, client: String) -> CallToolResult {
        let source = Source::Agent { client };
        let summary = Summary { protocol: Some("mcp".into()), method: Some("tools/call".into()), url: None, request_id: None };
        let plugin_tool = self.plugin_tool(name);
        let kind = if plugin_tool.is_some() { SpanKind::PluginCall } else { SpanKind::McpCall };
        let span = Span::start(self.engine.bus(), source, None, kind, name, summary);
        message(&span, Direction::Received, &arguments);

        let result = match plugin_tool {
            Some((plugin, tool)) => self.call_plugin_tool(&plugin, &tool, arguments, &span).await,
            None => tools::call(self, name, arguments, &span).await,
        };
        match result {
            Ok(value) => {
                message(&span, Direction::Sent, &value);
                span.finish_ok(None, None);
                CallToolResult::success(vec![ContentBlock::text(value.to_string())])
            }
            Err(error) => {
                span.finish_error(&error);
                CallToolResult::error(vec![ContentBlock::text(error.to_string())])
            }
        }
    }

    /// The plugin and tool name behind a `plugin__tool` name, if any.
    fn plugin_tool(&self, name: &str) -> Option<(Arc<post_tester_plugin_host::Plugin>, String)> {
        self.plugin_tools().into_iter().find(|(n, _, _)| n == name).map(|(_, plugin, info)| (plugin, info.name))
    }

    /// Plugin MCP tools agents may use (write-capable ones are hidden when read-only).
    fn plugin_tools(&self) -> Vec<(String, Arc<post_tester_plugin_host::Plugin>, post_tester_plugin_host::McpToolInfo)> {
        let Some(host) = &self.plugins else { return Vec::new() };
        host.mcp_tools().into_iter().filter(|(_, plugin, _)| !(self.options.read_only && writes(plugin))).collect()
    }

    async fn call_plugin_tool(&self, plugin: &post_tester_plugin_host::Plugin, tool: &str, arguments: Value, span: &Span) -> Result<Value> {
        let host = self.plugins.as_ref().ok_or_else(|| Error::Plugin("plugins are not loaded".into()))?;
        host.call_mcp_tool(plugin, tool, arguments, span).await
    }

    fn list_request_resources(&self) -> Result<Vec<rmcp::model::Resource>> {
        let tree = self.engine.require_workspace()?.tree()?;
        let mut resources = Vec::new();
        let mut folders = vec![&tree];
        while let Some(folder) = folders.pop() {
            for request in &folder.requests {
                resources.push(
                    rmcp::model::Resource::new(format!("{REQUEST_URI}{}", request.id), request.name.clone())
                        .with_description(format!("Saved request: {} {}", request.method, request.url))
                        .with_mime_type("application/json"),
                );
            }
            folders.extend(folder.folders.iter());
        }
        Ok(resources)
    }

    fn read_request_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        let id = uri
            .strip_prefix(REQUEST_URI)
            .ok_or_else(|| Error::NotFound(format!("resource `{uri}` (post-tester serves `{REQUEST_URI}<request id>`)")))?;
        let request = self.engine.require_workspace()?.read_request(id)?;
        let text = serde_json::to_string_pretty(&request).map_err(|e| Error::Mcp(e.to_string()))?;
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri).with_mime_type("application/json")]))
    }
}

/// True when a plugin may change files.
fn writes(plugin: &post_tester_plugin_host::Plugin) -> bool {
    plugin.manifest.parsed_permissions().contains(&post_tester_plugin_host::manifest::Permission::FsWrite)
}

/// Reports a JSON message on a span (direction is from post-tester's point of view).
fn message(span: &Span, direction: Direction, value: &Value) {
    let data = Bytes::from(value.to_string());
    let payload = span.bus().blobs().payload(&data, Some("application/json".into()));
    span.emit(EventKind::Message { direction, label: Some("tools/call".into()), payload });
}

/// The connected agent's name, e.g. `claude-code`. Sent in `initialize`
/// (or, from protocol 2026-07-28, with every request).
fn client_name(context: &RequestContext<RoleServer>) -> String {
    context.client_info().map(|info| info.name).filter(|name| !name.trim().is_empty()).unwrap_or_else(|| "AI agent".into())
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("post-tester", post_tester_core::VERSION))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        let mut list = tools::builtin(&self.options, self.plugins.is_some());
        for (name, _, info) in self.plugin_tools() {
            let schema = match info.input_schema {
                Value::Object(schema) => schema,
                _ => serde_json::Map::from_iter([("type".to_string(), Value::from("object"))]),
            };
            list.push(rmcp::model::Tool::new(name, info.description, schema));
        }
        Ok(ListToolsResult::with_all_items(list))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        let name = request.name.to_string();
        if !tools::is_builtin(&name, self.plugins.is_some()) && self.plugin_tool(&name).is_none() {
            return Err(McpError::invalid_params(format!("unknown tool `{name}`. Call tools/list to see the available tools"), None));
        }
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        Ok(self.call(&name, arguments, client_name(&context)).await.into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, McpError> {
        // No workspace simply means no resources.
        Ok(ListResourcesResult::with_all_items(self.list_request_resources().unwrap_or_default()))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, McpError> {
        self.read_request_resource(&request.uri).map(Into::into).map_err(|e| McpError::resource_not_found(e.to_string(), None))
    }
}

/// Serves MCP on stdin/stdout until the agent disconnects (`pt mcp`).
///
/// Nothing else may write to stdout while this runs: it carries the protocol.
pub async fn serve_stdio(engine: Arc<Engine>, plugins: Option<Arc<PluginHost>>, options: ServerOptions) -> Result<()> {
    let server = McpServer::new(engine, plugins, options);
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| Error::Mcp(format!("the agent did not complete the MCP handshake on stdin/stdout: {e}")))?;
    running.waiting().await.map_err(|e| Error::Mcp(format!("the MCP server stopped unexpectedly: {e}")))?;
    Ok(())
}

/// A Streamable HTTP MCP endpoint as a tower service. Mount it in axum:
/// `Router::new().nest_service("/mcp", http_service(..))`.
///
/// Only loopback `Host` headers (`localhost`, `127.0.0.1`, `::1`) are accepted,
/// which blocks DNS rebinding. Authentication is up to the router.
pub fn http_service(
    engine: Arc<Engine>,
    plugins: Option<Arc<PluginHost>>,
    options: ServerOptions,
) -> StreamableHttpService<McpServer, LocalSessionManager> {
    let server = McpServer::new(engine, plugins, options);
    StreamableHttpService::new(move || Ok(server.clone()), Arc::default(), StreamableHttpServerConfig::default())
}
