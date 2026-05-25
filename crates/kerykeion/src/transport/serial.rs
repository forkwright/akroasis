//! Serial transport for Meshtastic radio connections.
//!
//! Opens a serial port at 115 200 baud (no flow control, DTR/RTS disabled) and
//! wraps it in a [`tokio_util::codec::Framed`] with .

use futures::{SinkExt as _, StreamExt as _};
use tokio_serial::{SerialPort as _, SerialPortBuilderExt as _, SerialStream};
use tokio_util::codec::Framed;
use tracing::instrument;

use crate::Error;
use crate::codec::MeshCodec;
use crate::config::TransportConfig;
use crate::connection::MeshConnection;
use crate::error::ConnectionLostSnafu;
use crate::proto::{FromRadio, ToRadio};

// Historical default (max_backoff = 30 s) now lives in
// [`TransportConfig::default`].

/// Meshtastic transport over a USB serial port.
pub struct SerialTransport {
    /// Device path (e.g. `/dev/ttyUSB0`).
    port_path: String,
    /// Baud rate (Meshtastic uses 115 200).
    baud: u32,
    /// Framed codec sitting on top of the open serial stream.
    framed: Framed<SerialStream, MeshCodec>,
    /// Whether the port is currently open and healthy.
    connected: bool,
    /// Tuning applied to reconnect attempts.
    config: TransportConfig,
}

impl SerialTransport {
    /// Open a serial port with the default tuning.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerialConnect`] if the port cannot be opened.
    #[instrument(level = "debug", skip(port), fields(port = %port, baud))]
    pub async fn open(port: &str, baud: u32) -> Result<Self, Error> {
        Self::open_with_config(port, baud, &TransportConfig::default()).await
    }

    /// Open a serial port with the supplied tuning configuration.
    ///
    /// # Errors
    ///
    /// Returns [`Error::SerialConnect`] if the port cannot be opened.
    #[instrument(level = "debug", skip(port, config), fields(port = %port, baud))]
    pub async fn open_with_config(
        port: &str,
        baud: u32,
        config: &TransportConfig,
    ) -> Result<Self, Error> {
        let stream = open_serial_stream(port, baud)?;
        Ok(Self {
            port_path: port.to_owned(),
            baud,
            framed: Framed::new(stream, MeshCodec),
            connected: true,
            config: config.clone(),
        })
    }
}

/// Open the raw `SerialStream` with the Meshtastic-required settings.
fn open_serial_stream(port: &str, baud: u32) -> Result<SerialStream, Error> {
    // WHY: tokio-serial returns `tokio_serial::Error` (a serialport error type) which
    // does not implement `Into<std::io::Error>` directly; convert via std::io::Error::other.
    let mut stream = tokio_serial::new(port, baud)
        .flow_control(tokio_serial::FlowControl::None)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .data_bits(tokio_serial::DataBits::Eight)
        .open_native_async()
        .map_err(|e| Error::SerialConnect {
            source: std::io::Error::other(e),
            port: port.to_owned(),
            location: snafu::Location::new(file!(), line!(), column!()),
        })?;

    // WHY: Meshtastic firmware does not use hardware handshake lines; asserting
    // DTR/RTS causes some devices to reboot on connect.
    let _ = stream.write_data_terminal_ready(false);
    let _ = stream.write_request_to_send(false);

    Ok(stream)
}

impl MeshConnection for SerialTransport {
    async fn send(&mut self, packet: ToRadio) -> Result<(), Error> {
        let result = self.framed.send(packet).await;
        if result.is_err() {
            self.connected = false;
        }
        result
    }

    async fn recv(&mut self) -> Result<FromRadio, Error> {
        match self.framed.next().await {
            Some(Ok(msg)) => Ok(msg),
            Some(Err(e)) => {
                self.connected = false;
                Err(e)
            }
            None => {
                self.connected = false;
                ConnectionLostSnafu {
                    detail: format!("serial port {} closed (EOF)", self.port_path),
                }
                .fail()
            }
        }
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn reconnect(&mut self) -> Result<(), Error> {
        self.connected = false;
        let mut delay = self.config.reconnect_initial_delay();
        let max_backoff = self.config.reconnect_max_backoff();

        loop {
            tokio::time::sleep(delay).await;
            match open_serial_stream(&self.port_path, self.baud) {
                Ok(stream) => {
                    self.framed = Framed::new(stream, MeshCodec);
                    self.connected = true;
                    tracing::info!(port = %self.port_path, "serial reconnected");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        delay_secs = delay.as_secs(),
                        port = %self.port_path,
                        "serial reconnect failed; retrying"
                    );
                    delay = (delay * 2).min(max_backoff);
                }
            }
        }
    }
}
