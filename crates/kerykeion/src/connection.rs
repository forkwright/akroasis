//! Transport abstraction for serial/TCP/BLE packet I/O.

use crate::error::Error;

/// Uniform interface over serial, TCP, and BLE transports.
///
/// Implementations handle framing (serial/TCP use the 4-byte header,
/// BLE sends raw protobuf bytes).
pub trait MeshConnection: Send + Sync {
    /// Send a framed packet to the device.
    fn send(
        &mut self,
        packet: &[u8],
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;

    /// Receive the next complete packet from the device.
    fn recv(&mut self) -> impl std::future::Future<Output = Result<Vec<u8>, Error>> + Send;

    /// Whether the connection is currently alive.
    fn is_connected(&self) -> bool;

    /// Attempt to re-establish a dropped connection.
    fn reconnect(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send;
}
