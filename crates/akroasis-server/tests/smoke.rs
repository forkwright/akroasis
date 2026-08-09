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

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use akroasis_server::error::ApiError;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
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

// ── request hardening (#194) ────────────────────────────────────────────────
//
// These exercise `router::harden` directly against synthetic handlers rather
// than the production routes, since none of the real handlers offer a way to
// deterministically hang or accept an oversized body from a test.

#[tokio::test(start_paused = true)]
async fn timeout_layer_bounds_a_handler_that_never_returns() {
    let router = akroasis_server::router::harden(Router::new().route(
        "/slow",
        get(|| async {
            // WHY: any duration well past REQUEST_TIMEOUT proves the
            // layer -- not the handler -- ends the request. Paused tokio
            // time makes this resolve without real wall-clock delay.
            tokio::time::sleep(Duration::from_secs(3600)).await; // kanon:ignore TESTING/sleep-in-test -- runs under start_paused = true; tokio's virtual clock resolves this without real wall-clock delay, which is the deterministic time control this rule wants
        }),
    ));

    let request = Request::builder()
        .uri("/slow")
        .body(Body::empty())
        .expect("request is well-formed");

    let response = router
        .oneshot(request)
        .await
        .expect("the timeout layer must convert an elapsed request into a response, not an error");
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
}

#[tokio::test]
async fn body_limit_layer_rejects_oversized_bodies() {
    let router = akroasis_server::router::harden(Router::new().route(
        "/echo",
        post(|body: axum::body::Bytes| async move { body.len().to_string() }),
    ));

    let oversized = vec![0_u8; akroasis_server::router::MAX_BODY_BYTES + 1];
    let request = Request::builder()
        .method("POST")
        .uri("/echo")
        .body(Body::from(oversized))
        .expect("request is well-formed");

    let response = router
        .oneshot(request)
        .await
        .expect("router must produce a response");
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn body_limit_layer_accepts_bodies_within_the_cap() {
    let router = akroasis_server::router::harden(Router::new().route(
        "/echo",
        post(|body: axum::body::Bytes| async move { body.len().to_string() }),
    ));

    let within_cap = vec![0_u8; 1024];
    let request = Request::builder()
        .method("POST")
        .uri("/echo")
        .body(Body::from(within_cap))
        .expect("request is well-formed");

    let response = router
        .oneshot(request)
        .await
        .expect("router must produce a response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn concurrency_limit_bounds_simultaneously_executing_handlers() {
    let in_flight = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));
    let in_flight_handler = Arc::clone(&in_flight);
    let max_observed_handler = Arc::clone(&max_observed);

    let router = akroasis_server::router::harden(Router::new().route(
        "/probe",
        get(move || {
            let in_flight = Arc::clone(&in_flight_handler);
            let max_observed = Arc::clone(&max_observed_handler);
            async move {
                let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                max_observed.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await; // kanon:ignore TESTING/sleep-in-test -- real concurrent scheduling must be observed; virtual/paused time collapses the overlap this test measures
                in_flight.fetch_sub(1, Ordering::SeqCst);
            }
        }),
    ));

    // WHY: fire well beyond MAX_CONCURRENT_REQUESTS so the cap -- not simple
    // request volume -- is what this test proves.
    let total_requests = akroasis_server::router::MAX_CONCURRENT_REQUESTS * 4;
    let mut handles = Vec::with_capacity(total_requests);
    for _ in 0..total_requests {
        let router = router.clone();
        handles.push(tokio::spawn(async move {
            let request = Request::builder()
                .uri("/probe")
                .body(Body::empty())
                .expect("request is well-formed");
            router
                .oneshot(request)
                .await
                .expect("router must produce a response")
        }));
    }
    for handle in handles {
        let response = handle.await.expect("handler task must not panic");
        assert_eq!(response.status(), StatusCode::OK);
    }

    assert!(
        max_observed.load(Ordering::SeqCst) <= akroasis_server::router::MAX_CONCURRENT_REQUESTS,
        "observed {} simultaneously executing handlers, more than the configured cap of {}",
        max_observed.load(Ordering::SeqCst),
        akroasis_server::router::MAX_CONCURRENT_REQUESTS,
    );
}
