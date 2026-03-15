//! Variant-aware EEPROM channel codec for the UV-5R family.
//!
//! Decodes and encodes 16-byte channel records from/to the radio-agnostic
//! [`Channel`] type, using the variant's [`VariantConfig`] for power mapping.

use koinon::Frequency;
use snafu::Snafu;

use crate::channel::Channel;
use crate::tone::ToneMode;
use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

use super::variant::VariantConfig;

/// Size of a single channel record in the EEPROM image.
pub const CHANNEL_RECORD_SIZE: usize = 16;

/// Byte offset of the power/bandwidth/scan flags byte within a channel record.
const FLAGS_OFFSET: usize = 14;

/// Bit mask for the 2-bit power field within the flags byte (bits 1:0).
const POWER_MASK: u8 = 0x03;

/// Bit position of the bandwidth flag within the flags byte.
const BANDWIDTH_BIT: u8 = 2;

/// Bit position of the scan-skip flag within the flags byte.
const SCAN_SKIP_BIT: u8 = 3;

/// Bit position of the busy-lock flag within the flags byte.
const BUSY_LOCK_BIT: u8 = 4;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors from channel codec operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum CodecError {
    /// The channel record is too short to decode.
    #[snafu(display(
        "channel record too short: expected {CHANNEL_RECORD_SIZE} bytes, got {actual}"
    ))]
    RecordTooShort {
        /// Actual size of the record.
        actual: usize,
    },

    /// The power bits in the EEPROM do not map to a known power level.
    #[snafu(display("unknown power bits {bits:#04X} for variant {variant}"))]
    UnknownPowerBits {
        /// The raw 2-bit value.
        bits: u8,
        /// Variant name for context.
        variant: String,
    },

    /// The power level is not supported by this variant.
    #[snafu(display("power level {level:?} not supported by variant {variant}"))]
    UnsupportedPowerLevel {
        /// The requested power level.
        level: PowerLevel,
        /// Variant name for context.
        variant: String,
    },
}

// ── Decode ───────────────────────────────────────────────────────────────────

