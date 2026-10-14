//! `pt`: the post-tester command-line tool.

mod commands;
mod context;
mod printer;
mod style;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(name = "pt", version, about = "post-tester: build, send, test and watch your APIs live", max_term_width = 100)]
struct Cli {
    /// Workspace folder (default: the current folder or the nearest parent with post-tester.yaml).
    #[arg(long, short = 'w', global = true, env = "PT_WORKSPACE")]
    workspace: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new workspace.
    Init(commands::workspace::InitArgs),
    /// Send one request (a saved request id or a URL) and watch it live.
    Send(commands::request::SendArgs),
    /// Run every request in a folder (or the whole workspace) with tests.
    Run(commands::run::RunArgs),
    /// Send a request many times concurrently and measure.
    Load(commands::request::LoadArgs),
    /// Start the local server and open the app in your browser.
    Serve(commands::serve::ServeArgs),
    /// Run as an MCP server over stdio, so AI agents can use post-tester.
    Mcp(commands::serve::McpArgs),
    /// Import cURL, Postman, OpenAPI or plugin formats into the workspace.
    Import(commands::workspace::ImportArgs),
    /// Print code that sends a request (cURL, JavaScript, Python, plugins).
    Codegen(commands::request::CodegenArgs),
    /// List and show environments.
    #[command(subcommand)]
    Env(commands::workspace::EnvCommand),
    /// Store secrets in the OS keychain.
    #[command(subcommand)]
    Secret(commands::workspace::SecretCommand),
    /// Create, test and manage plugins.
    #[command(subcommand)]
    Plugin(commands::plugin::PluginCommand),
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("PT_LOG").unwrap_or_else(|_| "warn".into()))
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    let workspace = cli.workspace.as_deref();
    let result = match cli.command {
        Command::Init(args) => commands::workspace::init(args),
        Command::Send(args) => commands::request::send(workspace, args).await,
        Command::Run(args) => commands::run::run(workspace, args).await,
        Command::Load(args) => commands::request::load(workspace, args).await,
        Command::Serve(args) => commands::serve::serve(workspace, args).await,
        Command::Mcp(args) => commands::serve::mcp(workspace, args).await,
        Command::Import(args) => commands::workspace::import(workspace, args).await,
        Command::Codegen(args) => commands::request::codegen(workspace, args).await,
        Command::Env(command) => commands::workspace::env(workspace, command).await,
        Command::Secret(command) => commands::workspace::secret(workspace, command).await,
        Command::Plugin(command) => commands::plugin::plugin(workspace, command).await,
    };
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{} {error:#}", style::red("error:"));
            ExitCode::from(2)
        }
    }
}
