//! HTTP API error type for akroasis-server.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A typed API error that serializes to a JSON body with `{ "error": "..." }`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    /// Construct a 500 Internal Server Error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    /// Construct a 400 Bad Request.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    /// Construct a 404 Not Found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::to_string(&ErrorBody {
            error: &self.message,
        })
        .unwrap_or_else(|_| r#"{"error":"serialization failure"}"#.to_string());
        (
            self.status,
            [("content-type", "application/json")],
            body,
        )
            .into_response()
    }
}

/// Convenience alias for route handler results.
pub type ApiResult<T> = Result<T, ApiError>;
