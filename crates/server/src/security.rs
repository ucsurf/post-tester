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
//!
//! There is deliberately no CORS layer: browsers on other origins can neither
//! read responses nor pass the Origin check.

use crate::state::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};

/// Creates a random session token.
pub fn new_token() -> String {
    hex::encode(rand::random::<[u8; 24]>())
}

/// The `Host` values that name this server.
///
/// Always: `localhost` and loopback addresses with the server's port. When
/// listening on a specific address (`--host 192.168.1.5`), that address too.
/// When listening on all interfaces (`--host 0.0.0.0` or `::`), any IP address,
/// because the machine's addresses are not known in advance. Domain names other
/// than `localhost` are never accepted: an IP address in `Host` cannot be the
/// result of DNS rebinding, a domain name can.
#[derive(Debug, Clone)]
pub struct AllowedHosts {
    port: u16,
    bound: IpAddr,
}

impl AllowedHosts {
    pub fn for_listener(addr: SocketAddr) -> Self {
        Self { port: addr.port(), bound: addr.ip() }
    }

    /// Whether `authority` (`host[:port]`, from a `Host` or `Origin` header) is this server.
    pub fn allows(&self, authority: &str) -> bool {
        let Some((host, port)) = split_authority(authority) else { return false };
        if port.unwrap_or(80) != self.port {
            return false;
        }
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        match host.parse::<IpAddr>() {
            Ok(ip) => ip.is_loopback() || ip == self.bound || self.bound.is_unspecified(),
            Err(_) => false,
        }
    }
}

/// Splits `host:port`, `[v6]:port`, `host` or `[v6]`.
fn split_authority(authority: &str) -> Option<(&str, Option<u16>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after {
            "" => None,
            _ => Some(after.strip_prefix(':')?.parse().ok()?),
        };
        return Some((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Some((host, Some(port.parse().ok()?))),
        None => Some((authority, None)),
    }
}

/// Middleware applying all three checks, plus a few protective response headers.
pub async fn guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // HTTP/2 carries the host in the URI instead of a `Host` header.
    let host = request.headers().get(header::HOST).and_then(|h| h.to_str().ok()).or_else(|| request.uri().authority().map(|a| a.as_str()));
    if !host.is_some_and(|host| state.hosts.allows(host)) {
        return deny(StatusCode::FORBIDDEN, "unexpected Host header. Open post-tester through the address printed by `pt serve`");
    }
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let same_origin = origin.to_str().ok().and_then(|o| strip_prefix_ignore_case(o, "http://")).is_some_and(|a| state.hosts.allows(a));
        if !same_origin {
            return deny(StatusCode::FORBIDDEN, "requests from other websites are not allowed");
        }
    }
    let path = request.uri().path();
    let protected = path.starts_with("/api") || path.starts_with("/mcp");
    if protected && !has_token(&request, &state.token) {
        return deny(StatusCode::UNAUTHORIZED, "missing or wrong session token. Use the link printed by `pt serve`");
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    // The page URL holds the token until the UI removes it: never send it elsewhere as a Referer.
    headers.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    response
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

fn has_token(request: &Request, expected: &str) -> bool {
    let bearer = request.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer "));
    // Browsers cannot set headers on WebSocket connections, so the token may also be in the query.
    let query = request.uri().query().and_then(|q| q.split('&').find_map(|pair| pair.strip_prefix("token=")));
    [bearer, query].into_iter().flatten().any(|given| constant_time_eq(given.as_bytes(), expected.as_bytes()))
}

/// Compares without leaking, through timing, how many characters matched.
/// (The length is not secret: every token has the same length.)
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

    #[test]
    fn loopback_listener_accepts_only_loopback_names() {
        let hosts = AllowedHosts::for_listener("127.0.0.1:7777".parse().unwrap());
        for ok in ["127.0.0.1:7777", "localhost:7777", "LOCALHOST:7777", "[::1]:7777"] {
            assert!(hosts.allows(ok), "{ok}");
        }
        for bad in ["127.0.0.1:7778", "127.0.0.1", "evil.example.com:7777", "192.168.1.5:7777", "localhost.evil.com:7777", "", "[::1"] {
            assert!(!hosts.allows(bad), "{bad}");
        }
    }

    #[test]
    fn network_listeners_accept_ip_addresses_but_never_other_domains() {
        let specific = AllowedHosts::for_listener("192.168.1.5:7777".parse().unwrap());
        assert!(specific.allows("192.168.1.5:7777"));
        assert!(specific.allows("127.0.0.1:7777"));
        assert!(!specific.allows("10.0.0.2:7777"));

        let all = AllowedHosts::for_listener("0.0.0.0:7777".parse().unwrap());
        assert!(all.allows("10.0.0.2:7777"));
        assert!(all.allows("[fe80::1]:7777"));
        assert!(!all.allows("rebind.evil.com:7777"));
        assert!(!all.allows("10.0.0.2:80"));

        let port_80 = AllowedHosts::for_listener("127.0.0.1:80".parse().unwrap());
        assert!(port_80.allows("localhost"), "browsers omit the default port");
    }
}
