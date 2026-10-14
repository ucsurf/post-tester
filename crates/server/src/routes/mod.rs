//! HTTP routes. See `crates/server/API.md` for the contract.

pub mod live;
pub mod mcp;
pub mod run;
pub mod tools;
pub mod workspace;

use crate::state::AppState;
use axum::Router;
use axum::routing::{delete, get, post, put};

pub fn api() -> Router<AppState> {
    Router::new()
        // Workspace
        .route("/api/info", get(workspace::info))
        .route("/api/tree", get(workspace::tree))
        .route("/api/requests/{*id}", get(workspace::get_request).put(workspace::put_request).delete(workspace::delete_request))
        .route("/api/folders/settings/", get(workspace::get_folder_settings).put(workspace::put_folder_settings))
        .route("/api/folders/settings/{*id}", get(workspace::get_folder_settings).put(workspace::put_folder_settings))
        .route("/api/environments", get(workspace::list_environments))
        .route(
            "/api/environments/{name}",
            get(workspace::get_environment).put(workspace::put_environment).delete(workspace::delete_environment),
        )
        .route("/api/variables", get(workspace::variables))
        .route("/api/variables/runtime", delete(workspace::clear_runtime_variables))
        .route("/api/settings/default-environment", put(workspace::set_default_environment))
        // Running
        .route("/api/send", post(run::send))
        .route("/api/preview", post(run::preview))
        .route("/api/run", post(run::run))
        .route("/api/load-test", post(run::load_test))
        .route("/api/cancel/{span_id}", post(run::cancel))
        // Live
        .route("/api/live", get(live::live))
        .route("/api/events", get(live::events))
        .route("/api/blobs/{id}", get(live::blob))
        // Tools
        .route("/api/codegen/targets", get(tools::codegen_targets))
        .route("/api/codegen", post(tools::generate_code))
        .route("/api/import", post(tools::import))
        .route("/api/plugins", get(tools::plugins))
        .route("/api/plugins/reload", post(tools::reload_plugins))
        .route("/api/plugins/{name}/approve", post(tools::approve_plugin))
        .route("/api/viewers/render", post(tools::render_viewer))
        .route("/api/template-functions", get(tools::template_functions))
        .route("/api/secrets/{name}", put(tools::set_secret).delete(tools::delete_secret))
        .route("/api/cookies", get(tools::cookies).delete(tools::clear_cookies))
        .route("/api/ws/connect", post(tools::ws_connect))
        .route("/api/ws/{id}/send", post(tools::ws_send))
        .route("/api/ws/{id}/close", post(tools::ws_close))
        // MCP client (testing MCP servers)
        .route("/api/mcp/connect", post(mcp::connect))
        .route("/api/mcp/sessions", get(mcp::sessions))
        .route("/api/mcp/{id}", delete(mcp::close))
        .route("/api/mcp/{id}/refresh", post(mcp::refresh))
        .route("/api/mcp/{id}/call", post(mcp::call_tool))
        .route("/api/mcp/{id}/read", post(mcp::read_resource))
        .route("/api/mcp/{id}/prompt", post(mcp::get_prompt))
}
