//! The built-in tools agents use. Descriptions matter: agents choose tools
//! and fill arguments from them alone.

use super::{McpServer, ServerOptions, activity};
use post_tester_core::engine::Execution;
use post_tester_core::events::{Level, Outcome, Span};
use post_tester_core::model::Request;
use post_tester_core::runner::{RunOptions, RunReport, RunTarget};
use post_tester_core::workspace::Folder;
use post_tester_core::{Error, Result, SendOptions};
use rmcp::model::{Tool, ToolAnnotations};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

/// Response bodies longer than this are cut, so they fit in an agent's context.
const MAX_BODY: usize = 32 * 1024;
const DEFAULT_ACTIVITY: usize = 20;
const MAX_ACTIVITY: usize = 200;

const GET_WORKSPACE: &str = "get_workspace";
const GET_REQUEST: &str = "get_request";
const SEND_REQUEST: &str = "send_request";
const RUN_COLLECTION: &str = "run_collection";
const SAVE_REQUEST: &str = "save_request";
const LIST_ENVIRONMENTS: &str = "list_environments";
const GET_RECENT_ACTIVITY: &str = "get_recent_activity";
const LIST_PLUGINS: &str = "list_plugins";

/// Describes the request object accepted by `send_request` and `save_request`.
const REQUEST_SHAPE: &str = "A request object, the same shape `get_request` returns. Fields: `method` (GET, POST, …), \
`url` (may use {{variables}}), `query` and `headers` (lists of {key, value, enabled?}), `body` \
({type: \"json\", content: \"<JSON text>\"} | {type: \"text\", content, content_type} | {type: \"form\", fields: [{key, value}]} | \
{type: \"graphql\", query, variables?}), `auth` ({type: \"bearer\", token} | {type: \"basic\", username, password} | \
{type: \"api_key\", key, value, location: \"header\"|\"query\"} | {type: \"none\"}; omit to inherit from the folder), \
`tests` (assertions such as \"status == 200\", \"json($.id) exists\", \"time < 500\"), `extract` (variables to capture, \
e.g. {\"token\": \"json($.token)\"}), plus optional `name` and `description`. Use {{secret:name}} for secret values.";

fn tool(name: &'static str, description: &'static str, schema: Value, annotations: ToolAnnotations) -> Tool {
    let Value::Object(schema) = schema else { unreachable!("tool schemas are JSON objects") };
    Tool::new(name, description, schema).with_annotations(annotations)
}

fn read_only(title: &str) -> ToolAnnotations {
    ToolAnnotations::with_title(title).read_only(true).open_world(false)
}

