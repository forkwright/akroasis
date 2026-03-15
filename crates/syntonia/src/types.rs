//! Supporting types for channel configuration.

use koinon::Frequency;
use serde::{Deserialize, Serialize};

/// Frequency offset configuration for repeater operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FrequencyOffset {
    /// No offset — simplex operation.
    None,
    /// Positive offset from RX frequency.
    Plus(Frequency),
    /// Negative offset from RX frequency.
    Minus(Frequency),
    /// Arbitrary split — TX on a specific frequency unrelated to RX by standard offset.
    Split(Frequency),
}

/// Transmit power level.
///
/// Actual wattage is radio-specific and not stored here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PowerLevel {
    /// Maximum transmit power.
    High,
    /// Medium transmit power.
    Mid,
    /// Minimum transmit power.
    Low,
}

/// Channel bandwidth selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Bandwidth {
    /// Wide bandwidth (25 kHz).
    Wide,
    /// Narrow bandwidth (12.5 kHz).
    Narrow,
}

/// Scanner behavior for a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ScanMode {
    /// Include this channel in scan lists.
    Include,
    /// Skip this channel when scanning.
    Skip,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn frequency_offset_none_serde_roundtrip() {
        let offset = FrequencyOffset::None;
        let json = serde_json::to_string(&offset).unwrap();
        let restored: FrequencyOffset = serde_json::from_str(&json).unwrap();
        assert_eq!(offset, restored);
    }

    #[test]
    fn frequency_offset_plus_serde_roundtrip() {
        let offset = FrequencyOffset::Plus(Frequency::khz(600));
        let json = serde_json::to_string(&offset).unwrap();
        let restored: FrequencyOffset = serde_json::from_str(&json).unwrap();
        assert_eq!(offset, restored);
    }

    #[test]
    fn frequency_offset_split_serde_roundtrip() {
        let offset = FrequencyOffset::Split(Frequency::mhz(146));
        let json = serde_json::to_string(&offset).unwrap();
        let restored: FrequencyOffset = serde_json::from_str(&json).unwrap();
        assert_eq!(offset, restored);
    }

    #[test]
    fn power_level_serde_roundtrip() {
        for level in [PowerLevel::High, PowerLevel::Mid, PowerLevel::Low] {
            let json = serde_json::to_string(&level).unwrap();
            let restored: PowerLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(level, restored);
        }
    }

    #[test]
    fn bandwidth_serde_roundtrip() {
        for bw in [Bandwidth::Wide, Bandwidth::Narrow] {
            let json = serde_json::to_string(&bw).unwrap();
            let restored: Bandwidth = serde_json::from_str(&json).unwrap();
            assert_eq!(bw, restored);
        }
    }

    #[test]
    fn scan_mode_serde_roundtrip() {
        for mode in [ScanMode::Include, ScanMode::Skip] {
            let json = serde_json::to_string(&mode).unwrap();
            let restored: ScanMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, restored);
        }
    }
}
