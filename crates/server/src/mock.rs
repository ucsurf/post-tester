//! A mock API built from saved example responses (`examples:` in request files).
//!
//! Each request with examples becomes a route: its method plus the path of its
//! URL, where `{{name}}` and `:name` segments match anything and become
//! variables. The first example is returned unless the caller picks one with
//! the `X-Mock-Example: <name>` header. Every hit appears live in Live Activity.
//!
//! - Query strings are ignored when matching.
//! - When several routes match, the one with more literal segments wins, then
//!   the one whose first differing segment is literal (`/users/me` before
//!   `/users/{{id}}`, `/a/{{x}}` before `/{{y}}/b`), then file order.
//! - `HEAD` is answered by `GET` routes, without a body.
//! - Browsers may call the mock from any origin: responses allow CORS, and
//!   `OPTIONS` preflight requests are answered for every mocked path.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use post_tester_core::events::{Direction, EventKind, Level, Outcome, Source, Span, SpanKind, Summary};
use post_tester_core::model::{Example, OrderedMap};
use post_tester_core::template::{TemplateContext, Variables, render};
use post_tester_core::{Engine, Result, Workspace};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Header that selects an example by name.
pub const EXAMPLE_HEADER: &str = "x-mock-example";
/// Largest request body the mock accepts.
pub const BODY_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Param(String),
}

/// One mocked endpoint.
#[derive(Debug, Clone)]
pub struct MockRoute {
    pub request_id: String,
    pub method: String,
    /// The path pattern as shown to people, e.g. `/users/{{id}}`.
    pub path: String,
    segments: Vec<Segment>,
    pub examples: Vec<Example>,
}

impl MockRoute {
    /// Returns the path parameters when `method` and `path` match this route.
    fn matches(&self, method: &str, path: &str) -> Option<HashMap<String, String>> {
        let head_on_get = method.eq_ignore_ascii_case("HEAD") && self.method == "GET";
        if !self.method.eq_ignore_ascii_case(method) && !head_on_get {
            return None;
        }
        self.matches_path(path)
    }

    /// Returns the path parameters when `path` matches this route, whatever the method.
    fn matches_path(&self, path: &str) -> Option<HashMap<String, String>> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() != self.segments.len() {
            return None;
        }
        let mut params = HashMap::new();
        for (segment, part) in self.segments.iter().zip(parts) {
            match segment {
                Segment::Literal(text) if *text == percent_decode(part) => {}
                Segment::Literal(_) => return None,
                Segment::Param(name) => {
                    params.insert(name.clone(), percent_decode(part));
                }
            }
        }
        Some(params)
    }
}

/// Collects mock routes from every request with examples in a folder (`""` = all).
pub fn routes(workspace: &Workspace, folder: &str) -> Result<Vec<MockRoute>> {
    let tree = workspace.tree()?;
    let folder = tree.find(folder).ok_or_else(|| post_tester_core::Error::NotFound(format!("folder `{folder}`")))?;
    let mut routes = Vec::new();
    for id in folder.request_ids() {
        let request = workspace.read_request(&id)?;
        if request.examples.is_empty() {
            continue;
        }
        let path = path_of(&request.url);
        let segments = path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| match param_name(s) {
                Some(name) => Segment::Param(name),
                None => Segment::Literal(percent_decode(s)),
            })
            .collect();
        routes.push(MockRoute {
            request_id: id,
            method: request.method.as_str().to_ascii_uppercase(),
            path,
            segments,
            examples: request.examples,
        });
    }
    sort_by_precedence(&mut routes);
    Ok(routes)
}

/// More specific routes first: more literal segments, then literal segments
/// earlier in the path. The sort is stable, so file order breaks ties.
fn sort_by_precedence(routes: &mut [MockRoute]) {
    let is_param = |s: &Segment| matches!(s, Segment::Param(_));
    routes.sort_by(|a, b| {
        let literals = |r: &MockRoute| r.segments.iter().filter(|s| !is_param(s)).count();
        literals(b).cmp(&literals(a)).then_with(|| a.segments.iter().map(is_param).cmp(b.segments.iter().map(is_param)))
    });
}

