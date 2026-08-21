//! BLE GATT transport for Meshtastic radio connections.
//!
//! GATT is message-oriented rather than a byte stream, so this transport does
//! not sit on the [`crate::codec::MeshCodec`] framing the serial and TCP
//! transports share. It encodes and decodes protobuf message bodies directly
//! and delegates every radio interaction to an injected [`BlePeripheral`].
//!
//! That injection is the point: the transport's connect, send, receive,
//! notification-wait and reconnect behaviour is exercised against a
//! deterministic in-memory peripheral, so none of it depends on an adapter or a
//! radio being present.

use prost::Message as _;
use tracing::instrument;

use crate::Error;
use crate::config::TransportConfig;
use crate::connection::MeshConnection;
use crate::error::{BleConnectSnafu, ConnectionLostSnafu};
use crate::proto::{FromRadio, ToRadio};

/// The scan and GATT operations a BLE adapter must provide.
///
/// Payloads crossing this boundary are complete protobuf message bodies. A GATT
/// characteristic operation carries a whole value, so the stream framing the
/// byte-oriented transports apply does not belong here; an implementation that
/// needs a header for its own link is responsible for it on both sides.
///
/// Implementations are not required to be cancel-safe.
// WHY: matches `MeshConnection` — Rust 2024 native async fn in traits, no
// `async_trait` crate, so callers are generic over the implementation rather
// than boxing it.
#[expect(
    async_fn_in_trait,
    reason = "mirrors MeshConnection; implementations are Send and consumers are generic"
)]
pub trait BlePeripheral: Send + Sync {
    /// Scan for a device whose advertised name starts with `name_prefix` and
    /// establish a GATT connection to it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BleConnect`] when no advertising device matches the
    /// prefix, or when the connection or service discovery fails.
    async fn connect(&mut self, name_prefix: &str) -> Result<(), Error>;

    /// Write one encoded `ToRadio` body to the radio.
    ///
    /// # Errors
    ///
    /// Returns a transport error if the characteristic write fails.
    async fn write_to_radio(&mut self, payload: &[u8]) -> Result<(), Error>;

    /// Read the next queued `FromRadio` body, or `None` when the radio has
    /// nothing waiting.
    ///
    /// `None` means "empty right now", never "disconnected" — a dropped link is
    /// an error, so that a caller cannot mistake a dead radio for a quiet one.
    ///
    /// # Errors
    ///
    /// Returns a transport error if the characteristic read fails or the link
    /// has dropped.
    async fn read_from_radio(&mut self) -> Result<Option<Vec<u8>>, Error>;

    /// Wait until the radio signals that at least one message may be readable.
    ///
    /// # Errors
    ///
    /// Returns a transport error if the link drops while waiting.
    async fn wait_for_data(&mut self) -> Result<(), Error>;

    /// Returns `true` while the GATT link is established.
    fn is_connected(&self) -> bool;
}

/// Meshtastic transport over a BLE GATT link.
pub struct BleTransport<P> {
    /// Advertising name prefix used to find the device on every connect.
    device_name: String,
    /// The injected adapter every radio interaction is delegated to.
    peripheral: P,
    /// Whether the transport believes the link is usable.
    connected: bool,
    /// Tuning applied to reconnect attempts.
    config: TransportConfig,
}

impl<P> BleTransport<P>
where
    P: BlePeripheral,
{
    /// Connect `peripheral` to the first device advertising `device_name`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BleConnect`] if no matching device is reachable.
    #[instrument(level = "debug", skip(peripheral), fields(device = %device_name))]
    pub async fn connect(device_name: &str, peripheral: P) -> Result<Self, Error> {
        Self::connect_with_config(device_name, peripheral, &TransportConfig::default()).await
    }

    /// Connect `peripheral` with explicit transport tuning.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BleConnect`] if no matching device is reachable.
    #[instrument(level = "debug", skip(peripheral, config), fields(device = %device_name))]
    pub async fn connect_with_config(
        device_name: &str,
        mut peripheral: P,
        config: &TransportConfig,
    ) -> Result<Self, Error> {
        peripheral.connect(device_name).await?;
        Ok(Self {
            device_name: device_name.to_owned(),
            peripheral,
            connected: true,
            config: config.clone(),
        })
    }

    /// Borrow the injected peripheral.
    pub const fn peripheral(&self) -> &P {
        &self.peripheral
    }
}

impl<P> MeshConnection for BleTransport<P>
where
    P: BlePeripheral,
{
    async fn send(&mut self, packet: ToRadio) -> Result<(), Error> {
        let payload = packet.encode_to_vec();
        let result = self.peripheral.write_to_radio(&payload).await;
        if result.is_err() {
            self.connected = false;
        }
        result
    }

    async fn recv(&mut self) -> Result<FromRadio, Error> {
        loop {
            match self.peripheral.read_from_radio().await {
                Ok(Some(payload)) => {
                    return FromRadio::decode(payload.as_slice()).map_err(|source| {
                        Error::ProtobufDecode {
                            source,
                            location: snafu::location!(),
                        }
                    });
                }
                // WHY loop rather than return: an empty read is the radio being
                // quiet, not the link being closed, so the only correct response
                // is to wait for the notification that says otherwise. Returning
                // here would surface "no message yet" as a received message.
                Ok(None) => {
                    if let Err(error) = self.peripheral.wait_for_data().await {
                        self.connected = false;
                        return Err(error);
                    }
                }
                Err(error) => {
                    self.connected = false;
                    return Err(error);
                }
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.connected && self.peripheral.is_connected()
    }

    async fn reconnect(&mut self) -> Result<(), Error> {
        self.connected = false;
        let mut delay = self.config.reconnect_initial_delay();
        let max_backoff = self.config.reconnect_max_backoff();

        loop {
            tokio::time::sleep(delay).await;
            match self.peripheral.connect(&self.device_name).await {
                Ok(()) => {
                    self.connected = true;
                    tracing::info!(device = %self.device_name, "BLE reconnected");
                    return Ok(());
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        delay_secs = delay.as_secs(),
                        device = %self.device_name,
                        "BLE reconnect failed; retrying"
                    );
                    delay = (delay * 2).min(max_backoff);
                }
            }
        }
    }
}

/// Build the [`Error::BleConnect`] a peripheral returns when a scan finds no
/// device advertising the requested prefix.
///
/// # Errors
///
/// Always returns [`Error::BleConnect`]; the `Result` shape lets an
/// implementation return it with `?`.
pub fn no_matching_device<T>(device: &str) -> Result<T, Error> {
    BleConnectSnafu {
        device: device.to_owned(),
    }
    .fail()
}

/// Build the [`Error::ConnectionLost`] a peripheral returns when the GATT link
/// drops out from under an operation.
///
/// # Errors
///
/// Always returns [`Error::ConnectionLost`]; the `Result` shape lets an
/// implementation return it with `?`.
pub fn link_dropped<T>(detail: impl Into<String>) -> Result<T, Error> {
    ConnectionLostSnafu {
        detail: detail.into(),
    }
    .fail()
}

#[cfg(test)]
#[path = "ble_tests.rs"]
mod tests;