fn no_arguments() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// The built-in tools, as listed to agents.
pub(super) fn builtin(options: &ServerOptions, has_plugins: bool) -> Vec<Tool> {
    let mut tools = vec![
        tool(
            GET_WORKSPACE,
            "Start here. Returns the open post-tester workspace: its name and folder, the environments and the \
             default environment, the names of session variables, and the collection tree of saved requests \
             (folder and request ids, names, methods and URLs). Request ids (e.g. `auth/login`) are what the other \
             tools take.",
            no_arguments(),
            read_only("Get workspace"),
        ),
        tool(
            GET_REQUEST,
            "Returns one saved request in full: method, URL, query, headers, body, auth, tests and extraction rules, \
             plus the settings it inherits from its folders (shared headers, auth, variables). Read a request before \
             sending or changing it.",
            json!({
                "type": "object",
                "properties": { "id": { "type": "string", "description": "Request id from get_workspace, e.g. `auth/login`." } },
                "required": ["id"]
            }),
            read_only("Get request"),
        ),
        tool(
            SEND_REQUEST,
            "Sends an HTTP or GraphQL request through post-tester and returns the result: status, timing, response \
             headers and body (text, cut at 32 KB), test results, and variables extracted from the response. Give \
             `id` to send a saved request, or `request` to send an ad-hoc one (add `id` too to inherit that \
             request's folder settings). Variables, secrets, folder auth and plugins are applied exactly as when \
             the user clicks Send, and the user sees the request live. Extracted variables (e.g. a login token) \
             are kept for later requests.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Id of a saved request, e.g. `users/list`." },
                    "request": { "type": "object", "description": REQUEST_SHAPE },
                    "environment": { "type": "string", "description": "Environment name. Omit to use the workspace default." }
                }
            }),
            ToolAnnotations::with_title("Send request").read_only(false).destructive(false).open_world(true),
        ),
        tool(
            RUN_COLLECTION,
            "Runs every saved request in a folder (including sub-folders) in order, like a test suite, and returns \
             a summary: per request the outcome (ok, failed = a test failed, error = no response), status, duration \
             and failed tests. Use it as a regression check after changing an API.",
            json!({
                "type": "object",
                "properties": {
                    "folder": { "type": "string", "description": "Folder id from get_workspace, e.g. `auth`. Use \"\" to run every request in the workspace.", "default": "" },
                    "environment": { "type": "string", "description": "Environment name. Omit to use the workspace default." },
                    "bail": { "type": "boolean", "description": "Stop at the first failing request.", "default": false }
                }
            }),
            ToolAnnotations::with_title("Run collection").read_only(false).destructive(false).open_world(true),
        ),
    ];
    if !options.read_only {
        tools.push(tool(
            SAVE_REQUEST,
            "Creates or replaces a saved request in the user's workspace (a YAML file the user may commit). Only \
             use it when the user asked to create or change a request. To change one, read it with get_request \
             first and send back the full object with your edits.",
            json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Request id: folders and a file name separated by `/`, e.g. `users/create-user`. Letters, numbers, spaces, `-`, `_` and `.` only." },
                    "request": { "type": "object", "description": REQUEST_SHAPE }
                },
                "required": ["id", "request"]
            }),
            ToolAnnotations::with_title("Save request").read_only(false).destructive(true).idempotent(true).open_world(false),
        ));
    }
    tools.extend([
        tool(
            LIST_ENVIRONMENTS,
            "Lists the environments (e.g. local, staging, production) and the names of their variables. Values are \
             hidden unless `include_values` is true. Secrets are never stored in environments: they appear as \
             {{secret:name}} references.",
            json!({
                "type": "object",
                "properties": { "include_values": { "type": "boolean", "description": "Also return variable values.", "default": false } }
            }),
            read_only("List environments"),
        ),
        tool(
            GET_RECENT_ACTIVITY,
            "Returns what happened recently in post-tester, newest first: requests and runs by the user, AI agents, \
             the CLI or plugins, with source, title, method and URL, status, outcome and duration. Use it to see \
             what the user just tried, or to find a failing request.",
            json!({
                "type": "object",
                "properties": { "limit": { "type": "integer", "description": "How many entries to return (1–200).", "default": DEFAULT_ACTIVITY, "minimum": 1, "maximum": MAX_ACTIVITY } }
            }),
            read_only("Recent activity"),
        ),
    ]);
    if has_plugins {
        tools.push(tool(
            LIST_PLUGINS,
            "Lists the installed post-tester plugins and what each provides: template functions, auth methods, \
             hooks, importers, code generators, viewers and MCP tools, plus their permissions.",
            no_arguments(),
            read_only("List plugins"),
        ));
    }
    tools
}

/// True when `name` is a built-in tool currently offered.
pub(super) fn is_builtin(name: &str, has_plugins: bool) -> bool {
    match name {
        GET_WORKSPACE | GET_REQUEST | SEND_REQUEST | RUN_COLLECTION | LIST_ENVIRONMENTS | GET_RECENT_ACTIVITY => true,
        // Still accepted when read-only, so the agent gets a clear refusal.
        SAVE_REQUEST => true,
        LIST_PLUGINS => has_plugins,
        _ => false,
    }
}

/// Runs a built-in tool.
pub(super) async fn call(server: &McpServer, name: &str, arguments: Value, span: &Span) -> Result<Value> {
    match name {
        GET_WORKSPACE => get_workspace(server),
        GET_REQUEST => get_request(server, &parse(name, arguments)?),
        SEND_REQUEST => send_request(server, parse(name, arguments)?, span).await,
        RUN_COLLECTION => run_collection(server, parse(name, arguments)?, span).await,
        SAVE_REQUEST => save_request(server, parse(name, arguments)?, span),
        LIST_ENVIRONMENTS => list_environments(server, &parse(name, arguments)?),
        GET_RECENT_ACTIVITY => {
            let args: ActivityArgs = parse(name, arguments)?;
            let limit = args.limit.unwrap_or(DEFAULT_ACTIVITY).clamp(1, MAX_ACTIVITY);
            Ok(activity::recent(server.engine.bus(), limit, Some(span.id())))
        }
        LIST_PLUGINS => list_plugins(server),
        _ => Err(Error::Mcp(format!("unknown tool `{name}`"))),
    }
}

fn parse<T: DeserializeOwned>(tool: &str, arguments: Value) -> Result<T> {
    serde_json::from_value(arguments).map_err(|e| Error::InvalidRequest(format!("invalid arguments for `{tool}`: {e}")))
}

fn parse_request(value: Value) -> Result<Request> {
    serde_json::from_value(value).map_err(|e| Error::InvalidRequest(format!("`request` is not a valid request: {e}. {REQUEST_SHAPE}")))
}

fn to_json(value: impl serde::Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| Error::Mcp(format!("could not encode the result: {e}")))
}

// ── Workspace ───────────────────────────────────────────────────────────────

