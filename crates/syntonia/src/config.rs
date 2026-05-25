//! Behavioral tuning configuration for syntonia radio programming.
//!
//! This module groups timing, timeout, and retry parameters that vary by
//! deployment environment (cable model, USB hub quality, radio firmware
//! revision) but do not change the on-wire protocol. Separating them into
//! serde-serializable structs makes them discoverable, tunable via TOML /
//! agent knowledge-store overrides, and testable via non-default values.
//!
//! Protocol invariants — command bytes (`CMD_IDENT`, `CMD_READ`, `CMD_WRITE`),
//! magic sequences, EEPROM layout (main/aux block addresses), calibration
//! forbidden ranges, block sizes — remain as `const` items in their
//! respective modules. Changing any of those would violate the clone
//! protocol or risk damaging the radio.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Tuning for Baofeng UV-5R family programming timing + retries.
///
/// Every field has a default matching the historical hard-coded value
/// used across the codebase before this config was introduced. Serde
/// `#[serde(default)]` lets TOML files specify only the values that
/// actually need to deviate from the default.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BaofengTimingConfig {
    /// Delay between individual magic bytes during programming-mode entry,
    /// in milliseconds. Too short and some cables drop bytes; too long and
    /// the radio may reset the handshake.
    pub inter_byte_delay_ms: u64,
    /// Delay after sending or receiving an ACK before the next command,
    /// in milliseconds.
    pub post_ack_delay_ms: u64,
    /// Delay before retrying identification after a failure, in
    /// milliseconds.
    pub ident_retry_delay_ms: u64,
    /// Serial read timeout, in milliseconds. Dominated by the radio's
    /// worst-case turnaround after a block request.
    pub read_timeout_ms: u64,
    /// Maximum retry attempts per block operation before declaring
    /// failure.
    pub max_retries: u8,
}

impl Default for BaofengTimingConfig {
    fn default() -> Self {
        Self {
            inter_byte_delay_ms: 10,
            post_ack_delay_ms: 50,
            ident_retry_delay_ms: 2_000,
            read_timeout_ms: 1_500,
            max_retries: 3,
        }
    }
}

impl BaofengTimingConfig {
    /// Returns the inter-byte delay as a [`Duration`].
    #[must_use]
    pub const fn inter_byte_delay(&self) -> Duration {
        Duration::from_millis(self.inter_byte_delay_ms)
    }

    /// Returns the post-ACK delay as a [`Duration`].
    #[must_use]
    pub const fn post_ack_delay(&self) -> Duration {
        Duration::from_millis(self.post_ack_delay_ms)
    }

    /// Returns the ident-retry delay as a [`Duration`].
    #[must_use]
    pub const fn ident_retry_delay(&self) -> Duration {
        Duration::from_millis(self.ident_retry_delay_ms)
    }

    /// Returns the serial read timeout as a [`Duration`].
    #[must_use]
    pub const fn read_timeout(&self) -> Duration {
        Duration::from_millis(self.read_timeout_ms)
    }
}

/// Tuning for the hardware detection probe loop.
///
/// Controls how long each serial port is held open during the magic-byte
/// probe before moving on. Lower values speed up `detect_radios` when most
/// ports are unconnected; higher values are needed for slower cable /
/// radio combinations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HardwareProbeConfig {
    /// Timeout applied to the serial port during an individual probe
    /// attempt, in milliseconds.
    pub probe_timeout_ms: u64,
}

impl Default for HardwareProbeConfig {
    fn default() -> Self {
        Self {
            probe_timeout_ms: 500,
        }
    }
}

impl HardwareProbeConfig {
    /// Returns the per-probe timeout as a [`Duration`].
    #[must_use]
    pub const fn probe_timeout(&self) -> Duration {
        Duration::from_millis(self.probe_timeout_ms)
    }
}

/// Top-level syntonia behavioral tuning.
///
/// Aggregates the per-subsystem configs so callers can accept a single
/// `&SyntoniaConfig` and thread it down.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyntoniaConfig {
    /// Baofeng programming timing + retries.
    #[serde(default)]
    pub baofeng_timing: BaofengTimingConfig,
    /// Hardware detection probe tuning.
    #[serde(default)]
    pub hardware_probe: HardwareProbeConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_baofeng_timing_matches_historical_values() {
        let cfg = BaofengTimingConfig::default();
        assert_eq!(cfg.inter_byte_delay(), Duration::from_millis(10));
        assert_eq!(cfg.post_ack_delay(), Duration::from_millis(50));
        assert_eq!(cfg.ident_retry_delay(), Duration::from_secs(2));
        assert_eq!(cfg.read_timeout(), Duration::from_millis(1_500));
        assert_eq!(cfg.max_retries, 3);
    }

    #[test]
    fn default_hardware_probe_matches_historical_value() {
        let cfg = HardwareProbeConfig::default();
        assert_eq!(cfg.probe_timeout(), Duration::from_millis(500));
    }

    #[test]
    fn syntonia_config_toml_roundtrip() {
        // WHY: ensure an agent-written partial TOML round-trips cleanly and
        // preserves all non-default values.
        let cfg = SyntoniaConfig {
            baofeng_timing: BaofengTimingConfig {
                inter_byte_delay_ms: 25,
                post_ack_delay_ms: 100,
                ident_retry_delay_ms: 500,
                read_timeout_ms: 3_000,
                max_retries: 7,
            },
            hardware_probe: HardwareProbeConfig {
                probe_timeout_ms: 1_200,
            },
        };

        #[expect(clippy::unwrap_used, reason = "test-only: known-good values")]
        let toml_str = toml::to_string(&cfg).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only: just serialized")]
        let parsed: SyntoniaConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.baofeng_timing.max_retries, 7);
        assert_eq!(parsed.baofeng_timing.read_timeout_ms, 3_000);
        assert_eq!(parsed.hardware_probe.probe_timeout_ms, 1_200);
    }

    #[test]
    fn syntonia_config_partial_toml_uses_defaults() {
        // WHY: an agent should be able to override just one field without
        // specifying the full tree — serde(default) makes this work.
        let partial = r"
[baofeng_timing]
max_retries = 10
";
        #[expect(clippy::unwrap_used, reason = "test-only: known-good TOML")]
        let parsed: SyntoniaConfig = toml::from_str(partial).unwrap();
        assert_eq!(parsed.baofeng_timing.max_retries, 10);
        assert_eq!(
            parsed.baofeng_timing.read_timeout_ms,
            BaofengTimingConfig::default().read_timeout_ms,
            "unspecified fields must fall through to default"
        );
        assert_eq!(
            parsed.hardware_probe.probe_timeout_ms,
            HardwareProbeConfig::default().probe_timeout_ms
        );
    }
}
