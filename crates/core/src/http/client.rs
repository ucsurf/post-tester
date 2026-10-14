//! Sends one HTTP request and reports every step live.
//!
//! Instead of a high-level client, each step is done explicitly so it can be
//! timed: DNS → TCP connect → TLS handshake → send → first byte → download.
//! Each request uses a fresh connection, so timings are always complete and
//! comparable. Load tests opt into reusing connections ([`ConnectionPool`]),
//! so they measure the server rather than TCP and TLS handshakes.
//!
//! `options.timeout_ms` is a deadline for everything up to the response
//! headers (DNS, connect, TLS, sending, waiting). While the body downloads it
//! is an idle limit instead: each chunk must arrive within it, so a large
//! download that keeps making progress is never cut off.

use super::cookies::CookieJar;
use super::sse::SseParser;
use super::{PreparedRequest, Response, Timings, decode};
use crate::error::{Error, Result};
use crate::events::{Direction, EventKind, Header, Level, Phase, Span, TlsInfo};
use crate::model::{HttpVersionPref, RequestOptions};
use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::client::conn::{http1, http2};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_util::sync::CancellationToken;
use url::Url;

/// Largest body kept in memory. Bigger bodies are cut off with a warning.
const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// How long an event stream may stay silent before it is considered dead.
const STREAM_IDLE_LIMIT: Duration = Duration::from_hours(24);

const USER_AGENT: &str = concat!("post-tester/", env!("CARGO_PKG_VERSION"));

/// Headers that describe the body; dropped when a redirect turns the request into a GET.
const BODY_HEADERS: &[&str] =
    &["content-type", "content-length", "content-encoding", "content-language", "content-location", "transfer-encoding"];

/// Headers that must not follow a redirect to another origin. Cookies for the
/// new site are added again from the cookie jar.
const CREDENTIAL_HEADERS: &[&str] = &["authorization", "proxy-authorization", "cookie", "host"];

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Sends a request on fresh connections, following redirects if enabled.
pub async fn send(request: PreparedRequest, span: &Span, cookies: Option<&CookieJar>, cancel: &CancellationToken) -> Result<Response> {
    send_with_pool(request, span, cookies, cancel, None).await
}

/// Like [`send`], reusing connections from `pool` when one is given.
pub(crate) async fn send_with_pool(
    request: PreparedRequest,
    span: &Span,
    cookies: Option<&CookieJar>,
    cancel: &CancellationToken,
    pool: Option<&ConnectionPool>,
) -> Result<Response> {
    let started = Instant::now();
    let mut current = request;
    let mut redirects = 0;
    loop {
        let hop = Hop { request: &current, span, cookies, cancel, pool };
        let response = tokio::select! {
            result = hop.send() => result?,
            () = cancel.cancelled() => return Err(Error::Cancelled),
        };
        let next = redirect_target(&current, &response);
        match next {
            Some(next) if current.options.follow_redirects => {
                if redirects >= current.options.max_redirects {
                    return Err(Error::TooManyRedirects(current.options.max_redirects));
                }
                redirects += 1;
                span.log(Level::Info, format!("Redirect {} → {}", response.status, next.url));
                current = next;
            }
            _ => {
                return Ok(Response { redirects, total: started.elapsed(), ..response });
            }
        }
    }
}

/// Works out the follow-up request for a 3xx response, like a browser would.
fn redirect_target(request: &PreparedRequest, response: &Response) -> Option<PreparedRequest> {
    if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        return None;
    }
    let location = response.header("location")?;
    let url = request.url.join(location).ok()?;
    let mut next = request.clone();
    // 303 turns anything but HEAD into GET; 301/302 turn POST into GET (as
    // browsers do); 307/308 keep the method and body.
    let to_get = (response.status == 303 && request.method != "HEAD") || (matches!(response.status, 301 | 302) && request.method == "POST");
    if to_get {
        next.method = "GET".into();
        next.body = Bytes::new();
        remove_headers(&mut next.headers, BODY_HEADERS);
    }
    if url.origin() != request.url.origin() {
        // Never leak credentials to another site.
        remove_headers(&mut next.headers, CREDENTIAL_HEADERS);
    }
    next.url = url;
    Some(next)
}

