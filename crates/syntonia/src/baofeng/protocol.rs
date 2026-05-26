//! UV-5R EEPROM clone protocol  -  block planning and low-level serial driver.
//!
//! This module contains two layers:
//!
//! 1. **Block planning**  -  [`download_plan`] and [`upload_plan`] generate the
//!    list of [`BlockOp`]s needed for a complete EEPROM transfer, handling
//!    variant-specific aux blocks, warm-up reads, and the BF-F8HP dropped-byte
//!    workaround.
//!
//! 2. **Protocol driver**  -  [`Uv5rProtocol`] drives the actual serial session:
//!    entering programming mode, identifying the radio, and reading/writing
//!    EEPROM blocks. All I/O goes through the
//!    [`SerialPort`](crate::serial::SerialPort) trait so tests can use a mock
//!    without hardware.

use std::io;
use std::thread;

use snafu::Snafu;

use crate::serial::SerialPort;

use super::constants::{
    ACK, AUX_BLOCK_END, AUX_BLOCK_START, AUX_READ_BLOCK_SIZE, CMD_IDENT, CMD_READ,
    CMD_READ_RESPONSE, CMD_WRITE, FORBIDDEN_RANGES, IDENT_TERMINATOR, INTER_BYTE_DELAY,
    MAIN_BLOCK_END, MAIN_BLOCK_START, MAX_RETRIES, POST_ACK_DELAY, READ_BLOCK_SIZE, READ_TIMEOUT,
    UPLOAD_RANGES_AUX, UPLOAD_RANGES_MAIN, WRITE_BLOCK_SIZE,
};
use super::ident::RadioIdent;
use super::image::MemoryImage;
use super::variant::VariantConfig;

// ── Block planning constants ────────────────────────────────────────────────

/// Standard EEPROM block size for plan operations (16 bytes).
pub const BLOCK_SIZE: usize = 16;

/// Start of the main channel memory region.
pub const MAIN_START: u16 = 0x0000;

/// End of the main memory region (exclusive) for UV-5R (no aux).
pub const MAIN_END: u16 = 0x1800;

/// Start of the auxiliary EEPROM block (BF-F8HP and variants).
pub const AUX_START: u16 = 0x1E80;

/// End of the auxiliary EEPROM block (exclusive).
pub const AUX_END: u16 = 0x2000;

/// Address of the warm-up read block (required before aux access on BF-F8HP).
pub const AUX_WARMUP_ADDR: u16 = 0x1E80;

/// Address of the dropped-byte bug in BF-F8HP firmware.
///
/// Reading a full 16-byte block at this address may lose bytes. The
/// workaround is to read in smaller chunks around this address.
pub const DROPPED_BYTE_ADDR: u16 = 0x1FCF;

// ── Block plan ──────────────────────────────────────────────────────────────

/// A planned EEPROM read/write operation at a specific address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockOp {
    /// EEPROM address to read/write.
    pub addr: u16,
    /// Number of bytes to transfer.
    pub size: u16,
    /// Whether this is a warm-up read (data should be discarded).
    pub is_warmup: bool,
}

