//! Protocol constants for the Baofeng UV-5R radio family.
//!
//! All timing values, opcodes, and memory layout constants for the EEPROM
//! clone protocol at 9600 baud 8N1.

use std::time::Duration;

/// Serial baud rate for UV-5R programming mode.
pub const BAUD_RATE: u32 = 9600;

/// Acknowledgement byte sent/received during protocol exchanges.
pub const ACK: u8 = 0x06;

/// Command: request radio identification.
pub const CMD_IDENT: u8 = 0x02;

/// Command: read EEPROM block.
pub const CMD_READ: u8 = 0x53;

/// Response prefix for a read block (also used as write command opcode).
pub const CMD_READ_RESPONSE: u8 = 0x58;

/// Command: write EEPROM block (same opcode as read response).
pub const CMD_WRITE: u8 = 0x58;

/// Terminator byte in the identification response.
pub const IDENT_TERMINATOR: u8 = 0xDD;

// ---------------------------------------------------------------------------
// Magic byte sequences for entering programming mode (variant-specific)
// ---------------------------------------------------------------------------

/// UV-5R firmware BFB291 and later.
pub const MAGIC_UV5R_291: [u8; 7] = [0x50, 0xBB, 0xFF, 0x20, 0x12, 0x07, 0x25];

/// UV-5R original firmware.
pub const MAGIC_UV5R_ORIG: [u8; 7] = [0x50, 0xBB, 0xFF, 0x01, 0x25, 0x98, 0x4D];

/// BF-F8HP / BF-A58 variants.
pub const MAGIC_BF_F8HP: [u8; 7] = [0x50, 0xBB, 0xFF, 0x20, 0x14, 0x04, 0x13];

// ---------------------------------------------------------------------------
// EEPROM memory layout
// ---------------------------------------------------------------------------

/// Start of the main EEPROM block.
pub const MAIN_BLOCK_START: u16 = 0x0000;

/// End of the main EEPROM block (exclusive).
pub const MAIN_BLOCK_END: u16 = 0x1800;

/// Start of the auxiliary EEPROM block (first read is a warm-up, data discarded).
pub const AUX_BLOCK_START: u16 = 0x1E80;

/// End of the auxiliary EEPROM block (exclusive).
pub const AUX_BLOCK_END: u16 = 0x2000;

/// Read block size for the main region (64 bytes).
pub const READ_BLOCK_SIZE: u8 = 0x40;

/// Read block size for the auxiliary region (16 bytes).
pub const AUX_READ_BLOCK_SIZE: u8 = 0x10;

/// Write block size (always 16 bytes regardless of region).
pub const WRITE_BLOCK_SIZE: u8 = 0x10;

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

/// Delay between individual magic bytes during programming mode entry.
pub const INTER_BYTE_DELAY: Duration = Duration::from_millis(10);

/// Delay after sending or receiving an ACK before the next command.
pub const POST_ACK_DELAY: Duration = Duration::from_millis(50);

/// Delay before retrying identification after a failure.
pub const IDENT_RETRY_DELAY: Duration = Duration::from_millis(2000);

/// Read timeout for serial responses.
pub const READ_TIMEOUT: Duration = Duration::from_millis(1500);

/// Maximum retry attempts per block operation.
pub const MAX_RETRIES: u8 = 3;

// ---------------------------------------------------------------------------
// Calibration protection — ranges that must NEVER be written
// ---------------------------------------------------------------------------

/// EEPROM address ranges containing factory calibration data.
///
/// Writing to these ranges can permanently damage the radio's PA stage.
/// The protocol layer must reject any write that overlaps these ranges.
pub const FORBIDDEN_RANGES: &[(u16, u16)] = &[
    (0x1F00, 0x1F60),
    (0x1F70, 0x1F80),
    (0x1F90, 0x1FC0),
    (0x1FD0, 0x2000),
];

// ---------------------------------------------------------------------------
// Safe upload ranges
// ---------------------------------------------------------------------------

/// Safe main-block ranges for upload (write) operations.
pub const UPLOAD_RANGES_MAIN: &[(u16, u16)] =
    &[(0x0000, 0x0CF0), (0x0D00, 0x0DF0), (0x0E00, 0x1800)];

/// Safe auxiliary-block ranges for upload (write) operations.
pub const UPLOAD_RANGES_AUX: &[(u16, u16)] = &[
    (0x1EE0, 0x1EF0),
    (0x1F60, 0x1F70),
    (0x1F80, 0x1F90),
    (0x1FC0, 0x1FD0),
];
