//! Channel and plan validation against radio-specific constraints.

use std::collections::HashSet;

use koinon::Frequency;

use crate::channel::Channel;
use crate::plan::FrequencyPlan;
use crate::types::PowerLevel;

/// A validation finding — either a hard error or a soft warning.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationIssue {
    /// A hard failure that must be resolved.
    Error(String),
    /// A soft issue that may be acceptable.
    Warning(String),
}

/// Radio-specific constraints used to validate channels and plans.
#[derive(Debug, Clone)]
pub struct RadioConstraints {
    /// Maximum number of characters in a channel name.
    pub max_name_length: usize,
    /// Valid frequency bands as (low, high) inclusive pairs.
    pub valid_bands: Vec<(Frequency, Frequency)>,
    /// Supported power levels.
    pub power_levels: Vec<PowerLevel>,
    /// Maximum number of programmable channels.
    pub max_channels: u16,
}

/// Returns constraints for the Baofeng UV-5R.
#[must_use]
pub fn baofeng_uv5r_constraints() -> RadioConstraints {
    RadioConstraints {
        max_name_length: 7,
        valid_bands: vec![
            (Frequency::mhz(136), Frequency::mhz(174)), // VHF
            (Frequency::mhz(400), Frequency::mhz(520)), // UHF
        ],
        power_levels: vec![PowerLevel::High, PowerLevel::Low],
        max_channels: 128,
    }
}

/// Returns constraints for the Baofeng BF-F8HP.
#[must_use]
pub fn baofeng_f8hp_constraints() -> RadioConstraints {
    RadioConstraints {
        max_name_length: 7,
        valid_bands: vec![
            (Frequency::mhz(136), Frequency::mhz(174)), // VHF
            (Frequency::mhz(400), Frequency::mhz(520)), // UHF
        ],
        power_levels: vec![PowerLevel::High, PowerLevel::Mid, PowerLevel::Low],
        max_channels: 128,
    }
}

/// Checks whether a frequency falls within any of the valid bands.
fn freq_in_bands(freq: Frequency, bands: &[(Frequency, Frequency)]) -> bool {
    bands.iter().any(|&(lo, hi)| freq >= lo && freq <= hi)
}

/// Validates a single channel against radio constraints.
///
/// Returns a list of issues found. An empty list means the channel is valid.
#[must_use]
pub fn validate_channel(channel: &Channel, constraints: &RadioConstraints) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // RX frequency must be within valid bands.
    if !freq_in_bands(channel.rx_freq, &constraints.valid_bands) {
        issues.push(ValidationIssue::Error(format!(
            "channel {}: RX frequency {} is outside valid bands",
            channel.index, channel.rx_freq
        )));
    }

    // TX frequency (if explicit) must be within valid bands.
    if let Some(tx) = channel.tx_freq {
        if !freq_in_bands(tx, &constraints.valid_bands) {
            issues.push(ValidationIssue::Error(format!(
                "channel {}: TX frequency {} is outside valid bands",
                channel.index, tx
            )));
        }
    }

    // Name length check — warning, not error.
    if channel.name.len() > constraints.max_name_length {
        issues.push(ValidationIssue::Warning(format!(
            "channel {}: name '{}' exceeds max length of {} characters",
            channel.index, channel.name, constraints.max_name_length
        )));
    }

    // Power level must be supported by the radio.
    if !constraints.power_levels.contains(&channel.power) {
        issues.push(ValidationIssue::Error(format!(
            "channel {}: power level {:?} not supported by this radio",
            channel.index, channel.power
        )));
    }

    // Channel index must be within radio limits.
    if channel.index >= constraints.max_channels {
        issues.push(ValidationIssue::Error(format!(
            "channel {}: index exceeds maximum of {}",
            channel.index,
            constraints.max_channels - 1
        )));
    }

    issues
}

