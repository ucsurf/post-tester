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
/// Most events in one message (history is split into messages of this size too).
const BATCH_MAX: usize = 1_000;
/// History sent when a client connects.
const INITIAL_HISTORY: usize = 5_000;
/// A client that does not accept a message within this time is disconnected.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn live(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| stream(state, socket))
}

/// Sends history first, then new events in small batches until the client
/// leaves or the server stops. Each client has its own receiver, so a slow
/// client only delays itself.
async fn stream(state: AppState, mut socket: WebSocket) {
    let bus = state.engine.bus();
    // Subscribe before reading history so nothing is missed in between.
    let mut receiver = bus.subscribe();
    let history = bus.recent_last(INITIAL_HISTORY);
    // Events up to `floor` were in a history snapshot sent to this client
    // (snapshots hold every event up to their newest), so the copies that
    // also arrive through the channel are skipped.
    let mut floor = history.last().map_or(0, |e| e.seq);
    let mut newest_sent = floor;
    // The first message is the history, even when there is none yet (`[]`).
    let sent = if history.is_empty() { send(&mut socket, "[]".to_string()).await } else { send_chunked(&mut socket, &history).await };
    if sent.is_err() {
        return;
    }

    loop {
        let mut batch: Vec<Arc<Event>> = Vec::new();
        let mut lagged = false;
        // Wait for the first event, the client closing, or the server stopping.
        tokio::select! {
            () = state.shutdown.cancelled() => {
                let _ = tokio::time::timeout(Duration::from_secs(1), socket.send(Message::Close(None))).await;
                return;
            }
            received = receiver.recv() => match received {
                Ok(event) => batch.push(event),
                Err(RecvError::Lagged(_)) => lagged = true,
                Err(RecvError::Closed) => return,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                Some(Ok(_)) => continue,
            },
        }
        // Then collect whatever else arrives within the batch window.
        let deadline = tokio::time::Instant::now() + BATCH_WINDOW;
        while !lagged && batch.len() < BATCH_MAX {
            match tokio::time::timeout_at(deadline, receiver.recv()).await {
                Ok(Ok(event)) => batch.push(event),
                Ok(Err(RecvError::Lagged(_))) => lagged = true,
                Ok(Err(RecvError::Closed)) | Err(_) => break,
            }
        }
        if lagged {
            // The client fell so far behind that the channel dropped events for
            // it. Send what it missed from history instead; clients ignore
            // events whose `seq` they already have.
            let missed = bus.recent_since(newest_sent);
            floor = missed.last().map_or(newest_sent, |e| e.seq).max(floor);
            batch.retain(|e| e.seq > floor);
            batch.splice(0..0, missed);
        } else {
            batch.retain(|e| e.seq > floor);
        }
        newest_sent = batch.iter().map(|e| e.seq).fold(newest_sent, u64::max);
        if send_chunked(&mut socket, &batch).await.is_err() {
            return;
        }
    }
}

/// Sends events as JSON arrays of at most [`BATCH_MAX`] events each.
async fn send_chunked(socket: &mut WebSocket, events: &[Arc<Event>]) -> Result<(), ()> {
    for chunk in events.chunks(BATCH_MAX) {
        send(socket, serde_json::to_string(chunk).expect("events always serialize")).await?;
    }
    Ok(())
}

async fn send(socket: &mut WebSocket, json: String) -> Result<(), ()> {
    match tokio::time::timeout(SEND_TIMEOUT, socket.send(Message::Text(json.into()))).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(()),
    }
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    #[serde(default)]
    since: u64,
}

pub async fn events(State(state): State<AppState>, Query(q): Query<EventsQuery>) -> Json<Vec<Arc<Event>>> {
    Json(state.engine.bus().recent_since(q.since))
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
    // Bodies come from any server the user tested. Should one be opened directly
    // in a browser, the sandbox keeps an HTML body from running scripts on this origin.
    let headers = [(header::CONTENT_TYPE, content_type), (header::CONTENT_SECURITY_POLICY, "sandbox".to_string())];
    Ok((headers, Body::from(blob.data)).into_response())
}
