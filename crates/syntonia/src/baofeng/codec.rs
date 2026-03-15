//! Channel encode/decode codec for the Baofeng UV-5R EEPROM format.
//!
//! Translates between raw EEPROM bytes in a [`MemoryImage`] and the typed
//! [`Channel`] / [`FrequencyPlan`] data model.

use koinon::Frequency;

use crate::channel::Channel;
use crate::error::ChannelIndexOutOfRangeSnafu;
use crate::plan::FrequencyPlan;
use crate::tone::ToneMode;
use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

use super::bcd::{lbcd4_decode, lbcd4_encode};
use super::image::MemoryImage;
use super::memmap::{
    CHANNEL_BASE, CHANNEL_COUNT, CHANNEL_STRIDE, NAME_BASE, NAME_LENGTH, NAME_STRIDE,
};
use super::tone_codec::{decode_tone, encode_tone};

/// Extracts the power level from byte 14 of the channel structure.
///
/// Bits [1:0]: 0 = High, 1 = Low, 2 = Mid.
#[must_use]
pub const fn power_from_bits(byte14: u8) -> PowerLevel {
    match byte14 & 0x03 {
        1 => PowerLevel::Low,
        2 => PowerLevel::Mid,
        _ => PowerLevel::High, // 0 and 3 (undocumented) both map to high
    }
}

/// Encodes a power level into the low 2 bits for byte 14.
#[must_use]
pub const fn power_to_bits(power: PowerLevel) -> u8 {
    match power {
        PowerLevel::High => 0,
        PowerLevel::Low => 1,
        PowerLevel::Mid => 2,
    }
}

/// Extracts the bandwidth from byte 15 of the channel structure.
///
/// Bit 6: 1 = Wide, 0 = Narrow.
#[must_use]
pub const fn bandwidth_from_bit(byte15: u8) -> Bandwidth {
    if byte15 & 0x40 != 0 {
        Bandwidth::Wide
    } else {
        Bandwidth::Narrow
    }
}

/// Extracts the scan mode from byte 15 of the channel structure.
///
/// Bit 2: 1 = Include, 0 = Skip.
#[must_use]
pub const fn scan_from_bit(byte15: u8) -> ScanMode {
    if byte15 & 0x04 != 0 {
        ScanMode::Include
    } else {
        ScanMode::Skip
    }
}

/// Extracts the busy channel lockout flag from byte 15 of the channel structure.
///
/// Bit 3: 1 = enabled.
#[must_use]
pub const fn bcl_from_bit(byte15: u8) -> bool {
    byte15 & 0x08 != 0
}

/// Decodes the channel name from the name region of the EEPROM.
fn decode_name(image: &MemoryImage, index: u8) -> crate::error::Result<String> {
    let offset = usize::from(NAME_BASE) + usize::from(index) * usize::from(NAME_STRIDE);
    let name_bytes = image.slice(offset, NAME_LENGTH)?;
    let name: String = name_bytes
        .iter()
        .take_while(|&&b| b != 0xFF && b != 0x00)
        .map(|&b| char::from(b))
        .collect();
    Ok(name)
}

/// Encodes a channel name into the name region of the EEPROM.
#[allow(clippy::indexing_slicing)] // i is bounded by NAME_LENGTH (7) < buf.len() (16)
fn encode_name(image: &mut MemoryImage, index: u8, name: &str) -> crate::error::Result<()> {
    let offset = usize::from(NAME_BASE) + usize::from(index) * usize::from(NAME_STRIDE);
    let mut buf = [0xFFu8; 16]; // full 16-byte name slot
    for (i, byte) in name.bytes().take(NAME_LENGTH).enumerate() {
        buf[i] = byte;
    }
    image.write(offset, &buf)
}

/// Computes the frequency offset between RX and TX.
const fn compute_offset(rx_hz: u64, tx_hz: u64) -> FrequencyOffset {
    if tx_hz == rx_hz {
        FrequencyOffset::None
    } else if tx_hz > rx_hz {
        FrequencyOffset::Plus(Frequency::hz(tx_hz - rx_hz))
    } else {
        FrequencyOffset::Minus(Frequency::hz(rx_hz - tx_hz))
    }
}