/// The path part of a request URL, ignoring a leading `{{baseUrl}}` or scheme and host.
fn path_of(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or("");
    let rest = if without_query.starts_with("{{") {
        without_query.find("}}").map_or(without_query, |end| &without_query[end + 2..])
    } else if let Some(after_scheme) = without_query.split_once("://").map(|(_, rest)| rest) {
        after_scheme.find('/').map_or("", |slash| &after_scheme[slash..])
    } else {
        without_query
    };
    if rest.starts_with('/') { rest.to_string() } else { format!("/{rest}") }
}

fn param_name(segment: &str) -> Option<String> {
    if let Some(inner) = segment.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
        return Some(inner.trim().to_string());
    }
    segment.strip_prefix(':').filter(|s| !s.is_empty()).map(str::to_string)
}

/// Decodes `%XX` escapes in a path segment (`+` stays a plus sign in paths).
fn percent_decode(text: &str) -> String {
    percent_encoding::percent_decode_str(text).decode_utf8_lossy().into_owned()
}

/// A running mock server.
#[derive(Debug)]
pub struct RunningMock {
    pub addr: SocketAddr,
    pub routes: Vec<MockRoute>,
    pub task: tokio::task::JoinHandle<std::io::Result<()>>,
    /// Cancelling it stops the server; see [`RunningMock::shutdown`].
    pub shutdown: CancellationToken,
    bus: post_tester_core::events::EventBus,
}

impl RunningMock {
    /// Stops accepting connections and gives requests in flight (e.g. delayed
    /// examples) up to `grace` to finish, and writes out history.
    pub async fn shutdown(self, grace: Duration) -> std::io::Result<()> {
        self.shutdown.cancel();
        let stopped = crate::stop(self.task, grace).await;
        crate::flush(self.bus).await;
        stopped
    }
}

struct MockState {
    engine: Arc<Engine>,
    routes: Vec<MockRoute>,
    environment: Option<String>,
}

/// Starts serving the examples of `folder` on `addr`.
pub async fn start(engine: Arc<Engine>, folder: &str, environment: Option<String>, addr: SocketAddr) -> Result<RunningMock> {
    let routes = routes(&engine.require_workspace()?, folder)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    let bus = engine.bus().clone();
    let state = Arc::new(MockState { engine, routes: routes.clone(), environment });
    let app = Router::new().fallback(handle).with_state(state);
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone().cancelled_owned();
    let task = tokio::spawn(async move { axum::serve(listener, app).with_graceful_shutdown(signal).await });
    Ok(RunningMock { addr, routes, task, shutdown, bus })
}

async fn handle(State(state): State<Arc<MockState>>, request: Request) -> Response {
    let mut response = respond(&state, request).await;
    // Mock data is meant for front-ends in development, on any origin.
    response.headers_mut().entry(header::ACCESS_CONTROL_ALLOW_ORIGIN).or_insert(HeaderValue::from_static("*"));
    response
}