/// Generate the list of block operations needed to download a complete image.
///
/// For UV-5R: reads 0x0000–0x1800 in 16-byte blocks.
/// For BF-F8HP: same main region, plus aux warm-up at 0x1E80, then
/// 0x1E80–0x2000 with dropped-byte workaround at 0x1FCF.
#[must_use]
pub fn download_plan(config: &VariantConfig) -> Vec<BlockOp> {
    let mut ops = Vec::new();

    // Main memory region
    let mut addr = MAIN_START;
    while addr < MAIN_END {
        ops.push(BlockOp {
            addr,
            size: u16::try_from(BLOCK_SIZE).unwrap_or_default(),
            is_warmup: false,
        });
        addr += u16::try_from(BLOCK_SIZE).unwrap_or_default();
    }

    if config.has_aux_block {
        // Warm-up read: read 0x1E80 first, discard the data
        if config.needs_aux_warmup {
            ops.push(BlockOp {
                addr: AUX_WARMUP_ADDR,
                size: u16::try_from(BLOCK_SIZE).unwrap_or_default(),
                is_warmup: true,
            });
        }

        // Aux region with dropped-byte workaround
        let mut aux_addr = AUX_START;
        while aux_addr < AUX_END {
            let block_end = aux_addr + u16::try_from(BLOCK_SIZE).unwrap_or_default();

            if aux_addr <= DROPPED_BYTE_ADDR && DROPPED_BYTE_ADDR < block_end {
                // Split INTO smaller reads around the problem address.
                // Read bytes before the dropped-byte address.
                let before_size = DROPPED_BYTE_ADDR - aux_addr;
                if before_size > 0 {
                    ops.push(BlockOp {
                        addr: aux_addr,
                        size: before_size,
                        is_warmup: false,
                    });
                }
                // Read the problem byte individually.
                ops.push(BlockOp {
                    addr: DROPPED_BYTE_ADDR,
                    size: 1,
                    is_warmup: false,
                });
                // Read remaining bytes after.
                let after_start = DROPPED_BYTE_ADDR + 1;
                let after_size = block_end - after_start;
                if after_size > 0 {
                    ops.push(BlockOp {
                        addr: after_start,
                        size: after_size,
                        is_warmup: false,
                    });
                }
            } else {
                ops.push(BlockOp {
                    addr: aux_addr,
                    size: u16::try_from(BLOCK_SIZE).unwrap_or_default(),
                    is_warmup: false,
                });
            }

            aux_addr += u16::try_from(BLOCK_SIZE).unwrap_or_default();
        }
    }

    ops
}

/// Generate the list of block operations needed to upload a complete image.
///
/// Same regions as download, but without the warm-up read.
#[must_use]
pub fn upload_plan(config: &VariantConfig) -> Vec<BlockOp> {
    let mut ops = Vec::new();

    let mut addr = MAIN_START;
    while addr < MAIN_END {
        ops.push(BlockOp {
            addr,
            size: u16::try_from(BLOCK_SIZE).unwrap_or_default(),
            is_warmup: false,
        });
        addr += u16::try_from(BLOCK_SIZE).unwrap_or_default();
    }

    if config.has_aux_block {
        let mut aux_addr = AUX_START;
        while aux_addr < AUX_END {
            ops.push(BlockOp {
                addr: aux_addr,
                size: u16::try_from(BLOCK_SIZE).unwrap_or_default(),
                is_warmup: false,
            });
            aux_addr += u16::try_from(BLOCK_SIZE).unwrap_or_default();
        }
    }

    ops
}

// ── Errors ──────────────────────────────────────────────────────────────────

/// Errors produced by the UV-5R clone protocol.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum ProtocolError {
    /// Underlying serial I/O failure.
    #[snafu(display("serial I/O error: {source}"))]
    SerialIo {
        /// The underlying I/O error.
        source: io::Error,
    },

    /// The radio did not respond within the timeout window.
    #[snafu(display("timeout waiting for radio response"))]
    Timeout,

    /// The radio sent an unexpected acknowledgement byte.
    #[snafu(display("bad ACK: expected 0x{expected:02X}, got 0x{got:02X}"))]
    BadAck {
        /// Expected byte value.
        expected: u8,
        /// Actual byte received.
        got: u8,
    },

    /// The response header does not match the request.
    #[snafu(display("response header mismatch at address 0x{addr:04X}"))]
    BadResponseHeader {
        /// The address that was requested.
        addr: u16,
    },

    /// The radio did not respond to the identification request.
    #[snafu(display("radio identification failed"))]
    IdentFailed,

    /// Attempted write to a protected calibration address range.
    #[snafu(display("write to forbidden calibration address 0x{addr:04X}"))]
    ForbiddenAddress {
        /// The forbidden address.
        addr: u16,
    },

    /// All retry attempts for a block operation have been exhausted.
    #[snafu(display("block operation at 0x{addr:04X} failed after {attempts} attempts"))]
    RetryExhausted {
        /// The address of the failed block.
        addr: u16,
        /// Number of attempts made.
        attempts: u8,
    },
}

/// Specialized result type for protocol operations.
pub type Result<T> = std::result::Result<T, ProtocolError>;

// ── Protocol driver ─────────────────────────────────────────────────────────

/// Low-level UV-5R EEPROM clone protocol driver.
///
/// Generic over the serial port implementation so tests can substitute a mock.
/// For real hardware use `Uv5rProtocol<HardwareSerialPort>`.
pub struct Uv5rProtocol<P: SerialPort> {
    port: P,
}

