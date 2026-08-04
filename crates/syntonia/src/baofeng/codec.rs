//! Channel encode/decode between raw EEPROM bytes and the Channel model.

use koinon::Frequency;
use snafu::Snafu;

use crate::baofeng::bcd::{self, BcdError};
use crate::baofeng::image::MemoryImage;
use crate::baofeng::memmap::{
    CHANNEL_BASE, CHANNEL_COUNT, CHANNEL_STRIDE, NAME_BASE, NAME_LENGTH, NAME_STRIDE,
};
use crate::baofeng::tone_codec;
use crate::channel::Channel;
use crate::plan::FrequencyPlan;
use crate::tone::ToneMode;
use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

/// Channel codec errors.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum CodecError {
    /// Failed to decode BCD frequency data.
    #[snafu(display("BCD decode error for channel {index}: {source}"))]
    BcdDecode {
        /// Channel index WHERE the error occurred.
        index: u8,
        /// The underlying BCD error.
        source: BcdError,
    },

    /// Failed to encode BCD frequency data.
    #[snafu(display("BCD encode error for channel {index}: {source}"))]
    BcdEncode {
        /// Channel index WHERE the error occurred.
        index: u8,
        /// The underlying BCD error.
        source: BcdError,
    },
}

const fn power_from_bits(byte14: u8) -> PowerLevel {
    match byte14 & 0x03 {
        1 => PowerLevel::Low,
        2 => PowerLevel::Mid,
        _ => PowerLevel::High,
    }
}

const fn power_to_bits(power: PowerLevel) -> u8 {
    match power {
        PowerLevel::High => 0,
        PowerLevel::Low => 1,
        PowerLevel::Mid => 2,
    }
}

const fn bandwidth_from_bit(byte15: u8) -> Bandwidth {
    if byte15 & 0x40 != 0 {
        Bandwidth::Wide
    } else {
        Bandwidth::Narrow
    }
}

const fn scan_from_bit(byte15: u8) -> ScanMode {
    if byte15 & 0x04 != 0 {
        ScanMode::Include
    } else {
        ScanMode::Skip
    }
}

const fn bcl_from_bit(byte15: u8) -> bool {
    byte15 & 0x08 != 0
}

