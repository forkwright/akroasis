//! HTTP API error type for akroasis-server.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Body text returned for any failure whose cause is server-side.
///
/// WHY: the underlying `Display` strings name serial device paths, filesystem
/// paths and raw OS error text. The API is served without authentication, so
/// that detail goes to the server log and the caller sees only the status.
const INTERNAL_MESSAGE: &str = "internal server error";

/// A typed API error that serializes to a JSON body with `{ "error": "..." }`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    /// Serialized into the response body.
    message: String,
    /// Recorded in the server log when the response is built. Never serialized.
    detail: Option<String>,
}

impl ApiError {
    /// Construct a 500 Internal Server Error.
    ///
    /// `detail` is logged, not returned: the caller receives a fixed message.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: INTERNAL_MESSAGE.to_string(),
            detail: Some(detail.into()),
        }
    }

    /// Construct a 400 Bad Request. `message` is returned to the caller.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::client(StatusCode::BAD_REQUEST, message)
    }

    /// Construct a 404 Not Found. `message` is returned to the caller.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::client(StatusCode::NOT_FOUND, message)
    }

    /// Construct a client-error response with an explicit status.
    ///
    /// INVARIANT: `message` must describe a condition the caller can act on
    /// without disclosing server-side state. Anything derived from an internal
    /// error's `Display` belongs in [`ApiError::internal`] instead.
    pub fn client(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            detail: None,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Some(detail) = &self.detail {
            tracing::error!(status = %self.status, detail = %detail, "api request failed");
        }
        let body = serde_json::to_string(&ErrorBody {
            error: &self.message,
        })
        .unwrap_or_else(|_| r#"{"error":"serialization failure"}"#.to_string());
        (self.status, [("content-type", "application/json")], body).into_response()
    }
}

/// Convenience alias for route handler results.
pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;

    async fn parts_of(error: ApiError) -> (StatusCode, String, String) {
        let response = error.into_response();
        let status = response.status();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(err) => format!("<unreadable body: {err}>"),
        };
        (status, content_type, body)
    }

    #[tokio::test]
    async fn internal_reports_500_without_echoing_the_detail() {
        let (status, content_type, body) =
            parts_of(ApiError::internal("open /dev/ttyUSB0: permission denied")).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(content_type, "application/json");
        assert_eq!(body, r#"{"error":"internal server error"}"#);
        assert!(!body.contains("/dev/ttyUSB0"));
        assert!(!body.contains("permission denied"));
    }

    #[tokio::test]
    async fn bad_request_reports_400_and_returns_its_message() {
        let (status, _, body) = parts_of(ApiError::bad_request(
            "multiple radios detected; specify `port`",
        ))
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            r#"{"error":"multiple radios detected; specify `port`"}"#
        );
    }

    #[tokio::test]
    async fn not_found_reports_404_and_returns_its_message() {
        let (status, _, body) = parts_of(ApiError::not_found("no radio detected")).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, r#"{"error":"no radio detected"}"#);
    }

    #[tokio::test]
    async fn client_carries_the_status_it_is_given() {
        let (status, _, body) = parts_of(ApiError::client(
            StatusCode::FORBIDDEN,
            "port not accessible",
        ))
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, r#"{"error":"port not accessible"}"#);
    }

    #[tokio::test]
    async fn a_quote_in_the_message_stays_valid_json() {
        let (_, _, body) = parts_of(ApiError::bad_request(r#"unsupported format "x""#)).await;

        assert_eq!(body, r#"{"error":"unsupported format \"x\""}"#);
    }
}
