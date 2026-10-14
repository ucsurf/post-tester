//! Sends one HTTP request and reports every step live.
//!
//! Instead of a high-level client, each step is done explicitly so it can be
//! timed: DNS → TCP connect → TLS handshake → send → first byte → download.
//! Each request uses a fresh connection, so timings are always complete and
//! comparable.

use super::cookies::CookieJar;
use super::sse::SseParser;
use super::{PreparedRequest, Response, Timings, decode};
use crate::error::{Error, Result};
use crate::events::{Direction, EventKind, Header, Level, Phase, Span, TlsInfo};
use crate::model::HttpVersionPref;
use bytes::{Bytes, BytesMut};
use http_body_util::{BodyExt, Full};
use hyper::client::conn::{http1, http2};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::net::SocketAddr;
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
const STREAM_IDLE_LIMIT: Duration = Duration::from_secs(24 * 60 * 60);

const USER_AGENT: &str = concat!("post-tester/", env!("CARGO_PKG_VERSION"));

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Sends a request, following redirects if enabled.
pub async fn send(
    request: PreparedRequest,
    span: &Span,
    cookies: Option<&CookieJar>,
    cancel: &CancellationToken,
) -> Result<Response> {
    let started = Instant::now();
    let mut current = request;
    let mut redirects = 0;
    loop {
        let response = tokio::select! {
            result = send_once(&current, span, cookies, cancel) => result?,
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
    // 303 always becomes GET; 301/302 turn POST into GET (as browsers do).
    let to_get = response.status == 303 || (matches!(response.status, 301 | 302) && request.method == "POST");
    if to_get {
        next.method = "GET".into();
        next.body = Bytes::new();
        next.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("content-length"));
    }
    if url.origin() != request.url.origin() {
        // Never leak credentials to another site.
        next.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization") && !k.eq_ignore_ascii_case("cookie"));
    }
    next.url = url;
    Some(next)
}

async fn send_once(
    request: &PreparedRequest,
    span: &Span,
    cookies: Option<&CookieJar>,
    cancel: &CancellationToken,
) -> Result<Response> {
    let url = &request.url;
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
    let timeout = Duration::from_millis(request.options.timeout_ms);
    let deadline = Instant::now() + timeout;
    let mut timings = Timings::default();

    // 1. DNS
    let t = Instant::now();
    let addrs: Vec<SocketAddr> = within(deadline, tokio::net::lookup_host((host.as_str(), port)))
        .await
        .map_err(|_| Error::Timeout(request.options.timeout_ms))?
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
            Err(_) => return Err(Error::Timeout(request.options.timeout_ms)),
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
    let (io, tls, use_h2): (Box<dyn Io>, Option<TlsInfo>, bool) = if secure {
        let t = Instant::now();
        let config = super::tls::client_config(request.options.verify_tls, request.options.http_version);
        let server_name = ServerName::try_from(host.clone())
            .map_err(|e| Error::InvalidUrl { url: url.to_string(), reason: e.to_string() })?;
        let stream = within(deadline, TlsConnector::from(config).connect(server_name, tcp))
            .await
            .map_err(|_| Error::Timeout(request.options.timeout_ms))?
            .map_err(|e| Error::Tls { host: host.clone(), reason: e.to_string() })?;
        timings.tls = Some(t.elapsed());
        span.phase(Phase::Tls, t.elapsed());
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

    // 4. Build and send the request
    let mut headers = request.headers.clone();
    if let Some(cookie) = cookies.and_then(|jar| jar.header_for(url))
        && !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("cookie"))
    {
        headers.push(("Cookie".into(), cookie));
    }
    add_default(&mut headers, "User-Agent", USER_AGENT);
    add_default(&mut headers, "Accept", "*/*");
    add_default(&mut headers, "Accept-Encoding", "gzip, deflate, br");
    if !use_h2 {
        add_default(&mut headers, "Host", &host_header(url));
    }
    if !request.body.is_empty() || matches!(request.method.as_str(), "POST" | "PUT" | "PATCH") {
        add_default(&mut headers, "Content-Length", &request.body.len().to_string());
    }

    let http_version = if use_h2 { "HTTP/2" } else { "HTTP/1.1" };
    span.emit(EventKind::RequestSent {
        method: request.method.clone(),
        url: url.to_string(),
        http_version: http_version.into(),
        headers: headers.iter().map(|(name, value)| Header { name: name.clone(), value: value.clone() }).collect(),
        body: span.bus().blobs().payload(&request.body, request.header("content-type").map(str::to_string)),
    });

    let mut builder = hyper::Request::builder().method(
        http::Method::from_bytes(request.method.as_bytes())
            .map_err(|_| Error::InvalidRequest(format!("`{}` is not a valid HTTP method", request.method)))?,
    );
    builder = if use_h2 {
        builder.uri(url.as_str()).version(http::Version::HTTP_2)
    } else {
        builder.uri(&url[url::Position::BeforePath..url::Position::AfterQuery])
    };
    for (name, value) in &headers {
        if use_h2 && is_connection_specific(name) {
            continue; // forbidden in HTTP/2
        }
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| Error::InvalidRequest(format!("`{name}` is not a valid header name")))?;
        let value = http::HeaderValue::from_str(value)
            .map_err(|_| Error::InvalidRequest(format!("the value of header `{name}` contains invalid characters")))?;
        builder = builder.header(name, value);
    }
    let hyper_request = builder.body(Full::new(request.body.clone())).map_err(|e| Error::InvalidRequest(e.to_string()))?;

    let t = Instant::now();
    let response = if use_h2 {
        let (mut sender, connection) = http2::handshake(TokioExecutor::new(), TokioIo::new(io))
            .await
            .map_err(|e| Error::Http(format!("HTTP/2 handshake failed: {e}")))?;
        tokio::spawn(connection);
        within(deadline, sender.send_request(hyper_request)).await
    } else {
        let (mut sender, connection) =
            http1::handshake(TokioIo::new(io)).await.map_err(|e| Error::Http(format!("HTTP/1.1 handshake failed: {e}")))?;
        tokio::spawn(connection);
        within(deadline, sender.send_request(hyper_request)).await
    }
    .map_err(|_| Error::Timeout(request.options.timeout_ms))?
    .map_err(|e| Error::Http(e.to_string()))?;
    timings.waiting = t.elapsed();
    span.phase(Phase::Waiting, timings.waiting);

    let status = response.status();
    let response_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    let response_version = match response.version() {
        http::Version::HTTP_2 => "HTTP/2",
        http::Version::HTTP_10 => "HTTP/1.0",
        _ => "HTTP/1.1",
    };
    if let Some(jar) = cookies {
        jar.store(
            url,
            response_headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie")).map(|(_, v)| v.as_str()),
        );
    }
    let content_type =
        response_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone());
    let is_sse = content_type.as_deref().is_some_and(|ct| ct.starts_with("text/event-stream"));

    // 5. Download the body (streaming events live for SSE)
    let t = Instant::now();
    let mut body = response.into_body();
    let mut data = BytesMut::new();
    let mut sse = is_sse.then(SseParser::new);
    let mut cut_off = false;
    // Event streams may stay quiet for a long time; normal bodies may not.
    let idle_limit = if is_sse { STREAM_IDLE_LIMIT } else { timeout };
    loop {
        let frame = tokio::select! {
            frame = tokio::time::timeout(idle_limit, body.frame()) => {
                frame.map_err(|_| Error::Timeout(request.options.timeout_ms))?
            }
            () = cancel.cancelled() => {
                if is_sse { break } else { return Err(Error::Cancelled) }
            }
        };
        let Some(frame) = frame else { break };
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
            cut_off = true;
            if sse.is_none() {
                break;
            }
            continue;
        }
        data.extend_from_slice(&chunk);
    }
    timings.download = t.elapsed();
    span.phase(Phase::Download, timings.download);
    if cut_off {
        span.log(Level::Warn, format!("Body larger than {} MB; the rest was not kept", MAX_BODY_BYTES / 1024 / 1024));
    }

    let raw = data.freeze();
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

/// Limits a step to the time left before `deadline`.
fn within<F: std::future::IntoFuture>(deadline: Instant, step: F) -> tokio::time::Timeout<F::IntoFuture> {
    tokio::time::timeout_at(deadline.into(), step)
}

fn add_default(headers: &mut Vec<(String, String)>, name: &str, value: &str) {
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
        headers.push((name.to_string(), value.to_string()));
    }
}

fn host_header(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

fn is_connection_specific(name: &str) -> bool {
    ["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "host"]
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
}
