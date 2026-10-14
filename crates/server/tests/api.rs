//! End-to-end tests of the local server, using post-tester's own HTTP client.

use futures::StreamExt;
use post_tester_core::events::{EventBus, Source, Span, SpanKind, Summary};
use post_tester_core::http::{PreparedRequest, Response};
use post_tester_core::secrets::MemorySecrets;
use post_tester_core::{Engine, Workspace};
use post_tester_plugin_host::PluginHost;
use post_tester_server::{RunningServer, ServerConfig, serve};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;

const AUTH: (&str, &str) = ("Authorization", "Bearer secret-token");

struct TestServer {
    server: RunningServer,
    engine: Arc<Engine>,
    dir: tempfile::TempDir,
}

impl TestServer {
    async fn start() -> Self {
        Self::start_on("127.0.0.1".parse().unwrap()).await
    }

    async fn start_on(host: std::net::IpAddr) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::init(dir.path(), "Test").unwrap();
        let engine = Arc::new(Engine::new(EventBus::default(), Some(workspace), Arc::new(MemorySecrets::default())));
        let plugins = PluginHost::attach(&engine);
        let config = ServerConfig { host, port: 0, token: Some("secret-token".into()), ..Default::default() };
        let server = serve(config, engine.clone(), plugins).await.unwrap();
        Self { server, engine, dir }
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

