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
        let addr_h = (addr >> 8) as u8;
        let addr_l = (addr & 0xFF) as u8;
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

        let addr_h = (addr >> 8) as u8;
        let addr_l = (addr & 0xFF) as u8;
        let len = data.len() as u8;

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
                let data = image
                    .read_bytes(addr, block_size)
                    .ok_or(ProtocolError::BadResponseHeader { addr })?;

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
    fn read_exact_timeout(&mut self, buf: &mut [u8]) -> Result<()> {
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
mod tests {
    use super::*;
    use crate::baofeng::variant::{bf_f8hp_config, uv5r_config, uv5rm_plus_config};
    use crate::serial::mock::MockSerialPort;

    // -----------------------------------------------------------------------
    // Block planning tests
    // -----------------------------------------------------------------------

    #[test]
    fn uv5r_download_has_no_aux_blocks() {
        let config = uv5r_config();
        let plan = download_plan(&config);
        assert!(
            plan.iter().all(|op| op.addr < MAIN_END),
            "UV-5R should not read beyond main memory"
        );
        assert!(
            !plan.iter().any(|op| op.is_warmup),
            "UV-5R should not have warmup reads"
        );
    }

    #[test]
    fn uv5r_download_covers_full_main_region() {
        let config = uv5r_config();
        let plan = download_plan(&config);
        let expected_blocks = (MAIN_END - MAIN_START) / u16::try_from(BLOCK_SIZE).unwrap_or_default();
        assert_eq!(plan.len(), usize::try_from(expected_blocks).unwrap_or_default());
        assert_eq!(plan.get(0).copied().unwrap_or_default().addr, MAIN_START);
        let last = plan.last().unwrap();
        assert_eq!(last.addr + last.size, MAIN_END);
    }

    #[test]
    fn f8hp_download_includes_aux_warmup() {
        let config = bf_f8hp_config();
        let plan = download_plan(&config);
        let warmup_ops: Vec<_> = plan.iter().filter(|op| op.is_warmup).collect();
        assert_eq!(warmup_ops.len(), 1);
        assert_eq!(warmup_ops.get(0).copied().unwrap_or_default().addr, AUX_WARMUP_ADDR);
    }

    #[test]
    fn f8hp_download_reads_aux_region() {
        let config = bf_f8hp_config();
        let plan = download_plan(&config);
        let aux_ops: Vec<_> = plan
            .iter()
            .filter(|op| op.addr >= AUX_START && !op.is_warmup)
            .collect();
        assert!(!aux_ops.is_empty(), "F8HP should read aux region");

        // Verify aux region is fully covered
        let mut covered = vec![false; (AUX_END - AUX_START) as usize];
        for op in &aux_ops {
            let start = (op.addr - AUX_START) as usize;
            for slot in covered.iter_mut().skip(start).take(op.usize::try_from(size).unwrap_or_default()) {
                *slot = true;
            }
        }
        assert!(
            covered.iter().all(|&c| c),
            "aux region not fully covered by read ops"
        );
    }

    #[test]
    fn f8hp_download_splits_around_dropped_byte() {
        let config = bf_f8hp_config();
        let plan = download_plan(&config);
        // There should be a 1-byte read at the dropped byte address
        let single_byte_count = plan
            .iter()
            .filter(|op| op.addr == DROPPED_BYTE_ADDR && op.size == 1)
            .count();
        assert_eq!(
            single_byte_count, 1,
            "should have exactly one 1-byte read at dropped byte addr"
        );
    }

    #[test]
    fn uv5r_upload_matches_download_without_warmup() {
        let config = uv5r_config();
        let dl = download_plan(&config);
        let ul = upload_plan(&config);
        assert_eq!(dl, ul, "UV-5R upload and download should be identical");
    }

    #[test]
    fn f8hp_upload_has_no_warmup() {
        let config = bf_f8hp_config();
        let plan = upload_plan(&config);
        assert!(
            !plan.iter().any(|op| op.is_warmup),
            "upload should not have warmup reads"
        );
    }

    #[test]
    fn f8hp_upload_covers_aux_region() {
        let config = bf_f8hp_config();
        let plan = upload_plan(&config);
        let aux_ops: Vec<_> = plan.iter().filter(|op| op.addr >= AUX_START).collect();
        assert!(!aux_ops.is_empty());
        let expected_aux_blocks = (AUX_END - AUX_START) / u16::try_from(BLOCK_SIZE).unwrap_or_default();
        assert_eq!(aux_ops.len(), usize::try_from(expected_aux_blocks).unwrap_or_default());
    }

    #[test]
    fn uv5rm_plus_download_matches_f8hp_structure() {
        let f8hp_plan = download_plan(&bf_f8hp_config());
        let rm_plan = download_plan(&uv5rm_plus_config());
        assert_eq!(f8hp_plan.len(), rm_plan.len());
        for (f, r) in f8hp_plan.iter().zip(rm_plan.iter()) {
            assert_eq!(f.addr, r.addr);
            assert_eq!(f.size, r.size);
            assert_eq!(f.is_warmup, r.is_warmup);
        }
    }

    #[test]
    fn block_op_debug_format() {
        let op = BlockOp {
            addr: 0x1E80,
            size: 16,
            is_warmup: true,
        };
        let debug = format!("{op:?}");
        assert!(debug.contains("1E80") || debug.contains("7808"));
    }

    // -----------------------------------------------------------------------
    // Protocol driver helpers
    // -----------------------------------------------------------------------

    fn make_protocol(mock: MockSerialPort) -> Uv5rProtocol<MockSerialPort> {
        Uv5rProtocol::new(mock)
    }

    /// Build a read-response packet for the given address and data.
    fn read_response_packet(addr: u16, data: &[u8]) -> Vec<u8> {
        let mut pkt = vec![
            CMD_READ_RESPONSE,
            (addr >> 8) as u8,
            (addr & 0xFF) as u8,
            data.len() as u8,
        ];
        pkt.extend_from_slice(data);
        pkt
    }

    // -----------------------------------------------------------------------
    // Magic byte / programming mode tests
    // -----------------------------------------------------------------------

    #[test]
    fn enter_programming_mode_sends_magic_bytes() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[ACK]);

        let mut proto = make_protocol(mock);
        proto
            .enter_programming_mode(&super::super::constants::MAGIC_UV5R_291)
            .unwrap();

        // All 7 magic bytes should have been written.
        assert_eq!(
            &proto.port.written[..7],
            &super::super::constants::MAGIC_UV5R_291
        );
    }

    #[test]
    fn enter_programming_mode_bad_ack_returns_error() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[0xFF]);

        let mut proto = make_protocol(mock);
        let err = proto
            .enter_programming_mode(&super::super::constants::MAGIC_UV5R_291)
            .unwrap_err();

        assert!(matches!(
            err,
            ProtocolError::BadAck {
                expected: 0x06,
                got: 0xFF
            }
        ));
    }

    #[test]
    fn enter_programming_mode_timeout_returns_error() {
        let mock = MockSerialPort::new();
        let mut proto = make_protocol(mock);
        let err = proto
            .enter_programming_mode(&super::super::constants::MAGIC_UV5R_291)
            .unwrap_err();

        assert!(matches!(err, ProtocolError::Timeout));
    }

    // -----------------------------------------------------------------------
    // Ident tests
    // -----------------------------------------------------------------------

    #[test]
    fn identify_parses_8_byte_response() {
        let mut mock = MockSerialPort::new();
        let ident_bytes = [0x42, 0x46, 0x42, 0x32, 0x39, 0x31, 0xAA, 0xBB];
        let mut response = ident_bytes.to_vec();
        response.push(IDENT_TERMINATOR);
        mock.enqueue_response(&response);
        mock.enqueue_response(&[ACK]); // Radio's ready ACK.

        let mut proto = make_protocol(mock);
        let ident = proto.identify().unwrap();

        assert_eq!(ident.raw_bytes.len(), 8);
        assert_eq!(ident.normalized, ident_bytes);
    }

    #[test]
    fn identify_normalizes_12_byte_response() {
        let mut mock = MockSerialPort::new();
        let raw = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
        ];
        let mut response = raw.to_vec();
        response.push(IDENT_TERMINATOR);
        mock.enqueue_response(&response);
        mock.enqueue_response(&[ACK]);

        let mut proto = make_protocol(mock);
        let ident = proto.identify().unwrap();

        assert_eq!(ident.raw_bytes.len(), 12);
        assert_eq!(
            ident.normalized,
            [0x01, 0x04, 0x06, 0x08, 0x09, 0x0A, 0x0B, 0x0C]
        );
    }

    #[test]
    fn identify_timeout_returns_error() {
        let mock = MockSerialPort::new();
        let mut proto = make_protocol(mock);
        let err = proto.identify().unwrap_err();
        assert!(matches!(err, ProtocolError::Timeout));
    }

    #[test]
    fn identify_odd_length_returns_ident_failed() {
        let mut mock = MockSerialPort::new();
        let raw = [0x01, 0x02, 0x03, 0x04, 0x05];
        let mut response = raw.to_vec();
        response.push(IDENT_TERMINATOR);
        mock.enqueue_response(&response);

        let mut proto = make_protocol(mock);
        let err = proto.identify().unwrap_err();
        assert!(matches!(err, ProtocolError::IdentFailed));
    }

    // -----------------------------------------------------------------------
    // Read block tests
    // -----------------------------------------------------------------------

    #[test]
    fn read_block_constructs_correct_packet() {
        let mut mock = MockSerialPort::new();
        let addr: u16 = 0x1234;
        let len: u8 = 0x40;
        let payload = vec![0xAA; usize::from(len)];

        let resp = read_response_packet(addr, &payload);
        mock.enqueue_response(&resp);

        let mut proto = make_protocol(mock);
        let data = proto.read_block(addr, len).unwrap();

        assert_eq!(data, payload);
        // Verify the request packet: [0x53, 0x12, 0x34, 0x40].
        assert_eq!(&proto.port.written[..4], &[CMD_READ, 0x12, 0x34, 0x40]);
    }

    #[test]
    fn read_block_bad_header_returns_error() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[CMD_READ_RESPONSE, 0xFF, 0xFF, 0x40]);
        mock.enqueue_response(&[0u8; 64]);

        let mut proto = make_protocol(mock);
        let err = proto.read_block(0x0100, 0x40).unwrap_err();
        assert!(matches!(
            err,
            ProtocolError::BadResponseHeader { addr: 0x0100 }
        ));
    }

    #[test]
    fn read_block_timeout_on_partial_header() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[CMD_READ_RESPONSE]);

        let mut proto = make_protocol(mock);
        let err = proto.read_block(0x0000, 0x40).unwrap_err();
        assert!(matches!(err, ProtocolError::Timeout));
    }

    // -----------------------------------------------------------------------
    // Write block tests
    // -----------------------------------------------------------------------

    #[test]
    fn write_block_constructs_correct_packet() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[ACK]);

        let data = [0xBB; 16];
        let mut proto = make_protocol(mock);
        proto.write_block(0x0100, &data).unwrap();

        // Packet: [0x58, 0x01, 0x00, 0x10, ...16 bytes data]
        assert_eq!(proto.port.written.get(0).copied().unwrap_or_default(), CMD_WRITE);
        assert_eq!(proto.port.written.get(1).copied().unwrap_or_default(), 0x01);
        assert_eq!(proto.port.written.get(2).copied().unwrap_or_default(), 0x00);
        assert_eq!(proto.port.written.get(3).copied().unwrap_or_default(), 0x10);
        assert_eq!(&proto.port.written[4..20], &data);
    }

    #[test]
    fn write_block_receives_ack() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[ACK]);

        let data = [0xCC; 16];
        let mut proto = make_protocol(mock);
        assert!(proto.write_block(0x0200, &data).is_ok());
    }

    // -----------------------------------------------------------------------
    // Forbidden address tests
    // -----------------------------------------------------------------------

    #[test]
    fn write_block_rejects_forbidden_0x1f00() {
        let mock = MockSerialPort::new();
        let mut proto = make_protocol(mock);
        let err = proto.write_block(0x1F00, &[0x00; 16]).unwrap_err();
        assert!(matches!(
            err,
            ProtocolError::ForbiddenAddress { addr: 0x1F00 }
        ));
    }

    #[test]
    fn write_block_rejects_forbidden_0x1fd0() {
        let mock = MockSerialPort::new();
        let mut proto = make_protocol(mock);
        let err = proto.write_block(0x1FD0, &[0x00; 16]).unwrap_err();
        assert!(matches!(
            err,
            ProtocolError::ForbiddenAddress { addr: 0x1FD0 }
        ));
    }

    #[test]
    fn write_block_rejects_overlapping_forbidden_range() {
        let mock = MockSerialPort::new();
        let mut proto = make_protocol(mock);
        // 0x1EF0 + 32 → 0x1F10, overlaps forbidden 0x1F00–0x1F60.
        let err = proto.write_block(0x1EF0, &[0x00; 32]).unwrap_err();
        assert!(matches!(
            err,
            ProtocolError::ForbiddenAddress { addr: 0x1EF0 }
        ));
    }

    #[test]
    fn write_block_allows_safe_address() {
        let mut mock = MockSerialPort::new();
        mock.enqueue_response(&[ACK]);

        let mut proto = make_protocol(mock);
        assert!(proto.write_block(0x0100, &[0x00; 16]).is_ok());
    }

    #[test]
    fn write_block_no_serial_io_on_forbidden_address() {
        let mock = MockSerialPort::new();
        let mut proto = make_protocol(mock);
        let _ = proto.write_block(0x1F40, &[0x00; 16]);
        assert!(proto.port.written.is_empty());
    }

    // -----------------------------------------------------------------------
    // Retry tests
    // -----------------------------------------------------------------------

    #[test]
    fn read_block_retries_on_first_failure() {
        let mut mock = MockSerialPort::new();
        let addr: u16 = 0x0100;
        let len: u8 = 0x40;
        let payload = vec![0xEE; usize::from(len)];

        // First attempt: timeout (no data) → will fail.
        // (empty queue causes TimedOut on the second read_block's header read)
        // Second attempt: correct response.
        mock.enqueue_response(&read_response_packet(addr, &payload));

        let mut proto = make_protocol(mock);
        let data = proto.read_block_with_retry(addr, len).unwrap();
        assert_eq!(data, payload);
    }

    #[test]
    fn read_block_retry_exhaustion_returns_error() {
        let mock = MockSerialPort::new();
        let addr: u16 = 0x0100;
        let len: u8 = 0x40;

        // No data at all → all 3 attempts time out.

        let mut proto = make_protocol(mock);
        let err = proto.read_block_with_retry(addr, len).unwrap_err();
        assert!(matches!(
            err,
            ProtocolError::RetryExhausted {
                addr: 0x0100,
                attempts: 3
            }
        ));
    }

    // -----------------------------------------------------------------------
    // Download tests
    // -----------------------------------------------------------------------

    #[test]
    fn download_image_reads_correct_address_sequence() {
        let mut mock = MockSerialPort::new();

        // Main block: 0x0000..0x1800 in 64-byte chunks.
        let main_blocks = (MAIN_BLOCK_END - MAIN_BLOCK_START) / u16::from(READ_BLOCK_SIZE);
        for i in 0..main_blocks {
            let addr = MAIN_BLOCK_START + i * u16::from(READ_BLOCK_SIZE);
            let data = vec![u8::try_from(i).unwrap_or_default(); usize::from(READ_BLOCK_SIZE)];
            mock.enqueue_response(&read_response_packet(addr, &data));
        }

        // Aux block: 0x1E80..0x2000 in 16-byte chunks.
        let aux_blocks = (AUX_BLOCK_END - AUX_BLOCK_START) / u16::from(AUX_READ_BLOCK_SIZE);
        for i in 0..aux_blocks {
            let addr = AUX_BLOCK_START + i * u16::from(AUX_READ_BLOCK_SIZE);
            let data = vec![(128 + i) as u8; usize::from(AUX_READ_BLOCK_SIZE)];
            mock.enqueue_response(&read_response_packet(addr, &data));
        }

        let mut proto = make_protocol(mock);
        let image = proto.download_image().unwrap();

        assert_eq!(image.len(), usize::from(AUX_BLOCK_END));
        // First main block byte.
        assert_eq!(image.read_bytes(0x0000, 1), Some(&[0u8][..]));
        // Last aux block byte.
        let last_aux_addr = AUX_BLOCK_END - u16::from(AUX_READ_BLOCK_SIZE);
        let expected_val = (128 + aux_blocks - 1) as u8;
        assert_eq!(
            image.read_bytes(last_aux_addr, 1),
            Some(&[expected_val][..])
        );
    }

    // -----------------------------------------------------------------------
    // Upload tests
    // -----------------------------------------------------------------------

    #[test]
    fn upload_only_writes_safe_ranges() {
        let total_safe_bytes: usize = UPLOAD_RANGES_MAIN
            .iter()
            .chain(UPLOAD_RANGES_AUX.iter())
            .map(|(s, e)| usize::from(e - s))
            .sum();
        let total_blocks = total_safe_bytes / usize::from(WRITE_BLOCK_SIZE);

        let mut mock = MockSerialPort::new();
        for _ in 0..total_blocks {
            mock.enqueue_response(&[ACK]);
        }

        let image = MemoryImage::new(usize::from(AUX_BLOCK_END));
        let mut progress_calls = Vec::new();
        let mut proto = make_protocol(mock);

        proto
            .upload_image(&image, &mut |current, total| {
                progress_calls.push((current, total));
            })
            .unwrap();

        assert_eq!(progress_calls.len(), total_blocks);
        assert_eq!(progress_calls.last(), Some(&(total_blocks, total_blocks)));
    }

    #[test]
    fn upload_calls_progress_callback() {
        let total_safe_bytes: usize = UPLOAD_RANGES_MAIN
            .iter()
            .chain(UPLOAD_RANGES_AUX.iter())
            .map(|(s, e)| usize::from(e - s))
            .sum();
        let total_blocks = total_safe_bytes / usize::from(WRITE_BLOCK_SIZE);

        let mut mock = MockSerialPort::new();
        for _ in 0..total_blocks {
            mock.enqueue_response(&[ACK]);
        }

        let image = MemoryImage::new(usize::from(AUX_BLOCK_END));
        let mut called = false;
        let mut proto = make_protocol(mock);

        proto
            .upload_image(&image, &mut |_current, _total| {
                called = true;
            })
            .unwrap();

        assert!(called);
    }

    // -----------------------------------------------------------------------
    // MemoryImage tests
    // -----------------------------------------------------------------------

    #[test]
    fn memory_image_read_write_roundtrip() {
        let mut img = MemoryImage::new(256);
        let data = [0xDE, 0xAD, 0xBE, 0xEF];
        assert!(img.write_bytes(0x10, &data));
        assert_eq!(img.read_bytes(0x10, 4), Some(&data[..]));
    }

    #[test]
    fn memory_image_out_of_bounds_read_returns_none() {
        let img = MemoryImage::new(16);
        assert_eq!(img.read_bytes(0x10, 1), None);
    }

    #[test]
    fn memory_image_out_of_bounds_write_returns_false() {
        let mut img = MemoryImage::new(16);
        assert!(!img.write_bytes(0x10, &[0xFF]));
    }

    #[test]
    fn memory_image_zero_filled_on_creation() {
        let img = MemoryImage::new(8);
        assert_eq!(img.as_slice(), &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn memory_image_len_and_is_empty() {
        let img = MemoryImage::new(42);
        assert_eq!(img.len(), 42);
        assert!(!img.is_empty());

        let empty = MemoryImage::new(0);
        assert!(empty.is_empty());
    }

    #[test]
    fn memory_image_from_vec() {
        let data = vec![1, 2, 3, 4];
        let img = MemoryImage::from_vec(data.clone());
        assert_eq!(img.as_slice(), &data);
    }

    // -----------------------------------------------------------------------
    // is_forbidden tests
    // -----------------------------------------------------------------------

    #[test]
    fn is_forbidden_detects_exact_range_start() {
        assert!(is_forbidden(0x1F00, 1));
    }

    #[test]
    fn is_forbidden_detects_overlap_from_below() {
        assert!(is_forbidden(0x1EF0, 32));
    }

    #[test]
    fn is_forbidden_allows_address_before_range() {
        assert!(!is_forbidden(0x0100, 16));
    }

    #[test]
    fn is_forbidden_allows_gap_between_ranges() {
        // 0x1F60..0x1F70 is between forbidden ranges.
        assert!(!is_forbidden(0x1F60, 16));
    }

    // -----------------------------------------------------------------------
    // RadioIdent unit tests
    // -----------------------------------------------------------------------

    #[test]
    fn radio_ident_firmware_prefix_from_ascii() {
        let raw = [b'B', b'F', b'B', b'2', b'9', b'1', 0x00, 0x01];
        let ident = RadioIdent::from_raw(&raw).unwrap();
        assert_eq!(ident.firmware_prefix, "BFB291");
    }

    #[test]
    fn radio_ident_rejects_odd_length() {
        assert!(RadioIdent::from_raw(&[1, 2, 3]).is_none());
    }
}
