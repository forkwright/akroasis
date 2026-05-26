//! Server entry-point — bind and run the akroasis HTTP API.

use std::net::SocketAddr;

use snafu::Snafu;

use crate::router;

/// Errors from starting or running the server.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum ServeError {
    /// Failed to bind to the requested address.
    #[snafu(display("failed to bind to {addr}: {source}"))]
    Bind {
        /// The address that could not be bound.
        addr: SocketAddr,
        /// The underlying I/O error.
        source: std::io::Error,
    },

    /// The server encountered a fatal error while running.
    #[snafu(display("server error: {source}"))]
    Serve {
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

/// Start the akroasis HTTP API server on the given address.
///
/// Runs until the process exits or a fatal I/O error occurs.
///
/// # Errors
///
/// Returns [`ServeError`] if the address cannot be bound or the server
/// encounters a fatal error.
pub async fn serve(addr: SocketAddr) -> Result<(), ServeError> {
    let app = router::build();

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|source| ServeError::Bind { addr, source })?;

    tracing::info!(%addr, "akroasis-server listening");

    axum::serve(listener, app)
        .await
        .map_err(|source| ServeError::Serve { source })
}