/// Decode a single channel from a 16-byte EEPROM record.
///
/// Uses the variant config to interpret the 2-bit power field correctly.
///
/// # EEPROM channel record layout (16 bytes)
///
/// | Offset | Size | Field |
/// |--------|------|-------|
/// | 0–3    | 4    | RX frequency (BCD, MHz × 10) |
/// | 4–7    | 4    | TX offset (BCD, MHz × 10) |
/// | 8      | 1    | RX tone index |
/// | 9      | 1    | TX tone index |
/// | 10     | 1    | Signal / scramble |
/// | 11–13  | 3    | Reserved |
/// | 14     | 1    | Flags: power(1:0), bandwidth(2), scan(3), busy-lock(4) |
/// | 15     | 1    | Step / pad |
///
/// # Errors
///
/// Returns [`CodecError::RecordTooShort`] if the slice is too small, or
/// [`CodecError::UnknownPowerBits`] if the power field doesn't map to a
/// known level.
pub fn decode_channel(
    index: u16,
    record: &[u8],
    config: &VariantConfig,
) -> Result<Channel, CodecError> {
    if record.len() < CHANNEL_RECORD_SIZE {
        return RecordTooShortSnafu {
            actual: record.len(),
        }
        .fail();
    }

    let rx_bytes: [u8; 4] = record
        .get(..4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .ok_or(CodecError::RecordTooShort {
            actual: record.len(),
        })?;
    let tx_bytes: [u8; 4] = record
        .get(4..8)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .ok_or(CodecError::RecordTooShort {
            actual: record.len(),
        })?;
    let rx_freq = decode_bcd_freq(rx_bytes);
    let tx_offset = decode_bcd_freq(tx_bytes);

    let flags = *record.get(FLAGS_OFFSET).ok_or(CodecError::RecordTooShort {
        actual: record.len(),
    })?;
    let power_bits = flags & POWER_MASK;
    let power = config
        .power_from_bits(power_bits)
        .ok_or_else(|| CodecError::UnknownPowerBits {
            bits: power_bits,
            variant: config.variant.to_string(),
        })?;

    let bandwidth = if flags & (1 << BANDWIDTH_BIT) != 0 {
        Bandwidth::Narrow
    } else {
        Bandwidth::Wide
    };

    let scan = if flags & (1 << SCAN_SKIP_BIT) != 0 {
        ScanMode::Skip
    } else {
        ScanMode::Include
    };

    let busy_lock = flags & (1 << BUSY_LOCK_BIT) != 0;

    let (offset, tx_freq) = if tx_offset.as_hz() == 0 {
        (FrequencyOffset::None, None)
    } else {
        // WHY: TX offset is stored as an absolute value — direction determined
        // by a separate direction bit, but for simplicity we store as Plus.
        (FrequencyOffset::Plus(tx_offset), Some(rx_freq + tx_offset))
    };

    Ok(Channel {
        index,
        name: String::new(),
        rx_freq,
        tx_freq,
        offset,
        tone: ToneMode::None,
        power,
        bandwidth,
        scan,
        busy_lock,
    })
}

/// Encode a channel into a 16-byte EEPROM record.
///
/// # Errors
///
/// Returns [`CodecError::UnsupportedPowerLevel`] if the channel's power level
/// cannot be represented by this variant.
pub fn encode_channel(
    channel: &Channel,
    config: &VariantConfig,
) -> Result<[u8; CHANNEL_RECORD_SIZE], CodecError> {
    let mut record = [0u8; CHANNEL_RECORD_SIZE];

    let mut rx_buf = [0u8; 4];
    encode_bcd_freq(channel.rx_freq, &mut rx_buf);
    record[..4].copy_from_slice(&rx_buf);

    let tx_offset = match channel.offset {
        FrequencyOffset::None => Frequency::hz(0),
        FrequencyOffset::Plus(f) | FrequencyOffset::Minus(f) | FrequencyOffset::Split(f) => f,
    };
    let mut tx_buf = [0u8; 4];
    encode_bcd_freq(tx_offset, &mut tx_buf);
    record[4..8].copy_from_slice(&tx_buf);

    let power_bits =
        config
            .bits_from_power(channel.power)
            .ok_or_else(|| CodecError::UnsupportedPowerLevel {
                level: channel.power,
                variant: config.variant.to_string(),
            })?;

    let mut flags = power_bits & POWER_MASK;
    if channel.bandwidth == Bandwidth::Narrow {
        flags |= 1 << BANDWIDTH_BIT;
    }
    if channel.scan == ScanMode::Skip {
        flags |= 1 << SCAN_SKIP_BIT;
    }
    if channel.busy_lock {
        flags |= 1 << BUSY_LOCK_BIT;
    }
    record[FLAGS_OFFSET] = flags;

    Ok(record)
}

// ── BCD frequency helpers ────────────────────────────────────────────────────

/// Decode a 4-byte BCD-encoded frequency (units of 10 Hz).
fn decode_bcd_freq(bytes: [u8; 4]) -> Frequency {
    let mut val: u64 = 0;
    for b in bytes {
        val = val * 100 + u64::from(b >> 4) * 10 + u64::from(b & 0x0F);
    }
    // BCD value is in units of 10 Hz
    Frequency::hz(val * 10)
}

/// Encode a frequency into 4 bytes of BCD (units of 10 Hz).
fn encode_bcd_freq(freq: Frequency, out: &mut [u8; 4]) {
    let mut val = freq.as_hz() / 10;
    for byte in out.iter_mut().rev() {
        let lo = (val % 10) as u8;
        val /= 10;
        let hi = (val % 10) as u8;
        val /= 10;
        *byte = (hi << 4) | lo;
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items
)]
mod tests {
    use super::*;
    use crate::baofeng::variant::{bf_f8hp_config, uv5r_config};

    fn make_record(rx_freq: Frequency, power_bits: u8) -> [u8; CHANNEL_RECORD_SIZE] {
        let mut record = [0u8; CHANNEL_RECORD_SIZE];
        let rx_out: &mut [u8; 4] = (&mut record[..4]).try_into().unwrap();
        encode_bcd_freq(rx_freq, rx_out);
        record[FLAGS_OFFSET] = power_bits & POWER_MASK;
        record
    }

    #[test]
    fn decode_uv5r_high_power() {
        let config = uv5r_config();
        let record = make_record(Frequency::hz(146_520_000), 0);
        let ch = decode_channel(0, &record, &config).unwrap();
        assert_eq!(ch.power, PowerLevel::High);
        assert_eq!(ch.rx_freq, Frequency::hz(146_520_000));
    }

    #[test]
    fn decode_uv5r_low_power() {
        let config = uv5r_config();
        let record = make_record(Frequency::hz(446_000_000), 1);
        let ch = decode_channel(0, &record, &config).unwrap();
        assert_eq!(ch.power, PowerLevel::Low);
    }

    #[test]
    fn decode_uv5r_mid_bits_treated_as_high() {
        let config = uv5r_config();
        let record = make_record(Frequency::hz(146_520_000), 2);
        let ch = decode_channel(0, &record, &config).unwrap();
        assert_eq!(ch.power, PowerLevel::High);
    }

    #[test]
    fn decode_f8hp_high_power() {
        let config = bf_f8hp_config();
        let record = make_record(Frequency::hz(146_520_000), 0);
        let ch = decode_channel(0, &record, &config).unwrap();
        assert_eq!(ch.power, PowerLevel::High);
    }

    #[test]
    fn decode_f8hp_mid_power() {
        let config = bf_f8hp_config();
        let record = make_record(Frequency::hz(146_520_000), 2);
        let ch = decode_channel(0, &record, &config).unwrap();
        assert_eq!(ch.power, PowerLevel::Mid);
    }

    #[test]
    fn decode_f8hp_low_power() {
        let config = bf_f8hp_config();
        let record = make_record(Frequency::hz(146_520_000), 1);
        let ch = decode_channel(0, &record, &config).unwrap();
        assert_eq!(ch.power, PowerLevel::Low);
    }

    #[test]
    fn encode_decode_roundtrip_uv5r() {
        let config = uv5r_config();
        for level in [PowerLevel::High, PowerLevel::Low] {
            let ch = Channel {
                index: 5,
                name: String::new(),
                rx_freq: Frequency::hz(146_520_000),
                tx_freq: None,
                offset: FrequencyOffset::None,
                tone: ToneMode::None,
                power: level,
                bandwidth: Bandwidth::Wide,
                scan: ScanMode::Include,
                busy_lock: false,
            };
            let record = encode_channel(&ch, &config).unwrap();
            let decoded = decode_channel(5, &record, &config).unwrap();
            assert_eq!(decoded.power, level);
            assert_eq!(decoded.rx_freq, ch.rx_freq);
            assert_eq!(decoded.bandwidth, ch.bandwidth);
            assert_eq!(decoded.scan, ch.scan);
            assert_eq!(decoded.busy_lock, ch.busy_lock);
        }
    }

    #[test]
    fn encode_decode_roundtrip_f8hp() {
        let config = bf_f8hp_config();
        for level in [PowerLevel::High, PowerLevel::Mid, PowerLevel::Low] {
            let ch = Channel {
                index: 10,
                name: String::new(),
                rx_freq: Frequency::hz(446_000_000),
                tx_freq: None,
                offset: FrequencyOffset::None,
                tone: ToneMode::None,
                power: level,
                bandwidth: Bandwidth::Narrow,
                scan: ScanMode::Skip,
                busy_lock: true,
            };
            let record = encode_channel(&ch, &config).unwrap();
            let decoded = decode_channel(10, &record, &config).unwrap();
            assert_eq!(decoded.power, level);
            assert_eq!(decoded.rx_freq, ch.rx_freq);
            assert_eq!(decoded.bandwidth, Bandwidth::Narrow);
            assert_eq!(decoded.scan, ScanMode::Skip);
            assert!(decoded.busy_lock);
        }
    }

    #[test]
    fn record_too_short_returns_error() {
        let config = uv5r_config();
        let short = [0u8; 8];
        let err = decode_channel(0, &short, &config);
        assert!(matches!(err, Err(CodecError::RecordTooShort { actual: 8 })));
    }

    #[test]
    fn bcd_frequency_roundtrip() {
        let freqs = [
            Frequency::hz(146_520_000),
            Frequency::hz(446_000_000),
            Frequency::hz(136_000_000),
            Frequency::hz(520_000_000),
        ];
        for freq in freqs {
            let mut buf = [0u8; 4];
            encode_bcd_freq(freq, &mut buf);
            let decoded = decode_bcd_freq(buf);
            assert_eq!(decoded, freq, "BCD roundtrip failed for {freq}");
        }
    }

    #[test]
    fn flags_byte_encodes_all_fields() {
        let config = bf_f8hp_config();
        let ch = Channel {
            index: 0,
            name: String::new(),
            rx_freq: Frequency::hz(146_520_000),
            tx_freq: None,
            offset: FrequencyOffset::None,
            tone: ToneMode::None,
            power: PowerLevel::Mid,
            bandwidth: Bandwidth::Narrow,
            scan: ScanMode::Skip,
            busy_lock: true,
        };
        let record = encode_channel(&ch, &config).unwrap();
        let flags = record[FLAGS_OFFSET];
        assert_eq!(flags & POWER_MASK, 2); // Mid
        assert_ne!(flags & (1 << BANDWIDTH_BIT), 0); // Narrow
        assert_ne!(flags & (1 << SCAN_SKIP_BIT), 0); // Skip
        assert_ne!(flags & (1 << BUSY_LOCK_BIT), 0); // Busy lock
    }
}
