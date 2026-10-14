//! The live event bus: one stream that every front-end listens to.

use super::blob::BlobStore;
use super::event::{Event, EventKind, Source, SpanId};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use tokio::sync::broadcast;

/// How many events a slow subscriber may fall behind before it skips ahead.
const CHANNEL_CAPACITY: usize = 8_192;

/// Receives every event, e.g. to persist history. Must be fast and must not block.
pub trait EventSink: Send + Sync {
    fn record(&self, event: &Event);

    /// Waits until everything recorded so far is stored (e.g. written to disk).
    /// Sinks that store synchronously need not implement it.
    fn flush(&self) {}
}

/// Settings for an [`EventBus`].
#[derive(Debug, Clone, Copy)]
pub struct BusConfig {
    /// How many recent events stay in memory for late subscribers.
    pub history_len: usize,
    /// Maximum total bytes kept in the blob store.
    pub blob_capacity_bytes: usize,
}

impl Default for BusConfig {
    fn default() -> Self {
        Self { history_len: 20_000, blob_capacity_bytes: 256 * 1024 * 1024 }
    }
}

/// Broadcasts events to all subscribers and remembers recent ones.
///
/// Cheap to clone: all clones share the same stream.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

struct Inner {
    sender: broadcast::Sender<Arc<Event>>,
    recent: Mutex<Recent>,
    history_len: usize,
    next_seq: AtomicU64,
    blobs: BlobStore,
    /// Read-locked per event, so emitters on different threads never wait for
    /// each other here; written only when a sink is added.
    sinks: RwLock<Vec<Arc<dyn EventSink>>>,
}

/// Recent events, oldest first, plus an index by span so looking up one
/// span's events does not scan the whole history.
#[derive(Default)]
struct Recent {
    events: VecDeque<Arc<Event>>,
    by_span: HashMap<SpanId, VecDeque<Arc<Event>>>,
}

impl Recent {
    fn push_back(&mut self, event: Arc<Event>) {
        self.by_span.entry(event.span_id).or_default().push_back(event.clone());
        self.events.push_back(event);
    }

    /// Drops the oldest events until at most `len` remain.
    fn truncate_front(&mut self, len: usize) {
        while self.events.len() > len {
            let Some(oldest) = self.events.pop_front() else { break };
            // Events are indexed in the same order, so the oldest event of the
            // whole history is also the oldest one of its span.
            if let Some(list) = self.by_span.get_mut(&oldest.span_id) {
                list.pop_front();
                if list.is_empty() {
                    self.by_span.remove(&oldest.span_id);
                }
            }
        }
    }
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus").field("subscribers", &self.inner.sender.receiver_count()).finish_non_exhaustive()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(BusConfig::default())
    }
}

