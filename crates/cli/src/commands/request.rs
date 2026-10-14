//! `pt send`, `pt load` and `pt codegen`.

use crate::context::Context;
use crate::printer::{LivePrinter, PrintOptions};
use crate::style;
use anyhow::{Result, bail};
use clap::Args;
use post_tester_core::events::{Outcome, Source};
use post_tester_core::loadtest::LoadOptions;
use post_tester_core::model::{Body, KeyValue, Method, Request};
use post_tester_core::{SendOptions, codegen};
use std::path::Path;
use std::process::ExitCode;

/// A request given on the command line: a saved id, or a URL with flags.
#[derive(Debug, Args)]
pub struct RequestArgs {
    /// Saved request id (e.g. `auth/login`) or a URL.
    target: String,
    /// HTTP method for a URL (default GET, or POST with --data).
    #[arg(short = 'X', long)]
    method: Option<String>,
    /// Header for a URL, e.g. -H 'Accept: application/json'. Repeatable.
    #[arg(short = 'H', long = "header")]
    headers: Vec<String>,
    /// Body for a URL. JSON bodies get `Content-Type: application/json`.
    #[arg(short = 'd', long)]
    data: Option<String>,
    /// Environment to use (default: the workspace default).
    #[arg(short = 'e', long)]
    environment: Option<String>,
}

impl RequestArgs {
    /// Returns the request and, for saved requests, its id.
    fn resolve(&self, ctx: &Context) -> Result<(Request, Option<String>)> {
        if let Some(workspace) = ctx.engine.workspace()
            && workspace.request_exists(&self.target)
        {
            return Ok((workspace.read_request(&self.target)?, Some(self.target.clone())));
        }
        let looks_like_url = self.target.contains("://") || self.target.contains('.') || self.target.starts_with("localhost");
        if !looks_like_url {
            bail!("`{}` is neither a saved request nor a URL. List saved requests with the app or `ls collections/`", self.target);
        }
        let mut request = Request { url: self.target.clone(), ..Default::default() };
        for header in &self.headers {
            let Some((name, value)) = header.split_once(':') else { bail!("header `{header}` must look like 'Name: value'") };
            request.headers.push(KeyValue::new(name.trim(), value.trim()));
        }
        if let Some(data) = &self.data {
            request.body = if serde_json::from_str::<serde_json::Value>(data).is_ok() {
                Body::Json { content: data.clone() }
            } else {
                Body::Text { content: data.clone(), content_type: "text/plain".into() }
            };
        }
        let default_method = if self.data.is_some() { "POST" } else { "GET" };
        request.method = Method::from(self.method.as_deref().unwrap_or(default_method));
        Ok((request, None))
    }
}

#[derive(Debug, Args)]
pub struct SendArgs {
    #[command(flatten)]
    request: RequestArgs,
    /// Show request and response headers.
    #[arg(short, long)]
    verbose: bool,
    /// Print a JSON result instead of live output.
    #[arg(long)]
    json: bool,
    /// Don't print the response body.
    #[arg(long)]
    no_body: bool,
}

pub async fn send(workspace: Option<&Path>, args: SendArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    let (request, request_id) = args.request.resolve(&ctx)?;
    let printer = (!args.json).then(|| LivePrinter::start(ctx.engine.bus(), PrintOptions { verbose: args.verbose }));
    let mut options = SendOptions::new(Source::Cli).environment(args.request.environment.clone());
    options.request_id = request_id;
    let execution = ctx.engine.send(&request, options).await;
    if let Some(printer) = printer {
        printer.finish().await;
    }

    if args.json {
        let response = execution.response.as_ref();
        let output = serde_json::json!({
            "outcome": execution.outcome,
            "error": execution.error,
            "status": response.map(|r| r.status),
            "headers": response.map(|r| &r.headers),
            "body": response.map(|r| String::from_utf8_lossy(&r.body)),
            "time_ms": response.map(|r| post_tester_core::events::millis(r.total)),
            "tests": execution.tests,
            "extracted": execution.extracted,
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if let Some(response) = &execution.response
        && !args.no_body
        && !response.body.is_empty()
    {
        println!();
        println!("{}", pretty_body(&response.body));
    }
    Ok(if execution.outcome == Outcome::Ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

/// Pretty-prints JSON bodies; other bodies are printed as text.
fn pretty_body(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| String::from_utf8_lossy(body).into_owned())
}

#[derive(Debug, Args)]
pub struct LoadArgs {
    #[command(flatten)]
    request: RequestArgs,
    /// Requests in flight at the same time.
    #[arg(short, long, default_value_t = 10)]
    concurrency: u32,
    /// Total number of requests.
    #[arg(short = 'n', long, default_value_t = 100)]
    requests: u32,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
}

pub async fn load(workspace: Option<&Path>, args: LoadArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    let (request, request_id) = args.request.resolve(&ctx)?;
    let printer = (!args.json).then(|| LivePrinter::start(ctx.engine.bus(), PrintOptions::default()));
    let options = LoadOptions {
        source: Source::Cli,
        environment: args.request.environment.clone(),
        concurrency: args.concurrency,
        requests: args.requests,
    };
    let report = ctx.engine.load_test(&request, request_id, options).await?;
    if let Some(printer) = printer {
        printer.finish().await;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!();
        println!("  requests     {} ({} failed)", report.requests, report.failed);
        println!("  throughput   {:.1} req/s", report.requests_per_second);
        println!(
            "  latency      min {} · p50 {} · p90 {} · p99 {} · max {}",
            style::duration(report.min_ms),
            style::duration(report.p50_ms),
            style::duration(report.p90_ms),
            style::duration(report.p99_ms),
            style::duration(report.max_ms)
        );
        let codes: Vec<String> = report.status_codes.iter().map(|(code, n)| format!("{code}×{n}")).collect();
        println!("  status       {}", codes.join("  "));
    }
    Ok(if report.failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

#[derive(Debug, Args)]
pub struct CodegenArgs {
    #[command(flatten)]
    request: RequestArgs,
    /// Generator: curl, javascript-fetch, python-requests, or a plugin's `plugin/name`.
    #[arg(short = 't', long = "target", default_value = "curl")]
    generator: String,
}

pub async fn codegen(workspace: Option<&Path>, args: CodegenArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, false).await?;
    let (request, request_id) = args.request.resolve(&ctx)?;
    let prepared = ctx.engine.preview(&request, request_id.as_deref(), args.request.environment.clone()).await?;
    let code = match codegen::find(&args.generator) {
        Some(generator) => generator.generate(&prepared),
        None if args.generator.contains('/') => ctx.plugins.codegen(&args.generator, &prepared).await?,
        None => {
            let mut available: Vec<String> = codegen::GENERATORS.iter().map(|g| g.id.to_string()).collect();
            available.extend(ctx.plugins.codegen_targets().into_iter().map(|(id, _)| id));
            bail!("unknown target `{}`. Available: {}", args.generator, available.join(", "));
        }
    };
    println!("{code}");
    Ok(ExitCode::SUCCESS)
}
