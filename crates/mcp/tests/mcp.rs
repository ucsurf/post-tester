//! End-to-end tests: agents using post-tester's MCP server, and post-tester's
//! MCP client testing an MCP server (its own, over HTTP).

use axum::Router;
use axum::routing::get;
use post_tester_core::events::{Direction, Event, EventKind, Outcome, Source, SpanKind};
use post_tester_core::model::{Environment, Request};
use post_tester_core::secrets::MemorySecrets;
use post_tester_core::{Engine, Workspace};
use post_tester_mcp::{ConnectOptions, McpClients, McpServer, ServerOptions, Transport, http_service};
use rmcp::model::{CallToolRequestParams, CallToolResult, ClientCapabilities, ClientConfig, Implementation};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// A local API for requests to hit.
async fn start_api() -> String {
    let app = Router::new()
        .route("/json", get(|| async { axum::Json(json!({ "token": "abc", "user": { "id": 7 } })) }))
        // Echoes the query string, as some APIs do in errors.
        .route("/echo", get(|axum::extract::RawQuery(query): axum::extract::RawQuery| async move { query.unwrap_or_default() }));
    serve(app).await
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

/// An engine with a workspace holding one request, `api/json`, with tests.
async fn engine(dir: &tempfile::TempDir) -> Arc<Engine> {
    let api = start_api().await;
    let workspace = Workspace::init(dir.path(), "Demo").unwrap();
    let mut local = Environment::default();
    local.variables.insert("baseUrl".into(), api.into());
    workspace.write_environment("local", &local).unwrap();
    workspace
        .write_request(
            "api/json",
            &Request {
                name: "Get JSON".into(),
                url: "{{baseUrl}}/json".into(),
                tests: vec!["status == 200".into(), "json($.user.id) == 7".into()],
                ..Default::default()
            },
        )
        .unwrap();
    Arc::new(Engine::new(Default::default(), Some(workspace), Arc::new(MemorySecrets::default())))
}

type Agent = RunningService<RoleClient, ClientConfig>;

/// Connects an rmcp client named `test-agent` to an in-process server.
async fn agent(engine: Arc<Engine>, options: ServerOptions) -> Agent {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server = McpServer::new(engine, None, options);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.unwrap();
        let _ = running.waiting().await;
    });
    ClientConfig::new(ClientCapabilities::default(), Implementation::new("test-agent", "1.0.0")).serve(client_io).await.unwrap()
}

async fn call(agent: &Agent, tool: &str, arguments: Value) -> CallToolResult {
    let Value::Object(arguments) = arguments else { panic!("arguments must be an object") };
    agent.call_tool(CallToolRequestParams::new(tool.to_string()).with_arguments(arguments)).await.unwrap()
}

/// The JSON a tool returned as text.
fn json_of(result: &CallToolResult) -> Value {
    let text = &result.content[0].as_text().expect("text content").text;
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
}

fn agent_events(engine: &Engine) -> Vec<Arc<Event>> {
    engine.bus().recent().into_iter().filter(|e| e.source == Source::Agent { client: "test-agent".into() }).collect()
}

#[tokio::test]
async fn agents_read_the_workspace_and_send_requests_live() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let agent = agent(engine.clone(), ServerOptions::default()).await;

    let tools: Vec<String> = agent.list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
    for expected in
        ["get_workspace", "get_request", "send_request", "run_collection", "save_request", "list_environments", "get_recent_activity"]
    {
        assert!(tools.contains(&expected.to_string()), "missing tool {expected} in {tools:?}");
    }

    let workspace = json_of(&call(&agent, "get_workspace", json!({})).await);
    assert_eq!(workspace["name"], "Demo");
    assert_eq!(workspace["defaultEnvironment"], "local");
    let api = workspace["collections"]["folders"].as_array().unwrap().iter().find(|f| f["id"] == "api").unwrap();
    assert_eq!(api["requests"][0]["id"], "api/json");

    let result = call(&agent, "send_request", json!({ "id": "api/json" })).await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let sent = json_of(&result);
    assert_eq!(sent["status"], 200);
    assert_eq!(sent["outcome"], "ok");
    assert_eq!(sent["testSummary"], "2 passed, 0 failed");
    assert!(sent["body"].as_str().unwrap().contains("\"token\""));

    // Everything the agent did is on the bus, attributed to it.
    let events = agent_events(&engine);
    let started =
        |kind: SpanKind| events.iter().filter(move |e| matches!(&e.kind, EventKind::Started { kind: k, .. } if *k == kind)).count();
    assert_eq!(started(SpanKind::McpCall), 2, "one span per tool call");
    assert_eq!(started(SpanKind::Request), 1, "the request the agent sent");
    let request = events.iter().find(|e| matches!(&e.kind, EventKind::Started { kind: SpanKind::Request, .. })).unwrap();
    assert!(request.parent_id.is_some(), "the request is a child of the send_request call");
    assert!(events.iter().any(|e| matches!(&e.kind, EventKind::ResponseReceived { status: 200, .. })));

    let activity = json_of(&call(&agent, "get_recent_activity", json!({ "limit": 5 })).await);
    assert!(activity.as_array().unwrap().iter().any(|s| s["title"] == "Get JSON" && s["status"] == 200));

    agent.cancel().await.unwrap();
}