/// Decodes a single channel from the EEPROM image.
///
/// Returns `Ok(None)` if the channel slot is empty (first byte == 0xFF).
///
/// # Errors
///
/// Returns an error if the channel index is out of range or the EEPROM
/// data contains invalid encodings (bad BCD, unknown tone value, etc.).
#[allow(clippy::indexing_slicing)] // data is always 16 bytes from slice()
pub fn decode_channel(image: &MemoryImage, index: u8) -> crate::error::Result<Option<Channel>> {
    snafu::ensure!(
        index < CHANNEL_COUNT,
        ChannelIndexOutOfRangeSnafu {
            index,
            max: CHANNEL_COUNT
        }
    );

    let ch_offset = usize::from(CHANNEL_BASE) + usize::from(index) * usize::from(CHANNEL_STRIDE);
    let data = image.slice(ch_offset, 16)?;

    // Empty channel: first byte of rxfreq == 0xFF
    if data[0] == 0xFF {
        return Ok(None);
    }

    // RX frequency (bytes 0..4)
    let rx_bcd: [u8; 4] = [data[0], data[1], data[2], data[3]];
    let rx_hz = lbcd4_decode(rx_bcd)?.ok_or(crate::error::Error::EmptyFrequency)?;

    // TX frequency (bytes 4..8)
    let tx_bcd: [u8; 4] = [data[4], data[5], data[6], data[7]];
    let tx_decoded = lbcd4_decode(tx_bcd)?;

    // Determine TX freq and offset
    let (tx_freq, offset) = match tx_decoded {
        None => (None, FrequencyOffset::None), // TX disabled
        Some(tx_hz) if tx_hz == rx_hz => (None, FrequencyOffset::None), // simplex
        Some(tx_hz) => (Some(Frequency::hz(tx_hz)), compute_offset(rx_hz, tx_hz)),
    };

    // TX tone (bytes 10..12, little-endian u16)
    // WHY: The UV-5R stores RX and TX tones separately. We use the TX tone
    // as the channel's tone mode because it's the primary one users configure.
    let txtone_raw = u16::from_le_bytes([data[10], data[11]]);
    let tone = decode_tone(txtone_raw)?;

    // Power level (byte 14, bits 1:0)
    let power = power_from_bits(data[14]);

    // Bandwidth (byte 15, bit 6)
    let bandwidth = bandwidth_from_bit(data[15]);

    // Scan mode (byte 15, bit 2)
    let scan = scan_from_bit(data[15]);

    // Busy channel lockout (byte 15, bit 3)
    let busy_lock = bcl_from_bit(data[15]);

    // Channel name
    let name = decode_name(image, index)?;

    Ok(Some(Channel {
        index: u16::from(index),
        name,
        rx_freq: Frequency::hz(rx_hz),
        tx_freq,
        offset,
        tone,
        power,
        bandwidth,
        scan,
        busy_lock,
    }))
}

/// Encodes a single channel into the EEPROM image.
///
/// Writes both the channel data region and the name region.
///
/// # Errors
///
/// Returns an error if the channel index is out of range or encoding fails.
pub fn encode_channel(
    channel: &Channel,
    image: &mut MemoryImage,
    index: u8,
) -> crate::error::Result<()> {
    snafu::ensure!(
        index < CHANNEL_COUNT,
        ChannelIndexOutOfRangeSnafu {
            index,
            max: CHANNEL_COUNT
        }
    );

    let ch_offset = usize::from(CHANNEL_BASE) + usize::from(index) * usize::from(CHANNEL_STRIDE);

    let mut data = [0u8; 16];

    // RX frequency (bytes 0..4)
    let rx_bcd = lbcd4_encode(channel.rx_freq.as_hz())?;
    data[0..4].copy_from_slice(&rx_bcd);

    // TX frequency (bytes 4..8)
    let tx_hz = channel
        .tx_freq
        .map_or(channel.rx_freq.as_hz(), |f| f.as_hz());
    let tx_bcd = lbcd4_encode(tx_hz)?;
    data[4..8].copy_from_slice(&tx_bcd);

    // RX tone (bytes 8..10) — store same as TX tone for now
    let tone_raw = encode_tone(&channel.tone)?;
    data[8..10].copy_from_slice(&tone_raw.to_le_bytes());

    // TX tone (bytes 10..12)
    data[10..12].copy_from_slice(&tone_raw.to_le_bytes());

    // Byte 12: isuhf(1), unused(3), scode(4)
    let is_uhf = if channel.rx_freq.as_hz() >= 400_000_000 {
        0x80
    } else {
        0x00
    };
    data[12] = is_uhf;

    // Byte 13: unknown(7), txtoneicon(1)
    data[13] = u8::from(channel.tone != ToneMode::None);

    // Byte 14: mailicon(3), unknown(3), lowpower(2)
    data[14] = power_to_bits(channel.power);

    // Byte 15: unknown(1), wide(1), unknown(2), bcl(1), scan(1), pttid(2)
    let wide_bit: u8 = match channel.bandwidth {
        Bandwidth::Wide => 0x40,
        _ => 0x00,
    };
    let scan_bit: u8 = match channel.scan {
        ScanMode::Include => 0x04,
        _ => 0x00,
    };
    let bcl_bit = if channel.busy_lock { 0x08 } else { 0x00 };
    data[15] = wide_bit | bcl_bit | scan_bit;

    image.write(ch_offset, &data)?;

    // Write channel name
    encode_name(image, index, &channel.name)?;

    Ok(())
}

