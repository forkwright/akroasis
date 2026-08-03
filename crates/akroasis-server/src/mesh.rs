//! `/api/v1/mesh/*` route handlers.

use akroasis_lib::mesh::MeshError;
use axum::Json;

use crate::error::{ApiError, ApiResult};

/// Classify a mesh dispatch failure as a client or server error.
///
/// WHY: only `NodeNotFound` names something the caller supplied, so it is the
/// one variant whose text is safe to return. The rest wrap I/O and
/// serialization failures whose `Display` carries host filesystem paths and OS
/// error text, and this API is served without authentication, so they route to
/// [`ApiError::internal`] which logs the detail instead of returning it.
fn classify(err: &MeshError) -> ApiError {
    match err {
        MeshError::NodeNotFound { identifier } => {
            ApiError::not_found(format!("node not found: {identifier}"))
        }
        _ => ApiError::internal(err.to_string()),
    }
}

/// `GET /api/v1/mesh/status` — mesh network status.
///
/// Returns the same JSON schema as `akroasis mesh status --json`.
///
/// # Errors
/// Returns 404 if the mesh command names a node that does not exist, and an
/// HTTP 500 [`ApiError`] if the command otherwise fails or JSON parse fails.
pub async fn status() -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::mesh::MeshCommand::Status { json: true };
    akroasis_lib::mesh::dispatch(&cmd, &mut out).map_err(|e| classify(&e))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

/// `GET /api/v1/mesh/nodes` — list mesh nodes.
///
/// # Errors
/// Returns 404 if the mesh command names a node that does not exist, and an
/// HTTP 500 [`ApiError`] if the command otherwise fails or JSON parse fails.
pub async fn nodes() -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::mesh::MeshCommand::Nodes { json: true };
    akroasis_lib::mesh::dispatch(&cmd, &mut out).map_err(|e| classify(&e))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

/// `GET /api/v1/mesh/topology` — mesh network topology.
///
/// # Errors
/// Returns 404 if the mesh command names a node that does not exist, and an
/// HTTP 500 [`ApiError`] if the command otherwise fails or JSON parse fails.
pub async fn topology() -> ApiResult<Json<serde_json::Value>> {
    let mut out = Vec::new();
    let cmd = akroasis_lib::mesh::MeshCommand::Topology { json: true };
    akroasis_lib::mesh::dispatch(&cmd, &mut out).map_err(|e| classify(&e))?;
    let value: serde_json::Value =
        serde_json::from_slice(&out).map_err(|e| ApiError::internal(format!("json parse: {e}")))?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use super::*;

    #[test]
    fn a_missing_node_is_a_404() {
        let err = MeshError::NodeNotFound {
            identifier: "node-7".to_string(),
        };
        let response = classify(&err).into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn an_io_failure_is_a_500_that_does_not_echo_the_os_error() {
        let err = MeshError::Io {
            source: std::io::Error::other("/var/lib/akroasis/mesh.db is unreadable"),
        };
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