fn remove_headers(headers: &mut Vec<(String, String)>, names: &[&str]) {
    headers.retain(|(k, _)| !names.iter().any(|name| k.eq_ignore_ascii_case(name)));
}

/// Where a URL points: what a connection is opened to.
struct Target {
    secure: bool,
    /// Without IPv6 brackets, as DNS and TLS expect it.
    host: String,
    port: u16,
}

impl Target {
    fn of(url: &Url) -> Result<Self> {
        let secure = match url.scheme() {
            "https" => true,
            "http" => false,
            other => {
                return Err(Error::InvalidUrl { url: url.to_string(), reason: format!("unsupported scheme `{other}`, use http or https") });
            }
        };
        let host = url
            .host_str()
            .ok_or_else(|| Error::InvalidUrl { url: url.to_string(), reason: "missing host".into() })?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = url.port_or_known_default().unwrap_or(if secure { 443 } else { 80 });
        Ok(Self { secure, host, port })
    }
}

/// One request/response exchange (one hop of a redirect chain).
struct Hop<'a> {
    request: &'a PreparedRequest,
    span: &'a Span,
    cookies: Option<&'a CookieJar>,
    cancel: &'a CancellationToken,
    pool: Option<&'a ConnectionPool>,
}

/// Why an exchange failed.
enum Failure {
    /// A reused connection turned out to be closed before the request was
    /// answered (e.g. the server's keep-alive timeout). Safe to retry.
    Stale,
    Error(Error),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl Hop<'_> {
    async fn send(&self) -> Result<Response> {
        let target = Target::of(&self.request.url)?;
        let deadline = Instant::now() + Duration::from_millis(self.request.options.timeout_ms);
        let key = self.pool.map(|_| PoolKey::new(&target, &self.request.options));
        if let (Some(pool), Some(key)) = (self.pool, &key)
            && let Some(connection) = pool.checkout(key, deadline).await
        {
            match self.exchange(connection, Timings::default(), deadline, Some(key), true).await {
                // The server closed the idle connection: open a new one below.
                Err(Failure::Stale) => {}
                Err(Failure::Error(error)) => return Err(error),
                Ok(response) => return Ok(response),
            }
        }
        let mut timings = Timings::default();
        let connection = self.connect(&target, deadline, &mut timings).await?;
        if let (Some(pool), Some(key)) = (self.pool, &key) {
            pool.share(key, &connection);
        }
        self.exchange(connection, timings, deadline, key.as_ref(), false).await.map_err(|failure| match failure {
            Failure::Stale => Error::Http("the connection closed before the response arrived".into()),
            Failure::Error(error) => error,
        })
    }

    fn timeout(&self) -> Error {
        Error::Timeout(self.request.options.timeout_ms)
    }

