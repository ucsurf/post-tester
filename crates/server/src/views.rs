//! JSON views of engine results, shaped for the UI (see API.md).

use post_tester_core::Execution;
use post_tester_core::events::{EventBus, Outcome, Payload, SpanId, millis};
use post_tester_core::expr::AssertionResult;
use post_tester_core::http::{PreparedRequest, Response};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedView {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl From<&PreparedRequest> for PreparedView {
    fn from(r: &PreparedRequest) -> Self {
        Self {
            method: r.method.clone(),
            url: r.url.to_string(),
            headers: r.headers.clone(),
            body: String::from_utf8_lossy(&r.body).into_owned(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingsView {
    pub dns_ms: f64,
    pub connect_ms: f64,
    pub tls_ms: Option<f64>,
    pub waiting_ms: f64,
    pub download_ms: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseView {
    pub status: u16,
    pub status_text: String,
    pub http_version: String,
    pub headers: Vec<(String, String)>,
    pub body: Payload,
    pub size: u64,
    pub wire_size: u64,
    pub time_ms: f64,
    pub timings: TimingsView,
    pub remote_addr: String,
    pub url: String,
    pub redirects: u32,
}

impl ResponseView {
    pub fn new(r: &Response, bus: &EventBus) -> Self {
        Self {
            status: r.status,
            status_text: r.status_text.clone(),
            http_version: r.http_version.clone(),
            headers: r.headers.clone(),
            body: bus.blobs().payload(&r.body, r.header("content-type").map(str::to_string)),
            size: r.body.len() as u64,
            wire_size: r.wire_size,
            time_ms: millis(r.total),
            timings: TimingsView {
                dns_ms: millis(r.timings.dns),
                connect_ms: millis(r.timings.connect),
                tls_ms: r.timings.tls.map(millis),
                waiting_ms: millis(r.timings.waiting),
                download_ms: millis(r.timings.download),
            },
            remote_addr: r.remote_addr.clone(),
            url: r.url.to_string(),
            redirects: r.redirects,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionView {
    pub span_id: SpanId,
    pub outcome: Outcome,
    pub error: Option<String>,
    pub request: Option<PreparedView>,
    pub response: Option<ResponseView>,
    pub tests: Vec<AssertionResult>,
    pub extracted: Vec<(String, String)>,
}

impl ExecutionView {
    pub fn new(e: &Execution, bus: &EventBus) -> Self {
        Self {
            span_id: e.span_id,
            outcome: e.outcome,
            error: e.error.clone(),
            request: e.request.as_ref().map(PreparedView::from),
            response: e.response.as_ref().map(|r| ResponseView::new(r, bus)),
            tests: e.tests.clone(),
            extracted: e.extracted.clone(),
        }
    }
}