/// Decode a single channel FROM the memory image. Returns `None` for empty slots.
///
/// # Errors
///
/// Returns `CodecError::BcdDecode` if the frequency bytes contain invalid BCD.
pub fn decode_channel(image: &MemoryImage, index: u8) -> Result<Option<Channel>, CodecError> {
    let ch_addr = CHANNEL_BASE + u16::from(index) * CHANNEL_STRIDE;
    // SAFETY(indexing): read_bytes returns exactly 16 bytes
    let ch = image.read_bytes(ch_addr, 16);

    if ch.first().copied().unwrap_or_default() == 0xFF {
        return Ok(None);
    }

    let rx_bytes: [u8; 4] = [
        ch.first().copied().unwrap_or_default(),
        ch.get(1).copied().unwrap_or_default(),
        ch.get(2).copied().unwrap_or_default(),
        ch.get(3).copied().unwrap_or_default(),
    ];
    let tx_bytes: [u8; 4] = [
        ch.get(4).copied().unwrap_or_default(),
        ch.get(5).copied().unwrap_or_default(),
        ch.get(6).copied().unwrap_or_default(),
        ch.get(7).copied().unwrap_or_default(),
    ];

    let rx_hz =
        bcd::lbcd4_decode(rx_bytes).map_err(|source| CodecError::BcdDecode { index, source })?;
    let rx_freq = Frequency::hz(rx_hz);

    let (tx_freq, offset) = if tx_bytes == [0xFF, 0xFF, 0xFF, 0xFF] {
        (None, FrequencyOffset::None)
    } else {
        let tx_hz = bcd::lbcd4_decode(tx_bytes)
            .map_err(|source| CodecError::BcdDecode { index, source })?;
        let tx = Frequency::hz(tx_hz);
        match tx_hz.cmp(&rx_hz) {
            std::cmp::Ordering::Equal => (Some(tx), FrequencyOffset::None),
            std::cmp::Ordering::Greater => (
                Some(tx),
                FrequencyOffset::Plus(Frequency::hz(tx_hz - rx_hz)),
            ),
            std::cmp::Ordering::Less => (
                Some(tx),
                FrequencyOffset::Minus(Frequency::hz(rx_hz - tx_hz)),
            ),
        }
    };

    let rxtone_raw = u16::from_le_bytes([
        ch.get(8).copied().unwrap_or_default(),
        ch.get(9).copied().unwrap_or_default(),
    ]);
    let txtone_raw = u16::from_le_bytes([
        ch.get(10).copied().unwrap_or_default(),
        ch.get(11).copied().unwrap_or_default(),
    ]);

    // WHY: UV-5R stores separate TX/RX tones, but our model uses a single ToneMode.
    // Prefer the TX tone if SET, fall back to RX tone.
    let tx_tone = tone_codec::decode_tone(txtone_raw);
    let tone = if matches!(tx_tone, ToneMode::Ctcss(_) | ToneMode::Dcs(_, _)) {
        tx_tone
    } else {
        let rx_tone = tone_codec::decode_tone(rxtone_raw);
        if matches!(rx_tone, ToneMode::Ctcss(_) | ToneMode::Dcs(_, _)) {
            rx_tone
        } else {
            ToneMode::None
        }
    };

    let power = power_from_bits(ch.get(14).copied().unwrap_or_default());
    let bandwidth = bandwidth_from_bit(ch.get(15).copied().unwrap_or_default());
    let scan = scan_from_bit(ch.get(15).copied().unwrap_or_default());
    let busy_lock = bcl_from_bit(ch.get(15).copied().unwrap_or_default());

    let name_addr = NAME_BASE + u16::from(index) * NAME_STRIDE;
    let name_data = image.read_bytes(name_addr, NAME_LENGTH);
    let name = name_data
        .iter()
        .take_while(|&&b| b != 0xFF && b != 0x00)
        .map(|&b| char::from(b))
        .collect::<String>()
        .trim()
        .to_string();

    Ok(Some(Channel {
        index: u16::from(index),
        name,
        rx_freq,
        tx_freq,
        offset,
        tone,
        power,
        bandwidth,
        scan,
        busy_lock,
    }))
}

/// Encode a channel INTO the memory image at the given index.
///
/// # Errors
///
/// Returns `CodecError::BcdEncode` if the channel frequency cannot be BCD-encoded.
#[expect(
    clippy::indexing_slicing,
    reason = "write_bytes targets fixed 16-byte layout at CHANNEL_BASE + index*CHANNEL_STRIDE; indices bounded"
)]
pub fn encode_channel(
    channel: &Channel,
    image: &mut MemoryImage,
    index: u8,
) -> Result<(), CodecError> {
    let ch_addr = CHANNEL_BASE + u16::from(index) * CHANNEL_STRIDE;

    let rx_bytes = bcd::lbcd4_encode(channel.rx_freq.as_hz())
        .map_err(|source| CodecError::BcdEncode { index, source })?;

    let tx_bytes = match channel.tx_freq {
        Some(tx) => bcd::lbcd4_encode(tx.as_hz())
            .map_err(|source| CodecError::BcdEncode { index, source })?,
        None => [0xFF, 0xFF, 0xFF, 0xFF],
    };

    let tone_raw = tone_codec::encode_tone(channel.tone);
    let tone_bytes = tone_raw.to_le_bytes();

    // SAFETY(indexing): ch_data is always 16 bytes, indices are within bounds
    let mut ch_data = [0u8; 16];
    // WHY: split_at_mut over the fixed-size array yields disjoint sub-slices
    // without bracket-range indexing for the rx/tx frequency and tone fields.
    let (rx_slot, rest) = ch_data.split_at_mut(4);
    rx_slot.copy_from_slice(&rx_bytes);
    let (tx_slot, rest) = rest.split_at_mut(4);
    tx_slot.copy_from_slice(&tx_bytes);
    // WHY: rx and tx tone codes are stored as separate fields but always set
    // to the same encoded value in this simple encode path.
    let (rx_tone_slot, rest) = rest.split_at_mut(2);
    rx_tone_slot.copy_from_slice(&tone_bytes);
    let (tx_tone_slot, _rest) = rest.split_at_mut(2);
    tx_tone_slot.copy_from_slice(&tone_bytes);
    ch_data[14] = power_to_bits(channel.power); // kanon:ignore RUST/indexing-slicing -- ch_data is fixed-size [u8; 16]; index 14 is compile-time bounded

    let mut byte15: u8 = 0;
    if channel.bandwidth == Bandwidth::Wide {
        byte15 |= 0x40;
    }
    if channel.scan == ScanMode::Include {
        byte15 |= 0x04;
    }
    if channel.busy_lock {
        byte15 |= 0x08;
    }
    ch_data[15] = byte15; // kanon:ignore RUST/indexing-slicing -- ch_data is fixed-size [u8; 16]; index 15 is compile-time bounded

    image.write_bytes(ch_addr, &ch_data);

    let name_addr = NAME_BASE + u16::from(index) * NAME_STRIDE;
    let mut name_data = [0xFFu8; 16];
    for (i, &byte) in channel.name.as_bytes().iter().take(NAME_LENGTH).enumerate() {
        name_data[i] = byte; // kanon:ignore RUST/indexing-slicing -- name_data is fixed-size [u8; 16]; iterator take(NAME_LENGTH) bounds i
    }
    image.write_bytes(name_addr, &name_data);

    Ok(())
}

