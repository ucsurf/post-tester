//! End-to-end tests of the local server, using post-tester's own HTTP client.

use futures::StreamExt;
use post_tester_core::events::{EventBus, Source, Span, SpanKind, Summary};
use post_tester_core::http::{PreparedRequest, Response};
use post_tester_core::secrets::MemorySecrets;
use post_tester_core::{Engine, Workspace};
use post_tester_plugin_host::PluginHost;
use post_tester_server::{RunningServer, ServerConfig, serve};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

struct TestServer {
    server: RunningServer,
    _dir: tempfile::TempDir,
}

impl TestServer {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::init(dir.path(), "Test").unwrap();
        let engine = Arc::new(Engine::new(EventBus::default(), Some(workspace), Arc::new(MemorySecrets::default())));
        let plugins = PluginHost::attach(&engine);
        let config = ServerConfig { port: 0, token: Some("secret-token".into()), ..Default::default() };
        let server = serve(config, engine, plugins).await.unwrap();
        Self { server, _dir: dir }
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.server.addr.port())
    }

    async fn call(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<serde_json::Value>) -> Response {
        let mut all: Vec<(String, String)> = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if body.is_some() {
            all.push(("Content-Type".into(), "application/json".into()));
        }
        let request = PreparedRequest {
            method: method.into(),
            url: format!("{}{path}", self.base()).parse().unwrap(),
            headers: all,
            body: body.map(|b| b.to_string().into()).unwrap_or_default(),
            options: Default::default(),
        };
        let bus = EventBus::default();
        let span = Span::start(&bus, Source::System, None, SpanKind::Request, "test", Summary::default());
        post_tester_core::http::send(request, &span, None, &CancellationToken::new()).await.unwrap()
    }

    async fn api(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> (u16, serde_json::Value) {
        let response = self.call(method, path, &[("Authorization", "Bearer secret-token")], body).await;
        let json = serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null);
        (response.status, json)
    }
}

#[tokio::test]
async fn api_requires_the_token() {
    let s = TestServer::start().await;
    assert_eq!(s.call("GET", "/api/info", &[], None).await.status, 401);
    assert_eq!(s.call("GET", "/api/info", &[("Authorization", "Bearer wrong")], None).await.status, 401);
    assert_eq!(s.call("GET", "/api/info?token=secret-token", &[], None).await.status, 200);
    // The UI itself is public: it holds no data.
    assert_eq!(s.call("GET", "/", &[], None).await.status, 200);
}

#[tokio::test]
async fn foreign_hosts_and_origins_are_rejected() {
    let s = TestServer::start().await;
    let auth = ("Authorization", "Bearer secret-token");
    let rebinding = s.call("GET", "/api/info", &[auth, ("Host", "evil.example.com")], None).await;
    assert_eq!(rebinding.status, 403);
    let cross_site = s.call("GET", "/api/info", &[auth, ("Origin", "https://evil.example.com")], None).await;
    assert_eq!(cross_site.status, 403);
    let localhost = format!("localhost:{}", s.server.addr.port());
    let same_origin = format!("http://{localhost}");
    let ok = s.call("GET", "/api/info", &[auth, ("Host", &localhost), ("Origin", &same_origin)], None).await;
    assert_eq!(ok.status, 200);
}

#[tokio::test]
async fn workspace_endpoints() {
    let s = TestServer::start().await;
    let (status, info) = s.api("GET", "/api/info", None).await;
    assert_eq!(status, 200);
    assert_eq!(info["workspace"]["name"], "Test");
    assert_eq!(info["defaultEnvironment"], "local");
    assert!(info["mcpUrl"].as_str().unwrap().ends_with("/mcp"));

    let request = serde_json::json!({ "method": "POST", "url": "{{baseUrl}}/post", "tests": ["status == 200"] });
    assert_eq!(s.api("PUT", "/api/requests/demo/create", Some(request)).await.0, 204);
    let (_, saved) = s.api("GET", "/api/requests/demo/create", None).await;
    assert_eq!(saved["method"], "POST");
    let (_, tree) = s.api("GET", "/api/tree", None).await;
    let folders: Vec<&str> = tree["folders"].as_array().unwrap().iter().map(|f| f["id"].as_str().unwrap()).collect();
    assert!(folders.contains(&"demo"));

    let (status, error) = s.api("GET", "/api/requests/demo/missing", None).await;
    assert_eq!(status, 404);
    assert!(error["error"].as_str().unwrap().contains("not found"));
    let (status, _) = s.api("GET", "/api/requests/..%2F..%2Fetc", None).await;
    assert!(status == 400 || status == 404);

    let (_, envs) = s.api("GET", "/api/environments", None).await;
    assert_eq!(envs, serde_json::json!(["local"]));
    let (_, vars) = s.api("GET", "/api/variables", None).await;
    assert!(vars.as_array().unwrap().iter().any(|v| v["name"] == "baseUrl"));
}

