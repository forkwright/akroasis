//! Chain verification for [`super::TamperLog`]; split out to keep the
//! parent file under the RUST/file-too-long 800-line threshold.
//!
//! [`verify_chain`] proves one segment file, seeded from its own
//! authenticated `segment_start_hash` (genesis for a never-rotated log).
//! [`stream_verify`] is the underlying streaming primitive, parameterized
//! over an explicit start hash rather than always reading it from a seal —
//! `tamper_log_segments` reuses it directly, seeding each segment from the
//! terminal hash the PREVIOUS segment actually produced rather than from
//! what a segment's own seal merely claims.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use snafu::ResultExt;

use super::{ChainKey, IoSnafu, LogEntry, MAX_ENTRY_BYTES, TamperLogError, seal};

/// Status of a chain verification pass.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChainStatus {
    /// All entries verified successfully, and the entry count matches
    /// the authenticated seal.
    Intact,
    /// Chain is broken at the given sequence number.
    Broken {
        /// Sequence number of the first bad entry.
        sequence: u64,
        /// Hash that was expected (recomputed FROM content).
        expected_hash: [u8; 32],
        /// Hash that was stored on disk.
        actual_hash: [u8; 32],
    },
    /// File is empty (no entries) and no seal claims otherwise.
    Empty,
    /// File is truncated or otherwise unreadable at `byte_offset`.
    Corrupted {
        /// Byte OFFSET WHERE the problem was detected.
        byte_offset: u64,
    },
    /// The chain streamed cleanly (every link that exists verifies), but
    /// the entry count does not match the authenticated sidecar seal —
    /// the signature of a trailing run of entries deleted by an adversary
    /// who lacks the chain key needed to forge a matching seal.
    Truncated {
        /// Entry count from a validated seal, when one authenticated.
        /// `None` when no seal authenticated at all (missing or forged).
        sealed_entries: Option<u64>,
    },
    /// The chain streamed cleanly and every link verifies, but a
    /// *validly-authenticated* seal claims fewer entries than the stream
    /// contains — the signature of an append whose seal refresh failed to
    /// persist after the entry itself was durably written (akroasis#285),
    /// not tampering: producing additional entries that verify against the
    /// existing chain requires the same key that authenticates the seal, so
    /// this state is reachable only by the log's own legitimate writer.
    /// [`super::TamperLog::open_with_config`] resumes from this state and
    /// re-seals to the true count rather than refusing it.
    Unsealed {
        /// Entries the stream actually verified.
        verified_entries: u64,
        /// Entries the valid seal claims — always `< verified_entries`.
        sealed_entries: u64,
    },
}

/// Result of a chain verification pass.
// WHY: pure data — a verification result bag with no derived invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationResult {
    /// Number of entries that were successfully parsed and verified.
    pub entries_verified: u64,
    /// Overall chain status.
    pub status: ChainStatus,
}

/// Outcome of streaming one segment file's hash chain from a starting hash.
pub(super) struct StreamResult {
    /// Entries successfully parsed and hash-verified.
    pub(super) entries_verified: u64,
    /// Hash the chain reached after the last verified entry — equal to
    /// `start_hash` when `entries_verified == 0`. Meaningful only when
    /// `break_status` is `None` (the stream reached a clean EOF); a segment
    /// that broke mid-stream has no trustworthy terminal to hand the next
    /// segment.
    pub(super) terminal_hash: [u8; 32],
    /// `Some(status)` if the stream hit [`ChainStatus::Broken`] or
    /// [`ChainStatus::Corrupted`] partway through; `None` if it reached EOF
    /// cleanly, in which case the caller still owes a seal cross-check.
    pub(super) break_status: Option<ChainStatus>,
}

