//! Auto-detection of Baofeng UV-5R family radio variants.
//!
//! Tries multiple magic byte sequences to identify the connected radio,
//! returning the appropriate [`VariantConfig`] on success. Hardware
//! serial interaction is abstracted behind the [`SerialPort`] trait
//! for testability.

use snafu::Snafu;

use super::ident::RadioIdent;
use super::variant::{MAGIC_SETS, VariantConfig, VariantError, identify_variant};

// ── SerialPort trait ─────────────────────────────────────────────────────────

/// Minimal serial port abstraction for radio communication.
///
/// Implementations must handle baud rate, timeout, and framing.
/// This trait exists to enable mock-based testing of the detection flow
/// without requiring actual hardware.
pub(crate) trait SerialPort {
    /// Write bytes to the serial port.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn write_all(&mut self, buf: &[u8]) -> Result<(), DetectError>;

    /// Read exactly `len` bytes FROM the serial port.
    ///
    /// # Errors
    ///
    /// Returns [`DetectError::Timeout`] if the read times out, or
    /// [`DetectError::SerialIo`] on other I/O errors.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), DetectError>; // kanon:ignore RUST/indexing-slicing -- function parameter &mut [u8], not indexing

    /// Flush transmit buffer.
    ///
    /// # Errors
    ///
    /// Returns an error if the flush fails.
    fn flush(&mut self) -> Result<(), DetectError>;
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors FROM the auto-detection flow.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum DetectError {
    /// Serial I/O failure.
    #[snafu(display("serial I/O error: {message}"))]
    SerialIo {
        /// Description of the failure.
        message: String,
    },

    /// Read timed out waiting for radio response.
    #[snafu(display("timeout waiting for radio response"))]
    Timeout,

    /// All magic byte sequences failed  -  no radio detected.
    #[snafu(display(
        "no compatible radio detected after trying all magic sequences. \
         Troubleshooting: (1) check cable connection, (2) ensure radio is powered on, \
         (3) if UV-5RM Plus: this radio may use UV17Pro protocol (115200 baud)  -  \
         investigation needed"
    ))]
    NoRadioDetected,

    /// The radio responded but its firmware ident is unrecognized.
    #[snafu(display("variant identification failed: {source}"))]
    VariantIdentification {
        /// The underlying variant error.
        source: VariantError,
    },

    /// The ident response length does not match the wire framing (8 or 12 bytes).
    #[snafu(display("radio identification response could not be parsed ({len} bytes)"))]
    IdentFailed {
        /// Length of the received identification payload in bytes.
        len: usize,
    },
}

// ── Detection flow ───────────────────────────────────────────────────────────

/// Try to detect and identify a Baofeng UV-5R family radio.
///
/// Iterates through [`MAGIC_SETS`] in priority ORDER, attempting to enter
/// programming mode with each SET. On success, reads the firmware ident
/// and returns the matched [`VariantConfig`].
///
/// # Errors
///
/// - [`DetectError::NoRadioDetected`] if all magic sets fail (includes
///   troubleshooting hints for UV-5RM Plus / `UV17Pro`)
/// - [`DetectError::VariantIdentification`] if the radio responds but has
///   an unrecognized firmware ident
/// - [`DetectError::IdentFailed`] if the ident response length is not 8 or 12 bytes
/// - [`DetectError::SerialIo`] on I/O errors
pub(crate) fn auto_detect(
    port: &mut dyn SerialPort,
) -> Result<(RadioIdent, VariantConfig), DetectError> {
    for &magic in MAGIC_SETS {
        match try_magic(port, magic) {
            Ok((ident, config)) => return Ok((ident, config)),
            Err(DetectError::Timeout) => {}
            Err(other) => return Err(other),
        }
    }

    NoRadioDetectedSnafu.fail()
}