impl<P: SerialPort> Uv5rProtocol<P> {
    /// Create a new protocol driver wrapping the given serial port.
    pub const fn new(port: P) -> Self {
        Self { port }
    }

    /// Enter programming mode by sending the variant-specific magic sequence.
    ///
    /// Each byte is sent individually with a 10 ms inter-byte delay. The radio
    /// responds with `ACK` (0x06) when it enters programming mode.
    ///
    /// # Errors
    /// Returns [`ProtocolError::SerialIo`] on I/O failure, [`ProtocolError::Timeout`]
    /// if the radio does not respond, or [`ProtocolError::BadAck`] on unexpected reply.
    pub fn enter_programming_mode(&mut self, magic: &[u8; 7]) -> Result<()> {
        self.port
            .set_timeout(READ_TIMEOUT)
            .map_err(|source| ProtocolError::SerialIo { source })?;

        for &byte in magic {
            self.port
                .write_all(&[byte])
                .map_err(|source| ProtocolError::SerialIo { source })?;
            thread::sleep(INTER_BYTE_DELAY);
        }

        self.port
            .flush()
            .map_err(|source| ProtocolError::SerialIo { source })?;

        self.read_ack()?;
        Ok(())
    }

    /// Identify the radio after entering programming mode.
    ///
    /// Sends `CMD_IDENT` (0x02), reads the response until `IDENT_TERMINATOR`
    /// (0xDD), normalizes 12-byte responses to 8 bytes, then confirms clone
    /// mode with an ACK exchange.
    ///
    /// # Errors
    /// Returns [`ProtocolError::IdentFailed`] if the response length is invalid,
    /// [`ProtocolError::Timeout`] if no response, or [`ProtocolError::BadAck`].
    pub fn identify(&mut self) -> Result<RadioIdent> {
        self.port
            .write_all(&[CMD_IDENT])
            .map_err(|source| ProtocolError::SerialIo { source })?;

        // Read until we see the terminator byte.
        let mut buf = Vec::with_capacity(16);
        let mut byte = [0u8; 1];
        loop {
            match self.port.read_exact(&mut byte) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                    return Err(ProtocolError::Timeout);
                }
                Err(source) => return Err(ProtocolError::SerialIo { source }),
            }
            if byte.get(0).copied().unwrap_or_default() == IDENT_TERMINATOR {
                break;
            }
            buf.push(byte.get(0).copied().unwrap_or_default());
            // Guard against runaway reads.
            if buf.len() > 16 {
                return Err(ProtocolError::IdentFailed);
            }
        }

        let ident = RadioIdent::from_raw(&buf).ok_or(ProtocolError::IdentFailed)?;

        // Confirm clone mode: host sends ACK, radio responds with ACK.
        self.send_ack()?;
        self.read_ack()?;

        Ok(ident)
    }

    /// Read an EEPROM block at the given address.
    ///
    /// Constructs `[CMD_READ, addr_h, addr_l, len]`, validates the response
    /// header, and returns the data bytes. Sends ACK after successful read.
    ///
    /// # Errors
    /// Returns [`ProtocolError::BadResponseHeader`] on header mismatch,
    /// [`ProtocolError::Timeout`], or [`ProtocolError::SerialIo`].
    pub fn read_block(&mut self, addr: u16, len: u8) -> Result<Vec<u8>> {
        let addr_h = (addr >> 8) as u8; // SAFETY: u16 >> 8 yields the high byte; fits u8 by construction
        let addr_l = (addr & 0xFF) as u8; // SAFETY: u16 & 0xFF yields the low byte; fits u8 by construction
        let cmd = [CMD_READ, addr_h, addr_l, len];

        self.port
            .write_all(&cmd)
            .map_err(|source| ProtocolError::SerialIo { source })?;

        // Read response header: [CMD_READ_RESPONSE, addr_h, addr_l, len]
        let mut header = [0u8; 4];
        self.read_exact_timeout(&mut header)?;

        if header.get(0).copied().unwrap_or_default() != CMD_READ_RESPONSE
            || header.get(1).copied().unwrap_or_default() != addr_h
            || header.get(2).copied().unwrap_or_default() != addr_l
            || header.get(3).copied().unwrap_or_default() != len
        {
            return Err(ProtocolError::BadResponseHeader { addr });
        }

        // Read data payload.
        let mut data = vec![0u8; usize::from(len)];
        self.read_exact_timeout(&mut data)?;

        // ACK the received block.
        self.send_ack()?;
        thread::sleep(POST_ACK_DELAY);

        Ok(data)
    }

    /// Write an EEPROM block at the given address.
    ///
    /// Validates that the address does not fall within a forbidden calibration
    /// range, then sends `[CMD_WRITE, addr_h, addr_l, len, ...data]` and
    /// waits for ACK.
    ///
    /// # Errors
    /// Returns [`ProtocolError::ForbiddenAddress`] if the address overlaps
    /// calibration data, [`ProtocolError::Timeout`], or [`ProtocolError::SerialIo`].
    pub fn write_block(&mut self, addr: u16, data: &[u8]) -> Result<()> {
        // Reject writes to calibration data.
        if is_forbidden(addr, data.len()) {
            return Err(ProtocolError::ForbiddenAddress { addr });
        }

        let addr_h = (addr >> 8) as u8; // SAFETY: u16 >> 8 yields the high byte; fits u8 by construction
        let addr_l = (addr & 0xFF) as u8; // SAFETY: u16 & 0xFF yields the low byte; fits u8 by construction
        let len = data.len() as u8; // SAFETY: data.len() is bounded to CHUNK_SIZE (≤255) by framing; fits u8

        let mut packet = Vec::with_capacity(4 + data.len());
        packet.push(CMD_WRITE);
        packet.push(addr_h);
        packet.push(addr_l);
        packet.push(len);
        packet.extend_from_slice(data);

        self.port
            .write_all(&packet)
            .map_err(|source| ProtocolError::SerialIo { source })?;

        thread::sleep(POST_ACK_DELAY);
        self.read_ack()?;

        Ok(())
    }

    /// Download the full EEPROM image FROM the radio.
    ///
    /// Reads the main block (0x0000–0x1800) in 64-byte chunks and the
    /// auxiliary block (0x1E80–0x2000) in 16-byte chunks. A warm-up read at
    /// 0x1E80 primes the aux region.
    ///
    /// # Errors
    /// Returns [`ProtocolError::RetryExhausted`] if a block fails after all
    /// retries, or any underlying serial/protocol error.
    pub fn download_image(&mut self) -> Result<MemoryImage> {
        let image_size = usize::from(AUX_BLOCK_END);
        let mut image = MemoryImage::new(image_size);

        // Main block: 64-byte reads.
        let mut addr = MAIN_BLOCK_START;
        while addr < MAIN_BLOCK_END {
            let data = self.read_block_with_retry(addr, READ_BLOCK_SIZE)?;
            image.write_bytes(addr, &data);
            addr = addr.wrapping_add(u16::from(READ_BLOCK_SIZE));
        }

        // Auxiliary block: 16-byte reads.
        // First read at AUX_BLOCK_START is a warm-up (data still stored).
        let warmup = self.read_block_with_retry(AUX_BLOCK_START, AUX_READ_BLOCK_SIZE)?;
        image.write_bytes(AUX_BLOCK_START, &warmup);

        addr = AUX_BLOCK_START + u16::from(AUX_READ_BLOCK_SIZE);
        while addr < AUX_BLOCK_END {
            let data = self.read_block_with_retry(addr, AUX_READ_BLOCK_SIZE)?;
            image.write_bytes(addr, &data);
            addr = addr.wrapping_add(u16::from(AUX_READ_BLOCK_SIZE));
        }

        Ok(image)
    }

    /// Upload an EEPROM image to the radio.
    ///
    /// Only writes to safe address ranges (skipping forbidden calibration
    /// data). Writes in 16-byte blocks. Calls `progress(current, total)`
    /// after each block.
    ///
    /// # Errors
    /// Returns [`ProtocolError::RetryExhausted`] if a block fails after all
    /// retries, or any underlying serial/protocol error.
    pub fn upload_image(
        &mut self,
        image: &MemoryImage,
        progress: &mut dyn FnMut(usize, usize),
    ) -> Result<()> {
        let ranges: Vec<(u16, u16)> = UPLOAD_RANGES_MAIN
            .iter()
            .chain(UPLOAD_RANGES_AUX.iter())
            .copied()
            .collect();

        // Count total blocks for progress reporting.
        let total_blocks: usize = ranges
            .iter()
            .map(|(start, end)| usize::from(end - start) / usize::from(WRITE_BLOCK_SIZE))
            .sum();

        let mut blocks_written = 0usize;

        for (start, end) in &ranges {
            let mut addr = *start;
            while addr < *end {
                let block_size = usize::from(WRITE_BLOCK_SIZE);
                let data = image.read_bytes(addr, block_size);

                self.write_block_with_retry(addr, data)?;
                blocks_written += 1;
                progress(blocks_written, total_blocks);
                addr = addr.wrapping_add(u16::from(WRITE_BLOCK_SIZE));
            }
        }

        Ok(())
    }

    /// Exit programming mode (send ACK to signal completion).
    ///
    /// # Errors
    /// Returns [`ProtocolError::SerialIo`] on write failure.
    pub fn exit_programming_mode(&mut self) -> Result<()> {
        self.send_ack()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Read a block with up to `MAX_RETRIES` attempts.
    fn read_block_with_retry(&mut self, addr: u16, len: u8) -> Result<Vec<u8>> {
        for attempt in 1..=MAX_RETRIES {
            match self.read_block(addr, len) {
                Ok(data) => return Ok(data),
                Err(_) if attempt < MAX_RETRIES => {
                    thread::sleep(POST_ACK_DELAY);
                }
                Err(_) => {
                    return Err(ProtocolError::RetryExhausted {
                        addr,
                        attempts: MAX_RETRIES,
                    });
                }
            }
        }
        Err(ProtocolError::RetryExhausted {
            addr,
            attempts: MAX_RETRIES,
        })
    }

    /// Write a block with up to `MAX_RETRIES` attempts.
    fn write_block_with_retry(&mut self, addr: u16, data: &[u8]) -> Result<()> {
        for attempt in 1..=MAX_RETRIES {
            match self.write_block(addr, data) {
                Ok(()) => return Ok(()),
                // Forbidden address is not retryable.
                Err(ProtocolError::ForbiddenAddress { .. }) => {
                    return Err(ProtocolError::ForbiddenAddress { addr });
                }
                Err(_) if attempt < MAX_RETRIES => {
                    thread::sleep(POST_ACK_DELAY);
                }
                Err(_) => {
                    return Err(ProtocolError::RetryExhausted {
                        addr,
                        attempts: MAX_RETRIES,
                    });
                }
            }
        }
        Err(ProtocolError::RetryExhausted {
            addr,
            attempts: MAX_RETRIES,
        })
    }

    /// Read a single ACK byte, returning an error on mismatch or timeout.
    fn read_ack(&mut self) -> Result<()> {
        let mut byte = [0u8; 1];
        match self.port.read_exact(&mut byte) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                return Err(ProtocolError::Timeout);
            }
            Err(source) => return Err(ProtocolError::SerialIo { source }),
        }
        if byte.get(0).copied().unwrap_or_default() != ACK {
            return Err(ProtocolError::BadAck {
                expected: ACK,
                got: byte.get(0).copied().unwrap_or_default(),
            });
        }
        Ok(())
    }

    /// Send a single ACK byte.
    fn send_ack(&mut self) -> Result<()> {
        self.port
            .write_all(&[ACK])
            .map_err(|source| ProtocolError::SerialIo { source })
    }

    /// Read exactly `buf.len()` bytes, mapping timeouts to `ProtocolError`.
    fn read_exact_timeout(&mut self, buf: &mut [u8]) -> Result<()> { // kanon:ignore RUST/indexing-slicing -- function parameter &mut [u8], not indexing
        match self.port.read_exact(buf) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => Err(ProtocolError::Timeout),
            Err(source) => Err(ProtocolError::SerialIo { source }),
        }
    }
}

/// Check whether an address range overlaps any forbidden calibration region.
fn is_forbidden(addr: u16, len: usize) -> bool {
    let end = addr.saturating_add(u16::try_from(len).unwrap_or_default());
    FORBIDDEN_RANGES
        .iter()
        .any(|&(f_start, f_end)| addr < f_end && end > f_start)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "protocol_tests.rs"]
mod tests;
