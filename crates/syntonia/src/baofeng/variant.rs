//! Radio variant identification and configuration for the UV-5R family.

// WHY: variant API (RadioVariant, VariantConfig, MAGIC_SETS, identify_variant) is
// only consumed by the protocol module, which is hardware-serial gated.
#![cfg_attr(
    not(feature = "hardware-serial"),
    expect(
        dead_code,
        reason = "variant API used only with hardware-serial feature, tracked in #264"
    )
)]

use std::fmt;

use snafu::Snafu;

use super::ident::RadioIdent;
use crate::types::PowerLevel;

// ── Magic byte sequences ────────────────────────────────────────────────────

/// Magic bytes for UV-5R firmware version 2.91+ and most clones.
pub const MAGIC_UV5R_291: [u8; 7] = [0x50, 0xBB, 0xFF, 0x20, 0x12, 0x04, 0x11];

/// Magic bytes for the original UV-5R (pre-2.91 firmware).
pub const MAGIC_UV5R_ORIG: [u8; 7] = [0x50, 0xBB, 0xFF, 0x20, 0x12, 0x01, 0x11];

/// Magic bytes for BF-F8HP (shared with BF-A58).
pub const MAGIC_BF_F8HP: [u8; 7] = [0x50, 0xBB, 0xFF, 0x20, 0x14, 0x04, 0x13];

/// All magic byte sequences to try during auto-detection, in priority order.
pub const MAGIC_SETS: &[[u8; 7]] = &[MAGIC_UV5R_291, MAGIC_BF_F8HP, MAGIC_UV5R_ORIG];

// ── RadioVariant ─────────────────────────────────────────────────────────────

/// Baofeng UV-5R family radio variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RadioVariant {
    /// Standard UV-5R (firmware 2.91+, most clones).
    Uv5r,
    /// Original UV-5R (pre-2.91 firmware).
    Uv5rOriginal,
    /// BF-F8HP tri-power variant.
    BfF8hp,
    /// UV-5RM Plus (tentative — may require `UV17Pro` protocol).
    Uv5rmPlus,
}

impl fmt::Display for RadioVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Uv5r => "Baofeng UV-5R",
            Self::Uv5rOriginal => "Baofeng UV-5R (original)",
            Self::BfF8hp => "Baofeng BF-F8HP",
            Self::Uv5rmPlus => "Baofeng UV-5RM Plus",
        };
        f.write_str(name)
    }
}

// ── PowerMapping ─────────────────────────────────────────────────────────────

/// Maps a logical power level to its EEPROM bit representation and wattage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerMapping {
    /// Logical power level.
    pub level: PowerLevel,
    /// Transmit power in watts.
    pub watts: f32,
    /// 2-bit value stored in the EEPROM channel record.
    pub eeprom_bits: u8,
}

// ── VariantConfig ────────────────────────────────────────────────────────────

/// Per-variant configuration carrying magic bytes, power mapping, and EEPROM layout.
#[derive(Debug, Clone, PartialEq)]
pub struct VariantConfig {
    /// Which variant this config describes.
    pub variant: RadioVariant,
    /// 7-byte magic sequence for entering programming mode.
    pub magic: [u8; 7],
    /// Power level mappings (EEPROM bits <-> logical level + wattage).
    pub power_levels: Vec<PowerMapping>,
    /// Whether this variant has an auxiliary EEPROM block (0x1E80–0x2000).
    pub has_aux_block: bool,
    /// Whether reading aux requires a warm-up read at 0x1E80 first.
    pub needs_aux_warmup: bool,
    /// Number of programmable channels.
    pub channel_count: u8,
    /// Maximum channel name length in characters.
    pub max_name_length: usize,
    /// Serial baud rate.
    pub baud_rate: u32,
}

impl VariantConfig {
    /// Look up the power level for a given EEPROM bit value.
    ///
    /// For UV-5R, bit value 2 (Mid) is treated as High since the radio
    /// has no mid setting.
    #[must_use]
    pub fn power_from_bits(&self, bits: u8) -> Option<PowerLevel> {
        self.power_levels
            .iter()
            .find(|m| m.eeprom_bits == bits)
            .map(|m| m.level)
    }

