//! API errors: an HTTP status plus a message that says how to fix the problem.

use axum::Json;
use axum::body::to_bytes;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use post_tester_core::Error;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        let status = match &error {
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::InvalidRequest(_)
            | Error::InvalidUrl { .. }
            | Error::Expression { .. }
            | Error::Template(_)
            | Error::Import(_)
            | Error::File { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.message }))).into_response()
    }
}

/// Turns the plain-text errors axum produces itself (malformed JSON, a body
/// that is too large, a wrong method…) into the `{ "error": … }` shape that
/// every other API error has.
pub async fn json_errors(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    let status = response.status();
    let plain_text = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|content_type| content_type.starts_with("text/plain"));
    if !(status.is_client_error() || status.is_server_error()) || !plain_text {
        return response;
    }
    let (parts, body) = response.into_parts();
    let text = to_bytes(body, 64 * 1024).await.unwrap_or_default();
    let text = String::from_utf8_lossy(&text).trim().to_string();
    let message = match status {
        StatusCode::PAYLOAD_TOO_LARGE => format!(
            "the request body is too large (limit: {} MB, {} MB for imports)",
            crate::API_BODY_LIMIT / (1024 * 1024),
            crate::IMPORT_BODY_LIMIT / (1024 * 1024)
        ),
        StatusCode::METHOD_NOT_ALLOWED => "this endpoint does not accept this HTTP method; see API.md".to_string(),
        _ if text.is_empty() => status.canonical_reason().unwrap_or("error").to_string(),
        _ => text,
    };
    let mut wrapped = ApiError::new(status, message).into_response();
    for (name, value) in &parts.headers {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            wrapped.headers_mut().insert(name.clone(), value.clone());
        }
    }
    wrapped
}
