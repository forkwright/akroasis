//! Transport abstraction for Meshtastic radio connections.

use crate::error::Error;
use crate::proto::{FromRadio, ToRadio};

/// Uniform interface over serial, TCP, and BLE Meshtastic transports.
///
/// Each implementation wraps a [`tokio_util::codec::Framed`] stream that applies
/// the 4-byte Meshtastic frame header codec.
// WHY: Rust 2024 native async-fn-in-traits is intentional here; no async-trait crate used.
#[expect(
    async_fn_in_trait,
    reason = "Rust 2024 native async fn in traits is intentional; implementations are Send"
)]
pub trait MeshConnection: Send + Sync {
    /// Send a `ToRadio` message to the radio.
    ///
    /// The implementation encodes the message via prost and wraps it in the
    /// 4-byte Meshtastic frame header before writing to the underlying transport.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerialIo`], [`Error::SendFailed`], or a transport-specific
    /// error if the write fails.
    async fn send(&mut self, packet: ToRadio) -> Result<(), Error>;

    /// Receive the next `FromRadio` message from the radio.
    ///
    /// Reads one complete framed packet, strips the 4-byte header, and decodes
    /// the protobuf payload.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerialIo`], [`Error::ConnectionLost`], or a
    /// transport-specific error if the read fails.
    async fn recv(&mut self) -> Result<FromRadio, Error>;

    /// Returns `true` if the transport is currently connected.
    fn is_connected(&self) -> bool;

    /// Attempt to re-establish a dropped connection with exponential backoff.
    ///
    /// Backoff schedule: 1 s, 2 s, 4 s, 8 s, capped at 30 s.
    ///
    /// # Errors
    ///
    /// Returns a connection error if reconnection ultimately fails (implementations
    /// may loop indefinitely).
    async fn reconnect(&mut self) -> Result<(), Error>;
}