/// Validates an entire frequency plan against radio constraints.
///
/// Checks each channel individually, plus plan-level rules like
/// channel count limits and duplicate frequency detection.
#[must_use]
pub fn validate_plan(plan: &FrequencyPlan, constraints: &RadioConstraints) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    // Channel count.
    if plan.channels.len() > usize::from(constraints.max_channels) {
        issues.push(ValidationIssue::Error(format!(
            "plan has {} channels but radio supports at most {}",
            plan.channels.len(),
            constraints.max_channels
        )));
    }

    // Per-channel validation.
    for channel in &plan.channels {
        issues.extend(validate_channel(channel, constraints));
    }

    // Duplicate RX frequency detection.
    let mut seen_rx = HashSet::new();
    for channel in &plan.channels {
        if !seen_rx.insert(channel.rx_freq) {
            issues.push(ValidationIssue::Warning(format!(
                "channel {}: duplicate RX frequency {}",
                channel.index, channel.rx_freq
            )));
        }
    }

    issues
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::tone::ToneMode;
    use crate::types::{Bandwidth, FrequencyOffset, ScanMode};

    fn valid_vhf_channel(index: u16) -> Channel {
        Channel {
            index,
            name: "TEST".to_string(),
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

    #[test]
    fn valid_channel_passes_validation() {
        let ch = valid_vhf_channel(0);
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(issues.is_empty(), "expected no issues, got: {issues:?}");
    }

    #[test]
    fn out_of_band_rx_frequency_rejected() {
        let ch = Channel {
            rx_freq: Frequency::mhz(100), // outside all bands
            ..valid_vhf_channel(0)
        };
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(
            issues.iter().any(
                |i| matches!(i, ValidationIssue::Error(s) if s.contains("outside valid bands"))
            ),
            "expected band error, got: {issues:?}"
        );
    }

    #[test]
    fn out_of_band_tx_frequency_rejected() {
        let ch = Channel {
            tx_freq: Some(Frequency::mhz(100)),
            ..valid_vhf_channel(0)
        };
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(s) if s.contains("TX frequency"))),
            "expected TX band error, got: {issues:?}"
        );
    }

    #[test]
    fn name_too_long_produces_warning() {
        let ch = Channel {
            name: "LONGNAME".to_string(), // 8 chars, limit is 7
            ..valid_vhf_channel(0)
        };
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(
            issues.iter().any(
                |i| matches!(i, ValidationIssue::Warning(s) if s.contains("exceeds max length"))
            ),
            "expected name length warning, got: {issues:?}"
        );
    }

    #[test]
    fn unsupported_power_level_rejected() {
        let ch = Channel {
            power: PowerLevel::Mid, // UV-5R only has High/Low
            ..valid_vhf_channel(0)
        };
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(s) if s.contains("power level"))),
            "expected power level error, got: {issues:?}"
        );
    }

    #[test]
    fn f8hp_supports_mid_power() {
        let ch = Channel {
            power: PowerLevel::Mid,
            ..valid_vhf_channel(0)
        };
        let issues = validate_channel(&ch, &baofeng_f8hp_constraints());
        assert!(
            !issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(s) if s.contains("power level"))),
            "F8HP should support Mid power, got: {issues:?}"
        );
    }

    #[test]
    fn channel_index_exceeding_max_rejected() {
        let ch = valid_vhf_channel(128); // max is 128, so valid indices are 0-127
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(s) if s.contains("index exceeds"))),
            "expected index error, got: {issues:?}"
        );
    }

    #[test]
    fn plan_too_many_channels_rejected() {
        let channels: Vec<Channel> = (0..129).map(valid_vhf_channel).collect();
        let plan = FrequencyPlan {
            name: "Big Plan".to_string(),
            radio_model: None,
            channels,
            created: None,
        };
        let issues = validate_plan(&plan, &baofeng_uv5r_constraints());
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(s) if s.contains("channels but radio supports"))),
            "expected channel count error, got: {issues:?}"
        );
    }

    #[test]
    fn duplicate_rx_frequency_warned() {
        let plan = FrequencyPlan {
            name: "Dup Plan".to_string(),
            radio_model: None,
            channels: vec![valid_vhf_channel(0), valid_vhf_channel(1)], // same rx_freq
            created: None,
        };
        let issues = validate_plan(&plan, &baofeng_uv5r_constraints());
        assert!(
            issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Warning(s) if s.contains("duplicate RX"))),
            "expected duplicate warning, got: {issues:?}"
        );
    }

    #[test]
    fn uhf_frequency_valid_for_uv5r() {
        let ch = Channel {
            rx_freq: Frequency::hz(446_000_000),
            ..valid_vhf_channel(0)
        };
        let issues = validate_channel(&ch, &baofeng_uv5r_constraints());
        assert!(
            !issues
                .iter()
                .any(|i| matches!(i, ValidationIssue::Error(_))),
            "UHF 446 MHz should be valid, got: {issues:?}"
        );
    }
}
