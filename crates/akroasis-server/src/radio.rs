//! `/api/v1/radio/*` route handlers.
//!
//! Each handler delegates to the `akroasis_lib::radio` dispatch path and returns
//! the same schema-versioned JSON used by `radio detect --json` / `radio
//! import --json` etc.

use axum::Json;
use axum::extract::Query;
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};

/// Query params for `GET /api/v1/radio/detect`.
#[derive(Deserialize)]
pub struct DetectQuery {
    /// Optional serial port to probe directly (e.g. `/dev/ttyUSB0`).
    pub port: Option<String>,
}

/// `GET /api/v1/radio/detect` — detect connected radios.
///
/// Returns the same JSON schema as `akroasis radio detect --json`.
pub async fn detect(Query(params): Query<DetectQuery>) -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::radio::RadioCommand::Detect {
        port: params.port,
        json: true,
    };
    akroasis_lib::radio::dispatch(&cmd, &mut out).map_err(|e| ApiError::internal(e.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}
