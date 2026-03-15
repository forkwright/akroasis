//! Variant-aware EEPROM read/write protocol for the UV-5R family.
//!
//! Handles the differences between UV-5R (no aux block) and BF-F8HP
//! (aux block with warm-up read, dropped-byte workaround at 0x1FCF).

use snafu::Snafu;

use super::variant::VariantConfig;

/// Standard EEPROM block size for reads/writes.
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

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors from protocol operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ProtocolError {
    /// The serial port returned an I/O error.
    #[snafu(display("serial I/O error: {message}"))]
    SerialIo {
        /// Description of the I/O failure.
        message: String,
    },

    /// The radio did not acknowledge within the timeout.
    #[snafu(display("no ACK from radio within {timeout_ms}ms"))]
    Timeout {
        /// Timeout duration that expired.
        timeout_ms: u64,
    },

    /// The radio returned an unexpected response.
    #[snafu(display("unexpected response: expected {expected}, got {actual}"))]
    UnexpectedResponse {
        /// What was expected.
        expected: String,
        /// What was received.
        actual: String,
    },
}

// ── Block plan ───────────────────────────────────────────────────────────────

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
            size: BLOCK_SIZE as u16,
            is_warmup: false,
        });
        addr += BLOCK_SIZE as u16;
    }

    if config.has_aux_block {
        // Warm-up read: read 0x1E80 first, discard the data
        if config.needs_aux_warmup {
            ops.push(BlockOp {
                addr: AUX_WARMUP_ADDR,
                size: BLOCK_SIZE as u16,
                is_warmup: true,
            });
        }

        // Aux region with dropped-byte workaround
        let mut aux_addr = AUX_START;
        while aux_addr < AUX_END {
            let block_end = aux_addr + BLOCK_SIZE as u16;

            if aux_addr <= DROPPED_BYTE_ADDR && DROPPED_BYTE_ADDR < block_end {
                // Split into smaller reads around the problem address.
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
                    size: BLOCK_SIZE as u16,
                    is_warmup: false,
                });
            }

            aux_addr += BLOCK_SIZE as u16;
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
            size: BLOCK_SIZE as u16,
            is_warmup: false,
        });
        addr += BLOCK_SIZE as u16;
    }

    if config.has_aux_block {
        let mut aux_addr = AUX_START;
        while aux_addr < AUX_END {
            ops.push(BlockOp {
                addr: aux_addr,
                size: BLOCK_SIZE as u16,
                is_warmup: false,
            });
            aux_addr += BLOCK_SIZE as u16;
        }
    }

    ops
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items
)]
mod tests {
    use super::*;
    use crate::baofeng::variant::{bf_f8hp_config, uv5r_config, uv5rm_plus_config};

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
        let expected_blocks = (MAIN_END - MAIN_START) / BLOCK_SIZE as u16;
        assert_eq!(plan.len(), expected_blocks as usize);
        assert_eq!(plan[0].addr, MAIN_START);
        let last = plan.last().unwrap();
        assert_eq!(last.addr + last.size, MAIN_END);
    }

    #[test]
    fn f8hp_download_includes_aux_warmup() {
        let config = bf_f8hp_config();
        let plan = download_plan(&config);
        let warmup_ops: Vec<_> = plan.iter().filter(|op| op.is_warmup).collect();
        assert_eq!(warmup_ops.len(), 1);
        assert_eq!(warmup_ops[0].addr, AUX_WARMUP_ADDR);
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
            for slot in covered.iter_mut().skip(start).take(op.size as usize) {
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
        let expected_aux_blocks = (AUX_END - AUX_START) / BLOCK_SIZE as u16;
        assert_eq!(aux_ops.len(), expected_aux_blocks as usize);
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
}
