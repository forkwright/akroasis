//! Syntonia — radio-agnostic channel and frequency plan data model.
//!
//! This crate provides the core types for representing programmable radio
//! channel memories, frequency plans, and radio-specific validation. It sits
//! between raw EEPROM bytes and user-visible channel plans: every radio driver
//! encodes/decodes channels through these types.
//!
//! # Key types
//!
//! - [`Channel`] — a single programmable memory slot
//! - [`FrequencyPlan`] — a named collection of channels (serializable to JSON/TOML)
//! - [`ToneMode`], [`CtcssTone`], [`DcsCode`] — squelch tone configuration
//! - [`RadioConstraints`] — radio-specific limits for validation
//!
//! # Import / Export
//!
//! - [`import`] — CHIRP `.img` and `.csv` import
//! - [`export`] — CHIRP `.csv` export
//! - [`baofeng`] — Baofeng UV-5R EEPROM codec

#![deny(missing_docs)]

pub mod baofeng;
pub mod channel;
pub mod config;
pub mod error;
pub mod export;
pub mod import;
pub mod plan;
pub(crate) mod serial;
pub mod tone;
pub mod types;
pub mod validate;
pub mod yaesu;

pub use channel::Channel;
pub use config::{BaofengTimingConfig, HardwareProbeConfig, SyntoniaConfig};
pub use error::{Error, Result};
pub use plan::FrequencyPlan;
pub use tone::{ALL_CTCSS_TONES, ALL_DCS_CODES, CtcssTone, DcsCode, DcsPolarity, ToneMode};
pub use types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};
pub use validate::{
    RadioConstraints, ValidationIssue, baofeng_f8hp_constraints, baofeng_uv5r_constraints,
    validate_channel, validate_plan,
};
