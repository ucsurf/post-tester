//! End-to-end tests of the mock API (`pt mock`).

use post_tester_core::events::{EventBus, Source, Span, SpanKind, Summary};
use post_tester_core::http::{PreparedRequest, Response};
use post_tester_core::secrets::MemorySecrets;
use post_tester_core::{Engine, Workspace};
use post_tester_server::mock::{self, RunningMock};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

const USER: &str = r#"
name: Get user
url: "{{baseUrl}}/users/{{id}}"
examples:
  - name: found
    body: '{"id": "{{id}}", "name": "Ada"}'
  - name: missing
    status: 404
    body: '{"error": "not found"}'
"#;

const ME: &str = r#"
url: "{{baseUrl}}/users/me"
examples:
  - name: me
    body: '{"me": true}'
"#;

const SECRET: &str = r#"
method: POST
url: "/leak"
examples:
  - name: leak
    body: 'token={{secret:token}}'
"#;

struct TestMock {
    mock: RunningMock,
    _dir: tempfile::TempDir,
}

impl TestMock {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::init(dir.path(), "Mock").unwrap();
        let users = dir.path().join("collections/users");
        std::fs::create_dir_all(&users).unwrap();
        std::fs::write(users.join("get.yaml"), USER).unwrap();
        std::fs::write(users.join("me.yaml"), ME).unwrap();
        std::fs::write(dir.path().join("collections/leak.yaml"), SECRET).unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        let engine = Arc::new(Engine::new(EventBus::default(), Some(workspace), secrets.clone()));
        use post_tester_core::secrets::SecretStore;
        secrets.set("token", "s3cret").await.unwrap();
        let mock = mock::start(engine, "", None, "127.0.0.1:0".parse().unwrap()).await.unwrap();
        Self { mock, _dir: dir }
    }

    async fn call(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Vec<u8>) -> Response {
        let request = PreparedRequest {
            method: method.into(),
            url: format!("http://{}{path}", self.mock.addr).parse().unwrap(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.into(),
            options: Default::default(),
        };
        let bus = EventBus::default();
        let span = Span::start(&bus, Source::System, None, SpanKind::Request, "test", Summary::default());
        post_tester_core::http::send(request, &span, None, &CancellationToken::new()).await.unwrap()
    }

    async fn get(&self, path: &str, headers: &[(&str, &str)]) -> (u16, serde_json::Value) {
        let response = self.call("GET", path, headers, Vec::new()).await;
        (response.status, serde_json::from_slice(&response.body).unwrap_or(serde_json::Value::Null))
    }
}

#[tokio::test]
async fn serves_examples_with_path_parameters() {
    let m = TestMock::start().await;
    let response = m.call("GET", "/users/42?ignored=1", &[], Vec::new()).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("application/json"));
    assert_eq!(response.header("access-control-allow-origin"), Some("*"));
    let body: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
    assert_eq!(body["id"], "42");

    // A literal route wins over one with a parameter, whatever the file order.
    assert_eq!(m.get("/users/me", &[]).await.1["me"], true);

    let (status, body) = m.get("/users/42", &[("X-Mock-Example", "missing")]).await;
    assert_eq!((status, body["error"].as_str()), (404, Some("not found")));
}

#[tokio::test]
async fn unknown_examples_and_paths_explain_what_exists() {
    let m = TestMock::start().await;
    let (status, body) = m.get("/users/42", &[("X-Mock-Example", "nope")]).await;
    assert_eq!(status, 404);
    assert_eq!(body["examples"], serde_json::json!(["found", "missing"]));
    let (status, body) = m.get("/nothing/here", &[]).await;
    assert_eq!(status, 404);
    assert!(body["routes"].as_array().unwrap().iter().any(|r| r == "GET /users/{{id}}"), "{body}");
}

#[tokio::test]
async fn head_and_preflight_requests() {
    let m = TestMock::start().await;
    let head = m.call("HEAD", "/users/42", &[], Vec::new()).await;
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());

    let preflight = m
        .call(
            "OPTIONS",
            "/users/42",
            &[
                ("Origin", "http://localhost:5173"),
                ("Access-Control-Request-Method", "GET"),
                ("Access-Control-Request-Headers", "x-mock-example"),
            ],
            Vec::new(),
        )
        .await;
    assert_eq!(preflight.status, 204);
    assert_eq!(preflight.header("access-control-allow-origin"), Some("*"));
    assert_eq!(preflight.header("access-control-allow-methods"), Some("GET, HEAD, OPTIONS"));
    assert_eq!(preflight.header("access-control-allow-headers"), Some("x-mock-example"));
    assert_eq!(m.call("OPTIONS", "/nothing", &[], Vec::new()).await.status, 404);
}

#[tokio::test]
async fn secrets_are_never_served_and_big_bodies_are_refused() {
    let m = TestMock::start().await;
    let leak = m.call("POST", "/leak", &[], Vec::new()).await;
    assert_eq!(leak.status, 500);
    let text = String::from_utf8_lossy(&leak.body);
    assert!(text.contains("cannot use secrets") && !text.contains("s3cret"), "{text}");

    let big = m.call("POST", "/leak", &[], vec![b'a'; mock::BODY_LIMIT + 1]).await;
    assert_eq!(big.status, 413);
}

#[tokio::test]
async fn shutdown_stops_listening() {
    let m = TestMock::start().await;
    let addr = m.mock.addr;
    m.mock.shutdown(Duration::from_secs(5)).await.unwrap();
    assert!(tokio::net::TcpStream::connect(addr).await.is_err());
}
