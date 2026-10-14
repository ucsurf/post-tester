//! Makes every JSON-RPC message of a client session visible on the event bus.
//!
//! [`Traced`] wraps any rmcp client transport and reports each message as a
//! `Message` event. Messages that belong to a call made through
//! [`McpClients`](super::McpClients) (e.g. one `tools/call`) go to that call's
//! child span; everything else (`initialize`, list requests, notifications,
//! server-initiated requests) goes to the session span.

use bytes::Bytes;
use post_tester_core::events::{Direction, EventKind, Span, SpanId};
use rmcp::RoleClient;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::Transport;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Routes messages to spans. Shared by the transport and the session.
#[derive(Debug)]
pub(crate) struct Tracer {
    session: Span,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    /// Calls waiting for their request to be sent, oldest first.
    expected: Vec<(String, Span)>,
    /// Requests in flight, keyed by "sent by us?" and their JSON-RPC id
    /// (each side numbers its own requests).
    in_flight: HashMap<(bool, String), (String, Option<Span>)>,
}

impl Tracer {
    pub(crate) fn new(session: Span) -> Arc<Self> {
        Arc::new(Self { session, state: Mutex::new(State::default()) })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("tracer lock poisoned")
    }

    /// Announces that the next outgoing `method` request belongs to `span`.
    /// Keep the guard alive until the call returns.
    pub(crate) fn expect(self: &Arc<Self>, method: &str, span: &Span) -> Expectation {
        self.state().expected.push((method.to_string(), span.clone()));
        Expectation { tracer: self.clone(), span: span.id() }
    }

    fn record(&self, direction: Direction, message: &impl Serialize) {
        let Ok(value) = serde_json::to_value(message) else { return };
        let (label, span) = self.route(direction, &value);
        let target = span.as_ref().unwrap_or(&self.session);
        let data = Bytes::from(serde_json::to_vec(&value).unwrap_or_default());
        let payload = target.bus().blobs().payload(&data, Some("application/json".into()));
        target.emit(EventKind::Message { direction, label, payload });
    }

    /// Finds the label (the JSON-RPC method) and the span a message belongs to.
    fn route(&self, direction: Direction, value: &Value) -> (Option<String>, Option<Span>) {
        let method = value.get("method").and_then(Value::as_str);
        let id = value.get("id").map(Value::to_string);
        let mut state = self.state();
        match (method, id) {
            // A request: remember it so its response can be routed the same way.
            (Some(method), Some(id)) => {
                let sent = direction == Direction::Sent;
                let span = if sent { state.take_expected(method) } else { None };
                state.in_flight.insert((sent, id), (method.to_string(), span.clone()));
                (Some(method.to_string()), span)
            }
            // A notification.
            (Some(method), None) => (Some(method.to_string()), None),
            // A response (or error) to a request sent the other way.
            (None, Some(id)) => match state.in_flight.remove(&(direction == Direction::Received, id)) {
                Some((method, span)) => (Some(method), span),
                None => (None, None),
            },
            (None, None) => (None, None),
        }
    }
}

impl State {
    fn take_expected(&mut self, method: &str) -> Option<Span> {
        let index = self.expected.iter().position(|(m, _)| m == method)?;
        Some(self.expected.remove(index).1)
    }
}

/// Removes an expectation that was never matched, e.g. because the call
/// failed before anything was sent, so it cannot capture a later message.
#[derive(Debug)]
pub(crate) struct Expectation {
    tracer: Arc<Tracer>,
    span: SpanId,
}

impl Drop for Expectation {
    fn drop(&mut self) {
        self.tracer.state().expected.retain(|(_, span)| span.id() != self.span);
    }
}

/// A client transport that reports every message to a [`Tracer`].
pub(crate) struct Traced<T> {
    inner: T,
    tracer: Arc<Tracer>,
}

impl<T> Traced<T> {
    pub(crate) fn new(inner: T, tracer: Arc<Tracer>) -> Self {
        Self { inner, tracer }
    }
}

impl<T: Transport<RoleClient>> Transport<RoleClient> for Traced<T> {
    type Error = T::Error;

    fn send(&mut self, item: TxJsonRpcMessage<RoleClient>) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        self.tracer.record(Direction::Sent, &item);
        self.inner.send(item)
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleClient>>> + Send {
        let tracer = self.tracer.clone();
        let next = self.inner.receive();
        async move {
            let message = next.await;
            if let Some(message) = &message {
                tracer.record(Direction::Received, message);
            }
            message
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use post_tester_core::events::{EventBus, Source, SpanKind, Summary};
    use serde_json::json;

    fn messages(bus: &EventBus, span: &Span) -> Vec<(Direction, Option<String>)> {
        bus.span_events(span.id())
            .iter()
            .filter_map(|e| match &e.kind {
                EventKind::Message { direction, label, .. } => Some((*direction, label.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn calls_get_their_own_messages_and_the_rest_goes_to_the_session() {
        let bus = EventBus::default();
        let session = Span::start(&bus, Source::User, None, SpanKind::McpCall, "MCP test", Summary::default());
        let call = session.child(SpanKind::McpCall, "tools/call", Summary::default());
        let tracer = Tracer::new(session.clone());

        let expectation = tracer.expect("tools/call", &call);
        tracer.record(Direction::Sent, &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }));
        tracer.record(Direction::Sent, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {} }));
        tracer.record(Direction::Received, &json!({ "jsonrpc": "2.0", "id": 2, "result": {} }));
        tracer.record(Direction::Received, &json!({ "jsonrpc": "2.0", "id": 1, "result": {} }));
        tracer.record(Direction::Received, &json!({ "jsonrpc": "2.0", "method": "notifications/message" }));
        drop(expectation);

        let call_label = Some("tools/call".to_string());
        assert_eq!(messages(&bus, &call), vec![(Direction::Sent, call_label.clone()), (Direction::Received, call_label)]);
        let list = Some("tools/list".to_string());
        assert_eq!(
            messages(&bus, &session),
            vec![(Direction::Sent, list.clone()), (Direction::Received, list), (Direction::Received, Some("notifications/message".into()))]
        );
    }

    #[test]
    fn unused_expectations_are_dropped() {
        let bus = EventBus::default();
        let session = Span::start(&bus, Source::User, None, SpanKind::McpCall, "MCP test", Summary::default());
        let call = session.child(SpanKind::McpCall, "tools/call", Summary::default());
        let tracer = Tracer::new(session.clone());
        drop(tracer.expect("tools/call", &call));
        tracer.record(Direction::Sent, &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call" }));
        assert!(messages(&bus, &call).is_empty());
        assert_eq!(messages(&bus, &session).len(), 1);
    }
}
