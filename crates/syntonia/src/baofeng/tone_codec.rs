//! Tone encoding/decoding for the Baofeng UV-5R EEPROM format.
//!
//! The UV-5R encodes tones as a 16-bit unsigned little-endian value:
//! - `0x0000` or `0xFFFF` = no tone
//! - `>= 600` = CTCSS: value / 10.0 = tone in Hz
//! - `1..=104` = DCS normal polarity, index into the standard 104-code table (1-based)
//! - `105..=208` = DCS inverted polarity, index = value - 104 (1-based into table)

use crate::error::InvalidToneRawSnafu;
use crate::tone::{ALL_DCS_CODES, CtcssTone, DcsCode, DcsPolarity, ToneMode};

/// Decodes a raw 16-bit tone value into a [`ToneMode`].
///
/// # Errors
///
/// Returns an error if the raw value falls in an unrecognized range or
/// references an out-of-bounds DCS index.
pub fn decode_tone(raw: u16) -> crate::error::Result<ToneMode> {
    match raw {
        0x0000 | 0xFFFF => Ok(ToneMode::None),
        // DCS normal: 1-based index into 104-code table
        1..=104 => {
            let idx = (raw - 1) as usize;
            let code = ALL_DCS_CODES.get(idx).copied().ok_or(
                crate::error::Error::ToneIndexOutOfRange {
                    index: raw,
                    max: 104,
                },
            )?;
            Ok(ToneMode::Dcs(DcsCode::new(code)?, DcsPolarity::Normal))
        }
        // DCS inverted: 1-based index offset by 104
        105..=208 => {
            let idx = (raw - 105) as usize;
            let code = ALL_DCS_CODES.get(idx).copied().ok_or(
                crate::error::Error::ToneIndexOutOfRange {
                    index: raw,
                    max: 208,
                },
            )?;
            Ok(ToneMode::Dcs(DcsCode::new(code)?, DcsPolarity::Inverted))
        }
        // CTCSS: raw value is tone frequency * 10
        v if v >= 600 => {
            let hz = f32::from(v) / 10.0;
            let tone = CtcssTone::new(hz)?;
            Ok(ToneMode::Ctcss(tone))
        }
        _ => InvalidToneRawSnafu { raw }.fail(),
    }
}

/// Encodes a [`ToneMode`] into a raw 16-bit value for EEPROM storage.
///
/// # Errors
///
/// Returns an error if a DCS code is not found in the standard table.
pub fn encode_tone(tone: &ToneMode) -> crate::error::Result<u16> {
    match tone {
        ToneMode::Ctcss(ctcss) => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let raw = (ctcss.as_hz() * 10.0).round() as u16;
            Ok(raw)
        }
        ToneMode::Dcs(code, polarity) => {
            let idx = ALL_DCS_CODES
                .iter()
                .position(|&c| c == code.code())
                .ok_or_else(|| crate::error::Error::ToneIndexOutOfRange {
                    index: code.code(),
                    max: 104,
                })?;
            let raw = match polarity {
                DcsPolarity::Normal => (idx as u16) + 1,
                DcsPolarity::Inverted => (idx as u16) + 105,
            };
            Ok(raw)
        }
        _ => Ok(0x0000), // ToneMode::None and any future variants
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::tone::{ALL_CTCSS_TONES, ALL_DCS_CODES};

    #[test]
    fn ctcss_67_0_roundtrip() {
        let raw = 670u16;
        let tone = decode_tone(raw).unwrap();
        assert_eq!(tone, ToneMode::Ctcss(CtcssTone::new(67.0).unwrap()));
        assert_eq!(encode_tone(&tone).unwrap(), raw);
    }

    #[test]
    fn ctcss_100_0_roundtrip() {
        let raw = 1000u16;
        let tone = decode_tone(raw).unwrap();
        assert_eq!(tone, ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()));
        assert_eq!(encode_tone(&tone).unwrap(), raw);
    }

    #[test]
    fn dcs_023_normal_roundtrip() {
        // DCS 023 is at index 0 in ALL_DCS_CODES, so raw = 1
        let tone = ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Normal);
        let raw = encode_tone(&tone).unwrap();
        assert_eq!(raw, 1);
        let decoded = decode_tone(raw).unwrap();
        assert_eq!(decoded, tone);
    }

    #[test]
    fn dcs_023_inverted_roundtrip() {
        // DCS 023 inverted: index 0 + 105 = 105
        let tone = ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Inverted);
        let raw = encode_tone(&tone).unwrap();
        assert_eq!(raw, 105);
        let decoded = decode_tone(raw).unwrap();
        assert_eq!(decoded, tone);
    }

    #[test]
    fn none_from_0x0000() {
        assert_eq!(decode_tone(0x0000).unwrap(), ToneMode::None);
    }

    #[test]
    fn none_from_0xffff() {
        assert_eq!(decode_tone(0xFFFF).unwrap(), ToneMode::None);
    }

    #[test]
    fn roundtrip_all_50_ctcss_tones() {
        for &hz in &ALL_CTCSS_TONES {
            let tone = ToneMode::Ctcss(CtcssTone::new(hz).unwrap());
            let raw = encode_tone(&tone).unwrap();
            let decoded = decode_tone(raw).unwrap();
            assert_eq!(decoded, tone, "CTCSS {hz} Hz failed roundtrip");
        }
    }

    #[test]
    fn roundtrip_all_104_dcs_normal() {
        for &code_val in &ALL_DCS_CODES {
            let tone = ToneMode::Dcs(DcsCode::new(code_val).unwrap(), DcsPolarity::Normal);
            let raw = encode_tone(&tone).unwrap();
            let decoded = decode_tone(raw).unwrap();
            assert_eq!(decoded, tone, "DCS {code_val}N failed roundtrip");
        }
    }

    #[test]
    fn roundtrip_all_104_dcs_inverted() {
        for &code_val in &ALL_DCS_CODES {
            let tone = ToneMode::Dcs(DcsCode::new(code_val).unwrap(), DcsPolarity::Inverted);
            let raw = encode_tone(&tone).unwrap();
            let decoded = decode_tone(raw).unwrap();
            assert_eq!(decoded, tone, "DCS {code_val}R failed roundtrip");
        }
    }

    #[test]
    fn invalid_raw_tone_rejected() {
        // Values 209..599 are invalid
        assert!(decode_tone(300).is_err());
        assert!(decode_tone(500).is_err());
    }
}