    /// Opens a new connection: DNS, TCP connect, TLS, HTTP handshake.
    async fn connect(&self, target: &Target, deadline: Instant, timings: &mut Timings) -> Result<Connection> {
        let (request, span) = (self.request, self.span);
        let host = &target.host;

        // 1. DNS
        let t = Instant::now();
        let addrs: Vec<SocketAddr> = within(deadline, tokio::net::lookup_host((host.as_str(), target.port)))
            .await
            .map_err(|_| self.timeout())?
            .map_err(|source| Error::Dns { host: host.clone(), source })?
            .collect();
        timings.dns = t.elapsed();
        span.phase(Phase::Dns, timings.dns);

        // 2. TCP connect (try each address in order)
        let t = Instant::now();
        let mut last_error = None;
        let mut tcp = None;
        for addr in &addrs {
            match within(deadline, TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => {
                    tcp = Some((stream, *addr));
                    break;
                }
                Ok(Err(e)) => last_error = Some((addr.to_string(), e)),
                Err(_) => return Err(self.timeout()),
            }
        }
        let (tcp, remote_addr) = tcp.ok_or_else(|| match last_error {
            Some((addr, source)) => Error::Connect { addr, source },
            None => Error::Dns { host: host.clone(), source: std::io::Error::other("no addresses found") },
        })?;
        tcp.set_nodelay(true)?;
        timings.connect = t.elapsed();
        span.phase(Phase::Connect, timings.connect);

        // 3. TLS
        let (io, tls, use_h2): (Box<dyn Io>, Option<TlsInfo>, bool) = if target.secure {
            let t = Instant::now();
            let config = super::tls::client_config(request.options.verify_tls, request.options.http_version);
            let server_name = ServerName::try_from(host.clone())
                .map_err(|e| Error::InvalidUrl { url: request.url.to_string(), reason: e.to_string() })?;
            let stream = within(deadline, TlsConnector::from(config).connect(server_name, tcp))
                .await
                .map_err(|_| self.timeout())?
                .map_err(|e| Error::Tls { host: host.clone(), reason: e.to_string() })?;
            let elapsed = t.elapsed();
            timings.tls = Some(elapsed);
            span.phase(Phase::Tls, elapsed);
            let conn = stream.get_ref().1;
            let alpn = conn.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned());
            let info = TlsInfo {
                version: conn.protocol_version().map(|v| format!("{v:?}").replace('_', ".")).unwrap_or_default(),
                cipher: conn.negotiated_cipher_suite().map(|c| format!("{:?}", c.suite())).unwrap_or_default(),
                alpn: alpn.clone(),
            };
            (Box::new(stream), Some(info), alpn.as_deref() == Some("h2"))
        } else {
            // Plain-text HTTP/2 only when explicitly requested ("prior knowledge").
            (Box::new(tcp), None, request.options.http_version == HttpVersionPref::Http2)
        };
        span.emit(EventKind::Connected { remote_addr: remote_addr.to_string(), tls });

