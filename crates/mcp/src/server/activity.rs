//! Summaries of recent spans, built from the event bus history.

use post_tester_core::events::{EventBus, EventKind, SpanId};
use serde_json::{Value, json};
use std::collections::HashMap;

/// The `limit` most recent spans, newest first, without `exclude` (the
/// caller's own span, which is still running).
pub(super) fn recent(bus: &EventBus, limit: usize, exclude: Option<SpanId>) -> Value {
    let mut spans: Vec<Value> = Vec::new();
    let mut index: HashMap<SpanId, usize> = HashMap::new();
    let mut tests: HashMap<SpanId, (u32, u32)> = HashMap::new();

    for event in bus.recent() {
        if Some(event.span_id) == exclude {
            continue;
        }
        match &event.kind {
            EventKind::Started { kind, title, summary } => {
                index.insert(event.span_id, spans.len());
                let mut span = json!({
                    "spanId": event.span_id,
                    "kind": kind,
                    "title": title,
                    "source": event.source.label(),
                    "startedAt": event.at,
                    "outcome": "running",
                });
                if let Some(parent) = event.parent_id {
                    span["parentId"] = json!(parent);
                }
                if let Some(method) = &summary.method {
                    span["method"] = json!(method);
                }
                if let Some(url) = &summary.url {
                    span["url"] = json!(url);
                }
                if let Some(id) = &summary.request_id {
                    span["requestId"] = json!(id);
                }
                spans.push(span);
            }
            EventKind::TestResult { passed, .. } => {
                let counts = tests.entry(event.span_id).or_default();
                if *passed { counts.0 += 1 } else { counts.1 += 1 }
            }
            EventKind::Finished { outcome, duration_ms, status, error, .. } => {
                let Some(span) = index.get(&event.span_id).map(|&i| &mut spans[i]) else { continue };
                span["outcome"] = json!(outcome);
                span["durationMs"] = json!(duration_ms.round());
                if let Some(status) = status {
                    span["status"] = json!(status);
                }
                if let Some(error) = error {
                    span["error"] = json!(error);
                }
            }
            _ => {}
        }
    }

    for (id, (passed, failed)) in tests {
        if let Some(&i) = index.get(&id) {
            spans[i]["tests"] = json!(format!("{passed} passed, {failed} failed"));
        }
    }
    Value::from(spans.into_iter().rev().take(limit).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use post_tester_core::events::{Outcome, Source, Span, SpanKind, Summary};

    #[test]
    fn summarizes_spans_newest_first() {
        let bus = EventBus::default();
        let first = Span::start(&bus, Source::User, None, SpanKind::Request, "first", Summary::default());
        first.emit(EventKind::TestResult { name: "status == 200".into(), passed: false, detail: None });
        first.finish(Outcome::Failed, Some(500), None, None);
        let _second = Span::start(&bus, Source::Cli, None, SpanKind::Request, "second", Summary::default());

        let activity = recent(&bus, 10, None);
        assert_eq!(activity[0]["title"], "second");
        assert_eq!(activity[0]["outcome"], "running");
        assert_eq!(activity[1]["status"], 500);
        assert_eq!(activity[1]["tests"], "0 passed, 1 failed");
        assert_eq!(recent(&bus, 1, None).as_array().unwrap().len(), 1);
    }
}
