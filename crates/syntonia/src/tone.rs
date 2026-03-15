//! CTCSS tone and DCS code types for squelch systems.

use serde::{Deserialize, Serialize};
use snafu::ensure;

use crate::error::{InvalidCtcssToneSnafu, InvalidDcsCodeSnafu};

/// The 50 standard CTCSS tone frequencies in Hz.
pub const ALL_CTCSS_TONES: [f32; 50] = [
    67.0, 69.3, 71.9, 74.4, 77.0, 79.7, 82.5, 85.4, 88.5, 91.5, 94.8, 97.4, 100.0, 103.5, 107.2,
    110.9, 114.8, 118.8, 123.0, 127.3, 131.8, 136.5, 141.3, 146.2, 151.4, 156.7, 159.8, 162.2,
    165.5, 167.9, 171.3, 173.8, 177.3, 179.9, 183.5, 186.2, 189.9, 192.8, 196.6, 199.5, 203.5,
    206.5, 210.7, 218.1, 225.7, 229.1, 233.6, 241.8, 250.3, 254.1,
];

/// The 104 standard DCS codes.
pub const ALL_DCS_CODES: [u16; 104] = [
    23, 25, 26, 31, 32, 36, 43, 47, 51, 53, 54, 65, 71, 72, 73, 74, 114, 115, 116, 122, 125, 131,
    132, 134, 143, 145, 152, 155, 156, 162, 165, 172, 174, 205, 212, 223, 225, 226, 243, 244, 245,
    246, 251, 252, 255, 261, 263, 265, 266, 271, 274, 306, 311, 315, 325, 331, 332, 343, 346, 351,
    356, 364, 365, 371, 411, 412, 413, 423, 431, 432, 445, 446, 452, 454, 455, 462, 464, 465, 466,
    503, 506, 516, 523, 526, 532, 546, 565, 606, 612, 624, 627, 631, 632, 654, 662, 664, 703, 712,
    723, 731, 732, 734, 743, 754,
];

/// A validated CTCSS (Continuous Tone-Coded Squelch System) tone frequency.
///
/// Wraps one of the 50 standard sub-audible tones (67.0–254.1 Hz).
/// Construction validates against the known tone set.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct CtcssTone(f32);

impl CtcssTone {
    /// Creates a new `CtcssTone` if the value matches a standard tone.
    ///
    /// Comparison uses tenths-of-Hz integer matching to avoid float precision issues.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCtcssTone`](crate::error::Error::InvalidCtcssTone)
    /// if the value is not one of the 50 standard CTCSS tones.
    pub fn new(value: f32) -> crate::error::Result<Self> {
        #[allow(clippy::cast_sign_loss)] // CTCSS tones are always positive
        let tenths = (value * 10.0).round() as u32;
        #[allow(clippy::cast_sign_loss)] // CTCSS tones are always positive
        let valid = ALL_CTCSS_TONES
            .iter()
            .any(|&t| (t * 10.0).round() as u32 == tenths);
        ensure!(valid, InvalidCtcssToneSnafu { value });
        Ok(Self(value))
    }

    /// Returns the tone frequency in Hz.
    #[must_use]
    pub const fn as_hz(&self) -> f32 {
        self.0
    }
}

impl Eq for CtcssTone {}

impl<'de> Deserialize<'de> for CtcssTone {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f32::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// A validated DCS (Digital-Coded Squelch) code.
///
/// Wraps one of the 104 standard DCS codes. Construction validates
/// against the known code set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct DcsCode(u16);

impl DcsCode {
    /// Creates a new `DcsCode` if the value matches a standard code.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidDcsCode`](crate::error::Error::InvalidDcsCode)
    /// if the value is not one of the 104 standard DCS codes.
    pub fn new(value: u16) -> crate::error::Result<Self> {
        ensure!(
            ALL_DCS_CODES.contains(&value),
            InvalidDcsCodeSnafu { value }
        );
        Ok(Self(value))
    }

    /// Returns the raw DCS code number.
    #[must_use]
    pub const fn as_code(&self) -> u16 {
        self.0
    }
}

impl<'de> Deserialize<'de> for DcsCode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u16::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// DCS code polarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DcsPolarity {
    /// Normal polarity.
    Normal,
    /// Inverted polarity.
    Inverted,
}

/// Squelch tone configuration for a channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ToneMode {
    /// No tone squelch.
    None,
    /// CTCSS sub-audible tone.
    Ctcss(CtcssTone),
    /// DCS digital code with polarity.
    Dcs(DcsCode, DcsPolarity),
}

impl Eq for ToneMode {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn all_50_ctcss_tones_accepted() {
        for &tone in &ALL_CTCSS_TONES {
            assert!(
                CtcssTone::new(tone).is_ok(),
                "valid CTCSS tone {tone} was rejected"
            );
        }
    }

    #[test]
    fn invalid_ctcss_tone_rejected() {
        assert!(CtcssTone::new(0.0).is_err());
        assert!(CtcssTone::new(50.0).is_err());
        assert!(CtcssTone::new(100.1).is_err());
        assert!(CtcssTone::new(300.0).is_err());
    }

    #[test]
    fn ctcss_tone_value_preserved() {
        let tone = CtcssTone::new(100.0).unwrap();
        assert!((tone.as_hz() - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn all_104_dcs_codes_accepted() {
        for &code in &ALL_DCS_CODES {
            assert!(
                DcsCode::new(code).is_ok(),
                "valid DCS code {code} was rejected"
            );
        }
    }

    #[test]
    fn invalid_dcs_code_rejected() {
        assert!(DcsCode::new(0).is_err());
        assert!(DcsCode::new(1).is_err());
        assert!(DcsCode::new(999).is_err());
        assert!(DcsCode::new(100).is_err());
    }

    #[test]
    fn dcs_code_value_preserved() {
        let code = DcsCode::new(23).unwrap();
        assert_eq!(code.as_code(), 23);
    }

    #[test]
    fn ctcss_serde_roundtrip() {
        let tone = CtcssTone::new(146.2).unwrap();
        let json = serde_json::to_string(&tone).unwrap();
        let restored: CtcssTone = serde_json::from_str(&json).unwrap();
        assert_eq!(tone, restored);
    }

    #[test]
    fn dcs_serde_roundtrip() {
        let code = DcsCode::new(23).unwrap();
        let json = serde_json::to_string(&code).unwrap();
        let restored: DcsCode = serde_json::from_str(&json).unwrap();
        assert_eq!(code, restored);
    }

    #[test]
    fn invalid_ctcss_rejected_during_deserialization() {
        let json = "50.0";
        let result: std::result::Result<CtcssTone, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_dcs_rejected_during_deserialization() {
        let json = "999";
        let result: std::result::Result<DcsCode, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn tone_mode_none_serde_roundtrip() {
        let mode = ToneMode::None;
        let json = serde_json::to_string(&mode).unwrap();
        let restored: ToneMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, restored);
    }

    #[test]
    fn tone_mode_ctcss_serde_roundtrip() {
        let mode = ToneMode::Ctcss(CtcssTone::new(100.0).unwrap());
        let json = serde_json::to_string(&mode).unwrap();
        let restored: ToneMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, restored);
    }

    #[test]
    fn tone_mode_dcs_serde_roundtrip() {
        let mode = ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Inverted);
        let json = serde_json::to_string(&mode).unwrap();
        let restored: ToneMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, restored);
    }
}