/// Streams `path` from the beginning, recomputing every hash link keyed
/// with `chain_key` starting from `start_hash`, and reports the first break
/// found or a clean-EOF terminal.
///
/// This is O(n) in file size and streams the file; it never loads the whole
/// file INTO memory. Does not consult the seal — callers that need the
/// seal-cross-checked terminal status use [`verify_chain`]; callers walking
/// a segment set supply each segment's own start hash directly (see
/// `tamper_log_segments`).
pub(super) fn stream_verify(
    path: &Path,
    chain_key: &ChainKey,
    start_hash: [u8; 32],
) -> Result<StreamResult, TamperLogError> {
    let file = File::open(path).context(IoSnafu { path })?;
    let mut reader = BufReader::new(file);

    let mut prev_hash = start_hash;
    let mut entries_verified: u64 = 0;
    let mut byte_offset: u64 = 0;

    loop {
        // Read length prefix.
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(StreamResult {
                    entries_verified,
                    terminal_hash: prev_hash,
                    break_status: None,
                });
            }
            Err(e) => {
                return Err(TamperLogError::Io {
                    path: path.to_owned(),
                    source: e,
                });
            }
        }

        let payload_len = u64::from(u32::from_le_bytes(len_buf));
        if payload_len > MAX_ENTRY_BYTES {
            return Ok(StreamResult {
                entries_verified,
                terminal_hash: prev_hash,
                break_status: Some(ChainStatus::Corrupted { byte_offset }),
            });
        }

        // Read CBOR payload.
        // WHY: payload_len is already bounded by MAX_ENTRY_BYTES above, but
        // the usize conversion is still fallible on 32-bit-usize targets —
        // treat that the same as the oversized-payload case rather than
        // silently reading a zero-length (truncated) buffer.
        let Ok(payload_len_usize) = usize::try_from(payload_len) else {
            return Ok(StreamResult {
                entries_verified,
                terminal_hash: prev_hash,
                break_status: Some(ChainStatus::Corrupted { byte_offset }),
            });
        };
        let mut cbor_bytes = vec![0u8; payload_len_usize];
        match reader.read_exact(&mut cbor_bytes) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(StreamResult {
                    entries_verified,
                    terminal_hash: prev_hash,
                    break_status: Some(ChainStatus::Corrupted {
                        byte_offset: byte_offset + 4,
                    }),
                });
            }
            Err(e) => {
                return Err(TamperLogError::Io {
                    path: path.to_owned(),
                    source: e,
                });
            }
        }

        // Read stored hash.
        let mut stored_hash = [0u8; 32];
        match reader.read_exact(&mut stored_hash) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(StreamResult {
                    entries_verified,
                    terminal_hash: prev_hash,
                    break_status: Some(ChainStatus::Corrupted {
                        byte_offset: byte_offset + 4 + payload_len,
                    }),
                });
            }
            Err(e) => {
                return Err(TamperLogError::Io {
                    path: path.to_owned(),
                    source: e,
                });
            }
        }

        // Recompute the keyed hash.
        let mut hasher = blake3::Hasher::new_keyed(chain_key.as_bytes());
        hasher.update(&cbor_bytes);
        hasher.update(&prev_hash);
        let expected_hash: [u8; 32] = hasher.finalize().into();

        if expected_hash != stored_hash {
            let sequence = ciborium::from_reader::<LogEntry, _>(cbor_bytes.as_slice())
                .map_or(entries_verified, |e| e.sequence);

            return Ok(StreamResult {
                entries_verified,
                terminal_hash: prev_hash,
                break_status: Some(ChainStatus::Broken {
                    sequence,
                    expected_hash,
                    actual_hash: stored_hash,
                }),
            });
        }

        prev_hash = expected_hash;
        entries_verified += 1;
        byte_offset += 4 + payload_len + 32;
    }
}

