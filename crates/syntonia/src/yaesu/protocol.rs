//! Yaesu FTM-510DR clone-mode serial protocol.
//!
//! # Status
//!
//! Scaffolded. The clone-mode handshake and block transfer protocol have
//! NOT been reverse-engineered. This module will remain non-functional
//! until USB traffic from the ADMS-14 software is captured and analyzed.
//!
//! See forkwright/akroasis#80 for tracking.

use snafu::Snafu;

use crate::serial::SerialPort;

use super::variant;

/// Errors from Yaesu protocol operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum YaesuProtocolError {
    /// Protocol not yet reverse-engineered.
    #[snafu(display(
        "Yaesu FTM-510DR clone protocol not yet reversed (requires ADMS-14 traffic capture, see #80)"
    ))]
    ProtocolNotYetReversed,

    /// Serial I/O error.
    #[snafu(display("serial I/O error: {message}"))]
    SerialIo {
        /// Description of the I/O failure.
        message: String,
    },

    /// Handshake failed — radio did not respond to identification request.
    #[snafu(display("handshake failed: no response from radio"))]
    HandshakeFailed,
}

/// Yaesu clone-mode session.
///
/// Wraps a serial port connection to the FTM-510DR. All operations
/// currently return `ProtocolNotYetReversed` until the protocol
/// is documented.
pub struct YaesuSession<S: SerialPort> {
    _port: S,
}

impl<S: SerialPort> YaesuSession<S> {
    /// Open a clone-mode session to a Yaesu radio.
    ///
    /// Configures the serial port to [`variant::BAUD_RATE`] (38400 baud).
    ///
    /// # Errors
    ///
    /// Returns `YaesuProtocolError::ProtocolNotYetReversed`.
    pub fn open(_port: S) -> Result<Self, YaesuProtocolError> {
        // TODO(#80): configure port to 38400 baud, send identification
        // request, wait for model string response.
        //
        // Known from ADMS-14 observation:
        // - Baud: 38400
        // - Flow control: none
        // - The radio sends a model identification string after connection
        // - Clone read/write follows a block-transfer protocol
        //
        // Unknown (requires traffic capture):
        // - Exact handshake byte sequence
        // - Block size and addressing scheme
        // - Checksum algorithm (if any)
        // - Read vs write command differentiation
        let _ = variant::BAUD_RATE;
        Err(YaesuProtocolError::ProtocolNotYetReversed)
    }

    /// Download the full EEPROM image from the radio.
    ///
    /// # Errors
    ///
    /// Returns `YaesuProtocolError::ProtocolNotYetReversed`.
    #[expect(
        clippy::missing_const_for_fn,
        reason = "stub; real implementation will do serial I/O via `_port` and will not be const (see #80)"
    )]
    pub fn download(&mut self) -> Result<Vec<u8>, YaesuProtocolError> {
        Err(YaesuProtocolError::ProtocolNotYetReversed)
    }

    /// Upload an EEPROM image to the radio.
    ///
    /// # Errors
    ///
    /// Returns `YaesuProtocolError::ProtocolNotYetReversed`.
    #[expect(
        clippy::missing_const_for_fn,
        reason = "stub; real implementation will do serial I/O via `_port` and will not be const (see #80)"
    )]
    pub fn upload(&mut self, _image: &[u8]) -> Result<(), YaesuProtocolError> {
        Err(YaesuProtocolError::ProtocolNotYetReversed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::mock::MockSerialPort;

    #[test]
    fn open_returns_not_yet_reversed() {
        let port = MockSerialPort::new();
        let result = YaesuSession::open(port);
        let Err(err) = result else {
            unreachable!("open() must return the NotYetReversed stub error");
        };
        assert!(
            err.to_string().contains("not yet reversed"),
            "should indicate protocol is not reversed, got: {err}"
        );
    }
}