        // 4. HTTP handshake; the connection is driven by its own task.
        let sender = if use_h2 {
            let (sender, connection) = within(deadline, http2::handshake(TokioExecutor::new(), TokioIo::new(io)))
                .await
                .map_err(|_| self.timeout())?
                .map_err(|e| Error::Http(format!("HTTP/2 handshake failed: {e}")))?;
            tokio::spawn(connection);
            Sender::H2(sender)
        } else {
            let (sender, connection) = within(deadline, http1::handshake(TokioIo::new(io)))
                .await
                .map_err(|_| self.timeout())?
                .map_err(|e| Error::Http(format!("HTTP/1.1 handshake failed: {e}")))?;
            tokio::spawn(connection);
            Sender::H1(sender)
        };
        Ok(Connection { sender, remote_addr })
    }

    /// Sends the request on `connection` and reads the response.
    async fn exchange(
        &self,
        mut connection: Connection,
        mut timings: Timings,
        deadline: Instant,
        key: Option<&PoolKey>,
        reused: bool,
    ) -> Result<Response, Failure> {
        let (request, span) = (self.request, self.span);
        let url = &request.url;
        let remote_addr = connection.remote_addr;
        let use_h2 = connection.sender.is_h2();
        let headers = self.outgoing_headers(use_h2);
        let http_version = if use_h2 { "HTTP/2" } else { "HTTP/1.1" };
        span.emit(EventKind::RequestSent {
            method: request.method.clone(),
            url: url.to_string(),
            http_version: http_version.into(),
            headers: headers.iter().map(|(name, value)| Header { name: name.clone(), value: value.clone() }).collect(),
            body: span.bus().blobs().payload(&request.body, request.header("content-type").map(str::to_string)),
        });
        let hyper_request = build_request(request, &headers, use_h2)?;

        // 5. Send and wait for the response headers
        let t = Instant::now();
        let response = match within(deadline, connection.sender.send(hyper_request)).await {
            Err(_) => return Err(self.timeout().into()),
            Ok(Err(e)) if reused && is_stale(&e, &request.method) => return Err(Failure::Stale),
            Ok(Err(e)) => return Err(Error::Http(e.to_string()).into()),
            Ok(Ok(response)) => response,
        };
        timings.waiting = t.elapsed();
        span.phase(Phase::Waiting, timings.waiting);

        let status = response.status();
        let response_headers: Vec<(String, String)> =
            response.headers().iter().map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned())).collect();
        let response_version = match response.version() {
            http::Version::HTTP_2 => "HTTP/2",
            http::Version::HTTP_10 => "HTTP/1.0",
            _ => "HTTP/1.1",
        };
        if let Some(jar) = self.cookies {
            jar.store(url, response_headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.as_str()));
        }
        let content_type = response_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone());
        let is_sse = content_type.as_deref().is_some_and(|ct| ct.starts_with("text/event-stream"));

        // 6. Download the body (streaming events live for SSE)
        let t = Instant::now();
        let download = self.download(response.into_body(), is_sse).await?;
        timings.download = t.elapsed();
        span.phase(Phase::Download, timings.download);
        if download.too_large {
            span.log(Level::Warn, format!("Body larger than {} MB; the rest was not kept", MAX_BODY_BYTES / 1024 / 1024));
        }
        if download.complete
            && let (Some(pool), Some(key)) = (self.pool, key)
        {
            // Only a connection whose response was read completely can carry another request.
            pool.checkin(key, connection);
        }

        let raw = download.data;
        let body = match response_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-encoding")) {
            Some((_, encoding)) if !is_sse => decode::decode(encoding, raw.clone()).unwrap_or_else(|reason| {
                span.log(Level::Warn, reason);
                raw.clone()
            }),
            _ => raw.clone(),
        };

        let response = Response {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("").to_string(),
            http_version: response_version.to_string(),
            headers: response_headers,
            body,
            wire_size: raw.len() as u64,
            url: url.clone(),
            remote_addr: remote_addr.to_string(),
            timings,
            redirects: 0,
            total: Duration::ZERO,
        };
        span.emit(EventKind::ResponseReceived {
            status: response.status,
            status_text: response.status_text.clone(),
            http_version: response.http_version.clone(),
            headers: response.headers.iter().map(|(name, value)| Header { name: name.clone(), value: value.clone() }).collect(),
            body: span.bus().blobs().payload(&response.body, content_type),
        });
        Ok(response)
    }

    /// The request's headers plus cookies and defaults, as they will be sent.
    fn outgoing_headers(&self, use_h2: bool) -> Vec<(String, String)> {
        let request = self.request;
        let mut headers = request.headers.clone();
        if let Some(cookie) = self.cookies.and_then(|jar| jar.header_for(&request.url))
            && !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("cookie"))
        {
            headers.push(("Cookie".into(), cookie));
        }
        add_default(&mut headers, "User-Agent", USER_AGENT);
        add_default(&mut headers, "Accept", "*/*");
        add_default(&mut headers, "Accept-Encoding", "gzip, deflate, br");
        if !use_h2 {
            add_default(&mut headers, "Host", &host_header(&request.url));
        }
        if !request.body.is_empty() || matches!(request.method.as_str(), "POST" | "PUT" | "PATCH") {
            add_default(&mut headers, "Content-Length", &request.body.len().to_string());
        }
        headers
    }

    /// Reads the whole body, keeping at most [`MAX_BODY_BYTES`].
    async fn download(&self, mut body: Incoming, is_sse: bool) -> Result<Download> {
        let span = self.span;
        let mut data = BytesMut::new();
        let mut sse = is_sse.then(SseParser::new);
        let mut too_large = false;
        let mut complete = false;
        // Event streams may stay quiet for a long time; normal bodies may not.
        let idle_limit = if is_sse { STREAM_IDLE_LIMIT } else { Duration::from_millis(self.request.options.timeout_ms) };
        loop {
            let frame = tokio::select! {
                frame = tokio::time::timeout(idle_limit, body.frame()) => frame.map_err(|_| self.timeout())?,
                () = self.cancel.cancelled() => {
                    if is_sse {
                        // A cancelled stream ends normally, with what arrived so far.
                        break;
                    }
                    return Err(Error::Cancelled);
                }
            };
            let Some(frame) = frame else {
                complete = true;
                break;
            };
            let frame = frame.map_err(|e| Error::Http(format!("error while reading the body: {e}")))?;
            let Ok(chunk) = frame.into_data() else { continue };
            if let Some(parser) = sse.as_mut() {
                for event in parser.feed(&chunk) {
                    let bytes = Bytes::from(event.data);
                    span.emit(EventKind::Message {
                        direction: Direction::Received,
                        label: Some(event.event.unwrap_or_else(|| "message".into())),
                        payload: span.bus().blobs().payload(&bytes, None),
                    });
                }
            }
            if data.len() + chunk.len() > MAX_BODY_BYTES {
                too_large = true;
                if sse.is_none() {
                    break;
                }
                continue;
            }
            data.extend_from_slice(&chunk);
        }
        Ok(Download { data: data.freeze(), too_large, complete })
    }
}

