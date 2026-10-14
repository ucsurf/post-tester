//! HTTP: sending requests with full timing, cookies, redirects and SSE streaming.

mod client;
mod cookies;
pub mod decode;
pub mod sse;
mod tls;

pub use client::send;
pub use cookies::CookieJar;

use crate::expr::ResponseData;
use crate::model::RequestOptions;
use bytes::Bytes;
use serde::Serialize;
use std::time::Duration;
use url::Url;

/// A request ready to send: variables resolved, auth applied, body encoded.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRequest {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    pub options: RequestOptions,
}

impl PreparedRequest {
    /// First header with this name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Replaces a header, or adds it if missing.
    pub fn set_header(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            Some(slot) => slot.1 = value,
            None => self.headers.push((name.to_string(), value)),
        }
    }
}

/// How long each phase took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Timings {
    pub dns: Duration,
    pub connect: Duration,
    pub tls: Option<Duration>,
    pub waiting: Duration,
    pub download: Duration,
}

/// A received response.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    /// The decoded (decompressed) body.
    pub body: Bytes,
    /// Bytes received on the wire, before decompression.
    pub wire_size: u64,
    /// The final URL, after redirects.
    pub url: Url,
    pub remote_addr: String,
    /// Timings of the final hop.
    pub timings: Timings,
    pub redirects: u32,
    /// Total time including redirects.
    pub total: Duration,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// A view used by tests and extraction expressions.
    pub fn data(&self) -> ResponseData<'_> {
        ResponseData { status: self.status, headers: &self.headers, body: &self.body, time_ms: crate::events::millis(self.total) }
    }
}
