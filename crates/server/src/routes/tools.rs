//! Code generation, import, plugins, viewers, template functions, secrets,
//! cookies and WebSocket client sessions.

use super::run::SendBody;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use post_tester_core::events::{Source, SpanId};
use post_tester_core::extensions::ScriptResponse;
use post_tester_core::import::{self, ImportSummary};
use post_tester_core::ws::WsOptions;
use post_tester_core::{codegen, template};
use post_tester_plugin_host::PluginInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct CodegenTarget {
    id: String,
    name: String,
    language: String,
}

pub async fn codegen_targets(State(state): State<AppState>) -> Json<Vec<CodegenTarget>> {
    let built_in = codegen::GENERATORS.iter().map(|g| CodegenTarget { id: g.id.into(), name: g.name.into(), language: g.language.into() });
    let plugins = state.plugins.codegen_targets().into_iter().map(|(id, info)| CodegenTarget {
        id,
        name: info.display_name,
        language: info.language,
    });
    Json(built_in.chain(plugins).collect())
}

#[derive(Debug, Deserialize)]
pub struct CodegenBody {
    #[serde(flatten)]
    send: SendBody,
    target: String,
}

#[derive(Debug, Serialize)]
pub struct Code {
    code: String,
    language: String,
}

pub async fn generate_code(State(state): State<AppState>, Json(body): Json<CodegenBody>) -> ApiResult<Json<Code>> {
    let request = state.engine.preview(&body.send.request, body.send.request_id.as_deref(), body.send.environment).await?;
    if let Some(generator) = codegen::find(&body.target) {
        return Ok(Json(Code { code: generator.generate(&request), language: generator.language.into() }));
    }
    let language = state
        .plugins
        .codegen_targets()
        .into_iter()
        .find(|(id, _)| id == &body.target)
        .map(|(_, info)| info.language)
        .ok_or_else(|| ApiError::not_found(format!("no code generator `{}`", body.target)))?;
    Ok(Json(Code { code: state.plugins.codegen(&body.target, &request).await?, language }))
}

#[derive(Debug, Deserialize)]
pub struct ImportBody {
    text: String,
    #[serde(default)]
    into: Option<String>,
}

pub async fn import(State(state): State<AppState>, Json(body): Json<ImportBody>) -> ApiResult<Json<ImportSummary>> {
    let workspace = state.engine.require_workspace()?;
    let imported = match import::detect(&body.text) {
        Some(format) => import::import(&body.text, Some(format))?,
        None => match state.plugins.import(&body.text).await {
            Some(result) => result?,
            None => import::import(&body.text, None)?, // produces the "unrecognized format" explanation
        },
    };
    Ok(Json(imported.write_to(&workspace, body.into.as_deref().unwrap_or(""))?))
}

pub async fn plugins(State(state): State<AppState>) -> Json<Vec<PluginInfo>> {
    Json(state.plugins.plugins().iter().map(|p| p.info()).collect())
}

pub async fn reload_plugins(State(state): State<AppState>) -> ApiResult<Json<Vec<PluginInfo>>> {
    let workspace = state.engine.require_workspace()?;
    for plugin in state.plugins.plugins() {
        state.plugins.unload(&plugin.manifest.name);
    }
    state.plugins.load_workspace(&workspace).await;
    Ok(plugins(State(state)).await)
}

