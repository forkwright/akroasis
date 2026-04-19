//! Radio channel data model.

use koinon::Frequency;
use serde::{Deserialize, Serialize};

use crate::tone::ToneMode;
use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

/// A single programmable radio channel (memory slot).
///
/// This is a radio-agnostic representation — specific radio models enforce
/// constraints (name length, power levels, band limits) through
/// [`RadioConstraints`](crate::validate::RadioConstraints) validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    /// Channel memory index (0-based or 1-based depending on radio).
    pub index: u16,
    /// Channel label. Length limits are radio-specific.
    pub name: String,
    /// Receive frequency.
    pub rx_freq: Frequency,
    /// Explicit transmit frequency. `None` means simplex (TX on RX freq).
    pub tx_freq: Option<Frequency>,
    /// Repeater offset configuration.
    pub offset: FrequencyOffset,
    /// Squelch tone configuration.
    pub tone: ToneMode,
    /// Transmit power level.
    pub power: PowerLevel,
    /// Channel bandwidth.
    pub bandwidth: Bandwidth,
    /// Scanner behavior.
    pub scan: ScanMode,
    /// Busy channel lockout — prevents transmitting when channel is busy.
    pub busy_lock: bool,
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;
    use crate::tone::{CtcssTone, DcsCode, DcsPolarity};

    fn sample_simplex_channel() -> Channel {
        Channel {
            index: 0,
            name: "CALL".to_string(),
            rx_freq: Frequency::hz(146_520_000),
            tx_freq: None,
            offset: FrequencyOffset::None,
            tone: ToneMode::None,
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        }
    }

    fn sample_repeater_channel() -> Channel {
        Channel {
            index: 1,
            name: "RPT".to_string(),
            rx_freq: Frequency::hz(146_940_000),
            tx_freq: Some(Frequency::hz(146_340_000)),
            offset: FrequencyOffset::Minus(Frequency::khz(600)),
            tone: ToneMode::Ctcss(CtcssTone::new(100.0).unwrap()),
            power: PowerLevel::High,
            bandwidth: Bandwidth::Wide,
            scan: ScanMode::Include,
            busy_lock: false,
        }
    }

    #[test]
    fn channel_construction_simplex() {
        let ch = sample_simplex_channel();
        assert_eq!(ch.index, 0);
        assert_eq!(ch.name, "CALL");
        assert!(ch.tx_freq.is_none());
    }

    #[test]
    fn channel_construction_repeater() {
        let ch = sample_repeater_channel();
        assert_eq!(ch.index, 1);
        assert!(ch.tx_freq.is_some());
        assert_eq!(ch.offset, FrequencyOffset::Minus(Frequency::khz(600)));
    }

    #[test]
    fn channel_json_roundtrip() {
        let ch = sample_repeater_channel();
        let json = serde_json::to_string(&ch).unwrap();
        let restored: Channel = serde_json::from_str(&json).unwrap();
        assert_eq!(ch, restored);
    }

    #[test]
    fn channel_with_dcs_json_roundtrip() {
        let ch = Channel {
            index: 2,
            name: "DCS-CH".to_string(),
            rx_freq: Frequency::hz(446_000_000),
            tx_freq: None,
            offset: FrequencyOffset::None,
            tone: ToneMode::Dcs(DcsCode::new(23).unwrap(), DcsPolarity::Normal),
            power: PowerLevel::Low,
            bandwidth: Bandwidth::Narrow,
            scan: ScanMode::Skip,
            busy_lock: true,
        };
        let json = serde_json::to_string(&ch).unwrap();
        let restored: Channel = serde_json::from_str(&json).unwrap();
        assert_eq!(ch, restored);
    }
}
