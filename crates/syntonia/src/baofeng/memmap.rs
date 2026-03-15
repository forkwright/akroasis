//! EEPROM memory map constants for the Baofeng UV-5R.
//!
//! All addresses are byte offsets into the 8 KB EEPROM image.

/// Base address for channel data (128 channels, 16 bytes each).
pub const CHANNEL_BASE: u16 = 0x0000;

/// Stride between channel data entries.
pub const CHANNEL_STRIDE: u16 = 16;

/// Total number of programmable channels.
pub const CHANNEL_COUNT: u8 = 128;

/// Base address for channel name entries (128 names, 16 bytes each).
pub const NAME_BASE: u16 = 0x1000;

/// Stride between channel name entries.
pub const NAME_STRIDE: u16 = 16;

/// Maximum length of a channel name in bytes (ASCII).
pub const NAME_LENGTH: usize = 7;

/// Base address for radio settings block.
pub const SETTINGS_BASE: u16 = 0x0E20;

/// Base address for VFO A configuration.
pub const VFO_A_BASE: u16 = 0x0F08;

/// Base address for VFO B configuration.
pub const VFO_B_BASE: u16 = 0x0F28;

/// Base address for FM broadcast presets.
pub const FM_PRESETS_BASE: u16 = 0x0F4E;

/// Base address for DTMF memory.
pub const DTMF_BASE: u16 = 0x0B00;

/// Squelch level offset (relative to [`SETTINGS_BASE`]).
pub const SQUELCH_OFFSET: u16 = 0x00;

/// VOX level offset (relative to [`SETTINGS_BASE`]).
pub const VOX_OFFSET: u16 = 0x04;

/// Dual-watch toggle offset (relative to [`SETTINGS_BASE`]).
pub const DUAL_WATCH_OFFSET: u16 = 0x07;

/// Beep toggle offset (relative to [`SETTINGS_BASE`]).
pub const BEEP_OFFSET: u16 = 0x08;

/// Timeout timer offset (relative to [`SETTINGS_BASE`]).
pub const TIMEOUT_OFFSET: u16 = 0x09;
