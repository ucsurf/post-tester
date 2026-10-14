//! The event data model.
//!
//! Every operation is a **span** (a request, a collection run, a plugin call…).
//! Things that happen during a span are **events**. Spans can have a parent,
//! so a collection run owns its requests, and a request owns its redirects.
//!
//! Events are small on purpose: large payloads (bodies, raw frames) are stored
//! in the [`BlobStore`](super::BlobStore) and referenced by [`BlobId`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use ts_rs::TS;
use uuid::Uuid;

/// Unique id of an operation (span).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TS)]
#[ts(export, type = "string")]
pub struct SpanId(pub Uuid);

impl SpanId {
    /// Creates a new, time-ordered span id.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for SpanId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Id of a stored blob (a large payload such as a response body).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, TS)]
#[ts(export, type = "string")]
pub struct BlobId(pub Uuid);

impl BlobId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for BlobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for BlobId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Ids are written as plain UUID strings.
macro_rules! uuid_string_serde {
    ($t:ident) => {
        impl Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.serialize(serializer)
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Uuid::deserialize(deserializer).map(Self)
            }
        }
    };
}
uuid_string_serde!(SpanId);
uuid_string_serde!(BlobId);

/// Who started an operation. Always visible in Live Activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Source {
    /// A person using the app.
    User,
    /// An AI agent connected over MCP.
    Agent { client: String },
    /// The `pt` command-line tool.
    Cli,
    /// A plugin.
    Plugin { name: String },
    /// A scheduled job.
    Schedule,
    /// post-tester itself (e.g. startup, plugin loading).
    System,
}

impl Source {
    /// A short, human-readable label such as `"AI agent (claude-code)"`.
    pub fn label(&self) -> String {
        match self {
            Source::User => "You".into(),
            Source::Agent { client } => format!("AI agent ({client})"),
            Source::Cli => "CLI".into(),
            Source::Plugin { name } => format!("Plugin {name}"),
            Source::Schedule => "Schedule".into(),
            Source::System => "System".into(),
        }
    }
}

/// What kind of operation a span is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SpanKind {
    Request,
    CollectionRun,
    Script,
    PluginCall,
    McpCall,
    WebSocket,
    System,
}

/// The one-line summary of a span, shown before any click.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Summary {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub url: Option<String>,
    /// Workspace id of the saved request, if any (e.g. `auth/login`).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub request_id: Option<String>,
}

/// Connection phases measured for the timing waterfall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Phase {
    /// Resolving the host name to an IP address.
    Dns,
    /// Opening the TCP connection.
    Connect,
    /// The TLS handshake.
    Tls,
    /// From sending the request until the first response byte.
    Waiting,
    /// Receiving the response body.
    Download,
}

/// Direction of a message on a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Direction {
    Sent,
    Received,
}

/// Severity of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

/// How a span ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Outcome {
    /// Completed and every test passed.
    Ok,
    /// Completed, but at least one test failed.
    Failed,
    /// Could not complete (network error, invalid request…).
    Error,
    /// Stopped by the user or because its parent stopped.
    Cancelled,
}

/// A header as it was sent or received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Header {
    pub name: String,
    pub value: String,
}

/// A payload that may be large: a short preview plus an optional blob.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Payload {
    /// The first part of the payload as text (lossy UTF-8).
    pub preview: String,
    /// The full size in bytes.
    #[ts(type = "number")]
    pub size: u64,
    /// True when `preview` is shorter than the full payload.
    pub truncated: bool,
    /// Where to fetch the full payload, if it was stored.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blob: Option<BlobId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content_type: Option<String>,
}

/// Information about the TLS connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TlsInfo {
    pub version: String,
    pub cipher: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub alpn: Option<String>,
}

/// Everything that can happen during a span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum EventKind {
    /// A span began.
    Started {
        kind: SpanKind,
        title: String,
        summary: Summary,
    },
    /// A connection phase finished.
    Phase { phase: Phase, duration_ms: f64 },
    /// The connection was established.
    Connected {
        remote_addr: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        tls: Option<TlsInfo>,
    },
    /// The exact request that was sent (after variables, auth and hooks).
    RequestSent {
        method: String,
        url: String,
        http_version: String,
        headers: Vec<Header>,
        body: Payload,
    },
    /// The response arrived (headers first; `body` is filled when complete).
    ResponseReceived {
        status: u16,
        status_text: String,
        http_version: String,
        headers: Vec<Header>,
        body: Payload,
    },
    /// A message on a long-lived connection (WebSocket, SSE, MCP).
    Message {
        direction: Direction,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        label: Option<String>,
        payload: Payload,
    },
    /// A log line from post-tester, a script or a plugin.
    Log { level: Level, text: String },
    /// One test assertion.
    TestResult {
        name: String,
        passed: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        detail: Option<String>,
    },
    /// Progress of a multi-step operation.
    Progress { done: u32, total: u32 },
    /// A variable was set by a script or an extraction rule.
    VariableSet { name: String, value: String },
    /// The span ended. Always the last event of a span.
    Finished {
        outcome: Outcome,
        duration_ms: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        status: Option<u16>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional, type = "number")]
        size: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        error: Option<String>,
    },
}

/// One thing that happened, at one moment, inside one span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Event {
    /// Increasing sequence number, unique per bus. Lets clients detect gaps.
    #[ts(type = "number")]
    pub seq: u64,
    pub span_id: SpanId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent_id: Option<SpanId>,
    pub source: Source,
    pub at: DateTime<Utc>,
    pub kind: EventKind,
}
