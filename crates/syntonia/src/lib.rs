//! Syntonia — radio-agnostic channel and frequency plan data model with hardware detection.
//!
//! This crate provides the core types for representing programmable radio
//! channel memories, frequency plans, and radio-specific validation. It sits
//! between raw EEPROM bytes and user-visible channel plans: every radio driver
//! encodes/decodes channels through these types.
//!
//! The [`hardware`] module provides USB cable detection, radio auto-discovery,
//! and actionable diagnostics for connection issues.
//!
//! # Key types
//!
//! - [`Channel`] — a single programmable memory slot
//! - [`FrequencyPlan`] — a named collection of channels (serializable to JSON/TOML)
//! - [`ToneMode`], [`CtcssTone`], [`DcsCode`] — squelch tone configuration
//! - [`RadioConstraints`] — radio-specific limits for validation

pub mod baofeng;
pub mod channel;
pub mod error;
pub mod hardware;
pub mod plan;
pub mod tone;
pub mod types;
pub mod validate;

pub use channel::Channel;
pub use error::{Error, Result};
pub use plan::FrequencyPlan;
pub use tone::{ALL_CTCSS_TONES, ALL_DCS_CODES, CtcssTone, DcsCode, DcsPolarity, ToneMode};
pub use types::{Bandwidth, FrequencyOffset, PowerLevel, ScanMode};
pub use validate::{
    RadioConstraints, ValidationIssue, baofeng_f8hp_constraints, baofeng_uv5r_constraints,
    validate_channel, validate_plan,
};
