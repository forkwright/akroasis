//! Baofeng UV-5R family radio driver — variant identification, codec, and protocol.
//!
//! Supports the UV-5R, BF-F8HP, and UV-5RM Plus radios. All share the same
//! EEPROM clone protocol at 9600 baud but differ in magic bytes, power levels,
//! and auxiliary block handling.
//!
//! This module provides EEPROM memory map constants, BCD frequency encoding,
//! tone encoding, and a channel codec that translates between raw EEPROM bytes
//! and the typed [`Channel`](crate::Channel) data model.

pub mod bcd;
pub mod codec;
pub mod detect;
pub mod image;
pub mod memmap;
pub mod protocol;
pub mod tone_codec;
pub mod variant;

pub use detect::auto_detect;
pub use variant::{RadioVariant, VariantConfig, identify_variant};
