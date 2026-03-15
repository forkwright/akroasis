//! BCD (Binary-Coded Decimal) codec for Baofeng frequency encoding.
//!
//! The UV-5R stores frequencies as 4-byte little-endian packed BCD (lbcd4).
//! The BCD value represents the frequency in 10 Hz steps.
//!
//! Example: 146.520 MHz = 14 652 000 (in 10 Hz steps) = BCD `0014 6520`
//! stored little-endian as `[0x20, 0x65, 0x14, 0x00]`.

use crate::error::{FrequencyNotAlignedSnafu, InvalidBcdNibbleSnafu};

/// Sentinel value indicating "no frequency" (TX disabled).
const NO_FREQ: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];

/// Decodes a 4-byte little-endian BCD value to a frequency in Hz.
///
/// Returns `None` if the bytes are `0xFFFFFFFF` (no frequency / TX disabled).
///
/// # Errors
///
/// Returns an error if any BCD nibble is > 9.
pub fn lbcd4_decode(bytes: [u8; 4]) -> crate::error::Result<Option<u64>> {
    if bytes == NO_FREQ {
        return Ok(None);
    }

    let mut result: u64 = 0;
    // Process bytes from most significant (index 3) to least significant (index 0),
    // extracting high nibble then low nibble from each byte.
    for &byte in bytes.iter().rev() {
        let hi = byte >> 4;
        let lo = byte & 0x0F;
        snafu::ensure!(hi <= 9, InvalidBcdNibbleSnafu { value: hi });
        snafu::ensure!(lo <= 9, InvalidBcdNibbleSnafu { value: lo });
        result = result * 100 + u64::from(hi) * 10 + u64::from(lo);
    }

    // BCD value is frequency in 10 Hz steps.
    Ok(Some(result * 10))
}

/// Encodes a frequency in Hz to a 4-byte little-endian BCD value.
///
/// # Errors
///
/// Returns an error if the frequency is not a multiple of 10 Hz.
pub fn lbcd4_encode(freq_hz: u64) -> crate::error::Result<[u8; 4]> {
    snafu::ensure!(
        freq_hz % 10 == 0,
        FrequencyNotAlignedSnafu {
            freq_hz,
            step: 10u64
        }
    );

    let mut value = freq_hz / 10; // convert to 10 Hz steps
    let mut bytes = [0u8; 4];

    // Fill bytes from least significant (index 0) to most significant (index 3).
    for byte in &mut bytes {
        let lo = (value % 10) as u8;
        value /= 10;
        let hi = (value % 10) as u8;
        value /= 10;
        *byte = (hi << 4) | lo;
    }

    Ok(bytes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn decode_146_520_mhz() {
        // 146520000 Hz / 10 = 14652000, BCD: 14|65|20|00, LE: [00,20,65,14]
        let bytes = [0x00, 0x20, 0x65, 0x14];
        let freq = lbcd4_decode(bytes).unwrap().unwrap();
        assert_eq!(freq, 146_520_000);
    }

    #[test]
    fn decode_446_000_mhz() {
        // 446000000 Hz / 10 = 44600000, BCD: 44|60|00|00, LE: [00,00,60,44]
        let bytes = [0x00, 0x00, 0x60, 0x44];
        let freq = lbcd4_decode(bytes).unwrap().unwrap();
        assert_eq!(freq, 446_000_000);
    }

    #[test]
    fn decode_462_5625_mhz() {
        // 462.5625 MHz = 46256250 in 10 Hz steps = BCD 46256250
        // LE: [0x50, 0x62, 0x25, 0x46]
        let bytes = [0x50, 0x62, 0x25, 0x46];
        let freq = lbcd4_decode(bytes).unwrap().unwrap();
        assert_eq!(freq, 462_562_500);
    }

    #[test]
    fn decode_no_freq() {
        assert_eq!(lbcd4_decode([0xFF, 0xFF, 0xFF, 0xFF]).unwrap(), None);
    }

    #[test]
    fn encode_146_520_mhz() {
        let bytes = lbcd4_encode(146_520_000).unwrap();
        assert_eq!(bytes, [0x00, 0x20, 0x65, 0x14]);
    }

    #[test]
    fn encode_446_000_mhz() {
        let bytes = lbcd4_encode(446_000_000).unwrap();
        assert_eq!(bytes, [0x00, 0x00, 0x60, 0x44]);
    }

    #[test]
    fn encode_462_5625_mhz() {
        let bytes = lbcd4_encode(462_562_500).unwrap();
        assert_eq!(bytes, [0x50, 0x62, 0x25, 0x46]);
    }

    #[test]
    fn roundtrip_146_520() {
        let original = 146_520_000u64;
        let encoded = lbcd4_encode(original).unwrap();
        let decoded = lbcd4_decode(encoded).unwrap().unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_446_000() {
        let original = 446_000_000u64;
        let encoded = lbcd4_encode(original).unwrap();
        let decoded = lbcd4_decode(encoded).unwrap().unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn roundtrip_462_5625() {
        let original = 462_562_500u64;
        let encoded = lbcd4_encode(original).unwrap();
        let decoded = lbcd4_decode(encoded).unwrap().unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn invalid_nibble_detected() {
        // 0xAB has nibble A (10) which is invalid BCD
        let bytes = [0xAB, 0x00, 0x00, 0x00];
        assert!(lbcd4_decode(bytes).is_err());
    }

    #[test]
    fn frequency_not_aligned_rejected() {
        assert!(lbcd4_encode(146_520_001).is_err());
        assert!(lbcd4_encode(146_520_005).is_err());
    }
}
