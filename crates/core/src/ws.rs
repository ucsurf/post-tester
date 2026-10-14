//! WebSocket sessions.
//!
//! A session is a long-running span: every message sent or received appears
//! live as a `Message` event, and the span finishes when the connection closes.

use crate::engine::Engine;
use crate::error::{Error, Result};
use crate::events::{Direction, EventKind, Level, Span, SpanId, SpanKind, Source, Summary};
use crate::template::render;
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// Options for opening a WebSocket.
#[derive(Debug, Clone)]
pub struct WsOptions {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub source: Source,
    pub environment: Option<String>,
}

impl Engine {
    /// Opens a WebSocket and returns the session's span id.
    pub async fn ws_connect(self: &Arc<Self>, options: WsOptions) -> Result<SpanId> {
        let span = Span::start(
            self.bus(),
            options.source.clone(),
            None,
            SpanKind::WebSocket,
            format!("WebSocket {}", options.url),
            Summary { protocol: Some("websocket".into()), method: Some("WS".into()), url: Some(options.url.clone()), request_id: None },
        );
        match self.ws_open(&options, &span).await {
            Ok(id) => Ok(id),
            Err(e) => {
                span.finish_error(&e);
                Err(e)
            }
        }
    }

    async fn ws_open(self: &Arc<Self>, options: &WsOptions, span: &Span) -> Result<SpanId> {
        let vars = self.variables(self.resolve_environment(options.environment.clone()).as_deref(), &[])?;
        let ctx = self.template_context(vars, span);
        let url = render(&options.url, &ctx).await?.text;
        let mut request = url
            .as_str()
            .into_client_request()
            .map_err(|e| Error::InvalidUrl { url: url.clone(), reason: e.to_string() })?;
        for (name, value) in &options.headers {
            let value = render(value, &ctx).await?.text;
            let name = http::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| Error::InvalidRequest(format!("`{name}` is not a valid header name")))?;
            let value = http::HeaderValue::from_str(&value)
                .map_err(|_| Error::InvalidRequest(format!("the value of header `{name}` contains invalid characters")))?;
            request.headers_mut().insert(name, value);
        }

        let (stream, response) =
            tokio_tungstenite::connect_async(request).await.map_err(|e| Error::WebSocket(format!("could not connect: {e}")))?;
        span.log(Level::Info, format!("Connected ({})", response.status()));
        let (mut sink, mut incoming) = stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        self.ws_sessions().insert(span.id(), tx);

        let engine = self.clone();
        let span = span.clone();
        let id = span.id();
        tokio::spawn(async move {
            let guard = engine.track(&span, None);
            let result: Result<()> = async {
                loop {
                    tokio::select! {
                        () = guard.token.cancelled() => {
                            let _ = sink.send(Message::Close(None)).await;
                            span.log(Level::Info, "Closed by you");
                            return Ok(());
                        }
                        outgoing = rx.recv() => {
                            let Some(message) = outgoing else { return Ok(()) };
                            report(&span, Direction::Sent, &message);
                            sink.send(message).await.map_err(|e| Error::WebSocket(e.to_string()))?;
                        }
                        incoming = incoming.next() => {
                            match incoming {
                                None => return Ok(()),
                                Some(Err(e)) => return Err(Error::WebSocket(e.to_string())),
                                Some(Ok(Message::Close(frame))) => {
                                    let reason = frame.map(|f| format!("{} {}", f.code, f.reason)).unwrap_or_default();
                                    span.log(Level::Info, format!("Closed by server {reason}").trim_end().to_string());
                                    return Ok(());
                                }
                                Some(Ok(message)) => report(&span, Direction::Received, &message),
                            }
                        }
                    }
                }
            }
            .await;
            engine.ws_sessions().remove(&id);
            match result {
                Ok(()) => span.finish_ok(None, None),
                Err(e) => span.finish_error(e),
            }
        });
        Ok(id)
    }

    /// Sends a text message on an open session.
    pub fn ws_send(&self, session: SpanId, text: impl Into<String>) -> Result<()> {
        let sessions = self.ws_sessions();
        let sender = sessions.get(&session).ok_or_else(|| Error::NotFound(format!("open WebSocket session `{session}`")))?;
        sender.send(Message::Text(text.into().into())).map_err(|_| Error::WebSocket("the connection is closed".into()))
    }

    /// Ids of open WebSocket sessions.
    pub fn ws_open_sessions(&self) -> Vec<SpanId> {
        self.ws_sessions().keys().copied().collect()
    }
}

fn report(span: &Span, direction: Direction, message: &Message) {
    let (label, data) = match message {
        Message::Text(text) => ("text", Bytes::copy_from_slice(text.as_bytes())),
        Message::Binary(data) => ("binary", data.clone()),
        Message::Ping(data) => ("ping", data.clone()),
        Message::Pong(data) => ("pong", data.clone()),
        Message::Close(_) | Message::Frame(_) => return,
    };
    let payload = span.bus().blobs().payload(&data, None);
    span.emit(EventKind::Message { direction, label: Some(label.into()), payload });
}
