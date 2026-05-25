//! Transport implementations for Meshtastic radio connections.
//!
//! Provides concrete [`MeshConnection`] implementations for serial and TCP
//! transports, plus a factory function that creates the right transport from a
//! [`ConnectionConfig`].

pub mod serial;
pub mod tcp;

use self::serial::SerialTransport;
use self::tcp::TcpTransport;
use crate::Error;
use crate::config::{ConnectionConfig, TransportConfig};
use crate::connection::MeshConnection;
use crate::error::BleConnectSnafu;
use crate::proto::{FromRadio, ToRadio};
use tracing::instrument;

/// A concrete, enum-dispatched connection to a Meshtastic radio.
///
/// Implements [`MeshConnection`] by forwarding calls to the active transport
/// variant.  An enum is used instead of `Box<dyn MeshConnection>` because
/// native async fn in traits (Rust 2024) is not object-safe.
#[non_exhaustive]
pub enum ConnectionHandle {
    /// USB serial transport.
    Serial(SerialTransport),
    /// TCP/IP transport.
    Tcp(TcpTransport),
}

impl MeshConnection for ConnectionHandle {
    async fn send(&mut self, packet: ToRadio) -> Result<(), Error> {
        match self {
            Self::Serial(c) => c.send(packet).await,
            Self::Tcp(c) => c.send(packet).await,
        }
    }

    async fn recv(&mut self) -> Result<FromRadio, Error> {
        match self {
            Self::Serial(c) => c.recv().await,
            Self::Tcp(c) => c.recv().await,
        }
    }

    fn is_connected(&self) -> bool {
        match self {
            Self::Serial(c) => c.is_connected(),
            Self::Tcp(c) => c.is_connected(),
        }
    }

    async fn reconnect(&mut self) -> Result<(), Error> {
        match self {
            Self::Serial(c) => c.reconnect().await,
            Self::Tcp(c) => c.reconnect().await,
        }
    }
}

/// Create a [`ConnectionHandle`] from a [`ConnectionConfig`] with default
/// transport tuning.
///
/// # Errors
///
/// Returns a transport-specific connection error if the initial connect fails.
#[instrument(level = "debug", skip(config), fields(connection = ?config))]
pub async fn connect(config: &ConnectionConfig) -> Result<ConnectionHandle, Error> {
    connect_with_config(config, &TransportConfig::default()).await
}

/// Create a [`ConnectionHandle`] from a [`ConnectionConfig`] with explicit
/// transport tuning.
///
/// # Errors
///
/// Returns a transport-specific connection error if the initial connect fails.
#[instrument(
    level = "debug",
    skip(config, transport_config),
    fields(connection = ?config)
)]
pub async fn connect_with_config(
    config: &ConnectionConfig,
    transport_config: &TransportConfig,
) -> Result<ConnectionHandle, Error> {
    match config {
        ConnectionConfig::Serial { port, baud } => {
            let conn = SerialTransport::open_with_config(port, *baud, transport_config).await?;
            Ok(ConnectionHandle::Serial(conn))
        }
        ConnectionConfig::Tcp { addr, port } => {
            let conn = TcpTransport::connect_with_config(addr, *port, transport_config).await?;
            Ok(ConnectionHandle::Tcp(conn))
        }
        ConnectionConfig::Ble { device_name } => {
            // WHY: BLE transport is deferred; serial and TCP cover all hardware targets.
            BleConnectSnafu {
                device: device_name.clone(),
            }
            .fail()
        }
    }
}
