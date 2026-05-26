//! Axum router — assembles all API routes.

use axum::Router;
use axum::routing::get;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::{mesh, radio};

/// Build the complete akroasis API router.
///
/// Attach this to an axum `serve()` call or embed in a larger router.
#[must_use = "the router must be passed to axum::serve"]
pub fn build() -> Router {
    let api = Router::new()
        .route("/radio/detect", get(radio::detect))
        .route("/mesh/status", get(mesh::status))
        .route("/mesh/nodes", get(mesh::nodes))
        .route("/mesh/topology", get(mesh::topology));

    Router::new()
        .nest("/api/v1", api)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