impl EventBus {
    pub fn new(config: BusConfig) -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                sender,
                recent: Mutex::new(Recent { events: VecDeque::with_capacity(config.history_len.min(1024)), by_span: HashMap::new() }),
                history_len: config.history_len,
                next_seq: AtomicU64::new(1),
                blobs: BlobStore::new(config.blob_capacity_bytes),
                sinks: RwLock::new(Vec::new()),
            }),
        }
    }

    fn recent_lock(&self) -> MutexGuard<'_, Recent> {
        self.inner.recent.lock().expect("event history lock poisoned")
    }

    /// Publishes an event to every subscriber and returns it.
    pub fn emit(&self, span_id: SpanId, parent_id: Option<SpanId>, source: &Source, kind: EventKind) -> Arc<Event> {
        let event = Event { seq: 0, span_id, parent_id, source: source.clone(), at: chrono::Utc::now(), kind };
        self.publish(event, true)
    }

    /// Publishes an event that happened in another post-tester process (e.g. a
    /// `pt` command in a terminal). It gets a new sequence number here, and is
    /// not written to sinks, because the other process already recorded it.
    pub fn relay(&self, event: Event) -> Arc<Event> {
        self.publish(event, false)
    }

    fn publish(&self, mut event: Event, record: bool) -> Arc<Event> {
        let mut recent = self.recent_lock();
        // The sequence number is taken, and the event broadcast, under the lock,
        // so `recent` and every subscriber see events in `seq` order.
        event.seq = self.inner.next_seq.fetch_add(1, Ordering::Relaxed);
        let event = Arc::new(event);
        recent.push_back(event.clone());
        recent.truncate_front(self.inner.history_len);
        // An error only means nobody is listening right now, which is fine.
        let _ = self.inner.sender.send(event.clone());
        drop(recent);

        if record {
            for sink in self.inner.sinks.read().expect("sink lock poisoned").iter() {
                sink.record(&event);
            }
        }
        event
    }

    /// Subscribes to new events. Combine with [`recent`](Self::recent) to also
    /// see what happened before subscribing.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.inner.sender.subscribe()
    }

    /// Recent events, oldest first.
    pub fn recent(&self) -> Vec<Arc<Event>> {
        self.recent_lock().events.iter().cloned().collect()
    }

    /// Recent events with a sequence number above `seq`, oldest first. Cheaper
    /// than filtering [`recent`](Self::recent), which copies the whole history.
    pub fn recent_since(&self, seq: u64) -> Vec<Arc<Event>> {
        let recent = self.recent_lock();
        // Events are ordered by `seq`, so the new ones are at the end.
        let start = recent.events.partition_point(|e| e.seq <= seq);
        recent.events.range(start..).cloned().collect()
    }

    /// The newest `count` events, oldest first.
    pub fn recent_last(&self, count: usize) -> Vec<Arc<Event>> {
        let recent = self.recent_lock();
        let start = recent.events.len().saturating_sub(count);
        recent.events.range(start..).cloned().collect()
    }

    /// Recent events belonging to one span (not including child spans).
    pub fn span_events(&self, span_id: SpanId) -> Vec<Arc<Event>> {
        self.recent_lock().by_span.get(&span_id).map(|list| list.iter().cloned().collect()).unwrap_or_default()
    }

    /// Puts previously recorded events back into the history (without
    /// broadcasting them), before any events already published.
    ///
    /// Restored events are numbered anew, continuing after the newest number
    /// they had, so numbers stay unique even when several processes wrote them.
    /// Call it before publishing anything, so that `recent` stays in `seq` order.
    pub fn restore(&self, events: Vec<Event>) {
        let mut recent = self.recent_lock();
        let newest = events.iter().map(|e| e.seq).max().unwrap_or(0);
        let count = events.len() as u64;
        let first = self.inner.next_seq.fetch_max(newest + 1, Ordering::Relaxed).max(newest + 1);
        self.inner.next_seq.fetch_max(first + count, Ordering::Relaxed);

        let mut restored = Recent::default();
        for (seq, mut event) in (first..).zip(events) {
            event.seq = seq;
            restored.push_back(Arc::new(event));
        }
        for event in std::mem::take(&mut recent.events) {
            restored.push_back(event);
        }
        restored.truncate_front(self.inner.history_len);
        *recent = restored;
    }

    /// Storage for large payloads referenced by events.
    pub fn blobs(&self) -> &BlobStore {
        &self.inner.blobs
    }

    /// Adds a sink that receives every future event.
    pub fn add_sink(&self, sink: Arc<dyn EventSink>) {
        self.inner.sinks.write().expect("sink lock poisoned").push(sink);
    }

    /// Waits until every sink has stored the events recorded so far, e.g.
    /// before a short-lived process exits.
    pub fn flush(&self) {
        for sink in self.inner.sinks.read().expect("sink lock poisoned").iter() {
            sink.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::event::{Level, Outcome};

    #[tokio::test]
    async fn subscribers_receive_events_in_order() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let span = SpanId::new();
        bus.emit(span, None, &Source::User, EventKind::Log { level: Level::Info, text: "a".into() });
        bus.emit(
            span,
            None,
            &Source::User,
            EventKind::Finished { outcome: Outcome::Ok, duration_ms: 1.0, status: None, size: None, error: None },
        );

        let first = rx.recv().await.unwrap();
        let second = rx.recv().await.unwrap();
        assert!(first.seq < second.seq);
        assert!(matches!(second.kind, EventKind::Finished { .. }));
    }

    #[test]
    fn history_is_bounded() {
        let bus = EventBus::new(BusConfig { history_len: 3, ..BusConfig::default() });
        for i in 0..5 {
            bus.emit(SpanId::new(), None, &Source::System, EventKind::Log { level: Level::Info, text: i.to_string() });
        }
        let recent = bus.recent();
        assert_eq!(recent.len(), 3);
        assert!(matches!(&recent[0].kind, EventKind::Log { text, .. } if text == "2"));
    }

    fn log(bus: &EventBus, span: SpanId, text: &str) {
        bus.emit(span, None, &Source::System, EventKind::Log { level: Level::Info, text: text.into() });
    }

    #[test]
    fn span_index_follows_eviction() {
        let bus = EventBus::new(BusConfig { history_len: 4, ..BusConfig::default() });
        let (a, b) = (SpanId::new(), SpanId::new());
        for i in 0..3 {
            log(&bus, a, &format!("a{i}"));
            log(&bus, b, &format!("b{i}"));
        }
        // a0 and b0 were evicted.
        let texts = |span| -> Vec<String> {
            bus.span_events(span)
                .iter()
                .map(|e| match &e.kind {
                    EventKind::Log { text, .. } => text.clone(),
                    _ => unreachable!(),
                })
                .collect()
        };
        assert_eq!(texts(a), vec!["a1", "a2"]);
        assert_eq!(texts(b), vec!["b1", "b2"]);
        for _ in 0..4 {
            log(&bus, b, "more");
        }
        assert!(bus.span_events(a).is_empty(), "spans whose events are all gone leave no index entry");
        assert!(!bus.inner.recent.lock().unwrap().by_span.contains_key(&a));
    }

    #[test]
    fn recent_since_and_last() {
        let bus = EventBus::default();
        let span = SpanId::new();
        for i in 0..5 {
            log(&bus, span, &i.to_string());
        }
        let since: Vec<u64> = bus.recent_since(3).iter().map(|e| e.seq).collect();
        assert_eq!(since, vec![4, 5]);
        assert!(bus.recent_since(5).is_empty());
        assert_eq!(bus.recent_since(0).len(), 5);
        let last: Vec<u64> = bus.recent_last(2).iter().map(|e| e.seq).collect();
        assert_eq!(last, vec![4, 5]);
        assert_eq!(bus.recent_last(100).len(), 5);
    }

    #[test]
    fn restored_events_get_unique_increasing_numbers() {
        let bus = EventBus::default();
        let span = SpanId::new();
        // Two processes wrote events with overlapping numbers.
        let event = |seq| Event {
            seq,
            span_id: span,
            parent_id: None,
            source: Source::Cli,
            at: chrono::Utc::now(),
            kind: EventKind::Log { level: Level::Info, text: String::new() },
        };
        bus.restore(vec![event(7), event(3), event(7)]);
        log(&bus, span, "new");
        let seqs: Vec<u64> = bus.recent().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![8, 9, 10, 11]);
        assert_eq!(bus.span_events(span).len(), 4);
    }

    #[test]
    fn concurrent_emitters_are_seen_in_order() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..500 {
                        log(&bus, SpanId::new(), "x");
                    }
                });
            }
        });
        let mut last = 0;
        while let Ok(event) = rx.try_recv() {
            assert!(event.seq > last, "subscribers see events in seq order");
            last = event.seq;
        }
        assert_eq!(last, 2_000);
    }
}
