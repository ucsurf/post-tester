//! `pt serve` (the app) and `pt mcp` (MCP over stdio for AI agents).

use crate::context::Context;
use crate::style;
use anyhow::Result;
use clap::Args;
use post_tester_server::{ServerConfig, serve as start_server};
use std::net::IpAddr;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// How long requests in flight may take to finish after Ctrl+C.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Waits for Ctrl+C, or for the server to stop by itself (an error).
async fn wait_for_ctrl_c(task: &mut tokio::task::JoinHandle<std::io::Result<()>>) -> Result<bool> {
    tokio::select! {
        result = task => { result??; Ok(false) }
        signal = tokio::signal::ctrl_c() => { signal?; Ok(true) }
    }
}

/// Runs `shutdown`, unless Ctrl+C is pressed again to stop right away.
async fn stop_gracefully(shutdown: impl std::future::Future<Output = std::io::Result<()>>) -> Result<()> {
    println!("\nStopping (press Ctrl+C again to stop now)…");
    tokio::select! {
        result = shutdown => result?,
        _ = tokio::signal::ctrl_c() => {}
    }
    println!("Stopped.");
    Ok(())
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(long, default_value_t = 7777)]
    port: u16,
    /// Address to listen on. 0.0.0.0 (every interface) or a network address exposes
    /// post-tester to your network; other devices then open it by IP address.
    #[arg(long, default_value = "127.0.0.1")]
    host: IpAddr,
    /// Don't open the browser.
    #[arg(long)]
    no_open: bool,
    /// Don't let AI agents connected over MCP change the workspace.
    #[arg(long)]
    read_only: bool,
}

pub async fn serve(workspace: Option<&Path>, args: ServeArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    // Show work started elsewhere (`pt run` in a terminal, AI agents over stdio) live in the app.
    let _follower =
        ctx.engine.workspace().and_then(|ws| ws.local_dir().ok()).map(|dir| post_tester_core::history::follow(ctx.engine.bus(), &dir));
    let mcp = post_tester_mcp::ServerOptions { read_only: args.read_only };
    let config = ServerConfig { host: args.host, port: args.port, token: None, mcp };
    let mut server = start_server(config, ctx.engine.clone(), ctx.plugins.clone()).await?;

    let workspace_line = match ctx.engine.workspace() {
        Some(ws) => ws.root().display().to_string(),
        None => style::yellow("none (run `pt init` to create one)"),
    };
    println!();
    println!("  {} {} is running", style::bold("post-tester"), post_tester_core::VERSION);
    println!();
    println!("  Workspace  {workspace_line}");
    println!("  Open       {}", style::cyan(&server.url));
    println!("  MCP        {}/mcp  (header: Authorization: Bearer {})", server.base_url, server.token);
    println!("             or over stdio: claude mcp add post-tester -- pt mcp");
    if !args.host.is_loopback() {
        let address = if args.host.is_unspecified() { "<this computer's IP address>".to_string() } else { args.host.to_string() };
        println!();
        println!("  {} listening on {} — anyone on your network with the token can use it", style::yellow("warning:"), args.host);
        println!("  From another device, open http://{address}:{}/?token={}", server.addr.port(), server.token);
        println!("  (by IP address: other host names are rejected to block DNS rebinding)");
    }
    println!();
    println!("  Press Ctrl+C to stop.");
    if !args.no_open && open::that(&server.url).is_err() {
        println!("  (Could not open a browser; copy the link above.)");
    }

    if wait_for_ctrl_c(&mut server.task).await? {
        stop_gracefully(server.shutdown(SHUTDOWN_GRACE)).await?;
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Args)]
pub struct MockArgs {
    /// Folder whose examples to serve (default: the whole workspace).
    #[arg(default_value = "")]
    folder: String,
    #[arg(long, default_value_t = 4010)]
    port: u16,
    /// Environment for variables used in example responses.
    #[arg(short = 'e', long)]
    environment: Option<String>,
}

pub async fn mock(workspace: Option<&Path>, args: MockArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    if post_tester_server::mock::routes(&ctx.workspace()?, &args.folder)?.is_empty() {
        anyhow::bail!("no request in this folder has examples. Add `examples:` to a request file (see guide/cli.md)");
    }
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    let mut running =
        post_tester_server::mock::start(ctx.engine.clone(), &args.folder, args.environment, addr).await.map_err(|e| match e {
            post_tester_core::Error::Io(io) if io.kind() == std::io::ErrorKind::AddrInUse => {
                anyhow::anyhow!("port {} is already in use. Stop the other program or choose another port with --port", args.port)
            }
            other => other.into(),
        })?;
    let printer = crate::printer::LivePrinter::start(ctx.engine.bus(), crate::printer::PrintOptions::default());
    println!();
    println!("  {} on http://{}", style::bold("Mock API"), running.addr);
    println!();
    for route in &running.routes {
        let names: Vec<&str> = route.examples.iter().map(|e| e.name.as_str()).collect();
        println!("  {:<7} {:<32} {}", route.method, route.path, style::dimmed(names.join(", ")));
    }
    println!();
    println!("  Pick an example with the header `{}: <name>`. Press Ctrl+C to stop.", post_tester_server::mock::EXAMPLE_HEADER);
    println!();
    if wait_for_ctrl_c(&mut running.task).await? {
        stop_gracefully(running.shutdown(SHUTDOWN_GRACE)).await?;
    }
    printer.finish().await;
    Ok(ExitCode::SUCCESS)
}

#[derive(Debug, Args)]
pub struct McpArgs {
    /// Don't let agents change the workspace.
    #[arg(long)]
    read_only: bool,
}

/// Serves MCP over stdin/stdout. Everything else must go to stderr, because
/// stdout carries the protocol.
pub async fn mcp(workspace: Option<&Path>, args: McpArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    let options = post_tester_mcp::ServerOptions { read_only: args.read_only };
    post_tester_mcp::serve_stdio(ctx.engine.clone(), Some(ctx.plugins.clone()), options).await?;
    Ok(ExitCode::SUCCESS)
}
