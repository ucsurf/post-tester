//! API errors: an HTTP status plus a message that says how to fix the problem.

use axum::Json;
use axum::http::StatusCode;
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