fn get_workspace(server: &McpServer) -> Result<Value> {
    let workspace = server.engine.require_workspace()?;
    let settings = workspace.settings()?;
    let mut variables: Vec<String> = server.engine.runtime_variables().into_keys().collect();
    variables.sort();
    Ok(json!({
        "name": settings.name,
        "root": workspace.root().to_string_lossy(),
        "environments": workspace.environments()?,
        "defaultEnvironment": settings.default_environment,
        "workspaceVariables": settings.variables.keys().collect::<Vec<_>>(),
        "sessionVariables": variables,
        "collections": folder_json(&workspace.tree()?),
    }))
}

/// A compact collection tree: only what an agent needs to pick a request.
fn folder_json(folder: &Folder) -> Value {
    let requests: Vec<Value> = folder
        .requests
        .iter()
        .map(|r| {
            let mut entry = json!({ "id": r.id, "name": r.name, "method": r.method, "url": r.url });
            if let Some(error) = &r.error {
                entry["error"] = Value::from(error.as_str());
            }
            entry
        })
        .collect();
    let mut value = json!({
        "id": folder.id,
        "name": folder.name,
        "requests": requests,
        "folders": folder.folders.iter().map(folder_json).collect::<Vec<_>>(),
    });
    if !folder.settings.description.is_empty() {
        value["description"] = Value::from(folder.settings.description.as_str());
    }
    value
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdArgs {
    id: String,
}

fn get_request(server: &McpServer, args: &IdArgs) -> Result<Value> {
    let workspace = server.engine.require_workspace()?;
    let request = workspace.read_request(&args.id)?;
    let folders: Vec<Value> = workspace
        .folder_chain(&args.id)?
        .into_iter()
        .filter_map(|settings| to_json(settings).ok())
        .filter(|settings| settings.as_object().is_some_and(|o| !o.is_empty()))
        .collect();
    Ok(json!({ "id": args.id, "request": to_json(&request)?, "inheritedFromFolders": folders }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveArgs {
    id: String,
    request: Value,
}

fn save_request(server: &McpServer, args: SaveArgs, span: &Span) -> Result<Value> {
    if server.options.read_only {
        return Err(Error::Mcp(
            "save_request is disabled: post-tester's MCP server runs in read-only mode. Show the user the request \
             instead, so they can save it themselves"
                .into(),
        ));
    }
    let request = parse_request(args.request)?;
    let workspace = server.engine.require_workspace()?;
    let created = !workspace.request_exists(&args.id);
    workspace.write_request(&args.id, &request)?;
    span.log(Level::Info, format!("{} request `{}`", if created { "Created" } else { "Updated" }, args.id));
    Ok(json!({ "id": args.id, "saved": true, "created": created }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentArgs {
    #[serde(default)]
    include_values: bool,
}

fn list_environments(server: &McpServer, args: &EnvironmentArgs) -> Result<Value> {
    let workspace = server.engine.require_workspace()?;
    let mut environments = Vec::new();
    for name in workspace.environments()? {
        let environment = workspace.read_environment(&name)?;
        let variables = if args.include_values {
            Value::Object(environment.variables)
        } else {
            Value::from(environment.variables.keys().cloned().collect::<Vec<_>>())
        };
        environments.push(json!({ "name": name, "description": environment.description, "variables": variables }));
    }
    Ok(json!({ "default": workspace.settings()?.default_environment, "environments": environments }))
}

fn list_plugins(server: &McpServer) -> Result<Value> {
    let plugins: Vec<_> = server.plugins.iter().flat_map(|host| host.plugins()).map(|p| p.info()).collect();
    to_json(plugins)
}

// ── Running ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    id: Option<String>,
    request: Option<Value>,
    environment: Option<String>,
}

async fn send_request(server: &McpServer, args: SendArgs, span: &Span) -> Result<Value> {
    let mut options = SendOptions::new(span.source().clone()).environment(args.environment.filter(|e| !e.is_empty()));
    // The request is a child of this tool call, so Live Activity shows them together.
    options.parent = Some(span.clone());
    let (execution, url_template) = match (args.id, args.request) {
        (id, Some(request)) => {
            let request = parse_request(request)?;
            let url = request.url.clone();
            // With an id, the ad-hoc request inherits that request's folder settings.
            options.request_id = id;
            (server.engine.send(&request, options).await, url)
        }
        (Some(id), None) => {
            let url = server.engine.require_workspace()?.read_request(&id)?.url;
            (server.engine.send_saved(&id, options).await?, url)
        }
        (None, None) => {
            return Err(Error::InvalidRequest(
                "give either `id` (a saved request, see get_workspace) or `request` (an inline request)".into(),
            ));
        }
    };
    Ok(execution_json(&execution, &url_template))
}

/// What an agent needs to know about one execution.
fn execution_json(execution: &Execution, url_template: &str) -> Value {
    let failed = execution.tests.iter().filter(|t| !t.passed).count();
    let mut result = json!({
        "spanId": execution.span_id,
        "outcome": execution.outcome,
        "error": execution.error,
        "tests": execution.tests,
        "testSummary": format!("{} passed, {failed} failed", execution.tests.len() - failed),
        "extractedVariables": execution.extracted.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect::<Map<_, _>>(),
    });
    if let Some(request) = &execution.request {
        // The final URL may contain resolved secrets; show the template then.
        let url = if url_template.contains("{{secret:") { url_template.to_string() } else { request.url.to_string() };
        result["request"] = json!({ "method": request.method, "url": url });
    }
    if let Some(response) = &execution.response {
        let (body, note) = body_text(&response.body, response.header("content-type"));
        let mut headers = Map::new();
        for (name, value) in &response.headers {
            let joined = match headers.get(name).and_then(Value::as_str) {
                Some(previous) => format!("{previous}, {value}"),
                None => value.clone(),
            };
            headers.insert(name.clone(), Value::from(joined));
        }
        result["status"] = Value::from(response.status);
        result["statusText"] = Value::from(response.status_text.as_str());
        result["timeMs"] = Value::from((post_tester_core::events::millis(response.total) * 10.0).round() / 10.0);
        result["size"] = Value::from(response.body.len());
        result["headers"] = Value::Object(headers);
        result["body"] = Value::from(body);
        if let Some(note) = note {
            result["bodyNote"] = Value::from(note);
        }
    }
    result
}

/// The body as text, cut at [`MAX_BODY`], plus a note when it was cut or is binary.
fn body_text(body: &[u8], content_type: Option<&str>) -> (String, Option<String>) {
    let Ok(text) = std::str::from_utf8(body) else {
        let kind = content_type.unwrap_or("unknown type");
        return (String::new(), Some(format!("binary body ({kind}, {} bytes) not shown", body.len())));
    };
    if text.len() <= MAX_BODY {
        return (text.to_string(), None);
    }
    let mut end = MAX_BODY;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let note = format!("body truncated: showing the first {end} of {} bytes", text.len());
    (text[..end].to_string(), Some(note))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    #[serde(default)]
    folder: String,
    environment: Option<String>,
    #[serde(default)]
    bail: bool,
}

async fn run_collection(server: &McpServer, args: RunArgs, span: &Span) -> Result<Value> {
    let mut options = RunOptions::new(span.source().clone());
    options.environment = args.environment.filter(|e| !e.is_empty());
    options.bail = args.bail;
    let report = server.engine.run(RunTarget::Folder(args.folder), options).await?;
    span.log(Level::Info, format!("Ran \"{}\": {:?}", report.name, report.outcome));
    Ok(report_json(&report))
}

fn report_json(report: &RunReport) -> Value {
    let items: Vec<Value> = report
        .items
        .iter()
        .map(|item| {
            let failed_tests: Vec<Value> =
                item.tests.iter().filter(|t| !t.passed).map(|t| json!({ "name": t.name, "detail": t.detail })).collect();
            json!({
                "requestId": item.request_id,
                "name": item.name,
                "outcome": item.outcome,
                "status": item.status,
                "durationMs": item.duration_ms.round(),
                "failedTests": failed_tests,
                "error": item.error,
            })
        })
        .collect();
    json!({
        "spanId": report.span_id,
        "name": report.name,
        "outcome": report.outcome,
        "durationMs": report.duration_ms.round(),
        "requests": report.items.len(),
        "ok": report.count(Outcome::Ok),
        "failed": report.count(Outcome::Failed),
        "errors": report.count(Outcome::Error),
        "testsTotal": report.tests_total(),
        "testsFailed": report.tests_failed(),
        "items": items,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivityArgs {
    limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_bodies_are_cut_on_a_character_boundary() {
        let text = "é".repeat(MAX_BODY);
        let (body, note) = body_text(text.as_bytes(), Some("text/plain"));
        assert!(body.len() <= MAX_BODY);
        assert!(note.unwrap().contains("truncated"));
        let (_, note) = body_text(&[0xff, 0xfe], Some("image/png"));
        assert!(note.unwrap().contains("binary"));
    }

    #[test]
    fn read_only_hides_save_request() {
        let names = |options: &ServerOptions| builtin(options, false).into_iter().map(|t| t.name.to_string()).collect::<Vec<_>>();
        assert!(names(&ServerOptions::default()).contains(&SAVE_REQUEST.to_string()));
        assert!(!names(&ServerOptions { read_only: true }).contains(&SAVE_REQUEST.to_string()));
    }
}
