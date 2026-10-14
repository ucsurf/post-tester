//! `pt serve` (the app) and `pt mcp` (MCP over stdio for AI agents).

use crate::context::Context;
use crate::style;
use anyhow::Result;
use clap::Args;
use post_tester_server::{ServerConfig, serve as start_server};
use std::net::IpAddr;
use std::path::Path;
use std::process::ExitCode;

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(long, default_value_t = 7777)]
    port: u16,
    /// Address to listen on. Anything other than 127.0.0.1 exposes post-tester to your network.
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
    let server = start_server(config, ctx.engine.clone(), ctx.plugins.clone()).await?;

    let workspace_line = match ctx.engine.workspace() {
        Some(ws) => ws.root().display().to_string(),
        None => style::yellow("none (run `pt init` to create one)"),
    };
    println!();
    println!("  {} {} is running", style::bold("post-tester"), post_tester_core::VERSION);
    println!();
    println!("  Workspace  {workspace_line}");
    println!("  Open       {}", style::cyan(&server.url));
    println!("  MCP        http://127.0.0.1:{}/mcp  (header: Authorization: Bearer {})", server.addr.port(), server.token);
    println!("             or over stdio: claude mcp add post-tester -- pt mcp");
    if !args.host.is_loopback() {
        println!();
        println!("  {} listening on {} — anyone on your network with the token can use it", style::yellow("warning:"), args.host);
    }
    println!();
    println!("  Press Ctrl+C to stop.");
    if !args.no_open && open::that(&server.url).is_err() {
        println!("  (Could not open a browser; copy the link above.)");
    }

    tokio::select! {
        result = server.task => { result??; }
        _ = tokio::signal::ctrl_c() => println!("\nStopped."),
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
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], args.port));
    let printer = crate::printer::LivePrinter::start(ctx.engine.bus(), crate::printer::PrintOptions::default());
    let running = post_tester_server::mock::start(ctx.engine.clone(), &args.folder, args.environment, addr).await?;
    if running.routes.is_empty() {
        anyhow::bail!("no request in this folder has examples. Add `examples:` to a request file (see guide/cli.md)");
    }
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
    tokio::select! {
        result = running.task => { result??; }
        _ = tokio::signal::ctrl_c() => println!("\nStopped."),
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
