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
    let failures = report.count(Outcome::Failed);
    let errors = report.count(Outcome::Error) + report.count(Outcome::Cancelled);
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n  <testsuite name=\"{}\" tests=\"{}\" failures=\"{failures}\" errors=\"{errors}\" time=\"{:.3}\">\n",
        xml_escape(&report.name),
        report.items.len(),
        report.duration_ms / 1000.0
    );
    for item in &report.items {
        xml.push_str(&format!(
            "    <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\">\n",
            xml_escape(&item.request_id),
            xml_escape(&item.name),
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
                let summary = failed.first().cloned().unwrap_or_else(|| "failed".into());
                xml.push_str(&format!(
                    "      <failure message=\"{}\">{}</failure>\n",
                    xml_escape(&summary),
                    xml_escape(&failed.join("\n"))
                ));
            }
            Outcome::Error | Outcome::Cancelled => {
                let fallback = if item.outcome == Outcome::Cancelled { "cancelled" } else { "error" };
                let error = item.error.as_deref().unwrap_or(fallback);
                xml.push_str(&format!("      <error message=\"{}\">{}</error>\n", xml_escape(error), xml_escape(error)));
            }
            Outcome::Ok => {}
        }
        xml.push_str("    </testcase>\n");
    }
    xml.push_str("  </testsuite>\n</testsuites>\n");
    xml
}

/// Escapes text for XML attributes and elements. Line breaks and tabs become
/// character references (so attributes keep them), and characters XML 1.0
/// does not allow at all (other control characters) become `\u{fffd}`.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use post_tester_core::events::SpanId;
    use post_tester_core::expr::AssertionResult;
    use post_tester_core::runner::RunItem;

    fn item(id: &str, outcome: Outcome, tests: Vec<AssertionResult>, error: Option<&str>) -> RunItem {
        RunItem {
            request_id: id.into(),
            name: id.into(),
            span_id: SpanId::new(),
            outcome,
            status: None,
            duration_ms: 12.0,
            tests,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn junit_escapes_everything() {
        let failed =
            AssertionResult { name: "json($.a) == \"<b>\"".into(), passed: false, detail: Some("got 'x' & \u{1b}[31m\nnext".into()) };
        let report = RunReport {
            span_id: SpanId::new(),
            name: "R&D <api>".into(),
            outcome: Outcome::Failed,
            items: vec![
                item("a/ok", Outcome::Ok, vec![], None),
                item("a/failed", Outcome::Failed, vec![failed], None),
                item("a/error", Outcome::Error, vec![], Some("could not connect to \"x\"")),
                item("a/cancelled", Outcome::Cancelled, vec![], None),
            ],
            duration_ms: 1234.0,
        };
        let xml = junit(&report);
        assert!(xml.contains("name=\"R&amp;D &lt;api&gt;\""), "{xml}");
        assert!(xml.contains("failures=\"1\" errors=\"2\""), "{xml}");
        assert!(xml.contains("&quot;&lt;b&gt;&quot;"), "{xml}");
        assert!(xml.contains("&apos;x&apos; &amp; \u{fffd}[31m&#10;next"), "{xml}");
        assert!(!xml.chars().any(|c| c.is_control() && c != '\n'), "only line breaks between elements: {xml:?}");
        // Every opened element is closed.
        for (open, close) in [
            ("<testsuites>", "</testsuites>"),
            ("<testsuite ", "</testsuite>"),
            ("<testcase ", "</testcase>"),
            ("<failure ", "</failure>"),
            ("<error ", "</error>"),
        ] {
            assert_eq!(xml.matches(open).count(), xml.matches(close).count(), "{open}");
        }
    }
}
