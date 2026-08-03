//! `/api/v1/radio/*` route handlers.
//!
//! Each handler delegates to the `akroasis_lib::radio` dispatch path and returns
//! the same schema-versioned JSON used by `radio detect --json` / `radio
//! import --json` etc.

use akroasis_lib::radio::errors::RadioError;
use axum::Json;
use axum::extract::Query;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};

/// Query params for `GET /api/v1/radio/detect`.
#[derive(Deserialize)]
pub struct DetectQuery {
    /// Optional serial port to probe directly (e.g. `/dev/ttyUSB0`).
    pub port: Option<String>,
}

/// Classify a radio dispatch failure as a client or server error.
///
/// WHY: `RadioError` mixes conditions the caller can resolve (no radio
/// attached, an ambiguous port that `port` would disambiguate) with faults of
/// the host the server runs on. Reporting the first group as 500 tells the
/// caller to retry something that will never succeed on its own, and the
/// blanket 500 also echoed the error's `Display` text, which names the serial
/// device path. Server-side causes route to [`ApiError::internal`], which logs
/// the detail instead of returning it.
fn classify(err: &RadioError) -> ApiError {
    match err {
        RadioError::NoRadioDetected => ApiError::not_found(
            "no radio detected; check that the radio is on and the programming cable is connected",
        ),
        RadioError::MultipleRadiosDetected => {
            ApiError::bad_request("multiple radios detected; set the `port` query parameter")
        }
        RadioError::PermissionDenied { .. } => ApiError::client(
            StatusCode::FORBIDDEN,
            "the server is not permitted to open the requested serial port",
        ),
        RadioError::HardwareNotAvailable => ApiError::client(
            StatusCode::SERVICE_UNAVAILABLE,
            "radio hardware support is not available in this build",
        ),
        _ => ApiError::internal(err.to_string()),
    }
}

/// `GET /api/v1/radio/detect` — detect connected radios.
///
/// Returns the same JSON schema as `akroasis radio detect --json`.
///
/// # Errors
/// Returns 404 when no radio is attached, 400 when several are and `port` was
/// not given, 403 when the port cannot be opened, 503 when the build carries no
/// hardware support, and 500 for any other detection or JSON parse failure.
pub async fn detect(Query(params): Query<DetectQuery>) -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::radio::RadioCommand::Detect {
        port: params.port,
        json: true,
    };
    akroasis_lib::radio::dispatch(&cmd, &mut out).map_err(|e| classify(&e))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;

    use super::*;

    #[test]
    fn no_radio_is_a_404() {
        let response = classify(&RadioError::NoRadioDetected).into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn an_ambiguous_port_is_a_400() {
        let response = classify(&RadioError::MultipleRadiosDetected).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_denied_port_is_a_403() {
        let err = RadioError::PermissionDenied {
            port: "/dev/ttyUSB0".to_string(),
        };
        let response = classify(&err).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn an_unclassified_failure_is_a_500_that_does_not_echo_the_path() {
        let err = RadioError::ReadFile {
            path: std::path::PathBuf::from("/var/lib/akroasis/image.img"),
            source: std::io::Error::other("permission denied"),
        };
        assert!(
            err.to_string().contains("/var/lib/akroasis/image.img"),
            "the source error must carry the path, or this proves nothing"
        );

        let response = classify(&err).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(read_err) => format!("<unreadable body: {read_err}>"),
        };
        assert_eq!(body, r#"{"error":"internal server error"}"#);
        assert!(!body.contains("/var/lib/akroasis"));
    }
}
