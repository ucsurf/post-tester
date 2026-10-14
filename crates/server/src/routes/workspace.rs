//! Workspace: requests, folders, environments, variables and settings.

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use post_tester_core::Workspace;
use post_tester_core::model::{Environment, FolderSettings, Request};
use post_tester_core::workspace::Folder;
use serde::{Deserialize, Serialize};

fn workspace(state: &AppState) -> ApiResult<Workspace> {
    Ok(state.engine.require_workspace()?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    root: String,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    version: &'static str,
    workspace: Option<WorkspaceInfo>,
    environments: Vec<String>,
    default_environment: Option<String>,
    secret_store: String,
    mcp_url: String,
}

pub async fn info(State(state): State<AppState>) -> ApiResult<Json<Info>> {
    let ws = state.engine.workspace();
    let settings = ws.as_ref().map(Workspace::settings).transpose()?;
    Ok(Json(Info {
        version: post_tester_core::VERSION,
        workspace: ws.as_ref().zip(settings.as_ref()).map(|(w, s)| WorkspaceInfo {
            root: w.root().to_string_lossy().into_owned(),
            name: if s.name.is_empty() { "Workspace".into() } else { s.name.clone() },
        }),
        environments: ws.as_ref().map(Workspace::environments).transpose()?.unwrap_or_default(),
        default_environment: settings.and_then(|s| s.default_environment),
        secret_store: state.engine.secrets().name().to_string(),
        mcp_url: format!("{}/mcp", state.base_url),
    }))
}

pub async fn tree(State(state): State<AppState>) -> ApiResult<Json<Folder>> {
    Ok(Json(workspace(&state)?.tree()?))
}

pub async fn get_request(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Request>> {
    Ok(Json(workspace(&state)?.read_request(&id)?))
}

pub async fn put_request(State(state): State<AppState>, Path(id): Path<String>, Json(request): Json<Request>) -> ApiResult<StatusCode> {
    workspace(&state)?.write_request(&id, &request)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_request(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    workspace(&state)?.delete_request(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_folder_settings(State(state): State<AppState>, id: Option<Path<String>>) -> ApiResult<Json<FolderSettings>> {
    let id = id.map(|Path(id)| id).unwrap_or_default();
    Ok(Json(workspace(&state)?.read_folder_settings(&id)?))
}

pub async fn put_folder_settings(
    State(state): State<AppState>,
    id: Option<Path<String>>,
    Json(settings): Json<FolderSettings>,
) -> ApiResult<StatusCode> {
    let id = id.map(|Path(id)| id).unwrap_or_default();
    workspace(&state)?.write_folder_settings(&id, &settings)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_environments(State(state): State<AppState>) -> ApiResult<Json<Vec<String>>> {
    Ok(Json(workspace(&state)?.environments()?))
}

pub async fn get_environment(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<Environment>> {
    Ok(Json(workspace(&state)?.read_environment(&name)?))
}

pub async fn put_environment(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(env): Json<Environment>,
) -> ApiResult<StatusCode> {
    workspace(&state)?.write_environment(&name, &env)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_environment(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<StatusCode> {
    workspace(&state)?.delete_environment(&name)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct VariablesQuery {
    environment: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Variable {
    name: String,
    value: String,
}

pub async fn variables(State(state): State<AppState>, Query(q): Query<VariablesQuery>) -> ApiResult<Json<Vec<Variable>>> {
    let environment = state.engine.resolve_environment(q.environment);
    let vars = state.engine.variables(environment.as_deref(), &[])?;
    Ok(Json(vars.flatten().into_iter().map(|(name, value)| Variable { name, value }).collect()))
}

pub async fn clear_runtime_variables(State(state): State<AppState>) -> StatusCode {
    state.engine.clear_runtime_variables();
    StatusCode::NO_CONTENT
}

#[derive(Debug, Deserialize)]
pub struct DefaultEnvironment {
    environment: Option<String>,
}

pub async fn set_default_environment(State(state): State<AppState>, Json(body): Json<DefaultEnvironment>) -> ApiResult<StatusCode> {
    let ws = workspace(&state)?;
    if let Some(name) = &body.environment
        && !ws.environments()?.contains(name)
    {
        return Err(ApiError::not_found(format!("environment `{name}` does not exist")));
    }
    let mut settings = ws.settings()?;
    settings.default_environment = body.environment;
    ws.save_settings(&settings)?;
    Ok(StatusCode::NO_CONTENT)
}