    async fn call_raw(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Response {
        let request = PreparedRequest {
            method: method.into(),
            url: format!("{}{path}", self.base()).parse().unwrap(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.to_string().into(),
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

#[tokio::test]
async fn ids_cannot_escape_the_workspace() {
    let s = TestServer::start().await;
    let outside = s.dir.path().parent().unwrap().join("escaped.yaml");
    let request = serde_json::json!({ "url": "https://x.dev" });
    let attempts = [
        ("GET", "/api/requests/..%2F..%2Fpost-tester"),
        ("GET", "/api/requests/%2Fetc%2Fpasswd"),
        ("GET", "/api/requests/demo/..%2F..%2F..%2Fpost-tester"),
        ("GET", "/api/requests/..%5C..%5Cpost-tester"),
        ("GET", "/api/requests/.hidden"),
        ("PUT", "/api/requests/..%2F..%2Fescaped"),
        ("DELETE", "/api/requests/..%2F..%2Fpost-tester"),
        ("GET", "/api/folders/settings/..%2F.."),
        ("PUT", "/api/folders/settings/..%2F..%2Fx"),
        ("GET", "/api/environments/..%2F..%2Fpost-tester"),
        ("PUT", "/api/environments/..%2Fescaped"),
        ("DELETE", "/api/environments/.."),
    ];
    for (method, path) in attempts {
        let body = (method == "PUT").then(|| if path.contains("/requests/") { request.clone() } else { serde_json::json!({}) });
        let (status, error) = s.api(method, path, body).await;
        assert!(status == 400 || status == 404, "{method} {path} → {status} {error}");
        assert!(error["error"].is_string(), "{method} {path} should explain the problem: {error}");
    }
    assert!(!outside.exists(), "nothing may be written outside the workspace");
    assert!(s.dir.path().join("post-tester.yaml").is_file(), "nothing may be deleted");
}

#[tokio::test]
async fn api_errors_are_json() {
    let s = TestServer::start().await;
    let (status, error) = s.api("GET", "/api/no-such-endpoint", None).await;
    assert_eq!(status, 404);
    assert!(error["error"].as_str().unwrap().contains("API.md"), "{error}");

    let response = s.call("PUT", "/api/requests/demo/x", &[AUTH, ("Content-Type", "application/json")], None).await;
    assert!(response.status >= 400 && response.status < 500);
    let malformed = s.call_raw("PUT", "/api/requests/demo/x", &[AUTH, ("Content-Type", "application/json")], "{not json").await;
    assert_eq!(malformed.status, 400);
    let error: serde_json::Value = serde_json::from_slice(&malformed.body).expect("errors are JSON");
    assert!(error["error"].is_string(), "{error}");

    let (status, error) = s.api("DELETE", "/api/info", None).await;
    assert_eq!(status, 405);
    assert!(error["error"].is_string(), "{error}");
}

#[tokio::test]
async fn oversized_bodies_are_rejected_with_a_message() {
    let s = TestServer::start().await;
    let big = format!("{{\"text\": \"{}\"}}", "a".repeat(post_tester_server::API_BODY_LIMIT + 1));
    let response = s.call_raw("POST", "/api/preview", &[AUTH, ("Content-Type", "application/json")], &big).await;
    assert_eq!(response.status, 413);
    let error: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert!(error["error"].as_str().unwrap().contains("too large"), "{error}");
    // Imports may be larger than other requests.
    let response = s.call_raw("POST", "/api/import", &[AUTH, ("Content-Type", "application/json")], &big).await;
    assert_ne!(response.status, 413);
}

#[tokio::test]
async fn responses_carry_protective_headers_and_no_cors() {
    let s = TestServer::start().await;
    let response = s.call("GET", "/api/info", &[AUTH], None).await;
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(response.header("x-frame-options"), Some("DENY"));
    let origin = format!("http://127.0.0.1:{}", s.server.addr.port());
    let same_origin = s.call("OPTIONS", "/api/info", &[AUTH, ("Origin", &origin), ("Access-Control-Request-Method", "PUT")], None).await;
    assert!(same_origin.header("access-control-allow-origin").is_none());
    let preflight =
        s.call("OPTIONS", "/api/info", &[("Origin", "https://evil.example.com"), ("Access-Control-Request-Method", "PUT")], None).await;
    assert_eq!(preflight.status, 403);
    assert!(preflight.header("access-control-allow-origin").is_none());
    assert!(s.call("GET", "/api/info", &[AUTH, ("Origin", "null")], None).await.status == 403);
}

#[tokio::test]
async fn live_stream_rejects_other_origins() {
    let s = TestServer::start().await;
    let url = format!("ws://127.0.0.1:{}/api/live?token=secret-token", s.server.addr.port());
    let mut request = url.into_client_request().unwrap();
    request.headers_mut().insert("Origin", "https://evil.example.com".parse().unwrap());
    let error = tokio_tungstenite::connect_async(request).await.unwrap_err();
    assert!(error.to_string().contains("403"), "{error}");
}

#[tokio::test]
async fn mcp_endpoint_uses_the_same_host_rules() {
    let s = TestServer::start().await;
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let accept = ("Accept", "application/json, text/event-stream");
    let port = s.server.addr.port();
    let evil = format!("rebind.evil.com:{port}");
    let rebinding = s.call("POST", "/mcp", &[AUTH, accept, ("Host", &evil)], Some(body)).await;
    assert_eq!(rebinding.status, 403);
}

/// `pt serve --host 0.0.0.0`: other machines reach the server by IP address.
#[tokio::test]
async fn listening_on_all_interfaces_accepts_ip_hosts_including_mcp() {
    let s = TestServer::start_on("0.0.0.0".parse().unwrap()).await;
    let port = s.server.addr.port();
    let lan = format!("10.1.2.3:{port}");
    let lan_origin = format!("http://{lan}");
    let info = s.call("GET", "/api/info", &[AUTH, ("Host", &lan), ("Origin", &lan_origin)], None).await;
    assert_eq!(info.status, 200);
    let info: serde_json::Value = serde_json::from_slice(&info.body).unwrap();
    assert_eq!(info["mcpUrl"], format!("http://{lan}/mcp"), "the MCP URL uses the address the caller used");
    let evil = format!("rebind.evil.com:{port}");
    assert_eq!(s.call("GET", "/api/info", &[AUTH, ("Host", &evil)], None).await.status, 403);

    let initialize = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } }
    });
    let accept = ("Accept", "application/json, text/event-stream");
    let mcp = s.call("POST", "/mcp", &[AUTH, accept, ("Host", &lan)], Some(initialize)).await;
    assert_eq!(mcp.status, 200, "{}", String::from_utf8_lossy(&mcp.body));
}

#[tokio::test]
async fn send_reuses_the_stored_response_body() {
    let s = TestServer::start().await;
    let request = serde_json::json!({ "url": format!("{}/api/info", s.base()), "headers": [{ "key": "Authorization", "value": "Bearer secret-token" }] });
    let (_, execution) = s.api("POST", "/api/send", Some(serde_json::json!({ "request": request }))).await;
    let span_id = execution["spanId"].as_str().unwrap();
    let returned = execution["response"]["body"]["blob"].as_str().unwrap();
    let events = s.engine.bus().recent();
    let received = events
        .iter()
        .filter(|e| e.span_id.to_string() == span_id)
        .find_map(|e| match &e.kind {
            post_tester_core::events::EventKind::ResponseReceived { body, .. } => body.blob,
            _ => None,
        })
        .unwrap();
    assert_eq!(returned, received.to_string(), "one blob per response body, not two");
}

fn log_event(engine: &Engine, span: post_tester_core::events::SpanId, i: usize) {
    use post_tester_core::events::{EventKind, Level};
    engine.bus().emit(span, None, &Source::System, EventKind::Log { level: Level::Info, text: i.to_string() });
}

/// Reads live messages until `want` events of `span` arrived; returns the size of each message.
async fn read_span_events(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    span: post_tester_core::events::SpanId,
    want: usize,
) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    let mut sizes = Vec::new();
    while seen.len() < want {
        let message = tokio::time::timeout(Duration::from_secs(10), socket.next())
            .await
            .unwrap_or_else(|_| panic!("only {} of {want} events arrived", seen.len()))
            .unwrap()
            .unwrap();
        let batch: Vec<serde_json::Value> = serde_json::from_str(message.to_text().unwrap()).unwrap();
        sizes.push(batch.len());
        for event in batch.iter().filter(|e| e["span_id"] == span.to_string()) {
            seen.insert(event["seq"].as_u64().unwrap());
        }
    }
    sizes
}

