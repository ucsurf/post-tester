//! Sending, running collections, load tests, cancelling and previewing.

use crate::error::ApiResult;
use crate::state::AppState;
use crate::views::{ExecutionView, PreparedView};
use axum::Json;
use axum::extract::{Path, State};
use post_tester_core::SendOptions;
use post_tester_core::events::{Source, SpanId};
use post_tester_core::loadtest::{Connections, LoadOptions, LoadReport};
use post_tester_core::model::Request;
use post_tester_core::runner::{RunOptions, RunReport, RunTarget};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendBody {
    pub request: Request,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
}

pub async fn send(State(state): State<AppState>, Json(body): Json<SendBody>) -> Json<ExecutionView> {
    let mut options = SendOptions::new(Source::User).environment(body.environment);
    options.request_id = body.request_id;
    let execution = state.engine.send(&body.request, options).await;
    Json(ExecutionView::new(&execution, state.engine.bus()))
}

pub async fn preview(State(state): State<AppState>, Json(body): Json<SendBody>) -> ApiResult<Json<PreparedView>> {
    let prepared = state.engine.preview(&body.request, body.request_id.as_deref(), body.environment).await?;
    Ok(Json(PreparedView::from(&prepared)))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetBody {
    Folder(String),
    Requests(Vec<String>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBody {
    target: TargetBody,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    bail: bool,
    #[serde(default)]
    iterations: Option<u32>,
    #[serde(default)]
    delay_ms: Option<u64>,
}

pub async fn run(State(state): State<AppState>, Json(body): Json<RunBody>) -> ApiResult<Json<RunReport>> {
    let target = match body.target {
        TargetBody::Folder(id) => RunTarget::Folder(id),
        TargetBody::Requests(ids) => RunTarget::Requests(ids),
    };
    let options = RunOptions {
        source: Source::User,
        environment: body.environment,
        bail: body.bail,
        iterations: body.iterations.unwrap_or(1),
        delay: Duration::from_millis(body.delay_ms.unwrap_or(0)),
    };
    Ok(Json(state.engine.run(target, options).await?))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBody {
    request: Request,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    concurrency: u32,
    requests: u32,
    #[serde(default)]
    connections: ConnectionsBody,
}

/// `"reuse"` (default): keep connections open, like a real client.
/// `"fresh"`: a new connection for every request, to measure connection setup too.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionsBody {
    #[default]
    Reuse,
    Fresh,
}

pub async fn load_test(State(state): State<AppState>, Json(body): Json<LoadBody>) -> ApiResult<Json<LoadReport>> {
    let options =
        LoadOptions { source: Source::User, environment: body.environment, concurrency: body.concurrency, requests: body.requests };
    let connections = match body.connections {
        ConnectionsBody::Reuse => Connections::Reuse,
        ConnectionsBody::Fresh => Connections::Fresh,
    };
    Ok(Json(state.engine.load_test_with(&body.request, body.request_id, options, connections).await?))
}

#[derive(Debug, Serialize)]
pub struct Cancelled {
    cancelled: bool,
}

pub async fn cancel(State(state): State<AppState>, Path(span_id): Path<String>) -> ApiResult<Json<Cancelled>> {
    let id: SpanId = span_id.parse().map_err(|_| crate::error::ApiError::bad_request(format!("`{span_id}` is not a span id")))?;
    Ok(Json(Cancelled { cancelled: state.engine.cancel(id) }))
}