/// Resolves the hash a segment file's chain was linked from: the persisted,
/// authenticated `segment_start_hash` from a valid seal, or the keyed
/// genesis root as the safe default when no seal authenticates (a brand
/// new file, or one whose seal cannot be trusted — either way the stream
/// will simply fail to verify against a wrong seed rather than silently
/// accepting the wrong content).
/// Streams `path` and returns the hash the chain actually ends at.
///
/// Exists so provenance can be checked against where the chain *is* rather
/// than where a seal claims it is — the seal is the thing under examination,
/// so taking the terminal hash from it would let the record answer the
/// question being asked of it.
pub(super) fn stream_terminal_hash(
    path: &Path,
    chain_key: &ChainKey,
    sealed: seal::SealState,
) -> Result<[u8; 32], TamperLogError> {
    let start_hash = resolve_start_hash(sealed, chain_key);
    Ok(stream_verify(path, chain_key, start_hash)?.terminal_hash)
}

pub(super) fn resolve_start_hash(sealed: seal::SealState, chain_key: &ChainKey) -> [u8; 32] {
    match sealed {
        seal::SealState::Valid {
            segment_start_hash, ..
        } => segment_start_hash,
        seal::SealState::Absent | seal::SealState::Invalid => seal::genesis_hash(chain_key),
    }
}

/// Reads `path` FROM the beginning and recomputes every hash link keyed
/// with `chain_key`.
///
/// The chain is seeded from this segment's own authenticated
/// `segment_start_hash` when a valid seal supplies one, or the keyed
/// genesis root otherwise. Returns the first break found, or, if every
/// link verifies, the seal-cross-checked terminal status.
///
/// This is O(n) in file size and streams the file; it never loads the whole
/// file INTO memory. Verifies exactly one segment file — a rotated log's
/// earlier segments are not consulted. Use
/// [`verify_segment_chain`](super::verify_segment_chain) to walk and
/// cross-link an entire rotated segment set.
///
/// # Errors
///
/// Returns [`TamperLogError::Io`] if the file cannot be opened or read.
pub fn verify_chain(
    path: impl AsRef<Path>,
    chain_key: &ChainKey,
) -> Result<VerificationResult, TamperLogError> {
    let path = path.as_ref();
    let sealed = seal::read_seal(path, chain_key);
    let start_hash = resolve_start_hash(sealed, chain_key);

    let stream = stream_verify(path, chain_key, start_hash)?;

    let status = stream
        .break_status
        .unwrap_or_else(|| classify_terminal(sealed, stream.entries_verified));

    Ok(VerificationResult {
        entries_verified: stream.entries_verified,
        status,
    })
}

/// Determines the terminal status once a segment has streamed cleanly to
/// EOF with `entries_verified` links all verifying, by cross-checking
/// against the already-read, authenticated sidecar seal.
pub(super) const fn classify_terminal(
    sealed: seal::SealState,
    entries_verified: u64,
) -> ChainStatus {
    if entries_verified == 0 {
        match sealed {
            seal::SealState::Absent | seal::SealState::Valid { entry_count: 0, .. } => {
                ChainStatus::Empty
            }
            seal::SealState::Valid { entry_count, .. } => ChainStatus::Truncated {
                sealed_entries: Some(entry_count),
            },
            seal::SealState::Invalid => ChainStatus::Truncated {
                sealed_entries: None,
            },
        }
    } else {
        match sealed {
            seal::SealState::Valid { entry_count, .. } if entry_count == entries_verified => {
                ChainStatus::Intact
            }
            // WHY (#285): a VALID (MAC-authenticated) seal claiming FEWER
            // entries than the stream just verified can only have been
            // produced by an earlier, incomplete refresh_seal from the same
            // chain-key holder — nobody without the key can extend the
            // stream past a valid seal's count. Distinct from every other
            // arm here, which the seal being wrong-direction or untrusted
            // keeps fail-closed.
            seal::SealState::Valid { entry_count, .. } if entry_count < entries_verified => {
                ChainStatus::Unsealed {
                    verified_entries: entries_verified,
                    sealed_entries: entry_count,
                }
            }
            seal::SealState::Valid { entry_count, .. } => ChainStatus::Truncated {
                sealed_entries: Some(entry_count),
            },
            seal::SealState::Absent | seal::SealState::Invalid => ChainStatus::Truncated {
                sealed_entries: None,
            },
        }
    }
}
