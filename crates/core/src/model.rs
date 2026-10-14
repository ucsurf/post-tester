//! The data model, exactly as it is stored in workspace YAML files.
//!
//! A minimal request file:
//!
//! ```yaml
//! method: GET
//! url: "{{baseUrl}}/users"
//! ```

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A map that keeps keys in the order they were written, so saved YAML files
/// stay in the order people wrote them.
pub type OrderedMap = serde_json::Map<String, serde_json::Value>;

/// A saved request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct Request {
    /// Display name. Defaults to the file name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Longer explanation, shown in the app and to AI agents.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub method: Method,
    pub url: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Body::is_none")]
    pub body: Body,
    /// `None` means "inherit from the folder".
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auth: Option<Auth>,
    /// JavaScript that runs before the request is sent and after the response arrives.
    #[serde(skip_serializing_if = "Scripts::is_empty")]
    pub scripts: Scripts,
    /// Assertions such as `status == 200` or `json($.id) exists`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
    /// Variables to capture from the response, e.g. `token: json($.token)`.
    #[serde(skip_serializing_if = "OrderedMap::is_empty")]
    #[ts(type = "Record<string, string>")]
    pub extract: OrderedMap,
    #[serde(skip_serializing_if = "RequestOptions::is_default")]
    pub options: RequestOptions,
    /// Saved example responses. `pt mock` serves them as a mock API.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<Example>>", optional)]
    pub examples: Vec<Example>,
}

/// A saved example response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Example {
    pub name: String,
    #[serde(default = "ok_status")]
    pub status: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    /// Wait this long before answering, to simulate a slow server.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(optional, as = "Option<u32>")]
    pub delay_ms: u64,
}

fn ok_status() -> u16 {
    200
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// HTTP method. Any token is allowed, so custom methods work too.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
#[ts(export, type = "string")]
pub struct Method(pub String);

impl Serialize for Method {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Method {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|s| Self(s.to_ascii_uppercase()))
    }
}

impl Default for Method {
    fn default() -> Self {
        Self("GET".into())
    }
}

impl Method {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Method {
    fn from(s: &str) -> Self {
        Self(s.to_ascii_uppercase())
    }
}

/// A key/value pair that can be switched off without deleting it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    #[ts(as = "Option<bool>", optional)]
    pub enabled: bool,
}

impl KeyValue {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self { key: key.into(), value: value.into(), enabled: true }
    }
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// The request body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Body {
    #[default]
    None,
    /// Raw text with an explicit content type.
    Text {
        content: String,
        #[serde(default = "text_plain")]
        content_type: String,
    },
    /// JSON text. Sent with `Content-Type: application/json`.
    Json { content: String },
    /// `application/x-www-form-urlencoded`.
    Form { fields: Vec<KeyValue> },
    /// A GraphQL operation, sent as JSON over HTTP POST.
    Graphql {
        query: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        variables: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        operation_name: Option<String>,
    },
}

fn text_plain() -> String {
    "text/plain".into()
}

impl Body {
    pub fn is_none(&self) -> bool {
        matches!(self, Body::None)
    }
}

/// How the request authenticates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Auth {
    /// Explicitly no auth (stops folder inheritance).
    None,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    /// An API key in a header or query parameter.
    ApiKey {
        key: String,
        value: String,
        #[serde(default)]
        location: ApiKeyLocation,
    },
    /// Auth provided by a plugin, e.g. `{ type: plugin, name: "hmac-auth/signature", config: {...} }`.
    Plugin {
        name: String,
        #[serde(default)]
        #[ts(type = "Record<string, unknown>")]
        config: OrderedMap,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApiKeyLocation {
    #[default]
    Header,
    Query,
}

/// Scripts attached to a request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Scripts {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub before_request: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub after_response: String,
}

impl Scripts {
    pub fn is_empty(&self) -> bool {
        self.before_request.trim().is_empty() && self.after_response.trim().is_empty()
    }
}

/// Per-request transport options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct RequestOptions {
    /// Time limit until the response headers arrive.
    #[ts(type = "number")]
    pub timeout_ms: u64,
    pub follow_redirects: bool,
    #[ts(type = "number")]
    pub max_redirects: u32,
    /// Set to `false` to accept invalid or self-signed certificates.
    pub verify_tls: bool,
    pub http_version: HttpVersionPref,
}

impl Default for RequestOptions {
    fn default() -> Self {
        Self { timeout_ms: 30_000, follow_redirects: true, max_redirects: 10, verify_tls: true, http_version: HttpVersionPref::Auto }
    }
}

impl RequestOptions {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum HttpVersionPref {
    /// HTTP/2 when the server supports it over TLS, otherwise HTTP/1.1.
    #[default]
    Auto,
    Http1,
    Http2,
}

/// Settings shared by every request in a folder (`_folder.yaml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct FolderSettings {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auth: Option<Auth>,
    #[serde(skip_serializing_if = "OrderedMap::is_empty")]
    #[ts(type = "Record<string, string>")]
    pub variables: OrderedMap,
}

/// A named set of variables (`environments/<name>.yaml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct Environment {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[ts(type = "Record<string, string>")]
    pub variables: OrderedMap,
}

/// Workspace settings (`post-tester.yaml`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
#[ts(export)]
pub struct WorkspaceSettings {
    pub name: String,
    /// Variables available in every environment.
    #[serde(skip_serializing_if = "OrderedMap::is_empty")]
    #[ts(type = "Record<string, string>")]
    pub variables: OrderedMap,
    /// Plugin folders, relative to the workspace root.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<String>,
    /// Plugin permissions the user approved, by plugin name.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub approved_permissions: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub default_environment: Option<String>,
}

/// Turns a YAML/JSON scalar into the string used for variable substitution.
pub fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_request_round_trips() {
        let yaml = "method: POST\nurl: https://example.com\n";
        let req: Request = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(req.method.as_str(), "POST");
        assert_eq!(serde_yaml::to_string(&req).unwrap(), yaml);
    }

    #[test]
    fn full_request_parses() {
        let yaml = r#"
name: Login
method: POST
url: "{{baseUrl}}/login"
headers:
  - { key: X-Trace, value: "1", enabled: false }
body:
  type: json
  content: '{"user": "{{user}}"}'
auth:
  type: bearer
  token: "{{secret:token}}"
tests:
  - status == 200
extract:
  token: json($.token)
"#;
        let req: Request = serde_yaml::from_str(yaml).unwrap();
        assert!(!req.headers[0].enabled);
        assert!(matches!(req.body, Body::Json { .. }));
        assert!(matches!(req.auth, Some(Auth::Bearer { .. })));
        assert_eq!(req.extract["token"], "json($.token)");
    }

    #[test]
    fn unknown_fields_are_rejected_with_a_clear_error() {
        let err = serde_yaml::from_str::<Request>("method: GET\nurll: x\n").unwrap_err();
        assert!(err.to_string().contains("unknown field `urll`"));
    }
}