/// A downloaded body.
struct Download {
    data: Bytes,
    /// More than [`MAX_BODY_BYTES`] arrived; the rest was not kept.
    too_large: bool,
    /// The body was read to its end (not cut off or cancelled).
    complete: bool,
}

/// True when hyper says the connection was gone before the request could be
/// answered, so sending it again on a new connection is safe.
fn is_stale(error: &hyper::Error, method: &str) -> bool {
    let idempotent = matches!(method, "GET" | "HEAD" | "OPTIONS" | "PUT" | "DELETE" | "TRACE");
    // `canceled`: never sent at all. Otherwise the server closed the
    // connection (often its keep-alive timeout) without answering; it may
    // have seen the request, so only idempotent ones are sent again.
    error.is_canceled() || (idempotent && (error.is_closed() || error.is_incomplete_message() || is_reset(error)))
}

fn is_reset(error: &hyper::Error) -> bool {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset};
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return matches!(io.kind(), ConnectionReset | ConnectionAborted | BrokenPipe);
        }
        source = cause.source();
    }
    false
}

fn build_request(request: &PreparedRequest, headers: &[(String, String)], use_h2: bool) -> Result<hyper::Request<Full<Bytes>>> {
    let url = &request.url;
    let method = http::Method::from_bytes(request.method.as_bytes())
        .map_err(|_| Error::InvalidRequest(format!("`{}` is not a valid HTTP method", request.method)))?;
    let mut builder = hyper::Request::builder().method(method);
    builder = if use_h2 {
        builder.uri(url.as_str()).version(http::Version::HTTP_2)
    } else {
        builder.uri(&url[url::Position::BeforePath..url::Position::AfterQuery])
    };
    for (name, value) in headers {
        if use_h2 && is_connection_specific(name) {
            continue; // forbidden in HTTP/2
        }
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Error::InvalidRequest(format!("`{name}` is not a valid header name")))?;
        let value = http::HeaderValue::from_str(value)
            .map_err(|_| Error::InvalidRequest(format!("the value of header `{name}` contains invalid characters")))?;
        builder = builder.header(name, value);
    }
    builder.body(Full::new(request.body.clone())).map_err(|e| Error::InvalidRequest(e.to_string()))
}

/// Limits a step to the time left before `deadline`.
fn within<F: std::future::IntoFuture>(deadline: Instant, step: F) -> tokio::time::Timeout<F::IntoFuture> {
    tokio::time::timeout_at(deadline.into(), step)
}

fn add_default(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
        headers.push((name.to_string(), value.to_string()));
    }
}