#[tokio::test]
async fn agents_send_inline_requests_and_see_failing_tests() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let agent = agent(engine.clone(), ServerOptions::default()).await;

    let request = json!({ "url": "{{baseUrl}}/json", "tests": ["status == 201"] });
    let sent = json_of(&call(&agent, "send_request", json!({ "request": request })).await);
    assert_eq!(sent["outcome"], "failed");
    assert_eq!(sent["tests"][0]["passed"], false);

    let invalid = call(&agent, "send_request", json!({ "request": { "urll": "x" } })).await;
    assert_eq!(invalid.is_error, Some(true));
    assert!(json_of(&invalid).as_str().unwrap().contains("unknown field `urll`"));
    agent.cancel().await.unwrap();
}

#[tokio::test]
async fn save_request_writes_the_workspace_unless_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let request = json!({ "method": "post", "url": "{{baseUrl}}/users" });

    let agent_rw = agent(engine.clone(), ServerOptions::default()).await;
    let saved = json_of(&call(&agent_rw, "save_request", json!({ "id": "users/create", "request": request })).await);
    assert_eq!(saved["created"], true);
    assert_eq!(engine.workspace().unwrap().read_request("users/create").unwrap().method.as_str(), "POST");

    let agent_ro = agent(engine.clone(), ServerOptions { read_only: true }).await;
    let tools = agent_ro.list_all_tools().await.unwrap();
    assert!(!tools.iter().any(|t| t.name == "save_request"), "read-only servers do not offer save_request");
    let refused = call(&agent_ro, "save_request", json!({ "id": "users/other", "request": request })).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(json_of(&refused).as_str().unwrap().contains("read-only"));
    assert!(!engine.workspace().unwrap().request_exists("users/other"));

    agent_rw.cancel().await.unwrap();
    agent_ro.cancel().await.unwrap();
}

#[tokio::test]
async fn the_client_tests_an_mcp_server_over_http_and_shows_every_message() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let base = serve(Router::new().nest_service("/mcp", http_service(engine.clone(), None, ServerOptions::default()))).await;

    let clients = McpClients::new(engine.clone());
    let options = ConnectOptions {
        name: None,
        transport: Transport::Http { url: format!("{base}/mcp"), headers: vec![("X-Test".into(), "1".into())] },
    };
    let info = clients.connect(options, Source::User).await.unwrap();
    assert_eq!(info.server_name, "post-tester");
    assert_eq!(info.name, "post-tester");
    assert!(info.instructions.as_deref().unwrap().contains("get_workspace"));
    assert!(info.tools.iter().any(|t| t["name"] == "send_request"));
    assert!(info.resources.iter().any(|r| r["uri"] == "post-tester://requests/api/json"));
    assert_eq!(clients.sessions().len(), 1);

    let result = clients.call_tool(&info.id, "get_workspace", json!({}), Source::User).await.unwrap();
    assert_ne!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("api/json"));

    let resource = clients.read_resource(&info.id, "post-tester://requests/api/json", Source::User).await.unwrap();
    assert!(resource["contents"][0]["text"].as_str().unwrap().contains("{{baseUrl}}/json"));

    // An unknown tool is a protocol error: reported, and the call span fails.
    let error = clients.call_tool(&info.id, "nope", json!({}), Source::User).await.unwrap_err();
    assert!(error.to_string().contains("unknown tool `nope`"), "{error}");

    // The session span holds the handshake; each call is a child span with its own messages.
    let events = engine.bus().recent();
    let session = info.span_id;
    let messages = |span| -> Vec<(Direction, Option<String>, String)> {
        events
            .iter()
            .filter(|e| e.span_id == span)
            .filter_map(|e| match &e.kind {
                EventKind::Message { direction, label, payload } => Some((*direction, label.clone(), payload.preview.clone())),
                _ => None,
            })
            .collect()
    };
    let handshake = messages(session);
    assert!(handshake.iter().any(|(d, l, _)| *d == Direction::Sent && l.as_deref() == Some("initialize")));
    assert!(handshake.iter().any(|(d, l, _)| *d == Direction::Received && l.as_deref() == Some("tools/list")));
    let call_spans: Vec<_> =
        events.iter().filter(|e| e.parent_id == Some(session) && matches!(&e.kind, EventKind::Started { .. })).map(|e| e.span_id).collect();
    assert_eq!(call_spans.len(), 3);
    let first_call = messages(call_spans[0]);
    assert_eq!(first_call.len(), 2, "{first_call:?}");
    assert_eq!(first_call[0].0, Direction::Sent);
    assert!(first_call[0].2.contains("get_workspace"));
    assert_eq!(first_call[1].0, Direction::Received);
    assert!(first_call[1].2.contains("api/json"));
    let failed = events.iter().find(|e| e.span_id == call_spans[2] && matches!(e.kind, EventKind::Finished { .. })).unwrap();
    assert!(matches!(failed.kind, EventKind::Finished { outcome: Outcome::Error, .. }));

    // The server side saw the client as an agent named "post-tester".
    assert!(events.iter().any(|e| e.source == Source::Agent { client: "post-tester".into() }));

    clients.close(&info.id).await.unwrap();
    assert!(clients.sessions().is_empty());
    let finished = engine.bus().span_events(session).into_iter().last().unwrap();
    assert!(matches!(finished.kind, EventKind::Finished { outcome: Outcome::Ok, .. }), "{finished:?}");
    assert!(clients.call_tool(&info.id, "get_workspace", json!({}), Source::User).await.is_err());
}

