//! `pt run`: run a folder with tests, for people and for CI.

use crate::context::Context;
use crate::printer::{LivePrinter, PrintOptions};
use crate::style;
use anyhow::{Context as _, Result};
use clap::{Args, ValueEnum};
use post_tester_core::events::{Outcome, Source};
use post_tester_core::runner::{RunOptions, RunReport, RunTarget};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Reporter {
    /// Live output and a summary.
    Pretty,
    /// A JSON report on stdout.
    Json,
    /// A JUnit XML report (for CI systems), written to --output.
    Junit,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Folder id to run (default: every request in the workspace).
    #[arg(default_value = "")]
    folder: String,
    /// Run only these request ids, in this order (instead of a folder).
    #[arg(long = "request", short = 'r')]
    requests: Vec<String>,
    #[arg(short = 'e', long)]
    environment: Option<String>,
    /// Stop at the first failure.
    #[arg(long)]
    bail: bool,
    /// Run the whole list this many times.
    #[arg(long, default_value_t = 1)]
    iterations: u32,
    /// Pause between requests, in milliseconds.
    #[arg(long, default_value_t = 0)]
    delay: u64,
    #[arg(long, value_enum, default_value_t = Reporter::Pretty)]
    reporter: Reporter,
    /// Where to write the report (junit, or json instead of stdout).
    #[arg(long, short)]
    output: Option<PathBuf>,
    /// Show request and response headers.
    #[arg(short, long)]
    verbose: bool,
}

pub async fn run(workspace: Option<&Path>, args: RunArgs) -> Result<ExitCode> {
    let ctx = Context::open(workspace, true).await?;
    let target = if args.requests.is_empty() { RunTarget::Folder(args.folder.clone()) } else { RunTarget::Requests(args.requests.clone()) };
    let options = RunOptions {
        source: Source::Cli,
        environment: args.environment.clone(),
        bail: args.bail,
        iterations: args.iterations,
        delay: Duration::from_millis(args.delay),
    };
    let live =
        matches!(args.reporter, Reporter::Pretty) || (args.output.is_some() && matches!(args.reporter, Reporter::Json | Reporter::Junit));
    let printer = live.then(|| LivePrinter::start(ctx.engine.bus(), PrintOptions { verbose: args.verbose }));
    let report = ctx.engine.run(target, options).await?;
    if let Some(printer) = printer {
        printer.finish().await;
    }

    match args.reporter {
        Reporter::Pretty => print_summary(&report),
        Reporter::Json => write_output(args.output.as_deref(), &serde_json::to_string_pretty(&report)?)?,
        Reporter::Junit => {
            let path = args.output.as_deref().context("--reporter junit needs --output <file>, e.g. --output results.xml")?;
            write_output(Some(path), &junit(&report))?;
            print_summary(&report);
        }
    }
    Ok(if report.outcome == Outcome::Ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
}

fn print_summary(report: &RunReport) {
    let total = report.items.len();
    let ok = report.count(Outcome::Ok);
    let failed = report.count(Outcome::Failed);
    let errors = report.count(Outcome::Error);
    println!();
    println!(
        "{}  {} requests · {} · {} · {} · {}",
        style::bold(&report.name),
        total,
        style::green(format!("{ok} passed")),
        if failed > 0 { style::red(format!("{failed} failed")) } else { format!("{failed} failed") },
        if errors > 0 { style::red(format!("{errors} errors")) } else { format!("{errors} errors") },
        style::duration(report.duration_ms),
    );
    println!("  tests: {} total, {} failed", report.tests_total(), report.tests_failed());
}

fn write_output(path: Option<&Path>, content: &str) -> Result<()> {
    match path {
        Some(path) => std::fs::write(path, content).with_context(|| format!("cannot write {}", path.display())),
        None => {
            println!("{content}");
            Ok(())
        }
    }
}

/// A JUnit XML report: one test case per request, failures list the failed tests.
fn junit(report: &RunReport) -> String {
    let escape = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let failures = report.count(Outcome::Failed);
    let errors = report.count(Outcome::Error);
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n  <testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" time=\"{:.3}\">\n",
        escape(&report.name),
        report.items.len(),
        report.duration_ms / 1000.0
    );
    for item in &report.items {
        xml.push_str(&format!(
            "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">\n",
            escape(&item.request_id),
            escape(&item.name),
            item.duration_ms / 1000.0
        ));
        match item.outcome {
            Outcome::Failed => {
                let failed: Vec<String> = item
                    .tests
                    .iter()
                    .filter(|t| !t.passed)
                    .map(|t| format!("{}: {}", t.name, t.detail.as_deref().unwrap_or("failed")))
                    .collect();
                xml.push_str(&format!("      <failure message=\"{}\"/>\n", escape(&failed.join("; "))));
            }
            Outcome::Error | Outcome::Cancelled => {
                xml.push_str(&format!("      <error message=\"{}\"/>\n", escape(item.error.as_deref().unwrap_or("error"))));
            }
            Outcome::Ok => {}
        }
        xml.push_str("    </testcase>\n");
    }
    xml.push_str("  </testsuite>\n</testsuites>\n");
    xml
}
