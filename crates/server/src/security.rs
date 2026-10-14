//! Protects the local server from other websites and other users.
//!
//! A server on localhost can be reached by any web page the user visits. Since
//! post-tester can send requests anywhere, an unprotected server would let a
//! malicious page use it to reach the user's internal network. Three checks:
//!
//! 1. **Host header** must name this server, which blocks DNS rebinding.
//! 2. **Origin header**, when present, must be this server, which blocks
//!    cross-site requests and WebSocket hijacking.
//! 3. **Session token**, random per start, required for `/api` and `/mcp`.

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Creates a random session token.
pub fn new_token() -> String {
    hex::encode(rand::random::<[u8; 24]>())
}

/// Middleware applying all three checks.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or_default();
    if !state.allowed_hosts.iter().any(|allowed| allowed.eq_ignore_ascii_case(host)) {
        return deny(StatusCode::FORBIDDEN, "unexpected Host header. Open post-tester through the address printed by `pt serve`");
    }
    if let Some(origin) = request.headers().get(header::ORIGIN).and_then(|h| h.to_str().ok()) {
        let same_origin = state.allowed_hosts.iter().any(|allowed| origin.eq_ignore_ascii_case(&format!("http://{allowed}")));
        if !same_origin {
            return deny(StatusCode::FORBIDDEN, "requests from other websites are not allowed");
        }
    }
    let path = request.uri().path();
    let protected = path.starts_with("/api") || path.starts_with("/mcp");
    if protected && !has_token(&request, &state.token) {
        return deny(StatusCode::UNAUTHORIZED, "missing or wrong session token. Use the link printed by `pt serve`");
    }
    next.run(request).await
}

fn has_token(request: &Request, expected: &str) -> bool {
    let bearer = request.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer "));
    let query = request.uri().query().and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("token=")));
    [bearer, query].into_iter().flatten().any(|given| constant_time_eq(given.as_bytes(), expected.as_bytes()))
}

/// Compares without leaking, through timing, how many characters matched.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn deny(status: StatusCode, message: &str) -> Response {
    (status, axum::Json(serde_json::json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_long() {
        let (a, b) = (new_token(), new_token());
        assert_ne!(a, b);
        assert_eq!(a.len(), 48);
    }

    #[test]
    fn constant_time_comparison() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
