//! The live event bus: one stream that every front-end listens to.

use super::blob::BlobStore;
use super::event::{Event, EventKind, Source, SpanId};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// How many events a slow subscriber may fall behind before it skips ahead.
const CHANNEL_CAPACITY: usize = 8_192;

/// Receives every event, e.g. to persist history. Must be fast and must not block.
pub trait EventSink: Send + Sync {
    fn record(&self, event: &Event);
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
        Self {
            history_len: 20_000,
            blob_capacity_bytes: 256 * 1024 * 1024,
        }
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
    recent: Mutex<VecDeque<Arc<Event>>>,
    history_len: usize,
    next_seq: AtomicU64,
    blobs: BlobStore,
    sinks: Mutex<Vec<Arc<dyn EventSink>>>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("subscribers", &self.inner.sender.receiver_count())
            .finish_non_exhaustive()
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
                recent: Mutex::new(VecDeque::with_capacity(config.history_len.min(1024))),
                history_len: config.history_len,
                next_seq: AtomicU64::new(1),
                blobs: BlobStore::new(config.blob_capacity_bytes),
                sinks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Publishes an event to every subscriber and returns it.
    pub fn emit(
        &self,
        span_id: SpanId,
        parent_id: Option<SpanId>,
        source: &Source,
        kind: EventKind,
    ) -> Arc<Event> {
        let mut recent = self.inner.recent.lock().expect("event history lock poisoned");
        // The sequence number is taken under the lock so `recent` is always ordered.
        let event = Arc::new(Event {
            seq: self.inner.next_seq.fetch_add(1, Ordering::Relaxed),
            span_id,
            parent_id,
            source: source.clone(),
            at: chrono::Utc::now(),
            kind,
        });
        if recent.len() >= self.inner.history_len {
            recent.pop_front();
        }
        recent.push_back(event.clone());
        drop(recent);

        for sink in self.inner.sinks.lock().expect("sink lock poisoned").iter() {
            sink.record(&event);
        }
        // An error only means nobody is listening right now, which is fine.
        let _ = self.inner.sender.send(event.clone());
        event
    }

    /// Subscribes to new events. Combine with [`recent`](Self::recent) to also
    /// see what happened before subscribing.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.inner.sender.subscribe()
    }

    /// Recent events, oldest first.
    pub fn recent(&self) -> Vec<Arc<Event>> {
        self.inner
            .recent
            .lock()
            .expect("event history lock poisoned")
            .iter()
            .cloned()
            .collect()
    }

    /// Recent events belonging to one span (not including child spans).
    pub fn span_events(&self, span_id: SpanId) -> Vec<Arc<Event>> {
        self.inner
            .recent
            .lock()
            .expect("event history lock poisoned")
            .iter()
            .filter(|e| e.span_id == span_id)
            .cloned()
            .collect()
    }

    /// Storage for large payloads referenced by events.
    pub fn blobs(&self) -> &BlobStore {
        &self.inner.blobs
    }

    /// Adds a sink that receives every future event.
    pub fn add_sink(&self, sink: Arc<dyn EventSink>) {
        self.inner.sinks.lock().expect("sink lock poisoned").push(sink);
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
}