#[tokio::test]
async fn missing_commands_explain_how_to_fix_them() {
    let clients = McpClients::new(Arc::new(Engine::default()));
    let options = ConnectOptions {
        name: Some("broken".into()),
        transport: Transport::Stdio { command: "post-tester-no-such-command".into(), args: vec![], env: Default::default(), cwd: None },
    };
    let error = clients.connect(options, Source::User).await.unwrap_err().to_string();
    assert!(error.contains("command not found") && error.contains("PATH"), "{error}");
}

#[test]
fn connect_options_match_the_api_json() {
    let options: ConnectOptions = serde_json::from_value(json!({
        "transport": { "type": "stdio", "command": "npx", "args": ["-y", "server"] }
    }))
    .unwrap();
    assert_eq!(
        options.transport,
        Transport::Stdio { command: "npx".into(), args: vec!["-y".into(), "server".into()], env: Default::default(), cwd: None }
    );
    let http: Transport = serde_json::from_value(json!({ "type": "http", "url": "http://x/mcp" })).unwrap();
    assert_eq!(serde_json::to_value(&http).unwrap(), json!({ "type": "http", "url": "http://x/mcp", "headers": [] }));
}

/// Writes a plugin with one MCP tool, `run`, whose body is `body`.
fn tool_plugin(parent: &std::path::Path, name: &str, permissions: &[&str], body: &str) -> std::path::PathBuf {
    let dir = parent.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "name": name, "description": "A test plugin.", "version": "1.0.0", "engine": "*", "main": "index.js",
        "provides": ["mcp-tool"], "permissions": permissions,
    });
    std::fs::write(dir.join("plugin.json"), manifest.to_string()).unwrap();
    let code = format!("export default {{ mcpTools: {{ run: {{ description: \"Runs.\", run: async (input, ctx) => {{ {body} }} }} }} }};");
    std::fs::write(dir.join("index.js"), code).unwrap();
    dir
}

#[tokio::test]
async fn secret_values_never_reach_the_agent() {
    let dir = tempfile::tempdir().unwrap();
    let api = start_api().await;
    let workspace = Workspace::init(dir.path(), "Demo").unwrap();
    let mut local = Environment::default();
    local.variables.insert("baseUrl".into(), api.into());
    // Referenced through a variable, so the request itself never says `{{secret:...}}`.
    local.variables.insert("key".into(), "{{secret:api-key}}".into());
    workspace.write_environment("local", &local).unwrap();
    workspace
        .write_request(
            "api/echo",
            &Request { url: "{{baseUrl}}/echo?key={{key}}".into(), tests: vec!["body == \"x\"".into()], ..Default::default() },
        )
        .unwrap();
    let secrets = MemorySecrets::with(&[("api-key", "s3cr3t value/1")]);
    let engine = Arc::new(Engine::new(Default::default(), Some(workspace), Arc::new(secrets)));
    let agent = agent(engine.clone(), ServerOptions::default()).await;

    let saved = call(&agent, "send_request", json!({ "id": "api/echo", "environment": "local" })).await;
    let inline =
        call(&agent, "send_request", json!({ "request": { "url": "{{baseUrl}}/echo?key={{key}}" }, "environment": "local" })).await;
    let run = call(&agent, "run_collection", json!({ "folder": "api", "environment": "local" })).await;
    for result in [saved, inline, run] {
        let text = &result.content[0].as_text().unwrap().text;
        assert!(!text.contains("s3cr3t") && text.contains("•••"), "{text}");
    }
    // The request really used the secret.
    let execution = engine.send_saved("api/echo", post_tester_core::SendOptions::new(Source::User).environment(Some("local".into()))).await;
    assert_eq!(execution.unwrap().response.unwrap().body.as_ref(), b"key=s3cr3t%20value/1");
}

