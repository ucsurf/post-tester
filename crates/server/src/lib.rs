//! # post-tester server
//!
//! Serves the engine over HTTP and WebSocket for the web UI and desktop app,
//! plus an MCP endpoint for AI agents. See `API.md` for the full contract.
//!
//! | Module      | Responsibility                                     |
//! |-------------|----------------------------------------------------|
//! | [`security`] | Host/Origin checks and the session token          |
//! | [`routes`]  | The HTTP API, grouped by topic                     |
//! | [`views`]   | JSON shapes of engine results                      |
//! | [`assets`]  | The embedded web UI                                |

pub mod assets;
pub mod error;
pub mod routes;
pub mod security;
pub mod state;
pub mod views;

use axum::Router;
use post_tester_core::Engine;
use post_tester_mcp::{McpClients, ServerOptions as McpServerOptions};
use post_tester_plugin_host::PluginHost;
use state::AppState;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

/// Where and how to listen.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: IpAddr,
    /// `0` picks a free port.
    pub port: u16,
    /// A fixed token (e.g. for the desktop app); random when `None`.
    pub token: Option<String>,
    /// Options for the `/mcp` endpoint that AI agents connect to.
    pub mcp: McpServerOptions,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self { host: IpAddr::V4(Ipv4Addr::LOCALHOST), port: 7777, token: None, mcp: McpServerOptions::default() }
    }
}

/// A running server.
#[derive(Debug)]
pub struct RunningServer {
    pub addr: SocketAddr,
    pub token: String,
    /// The address to open in a browser, token included.
    pub url: String,
    pub task: tokio::task::JoinHandle<std::io::Result<()>>,
}

/// Builds the full router: API, MCP and UI, all behind the security checks.
pub fn router(state: AppState, mcp: McpServerOptions) -> Router {
    let mcp_service = post_tester_mcp::http_service(state.engine.clone(), Some(state.plugins.clone()), mcp);
    routes::api()
        .nest_service("/mcp", mcp_service)
        .fallback(assets::serve)
        .layer(axum::middleware::from_fn_with_state(state.clone(), security::guard))
        .with_state(state)
}

/// Starts listening.
pub async fn serve(config: ServerConfig, engine: Arc<Engine>, plugins: Arc<PluginHost>) -> std::io::Result<RunningServer> {
    let listener = tokio::net::TcpListener::bind((config.host, config.port)).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            std::io::Error::new(
                e.kind(),
                format!("port {} is already in use. Stop the other program or choose another port with --port", config.port),
            )
        } else {
            e
        }
    })?;
    let addr = listener.local_addr()?;
    let token = config.token.unwrap_or_else(security::new_token);
    let port = addr.port();
    let mut allowed_hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")];
    if !addr.ip().is_loopback() {
        allowed_hosts.push(addr.to_string());
    }
    let base_url = format!("http://127.0.0.1:{port}");
    let mcp = Arc::new(McpClients::new(engine.clone()));
    let state = AppState { engine, plugins, mcp, token: token.clone(), allowed_hosts: Arc::new(allowed_hosts), base_url: base_url.clone() };
    let app = router(state, config.mcp);
    let task = tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(RunningServer { addr, url: format!("{base_url}/?token={token}"), token, task })
}
