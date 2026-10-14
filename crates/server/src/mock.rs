//! A mock API built from saved example responses (`examples:` in request files).
//!
//! Each request with examples becomes a route: its method plus the path of its
//! URL, where `{{name}}` and `:name` segments match anything and become
//! variables. The first example is returned unless the caller picks one with
//! the `X-Mock-Example: <name>` header. Every hit appears live in Live Activity.

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use post_tester_core::events::{Direction, EventKind, Source, Span, SpanKind, Summary};
use post_tester_core::model::{Example, OrderedMap};
use post_tester_core::template::{TemplateContext, Variables, render};
use post_tester_core::{Engine, Result, Workspace};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

/// Header that selects an example by name.
pub const EXAMPLE_HEADER: &str = "x-mock-example";

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
        if !self.method.eq_ignore_ascii_case(method) {
            return None;
        }
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if parts.len() != self.segments.len() {
            return None;
        }
        let mut params = HashMap::new();
        for (segment, part) in self.segments.iter().zip(parts) {
            match segment {
                Segment::Literal(text) if text == part => {}
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
                None => Segment::Literal(s.to_string()),
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
    // More specific routes (more literal segments) win over ones with parameters.
    routes.sort_by_key(|r| std::cmp::Reverse(r.segments.iter().filter(|s| matches!(s, Segment::Literal(_))).count()));
    Ok(routes)
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

fn percent_decode(text: &str) -> String {
    url::form_urlencoded::parse(format!("x={text}").as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_default()
}

/// A running mock server.
#[derive(Debug)]
pub struct RunningMock {
    pub addr: SocketAddr,
    pub routes: Vec<MockRoute>,
    pub task: tokio::task::JoinHandle<std::io::Result<()>>,
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
    let state = Arc::new(MockState { engine, routes: routes.clone(), environment });
    let app = Router::new().fallback(handle).with_state(state);
    let task = tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(RunningMock { addr, routes, task })
}

async fn handle(State(state): State<Arc<MockState>>, request: Request) -> Response {
    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let wanted = request.headers().get(EXAMPLE_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);
    let title = format!("Mock {path}");
    let summary =
        Summary { protocol: Some("mock".into()), method: Some(method.clone()), url: Some(request.uri().to_string()), request_id: None };
    let span = Span::start(state.engine.bus(), Source::System, None, SpanKind::Request, title, summary);
    let body = to_bytes(request.into_body(), 16 * 1024 * 1024).await.unwrap_or_default();
    if !body.is_empty() {
        let payload = span.bus().blobs().payload(&body, None);
        span.emit(EventKind::Message { direction: Direction::Received, label: Some("request body".into()), payload });
    }

    let found = state.routes.iter().find_map(|route| route.matches(&method, &path).map(|params| (route, params)));
    let Some((route, params)) = found else {
        let available: Vec<String> = state.routes.iter().map(|r| format!("{} {}", r.method, r.path)).collect();
        let message = serde_json::json!({ "error": format!("no mock matches {method} {path}"), "routes": available });
        span.finish(post_tester_core::events::Outcome::Failed, Some(404), None, Some("no matching example".into()));
        return (StatusCode::NOT_FOUND, axum::Json(message)).into_response();
    };
    let example = match &wanted {
        Some(name) => route.examples.iter().find(|e| &e.name == name).unwrap_or(&route.examples[0]),
        None => &route.examples[0],
    };
    span.emit(EventKind::Log {
        level: post_tester_core::events::Level::Info,
        text: format!("Matched {} → example \"{}\"", route.request_id, example.name),
    });
    if example.delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(example.delay_ms)).await;
    }

    match build_response(&state, example, &params).await {
        Ok((response, body)) => {
            let payload = span.bus().blobs().payload(&body, None);
            span.emit(EventKind::Message { direction: Direction::Sent, label: Some(format!("response {}", example.status)), payload });
            span.finish_ok(Some(example.status), Some(body.len() as u64));
            response
        }
        Err(error) => {
            span.finish_error(&error);
            (StatusCode::INTERNAL_SERVER_ERROR, axum::Json(serde_json::json!({ "error": error.to_string() }))).into_response()
        }
    }
}

/// Renders the example, with path parameters available as `{{name}}`.
async fn build_response(state: &MockState, example: &Example, params: &HashMap<String, String>) -> Result<(Response, Bytes)> {
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
    let status = StatusCode::from_u16(example.status).unwrap_or(StatusCode::OK);
    let mut response = (status, Body::from(body.clone())).into_response();
    for header in example.headers.iter().filter(|h| h.enabled) {
        let value = render(&header.value, &ctx).await?.text;
        if let (Ok(name), Ok(value)) = (HeaderName::from_bytes(header.key.as_bytes()), HeaderValue::from_str(&value)) {
            response.headers_mut().insert(name, value);
        }
    }
    if !response.headers().contains_key(axum::http::header::CONTENT_TYPE) && serde_json::from_slice::<serde_json::Value>(&body).is_ok() {
        response.headers_mut().insert(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
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

    #[test]
    fn paths_and_parameters() {
        assert_eq!(path_of("{{baseUrl}}/users/{{id}}?x=1"), "/users/{{id}}");
        assert_eq!(path_of("https://api.dev/v1/items"), "/v1/items");
        assert_eq!(path_of("https://api.dev"), "/");
        let route = MockRoute {
            request_id: "users/get".into(),
            method: "GET".into(),
            path: "/users/{{id}}".into(),
            segments: vec![Segment::Literal("users".into()), Segment::Param("id".into())],
            examples: vec![],
        };
        assert_eq!(route.matches("get", "/users/42").unwrap()["id"], "42");
        assert!(route.matches("POST", "/users/42").is_none());
        assert!(route.matches("GET", "/users/42/posts").is_none());
    }
}
