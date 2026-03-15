//! Baofeng UV-5R family radio driver — variant identification, codec, and protocol.
//!
//! Supports the UV-5R, BF-F8HP, and UV-5RM Plus radios. All share the same
//! EEPROM clone protocol at 9600 baud but differ in magic bytes, power levels,
//! and auxiliary block handling.

pub mod codec;
pub mod detect;
pub mod protocol;
pub mod variant;

pub use codec::{decode_channel, encode_channel};
pub use detect::auto_detect;
pub use variant::{RadioVariant, VariantConfig, identify_variant};
