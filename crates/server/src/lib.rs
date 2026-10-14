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
//! | [`mock`]    | A mock API built from saved example responses      |

pub mod assets;
pub mod error;
pub mod mock;
pub mod routes;
pub mod security;
pub mod state;
pub mod views;

use axum::Router;
use post_tester_core::Engine;
use post_tester_core::events::EventBus;
use post_tester_mcp::{McpClients, McpServer, ServerOptions as McpServerOptions};
use post_tester_plugin_host::PluginHost;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use state::AppState;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Largest request body accepted by the API (requests with big bodies, saved files).
pub const API_BODY_LIMIT: usize = 32 * 1024 * 1024;
/// Largest body accepted by `/api/import` (large OpenAPI or Postman documents).
pub const IMPORT_BODY_LIMIT: usize = 128 * 1024 * 1024;

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
    /// The address of this server without the token, e.g. `http://127.0.0.1:7777`.
    pub base_url: String,
    /// The address to open in a browser, token included.
    pub url: String,
    pub task: JoinHandle<std::io::Result<()>>,
    /// Cancelling it stops the server; see [`RunningServer::shutdown`].
    pub shutdown: CancellationToken,
    bus: EventBus,
}

impl RunningServer {
    /// Stops accepting connections, ends live streams and MCP sessions, gives
    /// requests in flight up to `grace` to finish, and writes out history.
    pub async fn shutdown(self, grace: Duration) -> std::io::Result<()> {
        self.shutdown.cancel();
        let stopped = stop(self.task, grace).await;
        flush(self.bus).await;
        stopped
    }
}

/// Waits until the history of `bus` is written, without blocking the runtime.
pub(crate) async fn flush(bus: EventBus) {
    let _ = tokio::task::spawn_blocking(move || bus.flush()).await;
}

/// Waits up to `grace` for a server task that was asked to stop, then aborts it.
pub(crate) async fn stop(mut task: JoinHandle<std::io::Result<()>>, grace: Duration) -> std::io::Result<()> {
    match tokio::time::timeout(grace, &mut task).await {
        Ok(Ok(result)) => result,
        Ok(Err(join_error)) if join_error.is_cancelled() => Ok(()),
        Ok(Err(join_error)) => Err(std::io::Error::other(join_error)),
        Err(_) => {
            tracing::warn!("requests still running after {grace:?}; stopping anyway");
            task.abort();
            Ok(())
        }
    }
}

/// Builds the full router: API, MCP and UI, all behind the security checks.
pub fn router(state: AppState, mcp: McpServerOptions) -> Router {
    let server = McpServer::new(state.engine.clone(), Some(state.plugins.clone()), mcp);
    let config = StreamableHttpServerConfig::default()
        // `security::guard` wraps `/mcp` too and checks Host and Origin with the same
        // rules as the rest of the server (including `--host`), so the MCP
        // library's own loopback-only Host list would only get in the way.
        .disable_allowed_hosts()
        .with_cancellation_token(state.shutdown.child_token())
        .with_max_request_body_bytes(API_BODY_LIMIT);
    let sessions = Arc::new(LocalSessionManager::default());
    let mcp_service = StreamableHttpService::new(move || Ok(server.clone()), sessions, config);
    routes::api()
        .nest_service("/mcp", mcp_service)
        .fallback(assets::serve)
        .layer(axum::middleware::from_fn_with_state(state.clone(), security::guard))
        .with_state(state)
}

/// Starts listening.
pub async fn serve(config: ServerConfig, engine: Arc<Engine>, plugins: Arc<PluginHost>) -> std::io::Result<RunningServer> {
    let listener = tokio::net::TcpListener::bind((config.host, config.port)).await.map_err(|e| {
        let message = match e.kind() {
            std::io::ErrorKind::AddrInUse => {
                format!("port {} is already in use. Stop the other program or choose another port with --port", config.port)
            }
            std::io::ErrorKind::AddrNotAvailable => {
                format!("{} is not an address of this machine. Use 127.0.0.1, or 0.0.0.0 for every network interface", config.host)
            }
            _ => return e,
        };
        std::io::Error::new(e.kind(), message)
    })?;
    let addr = listener.local_addr()?;
    let token = config.token.unwrap_or_else(security::new_token);
    let base_url = format!("http://{}", SocketAddr::new(reachable_ip(addr.ip()), addr.port()));
    let mcp = Arc::new(McpClients::new(engine.clone()));
    let bus = engine.bus().clone();
    let shutdown = CancellationToken::new();
    let state = AppState {
        engine,
        plugins,
        mcp,
        token: token.clone(),
        hosts: security::AllowedHosts::for_listener(addr),
        base_url: base_url.clone(),
        shutdown: shutdown.clone(),
    };
    let app = router(state, config.mcp);
    let signal = shutdown.clone().cancelled_owned();
    let task = tokio::spawn(async move { axum::serve(listener, app).with_graceful_shutdown(signal).await });
    Ok(RunningServer { addr, url: format!("{base_url}/?token={token}"), base_url, token, task, shutdown, bus })
}

/// An address this machine can use to reach a listener bound to `ip`.
fn reachable_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) if v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(v6) if v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        _ => ip,
    }
}
