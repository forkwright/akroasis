//! `/api/v1/mesh/*` route handlers.

use axum::Json;

use crate::error::{ApiError, ApiResult};

/// `GET /api/v1/mesh/status` — mesh network status.
///
/// Returns the same JSON schema as `akroasis mesh status --json`.
pub async fn status() -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::mesh::MeshCommand::Status { json: true };
    akroasis_lib::mesh::dispatch(&cmd, &mut out).map_err(|e| ApiError::internal(e.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

/// `GET /api/v1/mesh/nodes` — list mesh nodes.
pub async fn nodes() -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::mesh::MeshCommand::Nodes { json: true };
    akroasis_lib::mesh::dispatch(&cmd, &mut out).map_err(|e| ApiError::internal(e.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

/// `GET /api/v1/mesh/topology` — mesh network topology.
pub async fn topology() -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::mesh::MeshCommand::Topology { json: true };
    akroasis_lib::mesh::dispatch(&cmd, &mut out).map_err(|e| ApiError::internal(e.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}
