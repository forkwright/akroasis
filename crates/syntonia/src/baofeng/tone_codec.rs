//! Tone encoding/decoding for UV-5R EEPROM format.

use crate::tone::{ALL_DCS_CODES, CtcssTone, DcsCode, DcsPolarity, ToneMode};

/// Decode a raw 16-bit unsigned value FROM EEPROM INTO a `ToneMode`.
///
/// Encoding rules:
/// - 0x0000 or 0xFFFF = no tone
/// - >= 600 = CTCSS tone (value / 10.0 = tone in Hz)
/// - 1..=104 = DCS normal polarity (1-indexed INTO DCS code table)
/// - 106..=209 = DCS inverted polarity (index = value - 105, 1-indexed)
pub fn decode_tone(raw: u16) -> ToneMode {
    if raw == 0 || raw == 0xFFFF {
        return ToneMode::None;
    }

    if raw >= 600 {
        let freq = f32::from(raw) / 10.0;
        return CtcssTone::new(freq).map_or(ToneMode::None, ToneMode::Ctcss);
    }

    if let Some(mode) = decode_dcs(raw, 1, 104, DcsPolarity::Normal) {
        return mode;
    }

    if let Some(mode) = decode_dcs(raw, 106, 209, DcsPolarity::Inverted) {
        return mode;
    }

    ToneMode::None
}

fn decode_dcs(raw: u16, lo: u16, hi: u16, polarity: DcsPolarity) -> Option<ToneMode> {
    if !(lo..=hi).contains(&raw) {
        return None;
    }
    let idx = (raw - lo) as usize; // SAFETY: bounds-checked by caller (raw ∈ lo..=hi where hi ≤ 209); fits usize
    let &code_val = ALL_DCS_CODES.get(idx)?;
    let code = DcsCode::new(code_val).ok()?;
    Some(ToneMode::Dcs(code, polarity))
}

/// Encode a `ToneMode` INTO a raw 16-bit value for EEPROM storage.
pub fn encode_tone(tone: ToneMode) -> u16 {
    match tone {
        ToneMode::None => 0,
        ToneMode::Ctcss(ct) => {
            #[expect(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "CTCSS tones are always positive and < 300 Hz; * 10.0 fits in u16 without sign loss"
            )]
            let raw = (ct.as_hz() * 10.0) as u16; // SAFETY: CTCSS tones are positive, <300 Hz; *10.0 fits u16
            raw
        }
        ToneMode::Dcs(code, polarity) => {
            let idx = ALL_DCS_CODES
                .iter()
                .position(|&c| c == code.as_code())
                .map(|i| i + 1);

            match (idx, polarity) {
                (Some(i), DcsPolarity::Normal) => i as u16, // SAFETY: idx ∈ 1..=104 from DCS table lookup; fits u16
                (Some(i), DcsPolarity::Inverted) => (i + 105) as u16, // SAFETY: idx ∈ 1..=104 from DCS table lookup; +105 fits u16
                (None, _) => 0,
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn ctcss_67_roundtrip() {
        let tone = ToneMode::Ctcss(CtcssTone::new(67.0).unwrap());
        let raw = encode_tone(tone);
        assert_eq!(raw, 670);
        assert_eq!(decode_tone(raw), tone);
    }

    #[test]
    fn ctcss_100_roundtrip() {
        let tone = ToneMode::Ctcss(CtcssTone::new(100.0).unwrap());
        let raw = encode_tone(tone);
        assert_eq!(raw, 1000);
        assert_eq!(decode_tone(raw), tone);
    }

    #[test]
    fn dcs_023_normal_roundtrip() {
        let tone = ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Normal);
        let raw = encode_tone(tone);
        assert_eq!(raw, 1);
        assert_eq!(decode_tone(raw), tone);
    }

    #[test]
    fn dcs_023_inverted_roundtrip() {
        let tone = ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Inverted);
        let raw = encode_tone(tone);
        assert_eq!(raw, 106);
        assert_eq!(decode_tone(raw), tone);
    }

    #[test]
    fn zero_and_ffff_decode_to_none() {
        assert_eq!(decode_tone(0), ToneMode::None);
        assert_eq!(decode_tone(0xFFFF), ToneMode::None);
    }

    #[test]
    fn all_ctcss_tones_roundtrip() {
        for &freq in &crate::tone::ALL_CTCSS_TONES {
            let tone = ToneMode::Ctcss(CtcssTone::new(freq).unwrap());
            let raw = encode_tone(tone);
            let decoded = decode_tone(raw);
            assert_eq!(decoded, tone, "CTCSS {freq} Hz round-trip failed");
        }
    }

    #[test]
    fn all_dcs_codes_roundtrip_both_polarities() {
        for &code_val in &ALL_DCS_CODES {
            let code = DcsCode::new(code_val).unwrap();

            let normal = ToneMode::Dcs(code, DcsPolarity::Normal);
            assert_eq!(decode_tone(encode_tone(normal)), normal);

            let inverted = ToneMode::Dcs(code, DcsPolarity::Inverted);
            assert_eq!(decode_tone(encode_tone(inverted)), inverted);
        }
    }
}
