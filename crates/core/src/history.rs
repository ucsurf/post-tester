//! Keeps Live Activity across restarts and across processes.
//!
//! Every post-tester process (the app, `pt send`, `pt run`, `pt mcp`…) appends
//! its events to `.post-tester/history.jsonl`, tagged with its process id.
//! - On start, recent events are loaded back ([`attach`]).
//! - The app follows the file ([`follow`]), so work started in a terminal or by
//!   an AI agent over stdio appears in Live Activity as it happens.
//!
//! Sensitive values (authorization and cookie headers, API keys and tokens in
//! headers, query parameters or extracted variables, URL passwords) are
//! replaced with `•••` before writing, so credentials never end up in plain
//! text on disk. Full bodies are not stored, only the short previews events
//! already carry.
//!
//! Lines are written by a background thread, so emitting an event never waits
//! for the disk. See [`HistoryFile`] for the guarantees this keeps.

use crate::events::{Event, EventBus, EventKind, EventSink, Header};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};

const FILE_NAME: &str = "history.jsonl";
/// When the file grows past this size, it is rotated to `history.1.jsonl`.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// How many past events are loaded on start.
const LOAD_EVENTS: usize = 10_000;

/// Header names whose values are never written to disk.
const SENSITIVE_HEADERS: &[&str] =
    &["authorization", "proxy-authorization", "cookie", "set-cookie", "x-api-key", "api-key", "x-auth-token", "x-access-token"];

/// Query parameter and variable names (any case) treated as credentials.
const SENSITIVE_NAMES: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "api_key",
    "apikey",
    "key",
    "secret",
    "client_secret",
    "password",
    "passwd",
    "pwd",
    "signature",
    "sig",
    "auth",
    "session",
    "code",
];
/// Names containing one of these (any case) are treated as credentials too,
/// e.g. `accessToken`, `X-Signature`, `sessionId`.
const SENSITIVE_PARTS: &[&str] =
    &["token", "secret", "password", "passwd", "apikey", "api_key", "api-key", "signature", "credential", "session"];

const MASK: &str = "•••";

/// Most events queued for the writer before emitters wait for it (backpressure
/// instead of unbounded memory when the disk is very slow).
const QUEUE_LEN: usize = 16_384;
/// Largest batch written at once. Bursts are joined into one write.
const MAX_BATCH_BYTES: usize = 1024 * 1024;

/// Loads previous events into `bus` and records every new event to `dir`.
pub fn attach(bus: &EventBus, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    rotate_if_large(&path)?;
    let mut events = load(&path, LOAD_EVENTS);
    close_unfinished(&mut events);
    bus.restore(events);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    bus.add_sink(Arc::new(HistoryFile::start(file, path)?));
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
    let mut offset = file_len(&path);
    async move {
        // Bytes, not text: a read may end in the middle of a UTF-8 character
        // (e.g. in `•••`), which must not be decoded until its line is complete.
        let mut partial: Vec<u8> = Vec::new();
        loop {
            tokio::time::sleep(FOLLOW_INTERVAL).await;
            let len = file_len(&path);
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
            partial.extend_from_slice(&chunk);
            // Only complete lines are parsed; the rest waits for the next round.
            let complete = partial.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            for line in partial[..complete].split(|&b| b == b'\n') {
                if let Ok(LineIn { origin, event }) = serde_json::from_slice::<LineIn>(line)
                    && origin != me
                {
                    bus.relay(event);
                }
            }
            partial.drain(..complete);
        }
    }
}

/// The file's size, or 0 if it does not exist (yet).
fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map_or(0, |m| m.len())
}

fn read_from(path: &Path, offset: u64) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// The last `max` readable events from a history file, oldest first.
///
/// Only the last `max` lines are parsed: the file may hold far more events
/// than are loaded, and parsing JSON is the expensive part. Unreadable lines
/// (e.g. from a crash mid-write or an older version) are skipped.
pub fn load(path: &Path, max: usize) -> Vec<Event> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mut lines: VecDeque<String> = VecDeque::with_capacity(max.min(1024));
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if lines.len() == max {
            lines.pop_front();
        }
        lines.push_back(line);
    }
    lines.iter().filter_map(|line| serde_json::from_str::<LineIn>(line).ok()).map(|line| line.event).collect()
}

