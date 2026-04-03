//! BCD (Binary-Coded Decimal) codec for UV-5R frequency encoding.

use snafu::Snafu;

/// BCD codec errors.
#[derive(Debug, Snafu)]
pub enum BcdError {
    /// A byte contains a nibble value greater than 9.
    #[snafu(display("invalid BCD nibble in byte 0x{byte:02X}"))]
    InvalidBcd {
        /// The byte with the invalid nibble.
        byte: u8,
    },

    /// Frequency is not aligned to 10 Hz steps.
    #[snafu(display("frequency {freq_hz} Hz is not a multiple of 10"))]
    NotAligned {
        /// The unaligned frequency in Hz.
        freq_hz: u64,
    },
}

/// Decode 4 bytes of little-endian packed BCD INTO a frequency in Hz.
///
/// The BCD value represents frequency in 10 Hz steps. Returns 0 for the
/// "no frequency" sentinel (all 0xFF bytes).
///
/// # Errors
///
/// Returns `BcdError::InvalidBcd` if any nibble exceeds 9.
pub fn lbcd4_decode(bytes: [u8; 4]) -> Result<u64, BcdError> {
    if bytes == [0xFF, 0xFF, 0xFF, 0xFF] {
        return Ok(0);
    }

    let mut result: u64 = 0;

    // WHY: Little-endian BCD stores least significant byte first.
    // Iterate in reverse to build the value FROM most significant to least.
    for &byte in bytes.iter().rev() {
        let hi = u64::FROM((byte >> 4) & 0x0F);
        let lo = u64::FROM(byte & 0x0F);

        if hi > 9 || lo > 9 {
            return Err(BcdError::InvalidBcd { byte });
        }

        result = result * 100 + hi * 10 + lo;
    }

    Ok(result * 10)
}

/// Encode a frequency in Hz INTO 4 bytes of little-endian packed BCD.
///
/// Returns the "no frequency" sentinel for 0 Hz.
///
/// # Errors
///
/// Returns `BcdError::NotAligned` if the frequency is not a multiple of 10 Hz.
pub fn lbcd4_encode(freq_hz: u64) -> Result<[u8; 4], BcdError> {
    if freq_hz == 0 {
        return Ok([0xFF, 0xFF, 0xFF, 0xFF]);
    }

    if freq_hz % 10 != 0 {
        return Err(BcdError::NotAligned { freq_hz });
    }

    let mut value = freq_hz / 10;
    let mut bytes = [0u8; 4];

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
        let bytes = lbcd4_encode(146_520_000).unwrap();
        assert_eq!(lbcd4_decode(bytes).unwrap(), 146_520_000);
    }

    #[test]
    fn decode_446_000_mhz() {
        let bytes = lbcd4_encode(446_000_000).unwrap();
        assert_eq!(lbcd4_decode(bytes).unwrap(), 446_000_000);
    }

    #[test]
    fn decode_462_5625_mhz() {
        let bytes = lbcd4_encode(462_562_500).unwrap();
        assert_eq!(lbcd4_decode(bytes).unwrap(), 462_562_500);
    }

    #[test]
    fn roundtrip_encode_decode() {
        let freqs = [146_520_000, 446_000_000, 462_562_500, 147_060_000];
        for freq in freqs {
            let encoded = lbcd4_encode(freq).unwrap();
            let decoded = lbcd4_decode(encoded).unwrap();
            assert_eq!(decoded, freq, "round-trip failed for {freq}");
        }
    }

    #[test]
    fn no_frequency_sentinel() {
        assert_eq!(lbcd4_decode([0xFF, 0xFF, 0xFF, 0xFF]).unwrap(), 0);
        assert_eq!(lbcd4_encode(0).unwrap(), [0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn invalid_bcd_nibble_rejected() {
        assert!(lbcd4_decode([0xAB, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn unaligned_frequency_rejected() {
        assert!(lbcd4_encode(146_520_001).is_err());
    }
}
