//! Transport abstraction for Meshtastic radio connections.

use crate::error::Error;

/// Uniform interface over serial, TCP, and BLE Meshtastic transports.
///
/// Implementations are in P2-02. This trait defines the contract only.
// WHY: Rust 2024 native async-fn-in-traits is intentional here; no async-trait crate used.
#[expect(
    async_fn_in_trait,
    reason = "Rust 2024 native async fn in traits is intentional; implementations are Send"
)]
pub trait MeshConnection: Send + Sync {
    /// Send a raw protobuf-encoded packet to the radio.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerialIo`], [`Error::SendFailed`], or a transport-specific
    /// error if the write fails.
    async fn send(&mut self, packet: &[u8]) -> Result<(), Error>;

    /// Receive the next raw protobuf-encoded packet from the radio.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerialIo`], [`Error::ConnectionLost`], or a
    /// transport-specific error if the read fails.
    async fn recv(&mut self) -> Result<Vec<u8>, Error>;

    /// Returns `true` if the transport is currently connected.
    fn is_connected(&self) -> bool;

    /// Attempt to re-establish a dropped connection.
    ///
    /// # Errors
    ///
    /// Returns a connection error if reconnection fails.
    async fn reconnect(&mut self) -> Result<(), Error>;
}
