//! Keeps Live Activity across restarts: events are appended to
//! `.post-tester/history.jsonl` and loaded back on start.
//!
//! Sensitive header values (authorization, cookies, API keys) are replaced with
//! `•••` before writing, so credentials never end up in plain text on disk.
//! Full bodies are not stored, only the short previews events already carry.

use crate::events::{Event, EventBus, EventKind, EventSink, Header};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
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
    bus.add_sink(Arc::new(HistoryFile { writer: Mutex::new(BufWriter::new(file)), path }));
    Ok(())
}

/// The last `max` readable events from a history file, oldest first.
pub fn load(path: &Path, max: usize) -> Vec<Event> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mut events: std::collections::VecDeque<Event> = std::collections::VecDeque::with_capacity(max.min(1024));
    // Unreadable lines (e.g. from a crash mid-write or an older version) are skipped.
    for event in BufReader::new(file).lines().map_while(Result::ok).filter_map(|line| serde_json::from_str(&line).ok()) {
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
    writer: Mutex<BufWriter<File>>,
}

impl EventSink for HistoryFile {
    fn record(&self, event: &Event) {
        let Ok(line) = serde_json::to_string(&redact(event)) else { return };
        let mut writer = self.writer.lock().expect("history lock poisoned");
        let written = writeln!(writer, "{line}");
        // Flushing when operations start and end keeps the file current (and survives a crash
        // mid-request) without a disk write per event.
        let flushed = if matches!(event.kind, EventKind::Started { .. } | EventKind::Finished { .. }) { writer.flush() } else { Ok(()) };
        if let Err(e) = written.and(flushed) {
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

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "not json\n{\"half\":\n").unwrap();
        assert!(load(&dir.path().join(FILE_NAME), 10).is_empty());
    }
}