#[tokio::test]
async fn read_only_mode_refuses_plugin_tools_that_write_files() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(&dir).await;
    let host = post_tester_plugin_host::PluginHost::attach(&engine);
    let plugins = tempfile::tempdir().unwrap();
    host.load(
        &tool_plugin(plugins.path(), "writer", &["fs:write"], "ctx.fs.write('x.txt', 'x'); return 'written';"),
        vec!["fs:write".into()],
    )
    .await
    .unwrap();
    host.load(&tool_plugin(plugins.path(), "reader", &[], "return 'read';"), vec![]).await.unwrap();

    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server = McpServer::new(engine.clone(), Some(host), ServerOptions { read_only: true });
    tokio::spawn(async move {
        let running = server.serve(server_io).await.unwrap();
        let _ = running.waiting().await;
    });
    let agent: Agent =
        ClientConfig::new(ClientCapabilities::default(), Implementation::new("test-agent", "1.0.0")).serve(client_io).await.unwrap();
    let tools: Vec<String> = agent.list_all_tools().await.unwrap().into_iter().map(|t| t.name.to_string()).collect();
    assert!(tools.contains(&"reader__run".to_string()) && !tools.iter().any(|t| t.starts_with("writer")), "{tools:?}");
    assert_eq!(json_of(&call(&agent, "reader__run", json!({})).await), "read");

    let refused = call(&agent, "writer__run", json!({})).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(json_of(&refused).as_str().unwrap().contains("read-only mode"), "{refused:?}");
    assert!(!dir.path().join("x.txt").exists());
}

#[tokio::test]
async fn hung_servers_time_out_with_a_clear_error() {
    // A server that accepts connections and never answers.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let silent = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            open.push(socket);
        }
    });
    let engine = Arc::new(Engine::default());
    let clients = McpClients::new(engine.clone()).with_timeouts(Duration::from_millis(300), Duration::from_millis(300));
    let started = std::time::Instant::now();
    let options = ConnectOptions { name: None, transport: Transport::Http { url: silent, headers: vec![] } };
    let error = clients.connect(options, Source::User).await.unwrap_err().to_string();
    assert!(error.contains("no answer within 300 ms"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(clients.sessions().is_empty());

    // A server that answers the handshake, then never finishes a tool call.
    let dir = tempfile::tempdir().unwrap();
    let engine = self::engine(&dir).await;
    let host = post_tester_plugin_host::PluginHost::attach(&engine);
    let plugins = tempfile::tempdir().unwrap();
    host.load(&tool_plugin(plugins.path(), "stuck", &[], "await new Promise(() => {});"), vec![]).await.unwrap();
    let base = serve(Router::new().nest_service("/mcp", http_service(engine.clone(), Some(host), ServerOptions::default()))).await;
    let clients = McpClients::new(engine.clone()).with_timeouts(Duration::from_secs(10), Duration::from_millis(300));
    let options = ConnectOptions { name: None, transport: Transport::Http { url: format!("{base}/mcp"), headers: vec![] } };
    let info = clients.connect(options, Source::User).await.unwrap();
    let error = clients.call_tool(&info.id, "stuck__run", json!({}), Source::User).await.unwrap_err().to_string();
    assert!(error.contains("tools/call timed out") && error.contains("300 ms"), "{error}");
    // The session still works.
    let result = clients.call_tool(&info.id, "get_workspace", json!({}), Source::User).await.unwrap();
    assert_ne!(result["isError"], true);
    clients.close(&info.id).await.unwrap();
}

/// A stdio server that never completes the handshake is stopped, not left running.
#[cfg(unix)]
#[tokio::test]
async fn stdio_servers_are_stopped_when_connecting_fails() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let clients = McpClients::new(Arc::new(Engine::default())).with_timeouts(Duration::from_millis(500), Duration::from_millis(500));
    let script = format!("echo $$ > '{}'; echo 'not an MCP server' >&2; exec sleep 30", pid_file.display());
    let options = ConnectOptions {
        name: None,
        transport: Transport::Stdio { command: "sh".into(), args: vec!["-c".into(), script], env: Default::default(), cwd: None },
    };
    let error = clients.connect(options, Source::User).await.unwrap_err().to_string();
    assert!(error.contains("did not complete the MCP handshake") && error.contains("not an MCP server"), "{error}");

    let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
    let running = || {
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    // Gone entirely: not running, and not a zombie either.
    while !running().is_empty() {
        assert!(std::time::Instant::now() < deadline, "process {pid} is still there: {}", running());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