    /// Look up the EEPROM bit value for a given power level.
    #[must_use]
    pub fn bits_from_power(&self, level: PowerLevel) -> Option<u8> {
        self.power_levels
            .iter()
            .find(|m| m.level == level)
            .map(|m| m.eeprom_bits)
    }
}

// ── Variant configs ──────────────────────────────────────────────────────────

/// Configuration for the standard UV-5R.
///
/// Two power levels. Bit value 2 (Mid) maps to High since this radio
/// has no mid setting — CHIRP images from tri-power radios may contain it.
#[must_use]
pub fn uv5r_config() -> VariantConfig {
    VariantConfig {
        variant: RadioVariant::Uv5r,
        magic: MAGIC_UV5R_291,
        power_levels: vec![
            PowerMapping {
                level: PowerLevel::High,
                watts: 4.0,
                eeprom_bits: 0,
            },
            PowerMapping {
                level: PowerLevel::Low,
                watts: 1.0,
                eeprom_bits: 1,
            },
            // WHY: UV-5R has no mid setting, but F8HP images may store bit value 2.
            // Treat it as High to avoid data loss on cross-radio imports.
            PowerMapping {
                level: PowerLevel::High,
                watts: 4.0,
                eeprom_bits: 2,
            },
        ],
        has_aux_block: false,
        needs_aux_warmup: false,
        channel_count: 128,
        max_name_length: 7,
        baud_rate: 9_600,
    }
}

/// Configuration for the original UV-5R (pre-2.91 firmware).
#[must_use]
pub fn uv5r_original_config() -> VariantConfig {
    VariantConfig {
        variant: RadioVariant::Uv5rOriginal,
        magic: MAGIC_UV5R_ORIG,
        ..uv5r_config()
    }
}

/// Configuration for the BF-F8HP.
///
/// Three power levels (8W / 4W / 1W). Has auxiliary EEPROM block
/// that requires a warm-up read before access.
#[must_use]
pub fn bf_f8hp_config() -> VariantConfig {
    VariantConfig {
        variant: RadioVariant::BfF8hp,
        magic: MAGIC_BF_F8HP,
        power_levels: vec![
            PowerMapping {
                level: PowerLevel::High,
                watts: 8.0,
                eeprom_bits: 0,
            },
            PowerMapping {
                level: PowerLevel::Low,
                watts: 1.0,
                eeprom_bits: 1,
            },
            PowerMapping {
                level: PowerLevel::Mid,
                watts: 4.0,
                eeprom_bits: 2,
            },
        ],
        has_aux_block: true,
        needs_aux_warmup: true,
        channel_count: 128,
        max_name_length: 7,
        baud_rate: 9_600,
    }
}