/// Operations still running when post-tester last stopped are marked cancelled,
/// so they don't look like they are running forever.
fn close_unfinished(events: &mut Vec<Event>) {
    // Span id → (position of its `Started` event, the event), in start order.
    let mut open: HashMap<crate::events::SpanId, (usize, &Event)> = HashMap::new();
    for (index, event) in events.iter().enumerate() {
        match event.kind {
            EventKind::Started { .. } => {
                open.insert(event.span_id, (index, event));
            }
            EventKind::Finished { .. } => {
                open.remove(&event.span_id);
            }
            _ => {}
        }
    }
    let mut open: Vec<(usize, &Event)> = open.into_values().collect();
    open.sort_unstable_by_key(|(index, _)| *index);
    let mut seq = events.iter().map(|e| e.seq).max().unwrap_or(0);
    let closing: Vec<Event> = open
        .into_iter()
        .map(|(_, started)| started)
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

/// The history sink. Events are serialized and redacted on the emitting
/// thread (cheap, no I/O), then a background thread writes them:
///
/// - **No interleaving between processes**: the file is opened in append
///   mode and each write is one batch of complete lines, so lines written by
///   several processes never mix.
/// - **Prompt**: the writer wakes as soon as a line is queued, so other
///   processes see events within their next poll ([`FOLLOW_INTERVAL`]).
/// - **Nothing lost on shutdown**: dropping the sink (when the last
///   [`EventBus`] clone goes away) writes everything queued before returning,
///   and [`EventBus::flush`] waits for the writer at any time.
struct HistoryFile {
    commands: Option<SyncSender<Command>>,
    writer: Option<std::thread::JoinHandle<()>>,
}

enum Command {
    Line(String),
    /// Answers once everything queued before it is written.
    Flush(SyncSender<()>),
}

impl HistoryFile {
    fn start(file: File, path: PathBuf) -> std::io::Result<Self> {
        let (commands, receiver) = std::sync::mpsc::sync_channel(QUEUE_LEN);
        let writer = std::thread::Builder::new().name("post-tester-history".into()).spawn(move || write_lines(file, &path, &receiver))?;
        Ok(Self { commands: Some(commands), writer: Some(writer) })
    }

    fn send(&self, command: Command) {
        let Some(commands) = &self.commands else { return };
        match commands.try_send(command) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            // The disk is not keeping up: wait rather than drop events.
            Err(TrySendError::Full(command)) => {
                let _ = commands.send(command);
            }
        }
    }
}

impl std::fmt::Debug for HistoryFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryFile").finish_non_exhaustive()
    }
}

impl EventSink for HistoryFile {
    fn record(&self, event: &Event) {
        let redacted = redact(event);
        let Ok(mut line) = serde_json::to_string(&LineOut { origin: std::process::id(), event: &redacted }) else { return };
        line.push('\n');
        self.send(Command::Line(line));
    }

    fn flush(&self) {
        let (done, wait) = std::sync::mpsc::sync_channel(1);
        self.send(Command::Flush(done));
        // An error means the writer has stopped, so there is nothing to wait for.
        let _ = wait.recv();
    }
}

impl Drop for HistoryFile {
    fn drop(&mut self) {
        // Closing the queue lets the writer finish what is queued, then stop.
        drop(self.commands.take());
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

/// The writer thread: joins whatever is queued into one write, until the queue closes.
fn write_lines(mut file: File, path: &Path, commands: &Receiver<Command>) {
    let mut batch: Vec<u8> = Vec::new();
    let mut waiting: Vec<SyncSender<()>> = Vec::new();
    let mut failing = false;
    while let Ok(first) = commands.recv() {
        let mut next = Some(first);
        while let Some(command) = next {
            match command {
                Command::Line(line) => batch.extend_from_slice(line.as_bytes()),
                Command::Flush(done) => waiting.push(done),
            }
            next = if batch.len() < MAX_BATCH_BYTES { commands.try_recv().ok() } else { None };
        }
        if !batch.is_empty() {
            match file.write_all(&batch) {
                Ok(()) => failing = false,
                // Warn once per failure streak, not once per event.
                Err(e) if !failing => {
                    failing = true;
                    tracing::warn!(path = %path.display(), "could not write history: {e}");
                }
                Err(_) => {}
            }
            batch.clear();
        }
        for done in waiting.drain(..) {
            let _ = done.send(());
        }
    }
}

/// A copy of the event with credentials masked.
fn redact(event: &Event) -> Cow<'_, Event> {
    let mask = |headers: &[Header]| -> Vec<Header> {
        headers
            .iter()
            .map(|h| {
                let sensitive = SENSITIVE_HEADERS.iter().any(|s| h.name.eq_ignore_ascii_case(s));
                Header { name: h.name.clone(), value: if sensitive { MASK.into() } else { h.value.clone() } }
            })
            .collect()
    };
    let kind = match &event.kind {
        EventKind::RequestSent { method, url, http_version, headers, body } => EventKind::RequestSent {
            method: method.clone(),
            url: redact_url(url).into_owned(),
            http_version: http_version.clone(),
            headers: mask(headers),
            body: body.clone(),
        },
        EventKind::ResponseReceived { status, status_text, http_version, headers, body } => EventKind::ResponseReceived {
            status: *status,
            status_text: status_text.clone(),
            http_version: http_version.clone(),
            headers: mask(headers),
            body: body.clone(),
        },
        EventKind::VariableSet { name, .. } if is_sensitive_name(name) => EventKind::VariableSet { name: name.clone(), value: MASK.into() },
        EventKind::Started { kind, title, summary } => {
            let (title, url) = (redact_text(title), summary.url.as_deref().map(redact_url));
            if matches!(title, Cow::Borrowed(_)) && !matches!(url, Some(Cow::Owned(_))) {
                return Cow::Borrowed(event);
            }
            let summary = crate::events::Summary { url: url.map(Cow::into_owned), ..summary.clone() };
            EventKind::Started { kind: *kind, title: title.into_owned(), summary }
        }
        _ => return Cow::Borrowed(event),
    };
    Cow::Owned(Event { kind, ..event.clone() })
}

/// True for names like `api_key`, `accessToken` or `X-Signature`.
fn is_sensitive_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SENSITIVE_NAMES.contains(&name.as_str()) || SENSITIVE_PARTS.iter().any(|part| name.contains(part))
}

