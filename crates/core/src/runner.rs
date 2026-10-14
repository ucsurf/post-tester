//! Runs many requests in order: a folder, a whole collection, or a hand-picked list.
//!
//! The run is one parent span; each request is a child span, so Live Activity
//! shows the run as a tree with live progress.

use crate::engine::{Engine, Execution, SendOptions};
use crate::error::Result;
use crate::events::{Level, Outcome, Source, Span, SpanId, SpanKind, Summary};
use crate::expr::AssertionResult;
use serde::Serialize;
use std::time::Duration;

/// What to run.
#[derive(Debug, Clone)]
pub enum RunTarget {
    /// A folder id (`""` for every request in the workspace).
    Folder(String),
    /// Specific request ids, in this order.
    Requests(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub source: Source,
    pub environment: Option<String>,
    /// Stop at the first request that fails or errors.
    pub bail: bool,
    /// How many times to run the whole list.
    pub iterations: u32,
    /// Pause between requests.
    pub delay: Duration,
}

impl RunOptions {
    pub fn new(source: Source) -> Self {
        Self { source, environment: None, bail: false, iterations: 1, delay: Duration::ZERO }
    }
}

/// One request's result inside a run.
#[derive(Debug, Clone, Serialize)]
pub struct RunItem {
    pub request_id: String,
    pub name: String,
    pub span_id: SpanId,
    pub outcome: Outcome,
    pub status: Option<u16>,
    pub duration_ms: f64,
    pub tests: Vec<AssertionResult>,
    pub error: Option<String>,
}

/// The result of a run.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub span_id: SpanId,
    pub name: String,
    pub outcome: Outcome,
    pub items: Vec<RunItem>,
    pub duration_ms: f64,
}

impl RunReport {
    pub fn count(&self, outcome: Outcome) -> usize {
        self.items.iter().filter(|i| i.outcome == outcome).count()
    }

    pub fn tests_total(&self) -> usize {
        self.items.iter().map(|i| i.tests.len()).sum()
    }

    pub fn tests_failed(&self) -> usize {
        self.items.iter().flat_map(|i| &i.tests).filter(|t| !t.passed).count()
    }
}

impl Engine {
    /// Runs a folder or list of requests and returns a report.
    pub async fn run(&self, target: RunTarget, options: RunOptions) -> Result<RunReport> {
        let workspace = self.require_workspace()?;
        let (name, ids) = match &target {
            RunTarget::Folder(id) => {
                let tree = workspace.tree()?;
                let folder = tree.find(id).ok_or_else(|| crate::error::Error::NotFound(format!("folder `{id}`")))?;
                (folder.name.clone(), folder.request_ids())
            }
            RunTarget::Requests(ids) => (format!("{} request(s)", ids.len()), ids.clone()),
        };
        let iterations = options.iterations.max(1);
        let total = u32::try_from(ids.len()).unwrap_or(u32::MAX).saturating_mul(iterations);

        let span =
            Span::start(self.bus(), options.source.clone(), None, SpanKind::CollectionRun, format!("Run \"{name}\""), Summary::default());
        let guard = self.track(&span, None);
        span.progress(0, total);
        if ids.is_empty() {
            span.log(Level::Warn, "Nothing to run: the folder has no requests");
        }

        let mut items = Vec::new();
        let mut done = 0;
        'outer: for iteration in 1..=iterations {
            if iterations > 1 {
                span.log(Level::Info, format!("Iteration {iteration} of {iterations}"));
            }
            for id in &ids {
                if guard.token.is_cancelled() {
                    break 'outer;
                }
                let execution = match workspace.read_request(id) {
                    Ok(request) => {
                        let send = SendOptions {
                            source: options.source.clone(),
                            environment: options.environment.clone(),
                            parent: Some(span.clone()),
                            request_id: Some(id.clone()),
                        };
                        let execution = self.send_with_token(&request, send, Some(&guard.token)).await;
                        item(id, &request.name, &execution)
                    }
                    Err(e) => {
                        span.log(Level::Error, format!("{id}: {e}"));
                        RunItem {
                            request_id: id.clone(),
                            name: id.clone(),
                            span_id: span.id(),
                            outcome: Outcome::Error,
                            status: None,
                            duration_ms: 0.0,
                            tests: Vec::new(),
                            error: Some(e.to_string()),
                        }
                    }
                };
                let failed = matches!(execution.outcome, Outcome::Failed | Outcome::Error);
                items.push(execution);
                done += 1;
                span.progress(done, total);
                if failed && options.bail {
                    span.log(Level::Warn, "Stopped after the first failure (bail)");
                    break 'outer;
                }
                if !options.delay.is_zero() {
                    tokio::time::sleep(options.delay).await;
                }
            }
        }

        let outcome = if guard.token.is_cancelled() {
            Outcome::Cancelled
        } else if items.iter().any(|i| matches!(i.outcome, Outcome::Failed | Outcome::Error)) {
            Outcome::Failed
        } else {
            Outcome::Ok
        };
        span.finish(outcome, None, None, None);
        Ok(RunReport { span_id: span.id(), name, outcome, items, duration_ms: crate::events::millis(span.elapsed()) })
    }
}

fn item(id: &str, name: &str, execution: &Execution) -> RunItem {
    let response = execution.response.as_ref();
    RunItem {
        request_id: id.to_string(),
        name: if name.is_empty() { id.to_string() } else { name.to_string() },
        span_id: execution.span_id,
        outcome: execution.outcome,
        status: response.map(|r| r.status),
        duration_ms: response.map_or(0.0, |r| crate::events::millis(r.total)),
        tests: execution.tests.clone(),
        error: execution.error.clone(),
    }
}
