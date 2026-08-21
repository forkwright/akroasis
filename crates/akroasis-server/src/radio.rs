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

/// Longest `port` value accepted. Real serial device names are far shorter;
/// the bound keeps a pathological query out of the dispatch path and the log.
const MAX_PORT_LEN: usize = 64;

/// Message returned for every rejected `port`.
///
/// WHY one fixed string: the endpoint is unauthenticated, so a message that
/// varied with the supplied value — or echoed it — would answer questions about
/// the server's filesystem. A caller that sent a valid path does not need to be
/// told which rule it broke.
const INVALID_PORT_MESSAGE: &str = "`port` must be a serial device path such as /dev/ttyUSB0";

/// Returns `true` when `port` names a serial device node directly under `/dev`.
///
/// The check is lexical and deliberately so: it runs before any filesystem
/// access, so it cannot be raced, and it holds identically on a host with no
/// radio attached. Requiring a single path component under `/dev` is what
/// rejects traversal, `/dev/serial/by-id` indirection, and anything that can
/// walk out of `/dev`; the OS still enforces that the node is openable.
fn is_serial_device_path(port: &str) -> bool {
    if port.len() > MAX_PORT_LEN {
        return false;
    }
    let Some(name) = port.strip_prefix("/dev/") else {
        return false;
    };
    if name.contains('/') {
        return false;
    }
    let Some(suffix) = name.strip_prefix("tty") else {
        return false;
    };
    // A bare `/dev/tty` is the controlling terminal, not a radio, so the
    // suffix must be non-empty as well as alphanumeric.
    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Validate the caller-supplied `port` before it reaches the device open.
///
/// # Errors
///
/// Returns a 400 carrying [`INVALID_PORT_MESSAGE`] when the value is present
/// and is not a serial device path. An absent `port` is valid and means
/// auto-detect.
fn validated_port(port: Option<String>) -> Result<Option<String>, ApiError> {
    match port {
        None => Ok(None),
        Some(port) if is_serial_device_path(&port) => Ok(Some(port)),
        Some(_) => Err(ApiError::bad_request(INVALID_PORT_MESSAGE)),
    }
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
/// Returns 400 when `port` is not a serial device path or when several radios
/// are attached and `port` was not given, 404 when no radio is attached, 403
/// when the port cannot be opened, 503 when the build carries no hardware
/// support, and 500 for any other detection or JSON parse failure.
pub async fn detect(Query(params): Query<DetectQuery>) -> ApiResult<Json<serde_json::Value>> {
    let port = validated_port(params.port)?;
    let mut out = Vec::new();
    let cmd = akroasis_lib::radio::RadioCommand::Detect { port, json: true };
    akroasis_lib::radio::dispatch(&cmd, &mut out).map_err(|e| classify(&e))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;

    use super::*;

    /// Anti-vacuity: the rejection cases below prove nothing if the validator
    /// refuses everything, so pin the shapes it must accept first.
    #[test]
    fn real_serial_device_paths_are_accepted() {
        for port in [
            "/dev/ttyUSB0",
            "/dev/ttyACM1",
            "/dev/ttyS0",
            "/dev/ttyUSB10",
        ] {
            assert!(
                is_serial_device_path(port),
                "{port} is a serial device and must be accepted"
            );
        }
    }

    #[test]
    fn an_absent_port_means_auto_detect_and_is_not_an_error() {
        let accepted = validated_port(None).expect("an absent port is valid");
        assert!(accepted.is_none(), "an absent port must stay absent");
    }

    #[test]
    fn paths_that_are_not_serial_devices_are_rejected() {
        for port in [
            "/dev/../etc/passwd",      // traversal out of /dev
            "/etc/passwd",             // outside /dev entirely
            "/dev/serial/by-id/usb-0", // a second path component
            "/dev/tty",                // the controlling terminal, not a radio
            "/dev/mem",                // a device, but not a serial one
            "/dev/ttyUSB0/../../mem",  // traversal behind a valid-looking prefix
            "/dev/ttyUSB0\0",          // an embedded NUL
            "dev/ttyUSB0",             // relative, so not anchored at /dev
            "",                        // empty
            "/dev/",                   // no device name at all
        ] {
            assert!(
                !is_serial_device_path(port),
                "{port:?} is not a serial device path and must be rejected"
            );
        }
    }

    #[test]
    fn an_overlong_port_is_rejected_before_anything_looks_at_it() {
        let port = format!("/dev/tty{}", "A".repeat(MAX_PORT_LEN));
        assert!(
            port.len() > MAX_PORT_LEN,
            "the fixture must exceed the bound, or this proves nothing"
        );
        assert!(!is_serial_device_path(&port));
    }

    /// The disclosure half of the finding: a rejection must not tell the caller
    /// anything about the path it supplied, or the endpoint is a filesystem
    /// probe oracle that happens to return 400.
    #[tokio::test]
    async fn a_rejected_port_is_a_400_that_does_not_echo_the_supplied_path() {
        let probe = "/dev/../root/.ssh/id_ed25519";
        let error =
            validated_port(Some(probe.to_owned())).expect_err("a traversal path must be rejected");

        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = match axum::body::to_bytes(response.into_body(), usize::MAX).await {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(read_err) => format!("<unreadable body: {read_err}>"),
        };
        assert!(
            !body.contains("/root") && !body.contains("id_ed25519") && !body.contains(probe),
            "the rejection body must not echo the supplied path, got {body}"
        );
        assert!(
            body.contains("serial device path"),
            "the caller still needs to know what shape is expected, got {body}"
        );
    }

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
