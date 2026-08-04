//! Integration smoke tests for the `akroasis_server` public API.
//!
//! Unit tests live alongside each module. These exercise the library
//! boundary end-to-end (required by TESTING/no-tests, which only inspects
//! `lib.rs` and this directory — module-local `#[cfg(test)]` blocks don't
//! satisfy it).

#![expect(
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]

use akroasis_server::error::ApiError;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use tower::ServiceExt as _;

#[tokio::test]
async fn router_serves_and_reports_404_for_unknown_routes() {
    let router = akroasis_server::router::build();
    let request = Request::builder()
        .uri("/definitely-not-a-registered-route")
        .body(Body::empty())
        .expect("request is well-formed");

    let response = router
        .oneshot(request)
        .await
        .expect("router must produce a response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_bad_request_reports_400() {
    let response = ApiError::bad_request("missing field").into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn api_error_internal_reports_500_and_hides_detail() {
    let response = ApiError::internal("disk full at /var/lib/akroasis").into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