/// `host[:port]`, with IPv6 addresses in brackets and the port only when it
/// is not the scheme's default.
fn host_header(url: &Url) -> String {
    // `host_str` keeps the brackets of IPv6 literals; `port` is `None` for the default port.
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

fn is_connection_specific(name: &str) -> bool {
    ["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "host"].iter().any(|h| name.eq_ignore_ascii_case(h))
}

// ── Connection reuse ────────────────────────────────────────────────────────

/// An open connection, ready to carry a request.
struct Connection {
    sender: Sender,
    remote_addr: SocketAddr,
}

enum Sender {
    H1(http1::SendRequest<Full<Bytes>>),
    H2(http2::SendRequest<Full<Bytes>>),
}

impl Sender {
    fn is_h2(&self) -> bool {
        matches!(self, Sender::H2(_))
    }

    async fn send(&mut self, request: hyper::Request<Full<Bytes>>) -> hyper::Result<hyper::Response<Incoming>> {
        match self {
            Sender::H1(sender) => sender.send_request(request).await,
            Sender::H2(sender) => sender.send_request(request).await,
        }
    }

    /// Waits until the connection can take a request; `false` if it closed.
    async fn ready(&mut self, deadline: Instant) -> bool {
        let ready = match self {
            Sender::H1(sender) => within(deadline, sender.ready()).await,
            Sender::H2(sender) => within(deadline, sender.ready()).await,
        };
        matches!(ready, Ok(Ok(())))
    }
}

/// Which connections can serve a request: same scheme, host, port and TLS settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PoolKey {
    secure: bool,
    host: String,
    port: u16,
    verify_tls: bool,
    http_version: HttpVersionPref,
}

impl PoolKey {
    fn new(target: &Target, options: &RequestOptions) -> Self {
        Self {
            secure: target.secure,
            host: target.host.clone(),
            port: target.port,
            verify_tls: options.verify_tls,
            http_version: options.http_version,
        }
    }
}

/// Keeps connections open between requests (keep-alive), for load tests.
///
/// HTTP/1.1 connections carry one request at a time, so idle ones are handed
/// out exclusively and returned once their response was read completely.
/// HTTP/2 connections multiplex, so one is shared by every request.
#[derive(Default)]
pub(crate) struct ConnectionPool {
    state: Mutex<PoolState>,
}

#[derive(Default)]
struct PoolState {
    idle: HashMap<PoolKey, Vec<Connection>>,
    shared: HashMap<PoolKey, (http2::SendRequest<Full<Bytes>>, SocketAddr)>,
}

impl std::fmt::Debug for ConnectionPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.lock().expect("connection pool lock poisoned");
        f.debug_struct("ConnectionPool").field("idle", &state.idle.values().map(Vec::len).sum::<usize>()).finish_non_exhaustive()
    }
}

impl ConnectionPool {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// An open connection for `key`, if one is available.
    async fn checkout(&self, key: &PoolKey, deadline: Instant) -> Option<Connection> {
        loop {
            let mut connection = self.take(key)?;
            // A connection the server closed while idle is dropped; try the next one.
            if connection.sender.ready(deadline).await {
                return Some(connection);
            }
        }
    }

    fn take(&self, key: &PoolKey) -> Option<Connection> {
        let mut state = self.state.lock().expect("connection pool lock poisoned");
        if let Some((sender, remote_addr)) = state.shared.get(key) {
            if !sender.is_closed() {
                return Some(Connection { sender: Sender::H2(sender.clone()), remote_addr: *remote_addr });
            }
            state.shared.remove(key);
        }
        state.idle.get_mut(key)?.pop()
    }

    /// Makes a new HTTP/2 connection available to every request.
    fn share(&self, key: &PoolKey, connection: &Connection) {
        if let Sender::H2(sender) = &connection.sender {
            let mut state = self.state.lock().expect("connection pool lock poisoned");
            state.shared.insert(key.clone(), (sender.clone(), connection.remote_addr));
        }
    }

