//! Shared state for every request handler.

use crate::security::AllowedHosts;
use post_tester_core::Engine;
use post_tester_mcp::McpClients;
use post_tester_plugin_host::PluginHost;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Engine>,
    pub plugins: Arc<PluginHost>,
    /// Connections to MCP servers the user is testing.
    pub mcp: Arc<McpClients>,
    pub token: String,
    /// Values accepted in the `Host` and `Origin` headers.
    pub hosts: AllowedHosts,
    /// The base URL of this server, e.g. `http://127.0.0.1:7777`.
    pub base_url: String,
    /// Cancelled when the server shuts down; long-lived streams end then.
    pub shutdown: CancellationToken,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").field("base_url", &self.base_url).finish_non_exhaustive()
    }
}