/// Configuration for the UV-5RM Plus (tentative).
///
/// Assumed to be a BF-F8HP variant with higher power output.
/// Power values are unverified — needs hardware testing.
#[must_use]
pub fn uv5rm_plus_config() -> VariantConfig {
    VariantConfig {
        variant: RadioVariant::Uv5rmPlus,
        magic: MAGIC_BF_F8HP,
        power_levels: vec![
            PowerMapping {
                level: PowerLevel::High,
                watts: 10.0,
                eeprom_bits: 0,
            },
            PowerMapping {
                level: PowerLevel::Low,
                watts: 1.0,
                eeprom_bits: 1,
            },
            PowerMapping {
                level: PowerLevel::Mid,
                watts: 5.0,
                eeprom_bits: 2,
            },
        ],
        has_aux_block: true,
        needs_aux_warmup: true,
        channel_count: 128,
        max_name_length: 7,
        baud_rate: 9_600,
    }
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors from variant identification.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum VariantError {
    /// The firmware ident bytes did not match any known variant.
    #[snafu(display("unknown radio variant — ident bytes: {raw_hex}"))]
    UnknownVariant {
        /// Hex representation of the raw ident bytes.
        raw_hex: String,
    },
}

// ── Firmware prefix matching ─────────────────────────────────────────────────

/// Known firmware prefixes for the standard UV-5R.
const UV5R_PREFIXES: &[&str] = &["BFB", "BFS", "N5R-2", "N5R2", "N5RV", "BTS", "D5R2", "B5R2"];

/// Known firmware prefixes for the BF-F8HP.
const BF_F8HP_PREFIXES: &[&str] = &["BFP3V3 F", "N5R-3", "N5R3", "F5R3", "BFT"];

/// Identify the radio variant from its firmware ident bytes.
///
/// Matches known firmware prefixes against the full 8-byte normalized ident
/// (not [`RadioIdent::firmware_prefix`], which is truncated to 6 chars for
/// display — `BF_F8HP_PREFIXES` includes an 8-char prefix that needs all
/// 8 bytes to match). Returns the appropriate [`VariantConfig`] for the
/// identified variant.
///
/// # Errors
///
/// Returns [`VariantError::UnknownVariant`] if the ident does not match any
/// known prefix, including the raw bytes as hex for debugging.
pub fn identify_variant(ident: &RadioIdent) -> Result<VariantConfig, VariantError> {
    for &pfx in BF_F8HP_PREFIXES {
        if ident.normalized.as_slice().starts_with(pfx.as_bytes()) {
            return Ok(bf_f8hp_config());
        }
    }

    for &pfx in UV5R_PREFIXES {
        if ident.normalized.as_slice().starts_with(pfx.as_bytes()) {
            return Ok(uv5r_config());
        }
    }

    UnknownVariantSnafu {
        raw_hex: hex_encode(&ident.raw_bytes),
    }
    .fail()
}

/// Encode bytes as a hex string with spaces between bytes.
fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(all(test, feature = "hardware-serial"))] // kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    // WHY: `RadioIdent::from_raw` only accepts 8- or 12-byte wire responses
    // (the real UV-5R framing), so these firmware strings are padded to 8
    // bytes with filler that never collides with a known prefix. `starts_with`
    // matching means the trailing filler bytes never affect which variant wins.
    #[test]
    fn identify_bfb_firmware_as_uv5r() -> Result<(), &'static str> {
        let ident = RadioIdent::from_raw(b"BFB297\x00\x00").ok_or("8-byte literal must parse")?;
        let config = identify_variant(&ident).unwrap();
        assert_eq!(config.variant, RadioVariant::Uv5r);
        Ok(())
    }

    #[test]
    fn identify_bfs_firmware_as_uv5r() -> Result<(), &'static str> {
        let ident = RadioIdent::from_raw(b"BFS300\x00\x00").ok_or("8-byte literal must parse")?;
        let config = identify_variant(&ident).unwrap();
        assert_eq!(config.variant, RadioVariant::Uv5r);
        Ok(())
    }

    #[test]
    fn identify_n5r2_firmware_as_uv5r() -> Result<(), &'static str> {
        let ident =
            RadioIdent::from_raw(b"N5R-2\x00\x00\x00").ok_or("8-byte literal must parse")?;
        let config = identify_variant(&ident).unwrap();
        assert_eq!(config.variant, RadioVariant::Uv5r);
        Ok(())
    }

    #[test]
    fn identify_bfp3v3_firmware_as_f8hp() -> Result<(), &'static str> {
        // WHY: "BFP3V3 F" is exactly 8 bytes — the one known prefix that needs
        // the full normalized ident, not the 6-char firmware_prefix field.
        let ident = RadioIdent::from_raw(b"BFP3V3 F").ok_or("8-byte literal must parse")?;
        let config = identify_variant(&ident).unwrap();
        assert_eq!(config.variant, RadioVariant::BfF8hp);
        Ok(())
    }

    #[test]
    fn identify_n5r3_firmware_as_f8hp() -> Result<(), &'static str> {
        let ident =
            RadioIdent::from_raw(b"N5R-3\x00\x00\x00").ok_or("8-byte literal must parse")?;
        let config = identify_variant(&ident).unwrap();
        assert_eq!(config.variant, RadioVariant::BfF8hp);
        Ok(())
    }

    #[test]
    fn identify_bft_firmware_as_f8hp() -> Result<(), &'static str> {
        let ident = RadioIdent::from_raw(b"BFT123\x00\x00").ok_or("8-byte literal must parse")?;
        let config = identify_variant(&ident).unwrap();
        assert_eq!(config.variant, RadioVariant::BfF8hp);
        Ok(())
    }

    #[test]
    fn unknown_firmware_returns_error_with_raw_bytes() -> Result<(), &'static str> {
        let ident = RadioIdent::from_raw(&[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00])
            .ok_or("8-byte literal must parse")?;
        let err = identify_variant(&ident).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("DE AD BE EF"),
            "error should contain hex bytes, got: {msg}"
        );
        Ok(())
    }

    #[test]
    fn uv5r_has_two_logical_power_levels() {
        let config = uv5r_config();
        assert_eq!(config.power_from_bits(0), Some(PowerLevel::High));
        assert_eq!(config.power_from_bits(1), Some(PowerLevel::Low));
        // WHY: bit 2 (Mid) maps to High — UV-5R has no mid setting
        assert_eq!(config.power_from_bits(2), Some(PowerLevel::High));
    }

    #[test]
    fn f8hp_has_three_power_levels() {
        let config = bf_f8hp_config();
        assert_eq!(config.power_from_bits(0), Some(PowerLevel::High));
        assert_eq!(config.power_from_bits(1), Some(PowerLevel::Low));
        assert_eq!(config.power_from_bits(2), Some(PowerLevel::Mid));
    }

    #[test]
    fn uv5r_bits_from_power_roundtrips() {
        let config = uv5r_config();
        let bits = config.bits_from_power(PowerLevel::High).unwrap();
        assert_eq!(config.power_from_bits(bits), Some(PowerLevel::High));
        let bits = config.bits_from_power(PowerLevel::Low).unwrap();
        assert_eq!(config.power_from_bits(bits), Some(PowerLevel::Low));
    }

    #[test]
    fn f8hp_bits_from_power_roundtrips() {
        let config = bf_f8hp_config();
        for level in [PowerLevel::High, PowerLevel::Mid, PowerLevel::Low] {
            let bits = config.bits_from_power(level).unwrap();
            assert_eq!(config.power_from_bits(bits), Some(level));
        }
    }

    #[test]
    fn f8hp_has_aux_block_and_warmup() {
        let config = bf_f8hp_config();
        assert!(config.has_aux_block);
        assert!(config.needs_aux_warmup);
    }

    #[test]
    fn uv5r_has_no_aux_block() {
        let config = uv5r_config();
        assert!(!config.has_aux_block);
        assert!(!config.needs_aux_warmup);
    }

    #[test]
    fn uv5rm_plus_assumed_same_as_f8hp_layout() {
        let config = uv5rm_plus_config();
        assert!(config.has_aux_block);
        assert!(config.needs_aux_warmup);
        assert_eq!(config.magic, MAGIC_BF_F8HP);
        // Higher wattage than F8HP
        let high = config
            .power_levels
            .iter()
            .find(|m| m.level == PowerLevel::High)
            .unwrap();
        assert!((high.watts - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn variant_display_names() {
        assert_eq!(RadioVariant::Uv5r.to_string(), "Baofeng UV-5R");
        assert_eq!(
            RadioVariant::Uv5rOriginal.to_string(),
            "Baofeng UV-5R (original)"
        );
        assert_eq!(RadioVariant::BfF8hp.to_string(), "Baofeng BF-F8HP");
        assert_eq!(RadioVariant::Uv5rmPlus.to_string(), "Baofeng UV-5RM Plus");
    }

    #[test]
    fn unknown_bits_returns_none() {
        let config = uv5r_config();
        assert_eq!(config.power_from_bits(3), None);
        assert_eq!(config.power_from_bits(255), None);
    }

    #[test]
    fn magic_sets_contains_all_three() {
        assert_eq!(MAGIC_SETS.len(), 3);
        assert_eq!(MAGIC_SETS[0], MAGIC_UV5R_291);
        assert_eq!(MAGIC_SETS[1], MAGIC_BF_F8HP);
        assert_eq!(MAGIC_SETS[2], MAGIC_UV5R_ORIG);
    }

    #[test]
    fn all_variants_have_128_channels() {
        for config in [
            uv5r_config(),
            uv5r_original_config(),
            bf_f8hp_config(),
            uv5rm_plus_config(),
        ] {
            assert_eq!(config.channel_count, 128);
        }
    }

    #[test]
    fn all_variants_use_9600_baud() {
        for config in [
            uv5r_config(),
            uv5r_original_config(),
            bf_f8hp_config(),
            uv5rm_plus_config(),
        ] {
            assert_eq!(config.baud_rate, 9_600);
        }
    }
}
