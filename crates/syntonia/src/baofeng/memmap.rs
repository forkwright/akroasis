//! UV-5R EEPROM memory map constants.

/// Base address for channel data (16 bytes per channel).
pub const CHANNEL_BASE: u16 = 0x0000;
/// Stride between channel data entries.
pub const CHANNEL_STRIDE: u16 = 16;
/// Maximum number of channels.
pub const CHANNEL_COUNT: u8 = 128;

/// Base address for channel name data.
pub const NAME_BASE: u16 = 0x1000;
/// Stride between channel name entries.
pub const NAME_STRIDE: u16 = 16;
/// Maximum channel name length in bytes.
pub const NAME_LENGTH: usize = 7;