/// Clear a channel slot in the memory image.
pub fn clear_channel(image: &mut MemoryImage, index: u8) {
    let ch_addr = CHANNEL_BASE + u16::from(index) * CHANNEL_STRIDE;
    image.write_bytes(ch_addr, &[0xFF; 16]);

    let name_addr = NAME_BASE + u16::from(index) * NAME_STRIDE;
    image.write_bytes(name_addr, &[0xFF; 16]);
}

/// Decode all non-empty channels FROM a memory image INTO a `FrequencyPlan`.
///
/// # Errors
///
/// Returns `CodecError` if any channel contains invalid BCD data.
pub fn decode_all_channels(image: &MemoryImage) -> Result<FrequencyPlan, CodecError> {
    let mut channels = Vec::new();

    for i in 0..CHANNEL_COUNT {
        if let Some(ch) = decode_channel(image, i)? {
            channels.push(ch);
        }
    }

    Ok(FrequencyPlan {
        name: String::new(),
        radio_model: None,
        channels,
        created: None,
    })
}

/// Encode all channels FROM a `FrequencyPlan` INTO a memory image.
///
/// # Errors
///
/// Returns `CodecError` if any channel frequency cannot be BCD-encoded.
pub fn encode_all_channels(
    plan: &FrequencyPlan,
    image: &mut MemoryImage,
) -> Result<(), CodecError> {
    for i in 0..CHANNEL_COUNT {
        clear_channel(image, i);
    }

    for channel in &plan.channels {
        if channel.index < u16::from(CHANNEL_COUNT) {
            let index = channel.index as u8; // SAFETY: guarded above, fits u8 by construction
            encode_channel(channel, image, index)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;
    use crate::tone::CtcssTone;

    fn make_test_image() -> MemoryImage {
        let mut image = MemoryImage::new(0x1800);

        let ch0 = Channel {
            index: 0,
            name: "CALL".to_string(),
            rx_freq: Frequency::hz(146_520_000),
            tx_freq: Some(Frequency::hz(146_520_000)),
            offset: FrequencyOffset::None,
            tone: ToneMode::None,
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        };
        encode_channel(&ch0, &mut image, 0).unwrap();

        let ch1 = Channel {
            index: 1,
            name: "RPT".to_string(),
            rx_freq: Frequency::hz(147_060_000),
            tx_freq: Some(Frequency::hz(147_660_000)),
            offset: FrequencyOffset::Plus(Frequency::hz(600_000)),
            tone: ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()),
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        };
        encode_channel(&ch1, &mut image, 1).unwrap();

        image
    }

    /// Write a raw 16-byte channel record straight into the image.
    ///
    /// WHY: `encode_channel` is the inverse of the function under test, so a
    /// test that builds its fixture with it can only show the two agree — not
    /// that either matches the EEPROM layout. These tests assert decode
    /// against hand-built bytes instead.
    fn write_raw_channel(
        image: &mut MemoryImage,
        index: u8,
        rx_hz: u64,
        tx_hz: Option<u64>,
        byte14: u8,
        byte15: u8,
    ) {
        let mut ch = [0xFFu8; 16];
        ch[0..4].copy_from_slice(&bcd::lbcd4_encode(rx_hz).unwrap());
        match tx_hz {
            Some(hz) => ch[4..8].copy_from_slice(&bcd::lbcd4_encode(hz).unwrap()),
            None => ch[4..8].copy_from_slice(&[0xFF; 4]),
        }
        // Tone slots: 0x0000 in both is "no tone".
        ch[8..12].copy_from_slice(&[0x00; 4]);
        ch[14] = byte14;
        ch[15] = byte15;
        image.write_bytes(CHANNEL_BASE + u16::from(index) * CHANNEL_STRIDE, &ch);
    }

    /// Byte-15 flag bits, per the UV-5R channel record.
    const FLAG_WIDE: u8 = 0x40;
    const FLAG_SCAN_INCLUDE: u8 = 0x04;
    const FLAG_BUSY_LOCK: u8 = 0x08;

    fn decode_raw(byte14: u8, byte15: u8) -> Channel {
        let mut image = MemoryImage::new(0x1800);
        write_raw_channel(
            &mut image,
            3,
            146_520_000,
            Some(146_520_000),
            byte14,
            byte15,
        );
        decode_channel(&image, 3).unwrap().unwrap()
    }

    // ── byte 15: bandwidth, scan, busy lock ──────────────────────────────

    #[test]
    fn decode_reads_narrow_bandwidth_when_the_wide_bit_is_clear() {
        assert_eq!(decode_raw(0, 0).bandwidth, Bandwidth::Narrow);
        assert_eq!(decode_raw(0, FLAG_WIDE).bandwidth, Bandwidth::Wide);
    }

    #[test]
    fn decode_reads_skip_scan_when_the_include_bit_is_clear() {
        assert_eq!(decode_raw(0, 0).scan, ScanMode::Skip);
        assert_eq!(decode_raw(0, FLAG_SCAN_INCLUDE).scan, ScanMode::Include);
    }

    #[test]
    fn decode_reads_busy_lock_from_its_own_bit() {
        assert!(!decode_raw(0, 0).busy_lock);
        assert!(decode_raw(0, FLAG_BUSY_LOCK).busy_lock);
    }

    // WHY: the three flags share byte 15, so a mask that is too wide reads one
    // as another. Setting all three at once catches that; the tests above,
    // each setting a single bit, cannot.
    #[test]
    fn decode_reads_the_byte_fifteen_flags_independently() {
        let ch = decode_raw(0, FLAG_WIDE | FLAG_SCAN_INCLUDE | FLAG_BUSY_LOCK);
        assert_eq!(ch.bandwidth, Bandwidth::Wide);
        assert_eq!(ch.scan, ScanMode::Include);
        assert!(ch.busy_lock);
    }

    // ── byte 14: power ───────────────────────────────────────────────────

    #[test]
    fn decode_reads_each_power_level_from_byte_fourteen() {
        assert_eq!(decode_raw(0, FLAG_WIDE).power, PowerLevel::High);
        assert_eq!(decode_raw(1, FLAG_WIDE).power, PowerLevel::Low);
        assert_eq!(decode_raw(2, FLAG_WIDE).power, PowerLevel::Mid);
    }

    // WHY: only the low two bits carry power; the rest of byte 14 is other
    // per-channel state, so a decoder reading the whole byte misreads every
    // channel that has any of it set.
    #[test]
    fn decode_ignores_the_high_bits_of_byte_fourteen() {
        assert_eq!(decode_raw(0xFC, FLAG_WIDE).power, PowerLevel::High);
        assert_eq!(decode_raw(0xFD, FLAG_WIDE).power, PowerLevel::Low);
        assert_eq!(decode_raw(0xFE, FLAG_WIDE).power, PowerLevel::Mid);
    }

    // ── TX/RX pair: derived offset ───────────────────────────────────────

    #[test]
    fn decode_derives_a_minus_offset_when_tx_is_below_rx() {
        let mut image = MemoryImage::new(0x1800);
        write_raw_channel(&mut image, 0, 147_060_000, Some(146_460_000), 0, FLAG_WIDE);
        let ch = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(ch.tx_freq, Some(Frequency::hz(146_460_000)));
        assert_eq!(ch.offset, FrequencyOffset::Minus(Frequency::hz(600_000)));
    }

    #[test]
    fn decode_derives_a_plus_offset_when_tx_is_above_rx() {
        let mut image = MemoryImage::new(0x1800);
        write_raw_channel(&mut image, 0, 147_060_000, Some(147_660_000), 0, FLAG_WIDE);
        let ch = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(ch.offset, FrequencyOffset::Plus(Frequency::hz(600_000)));
    }

    #[test]
    fn decode_reports_no_offset_when_tx_equals_rx() {
        let mut image = MemoryImage::new(0x1800);
        write_raw_channel(&mut image, 0, 147_060_000, Some(147_060_000), 0, FLAG_WIDE);
        let ch = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(ch.tx_freq, Some(Frequency::hz(147_060_000)));
        assert_eq!(ch.offset, FrequencyOffset::None);
    }

    #[test]
    fn decode_reports_a_receive_only_channel_when_tx_is_unprogrammed() {
        let mut image = MemoryImage::new(0x1800);
        write_raw_channel(&mut image, 0, 147_060_000, None, 0, FLAG_WIDE);
        let ch = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(ch.tx_freq, None);
        assert_eq!(ch.offset, FrequencyOffset::None);
    }

    #[test]
    fn decode_simplex_channel() {
        let image = make_test_image();
        let ch = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(ch.rx_freq, Frequency::hz(146_520_000));
        assert_eq!(ch.name, "CALL");
        assert_eq!(ch.tone, ToneMode::None);
        assert_eq!(ch.power, PowerLevel::High);
        assert_eq!(ch.bandwidth, Bandwidth::Wide);
    }

    #[test]
    fn decode_empty_channel_returns_none() {
        let image = MemoryImage::new(0x1800);
        assert!(decode_channel(&image, 5).unwrap().is_none());
    }

    #[test]
    fn decode_all_returns_correct_count() {
        let image = make_test_image();
        let plan = decode_all_channels(&image).unwrap();
        assert_eq!(plan.channel_count(), 2);
    }

    #[test]
    fn encode_decode_roundtrip() {
        let image = make_test_image();
        let plan = decode_all_channels(&image).unwrap();

        let mut image2 = MemoryImage::new(0x1800);
        encode_all_channels(&plan, &mut image2).unwrap();
        let plan2 = decode_all_channels(&image2).unwrap();

        assert_eq!(plan.channel_count(), plan2.channel_count());
        for (a, b) in plan.channels.iter().zip(plan2.channels.iter()) {
            assert_eq!(a.rx_freq, b.rx_freq);
            assert_eq!(a.name, b.name);
            assert_eq!(a.tone, b.tone);
            assert_eq!(a.power, b.power);
        }
    }

    #[test]
    fn encode_all_channels_skips_out_of_range_index_without_overwriting_slot_zero() {
        let ch0 = Channel {
            index: 0,
            name: "CALL".to_string(),
            rx_freq: Frequency::hz(146_520_000),
            tx_freq: Some(Frequency::hz(146_520_000)),
            offset: FrequencyOffset::None,
            tone: ToneMode::None,
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        };
        let ch_overrange = Channel {
            index: 300,
            name: "BOGUS".to_string(),
            rx_freq: Frequency::hz(433_000_000),
            tx_freq: None,
            offset: FrequencyOffset::None,
            tone: ToneMode::None,
            power: PowerLevel::Low,
            bandwidth: Bandwidth::Narrow,
            scan: ScanMode::Skip,
            busy_lock: false,
        };
        let plan = FrequencyPlan {
            name: String::new(),
            radio_model: None,
            channels: vec![ch0, ch_overrange],
            created: None,
        };

        let mut image = MemoryImage::new(0x1800);
        encode_all_channels(&plan, &mut image).unwrap();

        let decoded = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(decoded.name, "CALL");
        assert_eq!(decoded.rx_freq, Frequency::hz(146_520_000));

        let plan2 = decode_all_channels(&image).unwrap();
        assert_eq!(plan2.channel_count(), 1);
    }
}
