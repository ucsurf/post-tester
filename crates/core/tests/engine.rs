//! End-to-end tests: the real engine against a local HTTP server.

use axum::Router;
use axum::body::Body as AxumBody;
use axum::extract::ws::{Message as WsMessage, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{any, get, post};
use post_tester_core::events::{Direction, Event, EventKind, Outcome, Phase, Source};
use post_tester_core::loadtest::LoadOptions;
use post_tester_core::model::{Environment, Request};
use post_tester_core::runner::{RunOptions, RunTarget};
use post_tester_core::secrets::MemorySecrets;
use post_tester_core::ws::WsOptions;
use post_tester_core::{Engine, SendOptions, Workspace};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

async fn start_server() -> String {
    let app = Router::new()
        .route("/json", get(|| async { axum::Json(serde_json::json!({ "token": "abc", "user": { "id": 7 } })) }))
        .route("/redirect", get(|| async { Redirect::to("/json") }))
        .route("/loop", get(|| async { Redirect::to("/loop") }))
        .route("/set-cookie", get(|| async { ([(header::SET_COOKIE, "session=xyz; Path=/")], "ok") }))
        .route(
            "/echo-headers",
            get(|headers: HeaderMap| async move {
                let pairs: serde_json::Map<String, serde_json::Value> =
                    headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().into())).collect();
                axum::Json(pairs)
            }),
        )
        .route(
            "/echo",
            post(|headers: HeaderMap, body: String| async move {
                let content_type = headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                axum::Json(serde_json::json!({ "content_type": content_type, "body": body }))
            }),
        )
        .route(
            "/gzip",
            get(|| async {
                let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(b"compressed hello").unwrap();
                ([(header::CONTENT_ENCODING, "gzip")], encoder.finish().unwrap())
            }),
        )
        .route(
            "/sse",
            get(|| async {
                let stream = futures::stream::unfold(0, |n| async move {
                    if n == 3 {
                        return None;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let chunk = format!("event: tick\ndata: {n}\n\n");
                    Some((Ok::<_, std::io::Error>(chunk), n + 1))
                });
                Response::builder().header(header::CONTENT_TYPE, "text/event-stream").body(AxumBody::from_stream(stream)).unwrap()
            }),
        )
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(5)).await;
                "late"
            }),
        )
        .route(
            "/status/{code}",
            get(|axum::extract::Path(code): axum::extract::Path<u16>| async move { StatusCode::from_u16(code).unwrap().into_response() }),
        )
        .route(
            "/ws",
            any(|ws: WebSocketUpgrade| async {
                ws.on_upgrade(|mut socket| async move {
                    while let Some(Ok(message)) = socket.recv().await {
                        if let WsMessage::Text(text) = message {
                            let _ = socket.send(WsMessage::Text(format!("echo: {text}").into())).await;
                        }
                    }
                })
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn engine() -> Arc<Engine> {
    Arc::new(Engine::new(Default::default(), None, Arc::new(MemorySecrets::with(&[("token", "s3cret")]))))
}

fn request(yaml: &str) -> Request {
    serde_yaml::from_str(yaml).unwrap()
}

fn kinds(engine: &Engine, span: post_tester_core::events::SpanId) -> Vec<Arc<Event>> {
    engine.bus().span_events(span)
}

#[tokio::test]
async fn sends_tests_extracts_and_reports_every_phase() {
    let base = start_server().await;
    let engine = engine();
    engine.set_runtime_variable("base", &base);
    let r = request(
        r#"
url: "{{base}}/json"
tests: ["status == 200", "json($.user.id) == 7", "time < 5000"]
extract: { token: json($.token) }
"#,
    );
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(execution.outcome, Outcome::Ok, "{:?}", execution.error);
    assert_eq!(execution.tests.len(), 3);
    assert_eq!(engine.runtime_variables()["token"], "abc");

    let events = kinds(&engine, execution.span_id);
    let has = |pred: &dyn Fn(&EventKind) -> bool| events.iter().any(|e| pred(&e.kind));
    assert!(matches!(events[0].kind, EventKind::Started { .. }));
    assert!(has(&|k| matches!(k, EventKind::Phase { phase: Phase::Dns, .. })));
    assert!(has(&|k| matches!(k, EventKind::Phase { phase: Phase::Connect, .. })));
    assert!(has(&|k| matches!(k, EventKind::Phase { phase: Phase::Waiting, .. })));
    assert!(has(&|k| matches!(k, EventKind::RequestSent { .. })));
    assert!(has(&|k| matches!(k, EventKind::ResponseReceived { status: 200, .. })));
    assert!(has(&|k| matches!(k, EventKind::TestResult { passed: true, .. })));
    assert!(has(&|k| matches!(k, EventKind::VariableSet { name, value } if name == "token" && value == "abc")));
    assert!(matches!(events.last().unwrap().kind, EventKind::Finished { outcome: Outcome::Ok, status: Some(200), .. }));
}

#[tokio::test]
async fn failed_tests_mark_the_request_failed() {
    let base = start_server().await;
    let engine = engine();
    let r = request(&format!("url: {base}/status/404\ntests: [\"status == 200\"]"));
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(execution.outcome, Outcome::Failed);
    assert_eq!(execution.tests[0].detail.as_deref(), Some("expected 200, got 404"));
}

#[tokio::test]
async fn follows_redirects_and_detects_loops() {
    let base = start_server().await;
    let engine = engine();
    let execution = engine.send(&request(&format!("url: {base}/redirect")), SendOptions::new(Source::User)).await;
    let response = execution.response.unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.redirects, 1);
    assert!(response.url.as_str().ends_with("/json"));

    let execution = engine.send(&request(&format!("url: {base}/loop")), SendOptions::new(Source::User)).await;
    assert!(execution.error.unwrap().contains("too many redirects"));
}

#[tokio::test]
async fn cookies_are_kept_between_requests() {
    let base = start_server().await;
    let engine = engine();
    engine.send(&request(&format!("url: {base}/set-cookie")), SendOptions::new(Source::User)).await;
    let execution = engine.send(&request(&format!("url: {base}/echo-headers")), SendOptions::new(Source::User)).await;
    let body: serde_json::Value = serde_json::from_slice(&execution.response.unwrap().body).unwrap();
    assert_eq!(body["cookie"], "session=xyz");
}

#[tokio::test]
async fn secrets_and_auth_are_applied() {
    let base = start_server().await;
    let engine = engine();
    let r = request(&format!("url: {base}/echo-headers\nauth: {{ type: bearer, token: \"{{{{secret:token}}}}\" }}"));
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    let body: serde_json::Value = serde_json::from_slice(&execution.response.unwrap().body).unwrap();
    assert_eq!(body["authorization"], "Bearer s3cret");
}

#[tokio::test]
async fn json_bodies_are_sent_with_content_type() {
    let base = start_server().await;
    let engine = engine();
    let r = request(&format!("method: POST\nurl: {base}/echo\nbody: {{ type: json, content: '{{\"a\":1}}' }}"));
    let execution = engine.send(&r, SendOptions::new(Source::User)).await;
    let body: serde_json::Value = serde_json::from_slice(&execution.response.unwrap().body).unwrap();
    assert_eq!(body["content_type"], "application/json");
    assert_eq!(body["body"], r#"{"a":1}"#);
}

#[tokio::test]
async fn gzip_bodies_are_decoded() {
    let base = start_server().await;
    let execution = engine().send(&request(&format!("url: {base}/gzip")), SendOptions::new(Source::User)).await;
    let response = execution.response.unwrap();
    assert_eq!(&response.body[..], b"compressed hello");
    assert!(response.wire_size < 100 && response.wire_size != response.body.len() as u64);
}

#[tokio::test]
async fn server_sent_events_stream_live() {
    let base = start_server().await;
    let engine = engine();
    let execution = engine.send(&request(&format!("url: {base}/sse")), SendOptions::new(Source::User)).await;
    let messages: Vec<String> = kinds(&engine, execution.span_id)
        .iter()
        .filter_map(|e| match &e.kind {
            EventKind::Message { direction: Direction::Received, label, payload } => {
                Some(format!("{}={}", label.clone().unwrap_or_default(), payload.preview))
            }
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec!["tick=0", "tick=1", "tick=2"]);
}

#[tokio::test]
async fn timeouts_explain_how_to_fix_them() {
    let base = start_server().await;
    let r = request(&format!("url: {base}/slow\noptions: {{ timeout_ms: 200 }}"));
    let execution = engine().send(&r, SendOptions::new(Source::User)).await;
    assert_eq!(execution.outcome, Outcome::Error);
    assert!(execution.error.unwrap().contains("options.timeout_ms"));
}

#[tokio::test]
async fn requests_can_be_cancelled() {
    let base = start_server().await;
    let engine = engine();
    let mut events = engine.bus().subscribe();
    let r = request(&format!("url: {base}/slow"));
    let task = {
        let engine = engine.clone();
        tokio::spawn(async move { engine.send(&r, SendOptions::new(Source::User)).await })
    };
    let started = events.recv().await.unwrap();
    assert!(engine.cancel(started.span_id));
    let execution = task.await.unwrap();
    assert_eq!(execution.outcome, Outcome::Cancelled);
}

#[tokio::test]
async fn collection_runs_chain_requests_through_variables() {
    let base = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let workspace = Workspace::init(dir.path(), "Test").unwrap();
    let mut env = Environment::default();
    env.variables.insert("baseUrl".into(), base.clone().into());
    workspace.write_environment("local", &env).unwrap();
    workspace.delete_request("examples/get-ip").unwrap();
    workspace.write_request("flow/1-login", &request("url: \"{{baseUrl}}/json\"\nextract: { token: json($.token) }")).unwrap();
    workspace
        .write_request(
            "flow/2-me",
            &request("url: \"{{baseUrl}}/echo-headers\"\nheaders: [{ key: X-Token, value: \"{{token}}\" }]\ntests: [\"json($['x-token']) == abc\"]"),
        )
        .unwrap();

    let engine = Arc::new(Engine::new(Default::default(), Some(workspace), Arc::new(MemorySecrets::default())));
    let report = engine.run(RunTarget::Folder("flow".into()), RunOptions::new(Source::Cli)).await.unwrap();
    assert_eq!(report.outcome, Outcome::Ok, "{report:#?}");
    assert_eq!(report.items.len(), 2);
    assert_eq!(report.tests_failed(), 0);

    // Child requests point to the run's span.
    let child = engine.bus().span_events(report.items[1].span_id);
    assert_eq!(child[0].parent_id, Some(report.span_id));
}

#[tokio::test]
async fn websocket_messages_are_reported_live() {
    let base = start_server().await;
    let engine = engine();
    let url = base.replace("http://", "ws://") + "/ws";
    let session = engine.ws_connect(WsOptions { url, headers: vec![], source: Source::User, environment: None }).await.unwrap();
    let mut events = engine.bus().subscribe();
    engine.ws_send(session, "hi").unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if let EventKind::Message { direction: Direction::Received, payload, .. } = &event.kind {
                return payload.preview.clone();
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(reply, "echo: hi");
    assert!(engine.cancel(session));
}

#[tokio::test]
async fn load_tests_measure_throughput() {
    let base = start_server().await;
    let engine = engine();
    let report = engine
        .load_test(
            &request(&format!("url: {base}/json")),
            None,
            LoadOptions { source: Source::User, environment: None, concurrency: 4, requests: 20 },
        )
        .await
        .unwrap();
    assert_eq!(report.succeeded, 20);
    assert_eq!(report.failed, 0);
    assert_eq!(report.status_codes[&200], 20);
    assert!(report.p50_ms > 0.0);
}
