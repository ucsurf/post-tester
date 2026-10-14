//! MCP client endpoints: connect to an MCP server and call it like an API.

use crate::error::ApiResult;
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use post_tester_core::events::Source;
use post_tester_mcp::{ConnectOptions, SessionInfo};
use serde::Deserialize;
use serde_json::Value;

pub async fn connect(State(state): State<AppState>, Json(options): Json<ConnectOptions>) -> ApiResult<Json<SessionInfo>> {
    Ok(Json(state.mcp.connect(options, Source::User).await?))
}

pub async fn sessions(State(state): State<AppState>) -> Json<Vec<SessionInfo>> {
    Json(state.mcp.sessions())
}

pub async fn refresh(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<SessionInfo>> {
    Ok(Json(state.mcp.refresh(&id).await?))
}

pub async fn close(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    state.mcp.close(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct CallBody {
    tool: String,
    #[serde(default)]
    arguments: Value,
}

pub async fn call_tool(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<CallBody>) -> ApiResult<Json<Value>> {
    let arguments = if body.arguments.is_null() { Value::Object(Default::default()) } else { body.arguments };
    Ok(Json(state.mcp.call_tool(&id, &body.tool, arguments, Source::User).await?))
}

#[derive(Debug, Deserialize)]
pub struct ReadBody {
    uri: String,
}

pub async fn read_resource(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<ReadBody>) -> ApiResult<Json<Value>> {
    Ok(Json(state.mcp.read_resource(&id, &body.uri, Source::User).await?))
}

#[derive(Debug, Deserialize)]
pub struct PromptBody {
    name: String,
    #[serde(default)]
    arguments: serde_json::Map<String, Value>,
}

pub async fn get_prompt(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<PromptBody>) -> ApiResult<Json<Value>> {
    Ok(Json(state.mcp.get_prompt(&id, &body.name, body.arguments, Source::User).await?))
}
