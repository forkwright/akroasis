//! Yaesu FTM-510DR channel encode/decode.
//!
//! # Status
//!
//! Scaffolded with known fields from CHIRP source. The EEPROM memory map
//! offsets are estimated and marked `TODO(#80)` — they need verification
//! against an actual ADMS-14 traffic capture before use on real hardware.

use snafu::Snafu;

use crate::channel::Channel;

/// Errors from Yaesu codec operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum YaesuCodecError {
    /// Channel index exceeds the FTM-510DR's 900-channel limit.
    #[snafu(display("channel index {index} exceeds maximum 899"))]
    IndexOutOfRange {
        /// The invalid channel index.
        index: u16,
    },

    /// EEPROM image is too small for the expected memory layout.
    #[snafu(display("image too small: {size} bytes, expected at least {expected}"))]
    ImageTooSmall {
        /// Actual image size.
        size: usize,
        /// Minimum expected size.
        expected: usize,
    },

    /// Codec not yet implemented — protocol reverse-engineering pending.
    #[snafu(display("Yaesu FTM-510DR codec not yet implemented (see #80)"))]
    NotYetImplemented,
}

/// Decode a channel from a Yaesu FTM-510DR EEPROM image.
///
/// # Errors
///
/// Returns `YaesuCodecError::NotYetImplemented` — the memory layout
/// has not been verified against real hardware.
#[expect(
    clippy::missing_const_for_fn,
    reason = "stub; real implementation will read from `_image` and will not be const (see #80)"
)]
pub fn decode_channel(_image: &[u8], _index: u16) -> Result<Channel, YaesuCodecError> {
    // TODO(#80)[deliberate-prudent]: implement once EEPROM memory map is
    // verified via ADMS-14 traffic capture. Known fields from CHIRP source:
    // - Frequency: 4 bytes BCD (similar to Baofeng but different byte order)
    // - Offset: 4 bytes BCD
    // - Tone mode: 1 byte (CTCSS/DCS/cross-tone)
    // - CTCSS tone: 1 byte (index into tone table)
    // - DCS code: 2 bytes
    // - Power: 2 bits in flags byte
    // - Name: 6 bytes ASCII
    Err(YaesuCodecError::NotYetImplemented)
}

/// Encode a channel into a Yaesu FTM-510DR EEPROM image.
///
/// # Errors
///
/// Returns `YaesuCodecError::NotYetImplemented` — the memory layout
/// has not been verified against real hardware.
#[expect(
    clippy::missing_const_for_fn,
    reason = "stub; real implementation will write to `_image` and will not be const (see #80)"
)]
pub fn encode_channel(
    _image: &mut [u8], // kanon:ignore RUST/indexing-slicing -- function parameter &mut [u8], not indexing
    _index: u16,
    _channel: &Channel,
) -> Result<(), YaesuCodecError> {
    // TODO(#80)[deliberate-prudent]: implement once EEPROM memory map is verified
    Err(YaesuCodecError::NotYetImplemented)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn decode_returns_not_yet_implemented() {
        let image = vec![0u8; 65_536];
        let result = decode_channel(&image, 0);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not yet implemented"),
            "should return NotYetImplemented"
        );
    }

    #[test]
    fn encode_returns_not_yet_implemented() {
        let mut image = vec![0u8; 65_536];
        let channel = Channel {
            index: 0,
            name: String::new(),
            rx_freq: koinon::Frequency::mhz(146),
            tx_freq: None,
            offset: crate::types::FrequencyOffset::None,
            tone: crate::tone::ToneMode::None,
            power: crate::types::PowerLevel::High,
            bandwidth: crate::types::Bandwidth::Wide,
            scan: crate::types::ScanMode::Include,
            busy_lock: false,
        };
        let result = encode_channel(&mut image, 0, &channel);
        assert!(result.is_err());
    }
}
