//! Extension points: how plugins (and scripts) plug into request execution.
//!
//! The core never depends on a plugin runtime. It calls this trait, and the
//! plugin host implements it. Every method has a "do nothing" default, so an
//! implementation only overrides what it supports.

use crate::error::{Error, Result};
use crate::events::Span;
use crate::expr::AssertionResult;
use crate::http::{PreparedRequest, Response};
use crate::model::OrderedMap;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// When a script runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptPhase {
    BeforeRequest,
    AfterResponse,
}

/// A request as scripts and hooks see it (body as text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl ScriptRequest {
    pub fn from_prepared(request: &PreparedRequest) -> Self {
        Self {
            method: request.method.clone(),
            url: request.url.to_string(),
            headers: request.headers.clone(),
            body: String::from_utf8_lossy(&request.body).into_owned(),
        }
    }

    /// Applies changes made by a script or hook back onto a prepared request.
    pub fn apply_to(self, request: &mut PreparedRequest) -> Result<()> {
        request.method = self.method.to_ascii_uppercase();
        request.url = url::Url::parse(&self.url).map_err(|e| Error::InvalidUrl { url: self.url.clone(), reason: e.to_string() })?;
        request.headers = self.headers;
        request.body = self.body.into_bytes().into();
        Ok(())
    }
}

/// A response as scripts and hooks see it (body as text).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScriptResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub time_ms: f64,
}

impl ScriptResponse {
    pub fn from_response(response: &Response) -> Self {
        Self {
            status: response.status,
            status_text: response.status_text.clone(),
            headers: response.headers.clone(),
            body: String::from_utf8_lossy(&response.body).into_owned(),
            time_ms: crate::events::millis(response.total),
        }
    }
}

/// Everything a script can read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptInput {
    pub request: ScriptRequest,
    pub response: Option<ScriptResponse>,
    /// All variables visible to the request, resolved by priority.
    pub variables: Vec<(String, String)>,
    pub environment: Option<String>,
}

/// What a script changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScriptOutput {
    /// The request after the script changed it (before-request scripts only).
    pub request: Option<ScriptRequest>,
    /// Variables to set for the rest of the session.
    pub set_variables: Vec<(String, String)>,
    pub unset_variables: Vec<String>,
    /// Results of `pt.test(...)` / `pm.test(...)` calls.
    pub tests: Vec<AssertionResult>,
}

/// Hooks into request execution. Implemented by the plugin host.
#[async_trait]
pub trait Extensions: Send + Sync {
    /// Calls a `{{function(...)}}`. Returns `Ok(None)` if no plugin provides it.
    async fn template_function(&self, _name: &str, _args: &OrderedMap, _span: &Span) -> Result<Option<String>> {
        Ok(None)
    }

    /// Applies a plugin auth method (`auth: { type: plugin, name: ... }`).
    async fn apply_auth(
        &self,
        name: &str,
        _config: &OrderedMap,
        _request: PreparedRequest,
        _span: &Span,
    ) -> Result<PreparedRequest> {
        Err(Error::Plugin(format!("no plugin provides the auth method `{name}`. Install the plugin or change the request's auth")))
    }

    /// Runs every `beforeRequest` hook, in plugin order.
    async fn before_request(&self, request: PreparedRequest, _span: &Span) -> Result<PreparedRequest> {
        Ok(request)
    }

    /// Runs every `afterResponse` hook, in plugin order.
    async fn after_response(&self, _request: &PreparedRequest, response: Response, _span: &Span) -> Result<Response> {
        Ok(response)
    }

    /// Runs a request script.
    async fn run_script(&self, _phase: ScriptPhase, _code: &str, _input: ScriptInput, _span: &Span) -> Result<ScriptOutput> {
        Err(Error::Plugin("scripts are not available in this build".into()))
    }
}

/// Extensions that do nothing. The default when no plugin host is attached.
#[derive(Debug, Default)]
pub struct NoExtensions;

impl Extensions for NoExtensions {}