#[tokio::test]
async fn live_history_is_sent_in_chunks() {
    let s = TestServer::start().await;
    let span = post_tester_core::events::SpanId::new();
    for i in 0..2_500 {
        log_event(&s.engine, span, i);
    }
    let url = format!("ws://127.0.0.1:{}/api/live?token=secret-token", s.server.addr.port());
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let sizes = read_span_events(&mut socket, span, 2_500).await;
    assert!(sizes.len() >= 3 && sizes.iter().all(|&n| n <= 1_000), "{sizes:?}");
}

#[tokio::test]
async fn a_client_that_falls_behind_gets_the_missed_events() {
    let s = TestServer::start().await;
    let url = format!("ws://127.0.0.1:{}/api/live?token=secret-token", s.server.addr.port());
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), socket.next()).await.unwrap().unwrap().unwrap(); // history
    // Without yielding, so the server cannot keep up and its channel overflows (8192 events).
    let span = post_tester_core::events::SpanId::new();
    for i in 0..12_000 {
        log_event(&s.engine, span, i);
    }
    read_span_events(&mut socket, span, 12_000).await;
}

#[tokio::test]
async fn shutdown_closes_live_streams_and_stops_listening() {
    let s = TestServer::start().await;
    let addr = s.server.addr;
    let url = format!("ws://127.0.0.1:{}/api/live?token=secret-token", addr.port());
    let (mut socket, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), socket.next()).await.unwrap().unwrap().unwrap(); // history
    let started = std::time::Instant::now();
    s.server.shutdown(Duration::from_secs(10)).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(3), "an open live stream must not hold up shutdown: {:?}", started.elapsed());
    let closed = tokio::time::timeout(Duration::from_secs(2), socket.next()).await.unwrap();
    assert!(matches!(closed, None | Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | Some(Err(_))), "{closed:?}");
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}

#[tokio::test]
async fn load_test_can_use_fresh_connections() {
    let s = TestServer::start().await;
    let request = serde_json::json!({ "url": format!("{}/", s.base()) });
    for connections in [None, Some("reuse"), Some("fresh")] {
        let mut body = serde_json::json!({ "request": request, "concurrency": 2, "requests": 4 });
        if let Some(connections) = connections {
            body["connections"] = connections.into();
        }
        let (status, report) = s.api("POST", "/api/load-test", Some(body)).await;
        assert_eq!(status, 200, "{report}");
        assert_eq!(report["succeeded"], 4, "{connections:?}: {report}");
    }
    let bad = serde_json::json!({ "request": request, "concurrency": 1, "requests": 1, "connections": "sometimes" });
    assert_eq!(s.api("POST", "/api/load-test", Some(bad)).await.0, 422);
}

#[tokio::test]
async fn events_since_returns_only_newer_events() {
    let s = TestServer::start().await;
    let span = post_tester_core::events::SpanId::new();
    for i in 0..5 {
        log_event(&s.engine, span, i);
    }
    let all = s.engine.bus().recent();
    let since = all[all.len() - 3].seq;
    let (_, events) = s.api("GET", &format!("/api/events?since={since}"), None).await;
    let seqs: Vec<u64> = events.as_array().unwrap().iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, all[all.len() - 2..].iter().map(|e| e.seq).collect::<Vec<_>>());
}