async fn respond(state: &MockState, request: Request) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if method == Method::OPTIONS && !state.routes.iter().any(|r| r.method == "OPTIONS" && r.matches_path(&path).is_some()) {
        return preflight(state, &path, request.headers());
    }
    let wanted = request.headers().get(EXAMPLE_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
    let title = format!("Mock {path}");
    let summary =
        Summary { protocol: Some("mock".into()), method: Some(method.to_string()), url: Some(request.uri().to_string()), request_id: None };
    let span = Span::start(state.engine.bus(), Source::System, None, SpanKind::Request, title, summary);
    let body = match to_bytes(request.into_body(), BODY_LIMIT).await {
        Ok(body) => body,
        Err(_) => {
            let error = format!("the request body is larger than the mock's limit of {} MB", BODY_LIMIT / (1024 * 1024));
            span.finish(Outcome::Failed, Some(413), None, Some(error.clone()));
            return json_error(StatusCode::PAYLOAD_TOO_LARGE, serde_json::json!({ "error": error }));
        }
    };
    if !body.is_empty() {
        let payload = span.bus().blobs().payload(&body, None);
        span.emit(EventKind::Message { direction: Direction::Received, label: Some("request body".into()), payload });
    }

    let found = state.routes.iter().find_map(|route| route.matches(method.as_str(), &path).map(|params| (route, params)));
    let Some((route, params)) = found else {
        let available: Vec<String> = state.routes.iter().map(|r| format!("{} {}", r.method, r.path)).collect();
        let message = serde_json::json!({ "error": format!("no mock matches {method} {path}"), "routes": available });
        span.finish(Outcome::Failed, Some(404), None, Some("no matching example".into()));
        return json_error(StatusCode::NOT_FOUND, message);
    };
    // Routes only exist for requests with at least one example.
    let example = match &wanted {
        None => &route.examples[0],
        Some(name) => match route.examples.iter().find(|e| &e.name == name) {
            Some(example) => example,
            None => {
                let names: Vec<&str> = route.examples.iter().map(|e| e.name.as_str()).collect();
                let error = format!("{} {} has no example named `{name}`", route.method, route.path);
                span.finish(Outcome::Failed, Some(404), None, Some(error.clone()));
                return json_error(StatusCode::NOT_FOUND, serde_json::json!({ "error": error, "examples": names }));
            }
        },
    };
    span.emit(EventKind::Log { level: Level::Info, text: format!("Matched {} → example \"{}\"", route.request_id, example.name) });
    if example.delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(example.delay_ms)).await;
    }

    match build_response(state, example, &params, &span).await {
        Ok((mut response, body)) => {
            let payload = span.bus().blobs().payload(&body, None);
            span.emit(EventKind::Message { direction: Direction::Sent, label: Some(format!("response {}", example.status)), payload });
            span.finish_ok(Some(example.status), Some(body.len() as u64));
            if method == Method::HEAD {
                *response.body_mut() = Body::empty();
            }
            response
        }
        Err(error) => {
            span.finish_error(&error);
            json_error(StatusCode::INTERNAL_SERVER_ERROR, serde_json::json!({ "error": error.to_string() }))
        }
    }
}

/// Answers a CORS preflight (or a plain `OPTIONS`) for a mocked path.
fn preflight(state: &MockState, path: &str, headers: &HeaderMap) -> Response {
    let mut methods: Vec<&str> = state.routes.iter().filter(|r| r.matches_path(path).is_some()).map(|r| r.method.as_str()).collect();
    if methods.is_empty() {
        return json_error(StatusCode::NOT_FOUND, serde_json::json!({ "error": format!("no mock matches OPTIONS {path}") }));
    }
    if methods.contains(&"GET") {
        methods.push("HEAD");
    }
    methods.push("OPTIONS");
    methods.sort_unstable();
    methods.dedup();
    let allow = HeaderValue::from_str(&methods.join(", ")).unwrap_or(HeaderValue::from_static("GET"));
    let mut response = StatusCode::NO_CONTENT.into_response();
    let out = response.headers_mut();
    out.insert(header::ALLOW, allow.clone());
    out.insert(header::ACCESS_CONTROL_ALLOW_METHODS, allow);
    if let Some(requested) = headers.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
        out.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, requested.clone());
    }
    out.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    response
}

fn json_error(status: StatusCode, body: serde_json::Value) -> Response {
    (status, axum::Json(body)).into_response()
}

