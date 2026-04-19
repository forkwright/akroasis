//! Yaesu FTM-510DR variant configuration.
//!
//! Known parameters derived from FCC filings and CHIRP open-source driver
//! (Apache-2.0). The clone-mode protocol itself is not yet documented.

use koinon::Frequency;

use crate::validate::RadioConstraints;

/// Serial baud rate for clone mode (known from ADMS-14 documentation).
pub const BAUD_RATE: u32 = 38_400;

/// Maximum channel name length (6 characters, alphanumeric).
pub const MAX_NAME_LEN: usize = 6;

/// Total number of programmable memory channels.
pub const CHANNEL_COUNT: u16 = 900;

/// EEPROM image size in bytes (estimated from CHIRP source).
///
/// TODO(#80): verify against actual ADMS-14 traffic capture.
pub const IMAGE_SIZE: usize = 65_536;

/// Radio constraints for the FTM-510DR.
///
/// Frequency bands and power levels from FCC filing and owner's manual.
#[must_use]
pub fn ftm510dr_constraints() -> RadioConstraints {
    RadioConstraints {
        max_channels: CHANNEL_COUNT,
        max_name_length: MAX_NAME_LEN,
        valid_bands: vec![
            (Frequency::mhz(144), Frequency::mhz(148)), // VHF
            (Frequency::mhz(430), Frequency::mhz(450)), // UHF
        ],
        power_levels: vec![
            crate::types::PowerLevel::High,
            crate::types::PowerLevel::Mid,
            crate::types::PowerLevel::Low,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constraints_have_valid_frequency_ranges() {
        let c = ftm510dr_constraints();
        for (lo, hi) in &c.valid_bands {
            assert!(lo < hi, "frequency range must be ascending: {lo}..{hi}");
        }
    }

    #[test]
    fn constraints_support_three_power_levels() {
        let c = ftm510dr_constraints();
        assert_eq!(c.power_levels.len(), 3);
    }

    #[test]
    fn channel_count_matches_spec() {
        assert_eq!(CHANNEL_COUNT, 900);
    }
}