/// Clears a channel slot by filling both the channel data and name regions with `0xFF`.
///
/// # Errors
///
/// Returns an error if the index is out of range or the write fails.
pub fn clear_channel(image: &mut MemoryImage, index: u8) -> crate::error::Result<()> {
    snafu::ensure!(
        index < CHANNEL_COUNT,
        ChannelIndexOutOfRangeSnafu {
            index,
            max: CHANNEL_COUNT
        }
    );

    let ch_offset = usize::from(CHANNEL_BASE) + usize::from(index) * usize::from(CHANNEL_STRIDE);
    image.write(ch_offset, &[0xFF; 16])?;

    let name_offset = usize::from(NAME_BASE) + usize::from(index) * usize::from(NAME_STRIDE);
    image.write(name_offset, &[0xFF; 16])?;

    Ok(())
}

/// Decodes all non-empty channels from the EEPROM image into a [`FrequencyPlan`].
///
/// # Errors
///
/// Returns an error if any channel contains invalid data.
pub fn decode_all_channels(image: &MemoryImage) -> crate::error::Result<FrequencyPlan> {
    let mut channels = Vec::new();
    for i in 0..CHANNEL_COUNT {
        if let Some(ch) = decode_channel(image, i)? {
            channels.push(ch);
        }
    }
    Ok(FrequencyPlan {
        name: String::new(),
        radio_model: Some("Baofeng UV-5R".to_string()),
        channels,
        created: None,
    })
}

