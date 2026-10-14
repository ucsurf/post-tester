//! Keeps Live Activity across restarts and across processes.
//!
//! Every post-tester process (the app, `pt send`, `pt run`, `pt mcp`…) appends
//! its events to `.post-tester/history.jsonl`, tagged with its process id.
//! - On start, recent events are loaded back ([`attach`]).
//! - The app follows the file ([`follow`]), so work started in a terminal or by
//!   an AI agent over stdio appears in Live Activity as it happens.
//!
//! Sensitive header values (authorization, cookies, API keys) are replaced with
//! `•••` before writing, so credentials never end up in plain text on disk.
//! Full bodies are not stored, only the short previews events already carry.

use crate::events::{Event, EventBus, EventKind, EventSink, Header};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const FILE_NAME: &str = "history.jsonl";
/// When the file grows past this size, it is rotated to `history.1.jsonl`.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// How many past events are loaded on start.
const LOAD_EVENTS: usize = 10_000;

/// Header names whose values are never written to disk.
const SENSITIVE_HEADERS: &[&str] =
    &["authorization", "proxy-authorization", "cookie", "set-cookie", "x-api-key", "api-key", "x-auth-token", "x-access-token"];

/// Loads previous events into `bus` and records every new event to `dir`.
pub fn attach(bus: &EventBus, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    rotate_if_large(&path)?;
    let mut events = load(&path, LOAD_EVENTS);
    close_unfinished(&mut events);
    bus.restore(events);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    bus.add_sink(Arc::new(HistoryFile { file: Mutex::new(file), path }));
    Ok(())
}

/// One line of the history file: an event plus the process that wrote it.
#[derive(Serialize)]
struct LineOut<'a> {
    origin: u32,
    #[serde(flatten)]
    event: &'a Event,
}

#[derive(Deserialize)]
struct LineIn {
    #[serde(default)]
    origin: u32,
    #[serde(flatten)]
    event: Event,
}

/// How often the file is checked for events from other processes.
const FOLLOW_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// Relays events that other post-tester processes append to the history file
/// in `dir` onto `bus`, until the returned task is aborted.
pub fn follow(bus: &EventBus, dir: &Path) -> tokio::task::JoinHandle<()> {
    tokio::spawn(follower(bus, dir))
}

/// The work behind [`follow`], for callers that run it on their own executor.
pub fn follower(bus: &EventBus, dir: &Path) -> impl std::future::Future<Output = ()> + Send + 'static {
    let bus = bus.clone();
    let path = dir.join(FILE_NAME);
    let me = std::process::id();
    // Taken now, not inside the task, so nothing written in between is missed.
    let mut offset = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    async move {
        let mut partial = String::new();
        loop {
            tokio::time::sleep(FOLLOW_INTERVAL).await;
            let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if len < offset {
                // The file was rotated: start from the beginning of the new one.
                offset = 0;
                partial.clear();
            }
            if len == offset {
                continue;
            }
            let Ok(chunk) = read_from(&path, offset) else { continue };
            offset += chunk.len() as u64;
            partial.push_str(&String::from_utf8_lossy(&chunk));
            // Only complete lines are parsed; the rest waits for the next round.
            let complete = partial.rfind('\n').map_or(0, |i| i + 1);
            for line in partial[..complete].lines() {
                if let Ok(LineIn { origin, event }) = serde_json::from_str::<LineIn>(line)
                    && origin != me
                {
                    bus.relay(event);
                }
            }
            partial.drain(..complete);
        }
    }
}

fn read_from(path: &Path, offset: u64) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// The last `max` readable events from a history file, oldest first.
pub fn load(path: &Path, max: usize) -> Vec<Event> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mut events: std::collections::VecDeque<Event> = std::collections::VecDeque::with_capacity(max.min(1024));
    // Unreadable lines (e.g. from a crash mid-write or an older version) are skipped.
    for event in BufReader::new(file).lines().map_while(Result::ok).filter_map(|line| serde_json::from_str::<LineIn>(&line).ok()) {
        let event = event.event;
        if events.len() == max {
            events.pop_front();
        }
        events.push_back(event);
    }
    events.into()
}

/// Operations still running when post-tester last stopped are marked cancelled,
/// so they don't look like they are running forever.
fn close_unfinished(events: &mut Vec<Event>) {
    let mut open: Vec<&Event> = Vec::new();
    for event in events.iter() {
        match event.kind {
            EventKind::Started { .. } => open.push(event),
            EventKind::Finished { .. } => open.retain(|e| e.span_id != event.span_id),
            _ => {}
        }
    }
    let mut seq = events.iter().map(|e| e.seq).max().unwrap_or(0);
    let closing: Vec<Event> = open
        .into_iter()
        .map(|started| {
            seq += 1;
            Event {
                seq,
                span_id: started.span_id,
                parent_id: started.parent_id,
                source: started.source.clone(),
                at: chrono::Utc::now(),
                kind: EventKind::Finished {
                    outcome: crate::events::Outcome::Cancelled,
                    duration_ms: 0.0,
                    status: None,
                    size: None,
                    error: Some("post-tester stopped before this finished".into()),
                },
            }
        })
        .collect();
    events.extend(closing);
}

fn rotate_if_large(path: &Path) -> std::io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
        std::fs::rename(path, path.with_extension("1.jsonl"))?;
    }
    Ok(())
}

