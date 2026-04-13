//! Yaesu FTM-510DR radio programming support.
//!
//! # Status
//!
//! Scaffolded. The clone-mode serial protocol has not been reverse-engineered
//! yet (requires ADMS-14 USB traffic capture). The memory layout and channel
//! structure are derived from publicly available CHIRP source (Apache-2.0).
//!
//! See forkwright/akroasis#80 for tracking.

pub mod codec;
pub mod protocol;
pub mod variant;