/// Renders the example, with path parameters available as `{{name}}`.
async fn build_response(state: &MockState, example: &Example, params: &HashMap<String, String>, span: &Span) -> Result<(Response, Bytes)> {
    let mut vars = state.engine.variables(state.engine.resolve_environment(state.environment.clone()).as_deref(), &[])?;
    let mut layered = Variables::new();
    layered.push_strings(params.clone());
    for (name, value) in vars.flatten() {
        if !params.contains_key(&name) {
            layered.push_strings([(name, value)].into());
        }
    }
    vars = layered;
    let ctx = VarsOnly(vars);
    let body = Bytes::from(render(&example.body, &ctx).await?.text);
    let status = StatusCode::from_u16(example.status).unwrap_or_else(|_| {
        span.log(Level::Warn, format!("Example \"{}\" has an invalid status {}; answering 200", example.name, example.status));
        StatusCode::OK
    });
    let mut response = (status, Body::from(body.clone())).into_response();
    for header in example.headers.iter().filter(|h| h.enabled) {
        let value = render(&header.value, &ctx).await?.text;
        match (HeaderName::from_bytes(header.key.as_bytes()), HeaderValue::from_str(&value)) {
            (Ok(name), Ok(value)) => {
                response.headers_mut().insert(name, value);
            }
            _ => span
                .log(Level::Warn, format!("Header `{}` of example \"{}\" is not a valid HTTP header; skipped", header.key, example.name)),
        }
    }
    if !response.headers().contains_key(header::CONTENT_TYPE) && serde_json::from_slice::<serde_json::Value>(&body).is_ok() {
        response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    }
    Ok((response, body))
}

/// Mock responses may use variables and `{{$uuid}}`-style values, but never secrets.
struct VarsOnly(Variables);

#[async_trait::async_trait]
impl TemplateContext for VarsOnly {
    fn variable(&self, name: &str) -> Option<String> {
        self.0.get(name).map(str::to_string)
    }
    async fn secret(&self, name: &str) -> Result<String> {
        Err(post_tester_core::Error::Template(format!("mock responses cannot use secrets (`{name}`)")))
    }
    async fn function(&self, _name: &str, _args: &OrderedMap) -> Result<Option<String>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(method: &str, path: &str) -> MockRoute {
        let segments = path
            .split('/')
            .filter(|s| !s.is_empty())
            .map(|s| match param_name(s) {
                Some(name) => Segment::Param(name),
                None => Segment::Literal(percent_decode(s)),
            })
            .collect();
        MockRoute { request_id: path.into(), method: method.into(), path: path.into(), segments, examples: vec![] }
    }

    #[test]
    fn paths_and_parameters() {
        assert_eq!(path_of("{{baseUrl}}/users/{{id}}?x=1"), "/users/{{id}}");
        assert_eq!(path_of("https://api.dev/v1/items"), "/v1/items");
        assert_eq!(path_of("https://api.dev"), "/");
        let user = route("GET", "/users/{{id}}");
        assert_eq!(user.matches("get", "/users/42").unwrap()["id"], "42");
        assert_eq!(user.matches("GET", "/users/a%20b+c").unwrap()["id"], "a b+c", "`+` is literal in paths");
        assert!(user.matches("HEAD", "/users/42").is_some(), "GET routes answer HEAD");
        assert!(user.matches("POST", "/users/42").is_none());
        assert!(user.matches("GET", "/users/42/posts").is_none());
        assert!(route("GET", "/files/a%20b").matches("GET", "/files/a b").is_some());
    }

    #[test]
    fn specific_routes_win() {
        let mut routes = vec![route("GET", "/{{a}}/{{b}}"), route("GET", "/{{y}}/b"), route("GET", "/a/{{x}}"), route("GET", "/a/b")];
        sort_by_precedence(&mut routes);
        let order: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(order, ["/a/b", "/a/{{x}}", "/{{y}}/b", "/{{a}}/{{b}}"]);
        let first = routes.iter().find(|r| r.matches("GET", "/a/b").is_some()).unwrap();
        assert_eq!(first.path, "/a/b");
    }
}
