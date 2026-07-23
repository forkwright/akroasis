//! Baofeng radio protocol and codec support.

pub mod bcd;
pub mod codec;
pub(crate) mod constants;
#[cfg(feature = "hardware-serial")]
// kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
pub(crate) mod detect;
pub mod ident;
pub mod image;
pub mod memmap;
#[cfg(feature = "hardware-serial")]
// kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
pub mod protocol;
pub(crate) mod tone_codec;
#[cfg(feature = "hardware-serial")]
// kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
pub mod variant;
#[cfg(not(feature = "hardware-serial"))]
// kanon:ignore RUST/feature-gate-check -- declared in syntonia/Cargo.toml [features]
pub(crate) mod variant;
