//! The live event stream, event history and blobs.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use post_tester_core::events::{BlobId, Event};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

/// How long events are collected before being sent as one batch.
const BATCH_WINDOW: Duration = Duration::from_millis(50);
/// Most events in one batch.
const BATCH_MAX: usize = 1_000;
/// History sent when a client connects.
const INITIAL_HISTORY: usize = 5_000;

pub async fn live(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| stream(state, socket))
}

/// Sends history first, then new events in small batches until the client leaves.
async fn stream(state: AppState, mut socket: WebSocket) {
    // Subscribe before reading history so nothing is missed in between.
    let mut receiver = state.engine.bus().subscribe();
    let history = state.engine.bus().recent();
    let last_seq = history.last().map_or(0, |e| e.seq);
    let start = history.len().saturating_sub(INITIAL_HISTORY);
    if send_batch(&mut socket, &history[start..]).await.is_err() {
        return;
    }

    loop {
        let mut batch: Vec<Arc<Event>> = Vec::new();
        // Wait for the first event (or the client closing).
        tokio::select! {
            received = receiver.recv() => match received {
                Ok(event) => batch.push(event),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => continue,
            },
        }
        // Then collect whatever else arrives within the batch window.
        let deadline = tokio::time::Instant::now() + BATCH_WINDOW;
        while batch.len() < BATCH_MAX {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(Ok(event)) => batch.push(event),
                Ok(Err(RecvError::Lagged(_))) => continue,
                Ok(Err(RecvError::Closed)) | Err(_) => break,
            }
        }
        batch.retain(|e| e.seq > last_seq);
        if !batch.is_empty() && send_batch(&mut socket, &batch).await.is_err() {
            return;
        }
    }
}

async fn send_batch(socket: &mut WebSocket, events: &[Arc<Event>]) -> Result<(), axum::Error> {
    let json = serde_json::to_string(events).expect("events always serialize");
    socket.send(Message::Text(json.into())).await
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    since: u64,
}

pub async fn events(State(state): State<AppState>, Query(q): Query<EventsQuery>) -> Json<Vec<Arc<Event>>> {
    Json(state.engine.bus().recent().into_iter().filter(|e| e.seq > q.since).collect())
}

pub async fn blob(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let id: BlobId = id.parse().map_err(|_| ApiError::bad_request(format!("`{id}` is not a blob id")))?;
    let blob = state
        .engine
        .bus()
        .blobs()
        .get(id)
        .ok_or_else(|| ApiError::not_found("this payload is no longer in memory; send the request again to see it"))?;
    let content_type = blob.content_type.unwrap_or_else(|| "application/octet-stream".into());
    Ok(([(header::CONTENT_TYPE, content_type)], Body::from(blob.data)).into_response())
}