#[tokio::test]
async fn send_preview_codegen_and_blobs() {
    let s = TestServer::start().await;
    // The server itself is a handy target: /api/info returns JSON.
    let url = format!("{}/api/info", s.base());
    let request = serde_json::json!({
        "url": url,
        "headers": [{ "key": "Authorization", "value": "Bearer secret-token" }],
        "tests": ["status == 200", "json($.version) exists"]
    });
    let (status, execution) = s.api("POST", "/api/send", Some(serde_json::json!({ "request": request }))).await;
    assert_eq!(status, 200);
    assert_eq!(execution["outcome"], "ok", "{execution:#}");
    assert_eq!(execution["response"]["status"], 200);
    assert!(execution["response"]["timings"]["connectMs"].as_f64().is_some());

    let blob = execution["response"]["body"]["blob"].as_str().unwrap();
    let raw = s.call("GET", &format!("/api/blobs/{blob}"), &[("Authorization", "Bearer secret-token")], None).await;
    assert_eq!(raw.status, 200);
    assert!(String::from_utf8_lossy(&raw.body).contains("version"));

    let secret = serde_json::json!({ "url": "https://x.dev", "auth": { "type": "bearer", "token": "{{secret:t}}" } });
    let (_, preview) = s.api("POST", "/api/preview", Some(serde_json::json!({ "request": secret.clone() }))).await;
    assert!(preview["headers"].to_string().contains("Bearer •••"), "{preview}");

    let (_, code) = s.api("POST", "/api/codegen", Some(serde_json::json!({ "request": secret, "target": "curl" }))).await;
    assert!(code["code"].as_str().unwrap().starts_with("curl"));
    assert!(!code["code"].as_str().unwrap().contains("secret"), "secrets never leak into generated code");
}

#[tokio::test]
async fn import_creates_requests() {
    let s = TestServer::start().await;
    let body = serde_json::json!({ "text": "curl -X POST https://api.example.com/users -d '{}'", "into": "imported" });
    let (status, summary) = s.api("POST", "/api/import", Some(body)).await;
    assert_eq!(status, 200, "{summary}");
    assert_eq!(summary["requests"], serde_json::json!(["imported/post-users"]));
    let (status, error) = s.api("POST", "/api/import", Some(serde_json::json!({ "text": "hello" }))).await;
    assert_eq!(status, 400);
    assert!(error["error"].as_str().unwrap().contains("Supported"));
}

#[tokio::test]
async fn live_stream_sends_history_then_new_events() {
    let s = TestServer::start().await;
    let url = format!("ws://127.0.0.1:{}/api/live?token=secret-token", s.server.addr.port());
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let first = socket.next().await.unwrap().unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(first.to_text().unwrap()).unwrap().is_array());

    let request = serde_json::json!({ "url": format!("{}/", s.base()) });
    s.api("POST", "/api/send", Some(serde_json::json!({ "request": request }))).await;

    let mut kinds = Vec::new();
    while !kinds.iter().any(|k| k == "finished") {
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next()).await.unwrap().unwrap().unwrap();
        let batch: Vec<serde_json::Value> = serde_json::from_str(message.to_text().unwrap()).unwrap();
        kinds.extend(batch.iter().map(|e| e["kind"]["type"].as_str().unwrap().to_string()));
    }
    for expected in ["started", "phase", "request_sent", "response_received", "finished"] {
        assert!(kinds.iter().any(|k| k == expected), "missing {expected} in {kinds:?}");
    }
}

#[tokio::test]
async fn live_stream_rejects_missing_token() {
    let s = TestServer::start().await;
    let url = format!("ws://127.0.0.1:{}/api/live", s.server.addr.port());
    assert!(tokio_tungstenite::connect_async(url).await.is_err());
}

#[tokio::test]
async fn mcp_endpoint_requires_the_token() {
    let s = TestServer::start().await;
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let response = s.call("POST", "/mcp", &[("Accept", "application/json, text/event-stream")], Some(body)).await;
    assert_eq!(response.status, 401);
}

#[tokio::test]
async fn post_tester_can_test_its_own_mcp_server() {
    let s = TestServer::start().await;
    let connect = serde_json::json!({
        "name": "self",
        "transport": {
            "type": "http",
            "url": format!("{}/mcp", s.base()),
            "headers": [["Authorization", "Bearer secret-token"]]
        }
    });
    let (status, session) = s.api("POST", "/api/mcp/connect", Some(connect)).await;
    assert_eq!(status, 200, "{session}");
    assert_eq!(session["serverName"], "post-tester");
    let tools: Vec<&str> = session["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tools.contains(&"send_request"), "{tools:?}");

    let id = session["id"].as_str().unwrap();
    let (status, result) = s.api("POST", &format!("/api/mcp/{id}/call"), Some(serde_json::json!({ "tool": "get_workspace" }))).await;
    assert_eq!(status, 200, "{result}");
    assert!(result.to_string().contains("Test"), "the workspace name should be in the result: {result}");

    let (_, sessions) = s.api("GET", "/api/mcp/sessions", None).await;
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    assert_eq!(s.api("DELETE", &format!("/api/mcp/{id}"), None).await.0, 204);
}