    /// Returns an HTTP/1.1 connection after its response was read.
    fn checkin(&self, key: &PoolKey, connection: Connection) {
        if let Sender::H1(sender) = &connection.sender
            && !sender.is_closed()
        {
            let mut state = self.state.lock().expect("connection pool lock poisoned");
            state.idle.entry(key.clone()).or_default().push(connection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(method: &str, url: &str, headers: &[(&str, &str)], body: &'static [u8]) -> PreparedRequest {
        PreparedRequest {
            method: method.into(),
            url: Url::parse(url).unwrap(),
            headers: headers.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect(),
            body: Bytes::from_static(body),
            options: RequestOptions::default(),
        }
    }

    fn redirect(status: u16, location: &str) -> Response {
        Response {
            status,
            status_text: String::new(),
            http_version: "HTTP/1.1".into(),
            headers: vec![("Location".into(), location.into())],
            body: Bytes::new(),
            wire_size: 0,
            url: Url::parse("http://a.test/").unwrap(),
            remote_addr: String::new(),
            timings: Timings::default(),
            redirects: 0,
            total: Duration::ZERO,
        }
    }

    #[test]
    fn redirects_change_the_method_like_browsers() {
        let post = prepared("POST", "http://a.test/form", &[("Content-Type", "application/json"), ("X-Keep", "1")], b"{}");
        for status in [301, 302, 303] {
            let next = redirect_target(&post, &redirect(status, "/done")).unwrap();
            assert_eq!(next.method, "GET", "{status}");
            assert!(next.body.is_empty());
            assert_eq!(next.header("content-type"), None);
            assert_eq!(next.header("x-keep"), Some("1"));
        }
        for status in [307, 308] {
            let next = redirect_target(&post, &redirect(status, "/done")).unwrap();
            assert_eq!(next.method, "POST", "{status}");
            assert_eq!(&next.body[..], b"{}");
            assert_eq!(next.header("content-type"), Some("application/json"));
        }
        let head = prepared("HEAD", "http://a.test/", &[], b"");
        assert_eq!(redirect_target(&head, &redirect(303, "/x")).unwrap().method, "HEAD");
        let put = prepared("PUT", "http://a.test/", &[], b"x");
        assert_eq!(redirect_target(&put, &redirect(302, "/x")).unwrap().method, "PUT");
        assert!(redirect_target(&put, &redirect(304, "/x")).is_none());
    }

    #[test]
    fn credentials_never_follow_a_redirect_to_another_origin() {
        let headers =
            [("Authorization", "Bearer t"), ("Proxy-Authorization", "Basic p"), ("Cookie", "a=1"), ("Host", "a.test"), ("X-Other", "1")];
        let request = prepared("GET", "https://a.test/", &headers, b"");
        let same = redirect_target(&request, &redirect(302, "/next")).unwrap();
        assert_eq!(same.header("authorization"), Some("Bearer t"));
        assert_eq!(same.header("cookie"), Some("a=1"));
        for location in ["https://b.test/", "http://a.test/", "https://a.test:8443/"] {
            let next = redirect_target(&request, &redirect(302, location)).unwrap();
            for name in ["authorization", "proxy-authorization", "cookie", "host"] {
                assert_eq!(next.header(name), None, "{name} must not be sent to {location}");
            }
            assert_eq!(next.header("x-other"), Some("1"));
        }
    }

    #[test]
    fn host_header_handles_ipv6_and_ports() {
        let host = |url: &str| host_header(&Url::parse(url).unwrap());
        assert_eq!(host("http://example.com/"), "example.com");
        assert_eq!(host("http://example.com:80/"), "example.com");
        assert_eq!(host("https://example.com:8443/"), "example.com:8443");
        assert_eq!(host("http://[::1]/"), "[::1]");
        assert_eq!(host("http://[::1]:8080/x"), "[::1]:8080");
        let target = Target::of(&Url::parse("https://[::1]:8443/").unwrap()).unwrap();
        assert_eq!((target.host.as_str(), target.port), ("::1", 8443));
    }
}