struct HistoryFile {
    path: PathBuf,
    file: Mutex<File>,
}

impl EventSink for HistoryFile {
    fn record(&self, event: &Event) {
        let redacted = redact(event);
        let Ok(mut line) = serde_json::to_string(&LineOut { origin: std::process::id(), event: &redacted }) else { return };
        line.push('\n');
        // One write per line on an append-only file, so lines from several
        // processes never interleave, and other processes see events right away.
        let mut file = self.file.lock().expect("history lock poisoned");
        if let Err(e) = file.write_all(line.as_bytes()) {
            tracing::warn!(path = %self.path.display(), "could not write history: {e}");
        }
    }
}

/// A copy of the event with sensitive header values masked.
fn redact(event: &Event) -> std::borrow::Cow<'_, Event> {
    let mask = |headers: &[Header]| -> Vec<Header> {
        headers
            .iter()
            .map(|h| {
                let sensitive = SENSITIVE_HEADERS.iter().any(|s| h.name.eq_ignore_ascii_case(s));
                Header { name: h.name.clone(), value: if sensitive { "•••".into() } else { h.value.clone() } }
            })
            .collect()
    };
    match &event.kind {
        EventKind::RequestSent { method, url, http_version, headers, body } => std::borrow::Cow::Owned(Event {
            kind: EventKind::RequestSent {
                method: method.clone(),
                url: url.clone(),
                http_version: http_version.clone(),
                headers: mask(headers),
                body: body.clone(),
            },
            ..event.clone()
        }),
        EventKind::ResponseReceived { status, status_text, http_version, headers, body } => std::borrow::Cow::Owned(Event {
            kind: EventKind::ResponseReceived {
                status: *status,
                status_text: status_text.clone(),
                http_version: http_version.clone(),
                headers: mask(headers),
                body: body.clone(),
            },
            ..event.clone()
        }),
        _ => std::borrow::Cow::Borrowed(event),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Payload, Source, Span, SpanKind, Summary};

    #[test]
    fn events_survive_a_restart_with_secrets_masked() {
        let dir = tempfile::tempdir().unwrap();
        let first = EventBus::default();
        attach(&first, dir.path()).unwrap();
        let span = Span::start(&first, Source::User, None, SpanKind::Request, "login", Summary::default());
        span.emit(EventKind::RequestSent {
            method: "GET".into(),
            url: "https://x.dev".into(),
            http_version: "HTTP/1.1".into(),
            headers: vec![Header { name: "Authorization".into(), value: "Bearer s3cret".into() }],
            body: Payload::default(),
        });
        span.finish_ok(Some(200), None);
        let last_seq = first.recent().last().unwrap().seq;

        let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        assert!(!text.contains("s3cret"), "credentials must not be written to disk");

        let second = EventBus::default();
        attach(&second, dir.path()).unwrap();
        assert_eq!(second.recent().len(), 3);
        // New events continue the sequence instead of restarting at 1.
        let span = Span::start(&second, Source::User, None, SpanKind::Request, "again", Summary::default());
        span.finish_ok(None, None);
        assert!(second.recent().last().unwrap().seq > last_seq);
    }

    #[test]
    fn unfinished_operations_are_closed_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let first = EventBus::default();
        attach(&first, dir.path()).unwrap();
        let span = Span::start(&first, Source::User, None, SpanKind::Request, "interrupted", Summary::default());
        std::mem::forget(span); // simulates the process stopping mid-request

        let second = EventBus::default();
        attach(&second, dir.path()).unwrap();
        let last = second.recent().last().unwrap().clone();
        assert!(matches!(last.kind, EventKind::Finished { outcome: crate::events::Outcome::Cancelled, .. }));
    }

    #[tokio::test]
    async fn events_from_other_processes_are_relayed_live() {
        let dir = tempfile::tempdir().unwrap();
        let app = EventBus::default();
        attach(&app, dir.path()).unwrap();
        let _follower = follow(&app, dir.path());
        let mut live = app.subscribe();

        // Simulate another process appending an event.
        let other = Event {
            seq: 7,
            span_id: crate::events::SpanId::new(),
            parent_id: None,
            source: Source::Cli,
            at: chrono::Utc::now(),
            kind: EventKind::Log { level: crate::events::Level::Info, text: "from a terminal".into() },
        };
        let line = serde_json::to_string(&LineOut { origin: std::process::id() + 1, event: &other }).unwrap();
        let mut file = OpenOptions::new().append(true).open(dir.path().join(FILE_NAME)).unwrap();
        writeln!(file, "{line}").unwrap();

        let relayed = tokio::time::timeout(std::time::Duration::from_secs(3), live.recv()).await.unwrap().unwrap();
        assert_eq!(relayed.span_id, other.span_id);
        assert_eq!(relayed.source, Source::Cli);

        // Our own events are not relayed back to us.
        let span = Span::start(&app, Source::User, None, SpanKind::Request, "mine", Summary::default());
        span.finish_ok(None, None);
        tokio::time::sleep(FOLLOW_INTERVAL * 3).await;
        let mine = app.recent().iter().filter(|e| e.span_id == span.id()).count();
        assert_eq!(mine, 2, "started + finished, without duplicates");
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "not json\n{\"half\":\n").unwrap();
        assert!(load(&dir.path().join(FILE_NAME), 10).is_empty());
    }
}
