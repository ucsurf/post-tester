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