/// Approves every permission the plugin declares, then reloads it.
pub async fn approve_plugin(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<Json<PluginInfo>> {
    let workspace = state.engine.require_workspace()?;
    let plugin = state.plugins.get(&name).ok_or_else(|| ApiError::not_found(format!("plugin `{name}` is not loaded")))?;
    let mut settings = workspace.settings()?;
    settings.approved_permissions.insert(name.clone(), plugin.manifest.permissions.clone());
    workspace.save_settings(&settings)?;
    let reloaded = state
        .plugins
        .load(&plugin.dir, plugin.manifest.permissions.clone())
        .await
        .map_err(|problems| ApiError::bad_request(problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")))?;
    Ok(Json(reloaded.info()))
}

#[derive(Debug, Deserialize)]
pub struct ViewerBody {
    viewer: String,
    response: ScriptResponse,
}

#[derive(Debug, Serialize)]
pub struct Html {
    html: String,
}

pub async fn render_viewer(State(state): State<AppState>, Json(body): Json<ViewerBody>) -> ApiResult<Json<Html>> {
    Ok(Json(Html { html: state.plugins.render_viewer(&body.viewer, &body.response).await? }))
}

#[derive(Debug, Serialize)]
pub struct TemplateFunction {
    name: String,
    description: String,
    plugin: Option<String>,
}

pub async fn template_functions(State(state): State<AppState>) -> Json<Vec<TemplateFunction>> {
    let built_in = template::BUILTINS.iter().map(|(name, description)| TemplateFunction {
        name: name.to_string(),
        description: description.to_string(),
        plugin: None,
    });
    let plugins = state.plugins.plugins();
    let from_plugins = plugins.iter().flat_map(|p| {
        p.capabilities.template_functions.iter().map(|f| TemplateFunction {
            name: f.name.clone(),
            description: f.description.clone(),
            plugin: Some(p.manifest.name.clone()),
        })
    });
    Json(built_in.chain(from_plugins).collect())
}

#[derive(Debug, Deserialize)]
pub struct SecretBody {
    value: String,
}

pub async fn set_secret(State(state): State<AppState>, Path(name): Path<String>, Json(body): Json<SecretBody>) -> ApiResult<StatusCode> {
    state.engine.secrets().set(&name, &body.value).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_secret(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<StatusCode> {
    state.engine.secrets().delete(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub struct Cookie {
    domain: String,
    name: String,
    value: String,
}

pub async fn cookies(State(state): State<AppState>) -> Json<Vec<Cookie>> {
    Json(state.engine.cookies().list().into_iter().map(|(domain, name, value)| Cookie { domain, name, value }).collect())
}

pub async fn clear_cookies(State(state): State<AppState>) -> StatusCode {
    state.engine.cookies().clear();
    StatusCode::NO_CONTENT
}

#[derive(Debug, Deserialize)]
pub struct WsConnectBody {
    url: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    environment: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WsSession {
    session_id: SpanId,
}

pub async fn ws_connect(State(state): State<AppState>, Json(body): Json<WsConnectBody>) -> ApiResult<Json<WsSession>> {
    let options = WsOptions { url: body.url, headers: body.headers, source: Source::User, environment: body.environment };
    Ok(Json(WsSession { session_id: state.engine.ws_connect(options).await? }))
}

#[derive(Debug, Deserialize)]
pub struct WsSendBody {
    text: String,
}

fn span_id(text: &str) -> ApiResult<SpanId> {
    text.parse().map_err(|_| ApiError::bad_request(format!("`{text}` is not a session id")))
}

pub async fn ws_send(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<WsSendBody>) -> ApiResult<StatusCode> {
    state.engine.ws_send(span_id(&id)?, body.text)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ws_close(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    if state.engine.cancel(span_id(&id)?) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("this WebSocket session is not open"))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTarget {
    id: String,
    name: String,
    file_extension: String,
}

pub async fn export_targets(State(state): State<AppState>) -> Json<Vec<ExportTarget>> {
    let targets = state.plugins.export_targets().into_iter();
    Json(targets.map(|(id, info)| ExportTarget { id, name: info.display_name, file_extension: info.file_extension }).collect())
}

#[derive(Debug, Deserialize)]
pub struct ExportBody {
    /// Folder id (`""` for the whole workspace).
    #[serde(default)]
    folder: String,
    exporter: String,
}

#[derive(Debug, Serialize)]
pub struct Exported {
    content: String,
}

pub async fn export(State(state): State<AppState>, Json(body): Json<ExportBody>) -> ApiResult<Json<Exported>> {
    let workspace = state.engine.require_workspace()?;
    let tree = workspace.tree()?;
    let folder = tree.find(&body.folder).ok_or_else(|| ApiError::not_found(format!("folder `{}`", body.folder)))?;
    let requests =
        folder.request_ids().into_iter().map(|id| workspace.read_request(&id).map(|r| (id, r))).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(Exported { content: state.plugins.export(&body.exporter, &folder.name, &requests).await? }))
}
