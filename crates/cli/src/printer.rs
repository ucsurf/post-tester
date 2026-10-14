//! Live terminal output: prints events as they happen, indented by span depth.
//!
//! ```text
//! → POST https://api.example.com/login
//!   DNS 4 ms · Connect 11 ms · TLS 38 ms · Waiting 120 ms · Download 2 ms
//! ← 200 OK  HTTP/2  1.2 KB  182 ms
//!   ✓ status == 200
//!   = token = eyJhbGciOi…
//! ```

use crate::style;
use post_tester_core::events::{Direction, Event, EventBus, EventKind, Level, Outcome, Phase, SpanId, SpanKind};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

#[derive(Debug, Clone, Copy, Default)]
pub struct PrintOptions {
    /// Show request and response headers.
    pub verbose: bool,
}

/// Prints events from a bus until [`LivePrinter::finish`] is called.
#[derive(Debug)]
pub struct LivePrinter {
    bus: EventBus,
    printed_up_to: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}

impl LivePrinter {
    pub fn start(bus: &EventBus, options: PrintOptions) -> Self {
        let mut receiver = bus.subscribe();
        let printed_up_to = Arc::new(AtomicU64::new(bus.recent().last().map_or(0, |e| e.seq)));
        let progress = printed_up_to.clone();
        let task = tokio::spawn(async move {
            let mut state = State::default();
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        state.print(&event, options);
                        progress.store(event.seq, Ordering::Release);
                    }
                    Err(RecvError::Lagged(skipped)) => eprintln!("{}", style::dimmed(format!("… {skipped} events skipped"))),
                    Err(RecvError::Closed) => return,
                }
            }
        });
        Self { bus: bus.clone(), printed_up_to, task }
    }

    /// Waits until every event emitted so far has been printed, then stops.
    pub async fn finish(self) {
        let target = self.bus.recent().last().map_or(0, |e| e.seq);
        let wait = async {
            while self.printed_up_to.load(Ordering::Acquire) < target {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(2), wait).await;
        self.task.abort();
    }
}

#[derive(Default)]
struct State {
    depth: HashMap<SpanId, usize>,
    kinds: HashMap<SpanId, SpanKind>,
    phases: HashMap<SpanId, Vec<(Phase, f64)>>,
    /// Spans whose resolved URL was already printed.
    url_shown: HashSet<SpanId>,
    /// Spans whose error was already printed as a log line.
    error_shown: HashSet<SpanId>,
}

impl State {
    fn print(&mut self, event: &Event, options: PrintOptions) {
        let depth = match event.parent_id.and_then(|p| self.depth.get(&p)) {
            Some(parent) => parent + 1,
            None => 0,
        };
        self.depth.entry(event.span_id).or_insert(depth);
        let pad = "  ".repeat(depth);
        let id = event.span_id;
        match &event.kind {
            EventKind::Started { kind, title, summary } => {
                self.kinds.insert(id, *kind);
                let line = match kind {
                    SpanKind::CollectionRun => style::bold(format!("▶ {title}")),
                    SpanKind::System => return,
                    _ => format!("{} {}", style::cyan("→"), style::bold(title)),
                };
                // The resolved URL is printed when the request is sent, unless the title already is the URL.
                if summary.method.as_ref().is_some_and(|m| title.starts_with(m.as_str()) && title.contains("://") && !title.contains("{{")) {
                    self.url_shown.insert(id);
                }
                println!("{pad}{line}");
            }
            EventKind::Phase { phase, duration_ms } => self.phases.entry(id).or_default().push((*phase, *duration_ms)),
            EventKind::RequestSent { method, url, headers, .. } => {
                if self.url_shown.insert(id) {
                    println!("{pad}  {}", style::dimmed(format!("{method} {url}")));
                }
                if options.verbose {
                    for h in headers {
                        println!("{pad}  {}", style::dimmed(format!("> {}: {}", h.name, h.value)));
                    }
                }
            }
            EventKind::ResponseReceived { status, status_text, http_version, headers, body } => {
                if let Some(phases) = self.phases.remove(&id) {
                    let parts: Vec<String> = phases.iter().map(|(p, ms)| format!("{} {}", phase_name(*p), style::duration(*ms))).collect();
                    println!("{pad}  {}", style::dimmed(parts.join(" · ")));
                }
                println!("{pad}{} {}  {}  {}", style::cyan("←"), style::status(*status, status_text), style::dimmed(http_version), style::size(body.size));
                if options.verbose {
                    for h in headers {
                        println!("{pad}  {}", style::dimmed(format!("< {}: {}", h.name, h.value)));
                    }
                }
            }
            EventKind::Message { direction, label, payload } => {
                let arrow = if *direction == Direction::Sent { "⇡" } else { "⇣" };
                let label = label.as_deref().unwrap_or("message");
                println!("{pad}  {} {}: {}", style::magenta(arrow), label, one_line(&payload.preview, 160));
            }
            EventKind::Log { level, text } => {
                let marker = match level {
                    Level::Error => {
                        self.error_shown.insert(id);
                        style::red("✗")
                    }
                    Level::Warn => style::yellow("!"),
                    Level::Info => style::blue("i"),
                    Level::Debug => return,
                };
                println!("{pad}  {marker} {text}");
            }
            EventKind::TestResult { name, passed, detail } => {
                if *passed {
                    println!("{pad}  {} {name}", style::green("✓"));
                } else {
                    let detail = detail.as_deref().map(|d| format!(" — {d}")).unwrap_or_default();
                    println!("{pad}  {} {name}{}", style::red("✗"), style::red(detail));
                }
            }
            EventKind::VariableSet { name, value } => println!("{pad}  {} {name} = {}", style::dimmed("="), one_line(value, 60)),
            EventKind::Progress { .. } | EventKind::Connected { .. } => {}
            EventKind::Finished { outcome, duration_ms, error, .. } => {
                let kind = self.kinds.remove(&id);
                if kind == Some(SpanKind::System) {
                    return;
                }
                match (outcome, error) {
                    (Outcome::Error, Some(error)) if !self.error_shown.remove(&id) => {
                        println!("{pad}{} {}", style::red("✗"), style::red(error));
                    }
                    (Outcome::Cancelled, _) => println!("{pad}{}", style::yellow("■ cancelled")),
                    _ if kind == Some(SpanKind::Request) => println!("{pad}  {}", style::dimmed(style::duration(*duration_ms))),
                    _ => {}
                }
            }
        }
    }
}

fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Dns => "DNS",
        Phase::Connect => "Connect",
        Phase::Tls => "TLS",
        Phase::Waiting => "Waiting",
        Phase::Download => "Download",
    }
}

/// Shortens text to one line of at most `max` characters.
pub fn one_line(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max { format!("{}…", flat.chars().take(max).collect::<String>()) } else { flat }
}
