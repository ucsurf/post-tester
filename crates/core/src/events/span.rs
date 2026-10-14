//! [`Span`]: the handle code uses to report what it is doing.
//!
//! ```
//! # use post_tester_core::events::*;
//! let bus = EventBus::default();
//! let span = Span::start(&bus, Source::User, None, SpanKind::Request, "GET /users", Summary::default());
//! span.log(Level::Info, "Sending request");
//! span.finish_ok(None, None);
//! ```

use super::bus::EventBus;
use super::event::{EventKind, Level, Outcome, Phase, Source, SpanId, SpanKind, Summary};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A running operation. Emits events to the bus.
///
/// Cheap to clone. If the last clone is dropped without calling a `finish_*`
/// method, a `Cancelled` event is emitted so no span is left "running" forever.
#[derive(Clone)]
pub struct Span {
    inner: Arc<Inner>,
}

struct Inner {
    bus: EventBus,
    id: SpanId,
    parent: Option<SpanId>,
    source: Source,
    started: Instant,
    finished: AtomicBool,
}

impl std::fmt::Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Span").field("id", &self.inner.id).field("source", &self.inner.source).finish()
    }
}

impl Span {
    /// Starts a new span and emits its `Started` event.
    pub fn start(
        bus: &EventBus,
        source: Source,
        parent: Option<SpanId>,
        kind: SpanKind,
        title: impl Into<String>,
        summary: Summary,
    ) -> Self {
        let span = Self {
            inner: Arc::new(Inner {
                bus: bus.clone(),
                id: SpanId::new(),
                parent,
                source,
                started: Instant::now(),
                finished: AtomicBool::new(false),
            }),
        };
        span.emit(EventKind::Started { kind, title: title.into(), summary });
        span
    }

    /// Starts a child span with the same source.
    pub fn child(&self, kind: SpanKind, title: impl Into<String>, summary: Summary) -> Self {
        Self::start(&self.inner.bus, self.inner.source.clone(), Some(self.inner.id), kind, title, summary)
    }

    pub fn id(&self) -> SpanId {
        self.inner.id
    }

    pub fn source(&self) -> &Source {
        &self.inner.source
    }

    pub fn bus(&self) -> &EventBus {
        &self.inner.bus
    }

    pub fn elapsed(&self) -> Duration {
        self.inner.started.elapsed()
    }

    /// Emits any event for this span.
    pub fn emit(&self, kind: EventKind) {
        self.inner.bus.emit(self.inner.id, self.inner.parent, &self.inner.source, kind);
    }

    pub fn log(&self, level: Level, text: impl Into<String>) {
        self.emit(EventKind::Log { level, text: text.into() });
    }

    pub fn phase(&self, phase: Phase, duration: Duration) {
        self.emit(EventKind::Phase { phase, duration_ms: millis(duration) });
    }

    pub fn progress(&self, done: u32, total: u32) {
        self.emit(EventKind::Progress { done, total });
    }

    pub fn is_finished(&self) -> bool {
        self.inner.finished.load(Ordering::Acquire)
    }

    /// Ends the span. Only the first call has an effect.
    pub fn finish(&self, outcome: Outcome, status: Option<u16>, size: Option<u64>, error: Option<String>) {
        if self.inner.finished.swap(true, Ordering::AcqRel) {
            return;
        }
        self.inner.bus.emit(
            self.inner.id,
            self.inner.parent,
            &self.inner.source,
            EventKind::Finished { outcome, duration_ms: millis(self.elapsed()), status, size, error },
        );
    }

    pub fn finish_ok(&self, status: Option<u16>, size: Option<u64>) {
        self.finish(Outcome::Ok, status, size, None);
    }

    pub fn finish_error(&self, error: impl std::fmt::Display) {
        self.finish(Outcome::Error, None, None, Some(error.to_string()));
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if !self.finished.load(Ordering::Acquire) {
            self.bus.emit(
                self.id,
                self.parent,
                &self.source,
                EventKind::Finished {
                    outcome: Outcome::Cancelled,
                    duration_ms: millis(self.started.elapsed()),
                    status: None,
                    size: None,
                    error: None,
                },
            );
        }
    }
}

/// Converts a duration to fractional milliseconds.
pub fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_span_is_cancelled() {
        let bus = EventBus::default();
        let id = {
            let span = Span::start(&bus, Source::User, None, SpanKind::Request, "x", Summary::default());
            span.id()
        };
        let events = bus.span_events(id);
        assert!(matches!(events.last().unwrap().kind, EventKind::Finished { outcome: Outcome::Cancelled, .. }));
    }

    #[test]
    fn finish_is_emitted_once() {
        let bus = EventBus::default();
        let span = Span::start(&bus, Source::User, None, SpanKind::Request, "x", Summary::default());
        span.finish_ok(Some(200), None);
        span.finish_error("ignored");
        let finished = bus
            .span_events(span.id())
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Finished { .. }))
            .count();
        assert_eq!(finished, 1);
    }

    #[test]
    fn child_span_points_to_parent() {
        let bus = EventBus::default();
        let parent = Span::start(&bus, Source::Cli, None, SpanKind::CollectionRun, "run", Summary::default());
        let child = parent.child(SpanKind::Request, "req", Summary::default());
        let started = &bus.span_events(child.id())[0];
        assert_eq!(started.parent_id, Some(parent.id()));
        assert_eq!(started.source, Source::Cli);
    }
}
