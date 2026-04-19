//! Frequency plan — a named collection of channels for a radio.

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::channel::Channel;
use crate::error::{JsonSnafu, TomlDeserializeSnafu, TomlSerializeSnafu};

/// A frequency plan containing a set of programmed channels.
///
/// Represents a complete channel configuration that can be loaded
/// onto a radio. Serializable to JSON and TOML for import/export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrequencyPlan {
    /// Human-readable name for this plan.
    pub name: String,
    /// Target radio model, if known (e.g. "Baofeng UV-5R").
    pub radio_model: Option<String>,
    /// Ordered list of channels.
    pub channels: Vec<Channel>,
    /// ISO 8601 creation timestamp.
    pub created: Option<String>,
}

impl FrequencyPlan {
    /// Returns a reference to the channel at the given index, if present.
    #[must_use]
    pub fn channel(&self, index: u16) -> Option<&Channel> {
        self.channels.iter().find(|ch| ch.index == index)
    }

    /// Returns the number of channels in this plan.
    #[must_use]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// Serializes the plan to a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_json(&self) -> crate::error::Result<String> {
        serde_json::to_string_pretty(self).context(JsonSnafu)
    }

    /// Deserializes a plan from a JSON string.
    ///
    /// # Errors
    ///
    /// Returns an error if the JSON is invalid or contains invalid data
    /// (e.g. invalid CTCSS tones or DCS codes).
    pub fn from_json(s: &str) -> crate::error::Result<Self> {
        serde_json::from_str(s).context(JsonSnafu)
    }

    /// Serializes the plan to a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails.
    pub fn to_toml(&self) -> crate::error::Result<String> {
        toml::to_string_pretty(self).context(TomlSerializeSnafu)
    }

    /// Deserializes a plan from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid or contains invalid data
    /// (e.g. invalid CTCSS tones or DCS codes).
    pub fn from_toml(s: &str) -> crate::error::Result<Self> {
        toml::from_str(s).context(TomlDeserializeSnafu)
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use koinon::Frequency;

    use super::*;
    use crate::tone::ToneMode;
    use crate::types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};

    fn sample_plan() -> FrequencyPlan {
        FrequencyPlan {
            name: "Test Plan".to_string(),
            radio_model: Some("Baofeng UV-5R".to_string()),
            channels: vec![
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
                },
                Channel {
                    index: 1,
                    name: "RPT".to_string(),
                    rx_freq: Frequency::hz(146_940_000),
                    tx_freq: Some(Frequency::hz(146_340_000)),
                    offset: FrequencyOffset::Minus(Frequency::khz(600)),
                    tone: ToneMode::None,
                    power: PowerLevel::Mid,
                    bandwidth: Bandwidth::Wide,
                    scan: ScanMode::Include,
                    busy_lock: false,
                },
            ],
            created: Some("2026-03-15T00:00:00Z".to_string()),
        }
    }

    #[test]
    fn channel_lookup_by_index() {
        let plan = sample_plan();
        assert!(plan.channel(0).is_some());
        assert!(plan.channel(1).is_some());
        assert!(plan.channel(99).is_none());
    }

    #[test]
    fn channel_count() {
        let plan = sample_plan();
        assert_eq!(plan.channel_count(), 2);
    }

    #[test]
    fn json_roundtrip() {
        let plan = sample_plan();
        let json = plan.to_json().unwrap();
        let restored = FrequencyPlan::from_json(&json).unwrap();
        assert_eq!(plan, restored);
    }

    #[test]
    fn toml_roundtrip() {
        let plan = sample_plan();
        let toml_str = plan.to_toml().unwrap();
        let restored = FrequencyPlan::from_toml(&toml_str).unwrap();
        assert_eq!(plan, restored);
    }
}
