//! Axum router — assembles all API routes.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::error_handling::HandleErrorLayer;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use tokio::sync::Semaphore;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;

use crate::error::ApiError;
use crate::{mesh, radio};

/// Maximum accepted request body size, in bytes.
///
/// WHY(#194): no current route reads a request body, but the limit is
/// applied once at the router level rather than per-route so any future
/// body-reading route inherits the bound automatically instead of relying on
/// each handler to remember it.
pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Maximum number of requests the router services concurrently.
///
/// WHY(#194): the API is unauthenticated and reachable on the LAN. Without a
/// cap, an attacker can open unbounded simultaneous connections against a
/// slow handler (e.g. `/radio/detect?port=` probing an unresponsive serial
/// device) and exhaust server resources.
pub const MAX_CONCURRENT_REQUESTS: usize = 64;

/// Per-request timeout applied to every route.
///
/// WHY(#194): generous enough for USB radio detection, but bounds any
/// handler that would otherwise occupy its worker indefinitely.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Apply the request-hardening middleware common to every akroasis API router.
///
/// Layered outermost to innermost: a request body-size limit (reject before
/// doing any other work), a per-request timeout, and a global concurrency cap
/// (gates admission to a route handler).
///
/// Exposed separately from [`build`] so tests can exercise the middleware
/// stack against a synthetic handler without needing radio hardware.
pub fn harden(router: Router) -> Router {
    // WHY(#194): `tower::limit::ConcurrencyLimitLayer` bounds nothing when
    // applied via `Router::layer` — axum's router clones and calls the
    // matched route's service directly per request rather than routing
    // `poll_ready` backpressure through the layered stack, so the semaphore
    // that layer relies on is never actually contended. An explicit
    // `Semaphore`-gated `from_fn` middleware enforces the bound directly,
    // independent of that propagation gap.
    let concurrency = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

    router
        .layer(middleware::from_fn(move |request, next| {
            let concurrency = Arc::clone(&concurrency);
            limit_concurrency(concurrency, request, next)
        }))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(handle_middleware_error))
                .timeout(REQUEST_TIMEOUT),
        )
        .layer(RequestBodyLimitLayer::new(MAX_BODY_BYTES))
}

/// Hold a permit from `concurrency` for the duration of `next`, blocking
/// admission once [`MAX_CONCURRENT_REQUESTS`] handlers are already running.
async fn limit_concurrency(concurrency: Arc<Semaphore>, request: Request, next: Next) -> Response {
    #[expect(
        clippy::expect_used,
        reason = "the only closer is `Semaphore::close`, which `harden` never calls; the \
                  `Arc` this closure holds keeps the semaphore itself alive for the router's lifetime"
    )]
    let _permit = concurrency
        .acquire_owned()
        .await
        .expect("semaphore is never closed while `harden` holds the strong reference to it"); // SAFETY: harden() never calls Semaphore::close(); acquire_owned() can only fail after close()
    next.run(request).await
}

/// Convert a middleware failure into the same JSON envelope every other API
/// error uses.
async fn handle_middleware_error(err: tower::BoxError) -> ApiError {
    if err.is::<tower::timeout::error::Elapsed>() {
        ApiError::client(StatusCode::REQUEST_TIMEOUT, "request timed out")
    } else {
        ApiError::internal(err.to_string())
    }
}

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

    harden(Router::new().nest("/api/v1", api))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}
