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
#[allow(clippy::indexing_slicing)]
pub fn decode_channel(image: &MemoryImage, index: u8) -> Result<Option<Channel>, CodecError> {
    let ch_addr = CHANNEL_BASE + u16::from(index) * CHANNEL_STRIDE;
    // SAFETY(indexing): read_bytes returns exactly 16 bytes
    let ch = image.read_bytes(ch_addr, 16);

    if ch.first().copied().unwrap_or_default() == 0xFF {
        return Ok(None);
    }

    let rx_bytes: [u8; 4] = [ch.first().copied().unwrap_or_default(), ch.get(1).copied().unwrap_or_default(), ch.get(2).copied().unwrap_or_default(), ch.get(3).copied().unwrap_or_default()];
    let tx_bytes: [u8; 4] = [ch.get(4).copied().unwrap_or_default(), ch.get(5).copied().unwrap_or_default(), ch.get(6).copied().unwrap_or_default(), ch.get(7).copied().unwrap_or_default()];

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

    let rxtone_raw = u16::from_le_bytes([ch.get(8).copied().unwrap_or_default(), ch.get(9).copied().unwrap_or_default()]);
    let txtone_raw = u16::from_le_bytes([ch.get(10).copied().unwrap_or_default(), ch.get(11).copied().unwrap_or_default()]);

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
#[allow(clippy::indexing_slicing)]
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
    ch_data[0..4].copy_from_slice(&rx_bytes);
    ch_data[4..8].copy_from_slice(&tx_bytes);
    ch_data[8..10].copy_from_slice(&tone_bytes);
    ch_data[10..12].copy_from_slice(&tone_bytes);
    ch_data[14] = power_to_bits(channel.power);

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
    ch_data[15] = byte15;

    image.write_bytes(ch_addr, &ch_data);

    let name_addr = NAME_BASE + u16::from(index) * NAME_STRIDE;
    let mut name_data = [0xFFu8; 16];
    for (i, &byte) in channel.name.as_bytes().iter().take(NAME_LENGTH).enumerate() {
        name_data[i] = byte;
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
        let index = u8::try_from(channel.index).unwrap_or_default();
        if index < CHANNEL_COUNT {
            encode_channel(channel, image, index)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
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
}