/// Attempt a single magic byte sequence handshake.
fn try_magic(
    port: &mut dyn SerialPort,
    magic: [u8; 7],
) -> Result<(RadioIdent, VariantConfig), DetectError> {
    // Send magic bytes
    port.write_all(&magic)?;
    port.flush()?;

    // Read ACK (single byte: 0x06)
    let mut ack = [0u8; 1];
    port.read_exact(&mut ack)?;
    if ack.get(0).copied().unwrap_or_default() != 0x06 {
        return TimeoutSnafu.fail();
    }

    // Send identify command (0x02)
    port.write_all(&[0x02])?;
    port.flush()?;

    // Read ident response: length byte + ident data
    let mut len_buf = [0u8; 1];
    port.read_exact(&mut len_buf)?;
    let ident_len = len_buf.get(0).copied().unwrap_or_default() as usize; // SAFETY: u8→usize is lossless (u8 max 255, usize always ≥32-bit)

    let mut ident_data = vec![0u8; ident_len];
    port.read_exact(&mut ident_data)?;

    // WHY: RadioIdent::from_raw only accepts 8- or 12-byte wire responses
    // (the real UV-5R framing) — see baofeng::ident.
    let ident =
        RadioIdent::from_raw(&ident_data).ok_or(DetectError::IdentFailed { len: ident_len })?;

    let config =
        identify_variant(&ident).map_err(|e| DetectError::VariantIdentification { source: e })?;

    Ok((ident, config))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::baofeng::variant::RadioVariant;

    /// Mock serial port that replays pre-recorded responses.
    struct MockSerial {
        /// Queued responses, popped FROM front on each read.
        responses: VecDeque<MockResponse>,
        /// Bytes written by the caller (for verification).
        written: Vec<u8>,
    }

    enum MockResponse {
        Data(Vec<u8>),
        Timeout,
    }

    impl MockSerial {
        fn new() -> Self {
            Self {
                responses: VecDeque::new(),
                written: Vec::new(),
            }
        }

        fn queue_data(&mut self, data: &[u8]) {
            self.responses.push_back(MockResponse::Data(data.to_vec()));
        }

        fn queue_timeout(&mut self) {
            self.responses.push_back(MockResponse::Timeout);
        }
    }

    impl SerialPort for MockSerial {
        fn write_all(&mut self, buf: &[u8]) -> Result<(), DetectError> {
            self.written.extend_from_slice(buf);
            Ok(())
        }

        fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), DetectError> {
            match self.responses.pop_front() {
                Some(MockResponse::Data(data)) => {
                    let len = buf.len().min(data.len());
                    buf[..len].copy_from_slice(&data[..len]);
                    Ok(())
                }
                Some(MockResponse::Timeout) | None => TimeoutSnafu.fail(),
            }
        }

        fn flush(&mut self) -> Result<(), DetectError> {
            Ok(())
        }
    }

    // WHY: RadioIdent::from_raw only accepts 8- or 12-byte wire responses
    // (the real UV-5R framing), so firmware strings below are padded to 8
    // bytes with filler that never collides with a known prefix (mirrors the
    // convention in baofeng::variant's own tests).
    /// Queue a successful handshake for a UV-5R with given firmware prefix.
    fn queue_uv5r_handshake(mock: &mut MockSerial, firmware: &[u8]) {
        // ACK
        mock.queue_data(&[0x06]);
        // Ident response: length + data
        let mut response = vec![firmware.len() as u8];
        response.extend_from_slice(firmware);
        // Split INTO length byte and ident data as separate reads
        mock.queue_data(&response[..1]);
        mock.queue_data(firmware);
    }

    #[test]
    fn detect_uv5r_on_first_magic() {
        let mut mock = MockSerial::new();
        queue_uv5r_handshake(&mut mock, b"BFB297\x00\x00");
        let (ident, config) = auto_detect(&mut mock).unwrap();
        assert_eq!(config.variant, RadioVariant::Uv5r);
        assert!(ident.firmware_prefix.starts_with("BFB"));
    }

    #[test]
    fn detect_f8hp_after_first_magic_fails() {
        let mut mock = MockSerial::new();
        // First magic: timeout on ACK
        mock.queue_timeout();
        // Second magic (BF-F8HP): success
        queue_uv5r_handshake(&mut mock, b"BFP3V3 F");
        let (_, config) = auto_detect(&mut mock).unwrap();
        assert_eq!(config.variant, RadioVariant::BfF8hp);
    }

    #[test]
    fn all_magics_fail_returns_no_radio_detected() {
        let mut mock = MockSerial::new();
        // All three magic attempts timeout
        mock.queue_timeout();
        mock.queue_timeout();
        mock.queue_timeout();
        let err = auto_detect(&mut mock).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("no compatible radio"),
            "expected 'no compatible radio', got: {msg}"
        );
        assert!(
            msg.contains("UV17Pro"),
            "error should mention UV17Pro protocol, got: {msg}"
        );
    }

    #[test]
    fn detect_with_bad_ack_tries_next_magic() {
        let mut mock = MockSerial::new();
        // First magic: bad ACK (not 0x06)
        mock.queue_data(&[0xFF]);
        // Second magic: timeout
        mock.queue_timeout();
        // Third magic: success (original UV-5R)
        queue_uv5r_handshake(&mut mock, b"BFB100\x00\x00");
        let (_, config) = auto_detect(&mut mock).unwrap();
        assert_eq!(config.variant, RadioVariant::Uv5r);
    }

    #[test]
    fn unknown_ident_returns_variant_error() {
        let mut mock = MockSerial::new();
        // ACK + unknown ident (8-byte padded, per RadioIdent::from_raw framing)
        mock.queue_data(&[0x06]);
        mock.queue_data(&[8]); // length = 8
        mock.queue_data(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00]);
        let err = auto_detect(&mut mock).unwrap_err();
        assert!(
            matches!(err, DetectError::VariantIdentification { .. }),
            "expected VariantIdentification error, got: {err:?}"
        );
    }

    #[test]
    fn no_radio_detected_includes_troubleshooting() {
        let mut mock = MockSerial::new();
        mock.queue_timeout();
        mock.queue_timeout();
        mock.queue_timeout();
        let err = auto_detect(&mut mock).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("check cable"));
        assert!(msg.contains("powered on"));
        assert!(msg.contains("UV17Pro"));
        assert!(msg.contains("115200"));
    }
}