/// `text` with every URL in it redacted (titles such as `GET https://…`).
fn redact_text(text: &str) -> Cow<'_, str> {
    if !text.contains("://") {
        return Cow::Borrowed(text);
    }
    let mut changed = false;
    let words: Vec<Cow<'_, str>> = text
        .split(' ')
        .map(|word| {
            let redacted = redact_url(word);
            changed |= matches!(redacted, Cow::Owned(_));
            redacted
        })
        .collect();
    if changed { Cow::Owned(words.join(" ")) } else { Cow::Borrowed(text) }
}

/// The URL with its password and credential-like query values masked.
fn redact_url(text: &str) -> Cow<'_, str> {
    let Ok(mut url) = url::Url::parse(text) else { return Cow::Borrowed(text) };
    let sensitive_query = url.query_pairs().any(|(name, _)| is_sensitive_name(&name));
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    if !has_userinfo && !sensitive_query {
        return Cow::Borrowed(text);
    }
    if has_userinfo {
        // Both parts: a token is often passed as the user name (`https://TOKEN@host`).
        let _ = url.set_username(MASK);
        let _ = url.set_password(None);
    }
    if sensitive_query {
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(name, value)| {
                let value = if is_sensitive_name(&name) { MASK.to_string() } else { value.into_owned() };
                (name.into_owned(), value)
            })
            .collect();
        url.query_pairs_mut().clear().extend_pairs(pairs);
    }
    Cow::Owned(url.into())
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
        first.flush();

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
        first.flush();

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
    fn credentials_in_urls_and_variables_are_masked() {
        let dir = tempfile::tempdir().unwrap();
        let bus = EventBus::default();
        attach(&bus, dir.path()).unwrap();
        let span = Span::start(&bus, Source::User, None, SpanKind::Request, "x", Summary::default());
        span.emit(EventKind::RequestSent {
            method: "GET".into(),
            url: "https://ada:hunter2@x.dev/items?page=2&api_key=k3y&accessToken=t0k".into(),
            http_version: "HTTP/1.1".into(),
            headers: Vec::new(),
            body: Payload::default(),
        });
        span.emit(EventKind::VariableSet { name: "authToken".into(), value: "v4lue".into() });
        span.emit(EventKind::VariableSet { name: "userId".into(), value: "42".into() });
        let summary = Summary { url: Some("https://ghp_t0ken@x.dev/cb?code=c0de&state=ok".into()), ..Summary::default() };
        drop(Span::start(&bus, Source::User, None, SpanKind::Request, "GET https://x.dev/?SIG=s1g&Session=s3ss", summary));
        bus.flush();
        let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        for secret in ["hunter2", "ada", "k3y", "t0k", "v4lue", "ghp_t0ken", "c0de", "s1g", "s3ss"] {
            assert!(!text.contains(secret), "`{secret}` must not be written to disk: {text}");
        }
        for kept in ["page=2", "\"42\"", "state=ok", "GET https://x.dev/"] {
            assert!(text.contains(kept), "`{kept}` is kept: {text}");
        }
    }

    #[test]
    fn credential_names() {
        for name in ["api_key", "APIKEY", "accessToken", "client_secret", "X-Signature", "sessionId", "pwd", "code", "Key"] {
            assert!(is_sensitive_name(name), "{name}");
        }
        for name in ["page", "userId", "zipcode", "keyword", "author", "design"] {
            assert!(!is_sensitive_name(name), "{name}");
        }
        assert_eq!(redact_text("no urls here"), "no urls here");
        assert!(matches!(redact_text("GET https://x.dev/a?b=1"), Cow::Borrowed(_)));
    }

    #[test]
    fn dropping_the_bus_writes_everything_queued() {
        let dir = tempfile::tempdir().unwrap();
        {
            let bus = EventBus::default();
            attach(&bus, dir.path()).unwrap();
            for i in 0..2_000 {
                bus.emit(
                    crate::events::SpanId::new(),
                    None,
                    &Source::Cli,
                    EventKind::Log { level: crate::events::Level::Info, text: i.to_string() },
                );
            }
        }
        assert_eq!(load(&dir.path().join(FILE_NAME), usize::MAX).len(), 2_000);
    }

    #[test]
    fn lines_from_many_threads_never_interleave() {
        let dir = tempfile::tempdir().unwrap();
        let bus = EventBus::default();
        attach(&bus, dir.path()).unwrap();
        std::thread::scope(|scope| {
            for t in 0..4 {
                let bus = &bus;
                scope.spawn(move || {
                    for i in 0..500 {
                        let text = format!("{t}-{i}-{}", "x".repeat(i % 300));
                        bus.emit(
                            crate::events::SpanId::new(),
                            None,
                            &Source::Cli,
                            EventKind::Log { level: crate::events::Level::Info, text },
                        );
                    }
                });
            }
        });
        bus.flush();
        let text = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        assert_eq!(text.lines().count(), 2_000);
        assert!(text.lines().all(|line| serde_json::from_str::<LineIn>(line).is_ok()), "every line is complete JSON");
    }

    #[test]
    fn only_the_newest_lines_are_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let bus = EventBus::default();
        attach(&bus, dir.path()).unwrap();
        for i in 0..50 {
            bus.emit(
                crate::events::SpanId::new(),
                None,
                &Source::Cli,
                EventKind::Log { level: crate::events::Level::Info, text: i.to_string() },
            );
        }
        bus.flush();
        let events = load(&dir.path().join(FILE_NAME), 3);
        let texts: Vec<_> = events
            .iter()
            .map(|e| match &e.kind {
                EventKind::Log { text, .. } => text.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(texts, vec!["47", "48", "49"]);
    }

    #[tokio::test]
    async fn lines_split_inside_a_character_are_relayed_intact() {
        let dir = tempfile::tempdir().unwrap();
        let app = EventBus::default();
        attach(&app, dir.path()).unwrap();
        let _follower = follow(&app, dir.path());
        let mut live = app.subscribe();

        let other = Event {
            seq: 1,
            span_id: crate::events::SpanId::new(),
            parent_id: None,
            source: Source::Cli,
            at: chrono::Utc::now(),
            kind: EventKind::Log { level: crate::events::Level::Info, text: "•••".into() },
        };
        let line = serde_json::to_string(&LineOut { origin: std::process::id() + 1, event: &other }).unwrap() + "\n";
        let split = line.find('•').unwrap() + 1; // inside the 3-byte character
        let mut file = OpenOptions::new().append(true).open(dir.path().join(FILE_NAME)).unwrap();
        file.write_all(&line.as_bytes()[..split]).unwrap();
        tokio::time::sleep(FOLLOW_INTERVAL * 2).await;
        file.write_all(&line.as_bytes()[split..]).unwrap();

        let relayed = tokio::time::timeout(std::time::Duration::from_secs(3), live.recv()).await.unwrap().unwrap();
        assert!(matches!(&relayed.kind, EventKind::Log { text, .. } if text == "•••"), "{:?}", relayed.kind);
    }

    /// `cargo test --release -p post-tester-core -- --ignored --nocapture bench_emit`
    #[test]
    #[ignore = "benchmark"]
    fn bench_emit_with_history() {
        let dir = tempfile::tempdir().unwrap();
        let bus = EventBus::default();
        attach(&bus, dir.path()).unwrap();
        let started = std::time::Instant::now();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    let span =
                        Span::start(&bus, Source::Agent { client: "bench".into() }, None, SpanKind::Request, "x", Summary::default());
                    for i in 0..50_000 {
                        span.log(crate::events::Level::Info, format!("line {i}"));
                    }
                });
            }
        });
        let emitted = started.elapsed();
        bus.flush();
        eprintln!("200k events: emit {emitted:?}, on disk {:?}", started.elapsed());
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "not json\n{\"half\":\n").unwrap();
        assert!(load(&dir.path().join(FILE_NAME), 10).is_empty());
    }
}