/// Encodes all channels from a [`FrequencyPlan`] into the EEPROM image.
///
/// First clears all 128 channel slots, then writes each channel from the plan.
///
/// # Errors
///
/// Returns an error if any channel encoding fails.
pub fn encode_all_channels(
    plan: &FrequencyPlan,
    image: &mut MemoryImage,
) -> crate::error::Result<()> {
    // Clear all slots first
    for i in 0..CHANNEL_COUNT {
        clear_channel(image, i)?;
    }
    // Write each channel
    for channel in &plan.channels {
        #[allow(clippy::cast_possible_truncation)] // index validated by CHANNEL_COUNT
        let index = channel.index as u8;
        encode_channel(channel, image, index)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::tone::{CtcssTone, DcsCode, DcsPolarity};

    /// Builds a test EEPROM image with known channels.
    fn build_test_image() -> MemoryImage {
        let mut image = MemoryImage::blank();

        // Channel 0: 146.520 MHz simplex, no tone, high power, wide
        // 146520000 / 10 = 14652000, BCD: 14|65|20|00, LE: [00,20,65,14]
        let ch0_data: [u8; 16] = [
            0x00, 0x20, 0x65, 0x14, // rxfreq: 146.520 MHz
            0x00, 0x20, 0x65, 0x14, // txfreq: 146.520 MHz (simplex)
            0x00, 0x00, // rxtone: none
            0x00, 0x00, // txtone: none
            0x00, // byte12: VHF
            0x00, // byte13
            0x00, // byte14: high power (0)
            0x44, // byte15: wide(0x40) | scan(0x04)
        ];
        image.write(0x0000, &ch0_data).unwrap();
        image
            .write(
                0x1000,
                b"CALL\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF",
            )
            .unwrap();

        // Channel 1: 147.060 MHz, +0.600 offset, CTCSS 100.0 Hz, high power, wide
        // rx: 147060000/10 = 14706000, BCD: 14|70|60|00, LE: [00,60,70,14]
        // tx: 147660000/10 = 14766000, BCD: 14|76|60|00, LE: [00,60,76,14]
        let ch1_data: [u8; 16] = [
            0x00, 0x60, 0x70, 0x14, // rxfreq: 147.060 MHz
            0x00, 0x60, 0x76, 0x14, // txfreq: 147.660 MHz (+600 kHz)
            0xE8, 0x03, // rxtone: 1000 = CTCSS 100.0 Hz
            0xE8, 0x03, // txtone: 1000 = CTCSS 100.0 Hz
            0x00, // byte12: VHF
            0x01, // byte13: txtoneicon
            0x00, // byte14: high power (0)
            0x44, // byte15: wide(0x40) | scan(0x04)
        ];
        image.write(0x0010, &ch1_data).unwrap();
        image
            .write(
                0x1010,
                &[
                    b'R', b'P', b'T', 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                    0xFF, 0xFF, 0xFF,
                ],
            )
            .unwrap();

        // Channel 2: 446.000 MHz, DCS 023 normal, low power, narrow
        // 446000000/10 = 44600000, BCD: 44|60|00|00, LE: [00,00,60,44]
        let ch2_data: [u8; 16] = [
            0x00, 0x00, 0x60, 0x44, // rxfreq: 446.000 MHz
            0x00, 0x00, 0x60, 0x44, // txfreq: 446.000 MHz (simplex)
            0x01, 0x00, // rxtone: DCS 023 normal (index 1)
            0x01, 0x00, // txtone: DCS 023 normal (index 1)
            0x80, // byte12: UHF
            0x01, // byte13: txtoneicon
            0x01, // byte14: low power (1)
            0x08, // byte15: narrow(0) | bcl(0x08)
        ];
        image.write(0x0020, &ch2_data).unwrap();
        image
            .write(0x1020, b"UHF-CH\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF")
            .unwrap();

        // Channels 3..127 are already 0xFF (blank image)
        image
    }

    #[test]
    fn decode_channel_0_simplex() {
        let image = build_test_image();
        let ch = decode_channel(&image, 0).unwrap().unwrap();
        assert_eq!(ch.index, 0);
        assert_eq!(ch.name, "CALL");
        assert_eq!(ch.rx_freq, Frequency::hz(146_520_000));
        assert!(ch.tx_freq.is_none());
        assert_eq!(ch.offset, FrequencyOffset::None);
        assert_eq!(ch.tone, ToneMode::None);
        assert_eq!(ch.power, PowerLevel::High);
        assert_eq!(ch.bandwidth, Bandwidth::Wide);
        assert_eq!(ch.scan, ScanMode::Include);
        assert!(!ch.busy_lock);
    }

    #[test]
    fn decode_channel_1_repeater() {
        let image = build_test_image();
        let ch = decode_channel(&image, 1).unwrap().unwrap();
        assert_eq!(ch.index, 1);
        assert_eq!(ch.name, "RPT");
        assert_eq!(ch.rx_freq, Frequency::hz(147_060_000));
        assert_eq!(ch.tx_freq, Some(Frequency::hz(147_660_000)));
        assert_eq!(ch.offset, FrequencyOffset::Plus(Frequency::khz(600)));
        assert_eq!(ch.tone, ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()));
        assert_eq!(ch.power, PowerLevel::High);
        assert_eq!(ch.bandwidth, Bandwidth::Wide);
    }

    #[test]
    fn decode_channel_2_dcs() {
        let image = build_test_image();
        let ch = decode_channel(&image, 2).unwrap().unwrap();
        assert_eq!(ch.index, 2);
        assert_eq!(ch.name, "UHF-CH");
        assert_eq!(ch.rx_freq, Frequency::hz(446_000_000));
        assert_eq!(
            ch.tone,
            ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Normal)
        );
        assert_eq!(ch.power, PowerLevel::Low);
        assert_eq!(ch.bandwidth, Bandwidth::Narrow);
        assert!(ch.busy_lock);
    }

    #[test]
    fn decode_empty_channel_returns_none() {
        let image = build_test_image();
        assert!(decode_channel(&image, 3).unwrap().is_none());
        assert!(decode_channel(&image, 127).unwrap().is_none());
    }

    #[test]
    fn channel_index_out_of_range() {
        let image = build_test_image();
        assert!(decode_channel(&image, 128).is_err());
    }

    #[test]
    fn encode_then_decode_roundtrip() {
        let original = Channel {
            index: 5,
            name: "TEST".to_string(),
            rx_freq: Frequency::hz(146_520_000),
            tx_freq: None,
            offset: FrequencyOffset::None,
            tone: ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()),
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        };

        let mut image = MemoryImage::blank();
        encode_channel(&original, &mut image, 5).unwrap();
        let decoded = decode_channel(&image, 5).unwrap().unwrap();

        assert_eq!(decoded.index, original.index);
        assert_eq!(decoded.name, original.name);
        assert_eq!(decoded.rx_freq, original.rx_freq);
        assert_eq!(decoded.tone, original.tone);
        assert_eq!(decoded.power, original.power);
        assert_eq!(decoded.bandwidth, original.bandwidth);
        assert_eq!(decoded.scan, original.scan);
        assert_eq!(decoded.busy_lock, original.busy_lock);
    }

    #[test]
    fn decode_encode_byte_level_roundtrip() {
        let image = build_test_image();

        // Decode channel 0
        let ch = decode_channel(&image, 0).unwrap().unwrap();

        // Encode into a fresh image
        let mut new_image = MemoryImage::blank();
        encode_channel(&ch, &mut new_image, 0).unwrap();

        // Compare the raw channel data bytes
        let original_bytes = image.slice(0x0000, 16).unwrap();
        let new_bytes = new_image.slice(0x0000, 16).unwrap();
        assert_eq!(
            original_bytes, new_bytes,
            "channel 0 data bytes differ after roundtrip"
        );

        // Compare the raw name bytes
        let original_name = image.slice(0x1000, 16).unwrap();
        let new_name = new_image.slice(0x1000, 16).unwrap();
        assert_eq!(
            original_name, new_name,
            "channel 0 name bytes differ after roundtrip"
        );
    }

    #[test]
    fn name_padding_with_0xff() {
        let mut image = MemoryImage::blank();
        let ch = Channel {
            index: 0,
            name: "AB".to_string(),
            rx_freq: Frequency::hz(146_520_000),
            tx_freq: None,
            offset: FrequencyOffset::None,
            tone: ToneMode::None,
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        };
        encode_channel(&ch, &mut image, 0).unwrap();

        let name_bytes = image.slice(0x1000, 7).unwrap();
        assert_eq!(name_bytes[0], b'A');
        assert_eq!(name_bytes[1], b'B');
        for &byte in &name_bytes[2..] {
            assert_eq!(byte, 0xFF, "unused name bytes must be 0xFF");
        }
    }

    #[test]
    fn power_bits_high() {
        assert_eq!(power_from_bits(0x00), PowerLevel::High);
        assert_eq!(power_to_bits(PowerLevel::High), 0);
    }

    #[test]
    fn power_bits_low() {
        assert_eq!(power_from_bits(0x01), PowerLevel::Low);
        assert_eq!(power_to_bits(PowerLevel::Low), 1);
    }

    #[test]
    fn power_bits_mid() {
        assert_eq!(power_from_bits(0x02), PowerLevel::Mid);
        assert_eq!(power_to_bits(PowerLevel::Mid), 2);
    }

    #[test]
    fn bandwidth_bit_wide() {
        assert_eq!(bandwidth_from_bit(0x40), Bandwidth::Wide);
    }

    #[test]
    fn bandwidth_bit_narrow() {
        assert_eq!(bandwidth_from_bit(0x00), Bandwidth::Narrow);
    }

    #[test]
    fn scan_bit_include() {
        assert_eq!(scan_from_bit(0x04), ScanMode::Include);
    }

    #[test]
    fn scan_bit_skip() {
        assert_eq!(scan_from_bit(0x00), ScanMode::Skip);
    }

    #[test]
    fn bcl_bit_enabled() {
        assert!(bcl_from_bit(0x08));
    }

    #[test]
    fn bcl_bit_disabled() {
        assert!(!bcl_from_bit(0x00));
    }

    #[test]
    fn clear_channel_fills_with_0xff() {
        let mut image = build_test_image();
        clear_channel(&mut image, 0).unwrap();

        // Verify channel data is 0xFF
        let data = image.slice(0x0000, 16).unwrap();
        assert!(data.iter().all(|&b| b == 0xFF));

        // Verify name data is 0xFF
        let name = image.slice(0x1000, 16).unwrap();
        assert!(name.iter().all(|&b| b == 0xFF));

        // Verify it now decodes as empty
        assert!(decode_channel(&image, 0).unwrap().is_none());
    }

    #[test]
    fn decode_all_channels_count() {
        let image = build_test_image();
        let plan = decode_all_channels(&image).unwrap();
        assert_eq!(plan.channel_count(), 3);
        assert_eq!(plan.radio_model.as_deref(), Some("Baofeng UV-5R"));
    }

    #[test]
    fn encode_decode_all_channels_roundtrip() {
        let image = build_test_image();
        let plan = decode_all_channels(&image).unwrap();

        let mut new_image = MemoryImage::blank();
        encode_all_channels(&plan, &mut new_image).unwrap();

        let restored = decode_all_channels(&new_image).unwrap();
        assert_eq!(plan.channel_count(), restored.channel_count());

        for (original, decoded) in plan.channels.iter().zip(restored.channels.iter()) {
            assert_eq!(original.index, decoded.index);
            assert_eq!(original.name, decoded.name);
            assert_eq!(original.rx_freq, decoded.rx_freq);
            assert_eq!(original.tone, decoded.tone);
            assert_eq!(original.power, decoded.power);
            assert_eq!(original.bandwidth, decoded.bandwidth);
        }
    }
}
