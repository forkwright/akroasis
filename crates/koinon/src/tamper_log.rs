//! Tamper-evident append-only log with keyed BLAKE3 hash chaining.
//!
//! # Binary format (per entry)
//!
//! ```text
//! [4 bytes: payload length (little-endian u32)]
//! [N bytes: CBOR-encoded LogEntry]
//! [32 bytes: keyed BLAKE3 hash = BLAKE3_keyed(cbor_bytes || prev_hash)]
//! ```
//!
//! The first entry's `prev_hash` is a keyed genesis root (see
//! [`ChainKey`]), not a public constant: recomputing a valid chain, from
//! the genesis onward, requires the key.
//!
//! # Truncation seal
//!
//! A hash chain alone cannot detect trailing truncation — any prefix of a
//! valid chain is itself internally consistent, so reading to EOF looks
//! identical whether the file legitimately ends there or was cut short.
//! [`TamperLog::open`] and [`TamperLog::append`] refresh a sidecar
//! `{log}.seal` file authenticating the current entry count; [`verify_chain`]
//! cross-checks the streamed count against it and reports
//! [`ChainStatus::Truncated`] on any mismatch against a valid seal claiming
//! *fewer* entries than verified, or [`ChainStatus::Unsealed`] when the seal
//! is valid but stale — claiming *fewer* entries than the stream, which is
//! reachable only by whoever holds the chain key (see akroasis#285). See
//! `tamper_log_seal` for the seal and key machinery.
//!
//! # Single-writer lock
//!
//! [`TamperLog::open_with_config`] acquires an exclusive advisory lock on a
//! `{log}.lock` sidecar and holds it for the writer's lifetime, refusing a
//! second concurrent writer rather than letting two handles independently
//! recover the same tail and fork the chain (akroasis#226). See
//! `tamper_log_lock`.
//!
//! # Rotation
//!
//! Rotating a log renames the current file aside and starts a fresh one,
//! but the chain is not reset: the new segment's first entry links from the
//! outgoing segment's terminal hash, authenticated in the new segment's own
//! seal as `segment_start_hash`. A rotated-away segment therefore remains
//! provable, and [`verify_segment_chain`] walks the whole segment set and
//! reports a break if any segment — including the seal that would have
//! named it — is missing (akroasis#211). See `tamper_log_segments`.

use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

#[path = "tamper_log_seal.rs"]
mod seal;

#[path = "tamper_log_rotation.rs"]
mod rotation;

#[path = "tamper_log_lock.rs"]
mod lock;

#[path = "tamper_log_verify.rs"]
mod verify;

#[path = "tamper_log_segments.rs"]
mod segments;

use rotation::rotation_path;

pub use seal::{CHAIN_KEY_LEN, ChainKey};
pub use segments::{SegmentChainStatus, verify_segment_chain};
pub use verify::{ChainStatus, VerificationResult, verify_chain};

/// Maximum allowed entry payload size (16 MiB).
///
/// This is a sanity limit that bounds memory allocation during recovery
/// and decode, not a tunable: an entry larger than this almost certainly
/// indicates corruption or a malicious write. Exposed publicly so callers
/// can validate payloads before attempting [`TamperLog::append`].
pub const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;

/// Default rotation threshold (100 MiB).
///
/// Override via [`TamperLogConfig::max_file_bytes`] +
/// [`TamperLog::open_with_config`], or via the builder-style
/// [`TamperLog::with_max_file_bytes`].
pub const DEFAULT_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Behavioral tuning for [`TamperLog`].
///
/// Currently controls rotation only; future additions (fsync policy,
/// compression, retention) will land here.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TamperLogConfig {
    /// Rotation threshold in bytes. When `bytes_written` exceeds this,
    /// the current file is renamed and a fresh log is opened.
    pub max_file_bytes: u64,
}

impl Default for TamperLogConfig {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by [`TamperLog`] and [`verify_chain`].
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum TamperLogError {
    /// I/O error accessing the log file.
    #[snafu(display("I/O error on {}: {source}", path.display()))]
    Io {
        /// Path of the file that triggered the error.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// CBOR serialization error.
    #[snafu(display("CBOR encode error: {source}"))]
    CborEncode {
        /// Underlying ciborium serialization error.
        source: ciborium::ser::Error<std::io::Error>,
    },

    /// CBOR deserialization error.
    #[snafu(display("CBOR decode error: {source}"))]
    CborDecode {
        /// Underlying ciborium deserialization error.
        source: ciborium::de::Error<std::io::Error>,
    },

    /// File is corrupted or truncated at the given byte offset.
    #[snafu(display("log file corrupted at byte offset {offset}"))]
    Corrupted {
        /// Byte offset where corruption was detected.
        offset: u64,
    },

    /// Entry payload exceeds the sanity LIMIT.
    #[snafu(display("entry too large: {size} bytes (max {max})"))]
    EntryTooLarge {
        /// Actual size of the entry.
        size: u64,
        /// Maximum allowed size.
        max: u64,
    },

    /// The existing chain failed verification and cannot be safely
    /// resumed. Only [`ChainStatus::Intact`] or [`ChainStatus::Empty`] may
    /// be resumed — appending onto anything else would silently launder
    /// the tampering, since the next seal refresh would authenticate a
    /// shorter-but-internally-consistent chain as if it were the whole
    /// story.
    #[snafu(display(
        "refusing to resume compromised tamper log at {}: {status:?}",
        path.display()
    ))]
    ChainCompromised {
        /// Path of the log file that failed pre-resume verification.
        path: PathBuf,
        /// The verification status that caused the refusal.
        status: ChainStatus,
    },

    /// Another writer already holds the exclusive lock on this log.
    ///
    /// `TamperLog::open_with_config` fails fast rather than blocking (see
    /// `tamper_log_lock`) — a second concurrent opener must not recover the
    /// same tail state a live writer already holds, which would fork the
    /// hash chain.
    #[snafu(display(
        "tamper log at {} is already held by another writer",
        path.display()
    ))]
    Locked {
        /// Path of the log file that could not be locked.
        path: PathBuf,
    },
}

// ---------------------------------------------------------------------------
// Log entry types
// ---------------------------------------------------------------------------

#[path = "tamper_log_entry.rs"]
mod entry;

pub use entry::LogEntryKind;

/// A single record in the tamper-evident log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogEntry {
    /// Monotonically increasing sequence number within a log file (0-based).
    pub sequence: u64,
    /// Wall-clock time at which the entry was appended (Unix milliseconds).
    pub timestamp_ms: i64,
    /// Event payload.
    pub kind: LogEntryKind,
}

// ---------------------------------------------------------------------------
// Encode / decode
// ---------------------------------------------------------------------------

/// Serializes `entry` to CBOR, computes its keyed hash, and returns
/// `(wire_bytes, entry_hash)`.
///
/// `wire_bytes` is the complete on-disk representation:
/// `[4-byte LE length][CBOR payload][32-byte keyed BLAKE3 hash]`.
///
/// # Errors
///
/// Returns [`TamperLogError::CborEncode`] if serialization fails.
/// Returns [`TamperLogError::EntryTooLarge`] if the CBOR payload exceeds
/// [`MAX_ENTRY_BYTES`], which is the same bound the decoder enforces.
pub fn encode_entry(
    entry: &LogEntry,
    prev_hash: &[u8; 32],
    chain_key: &ChainKey,
) -> Result<(Vec<u8>, [u8; 32]), TamperLogError> {
    let mut cbor_bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(entry, &mut cbor_bytes).context(CborEncodeSnafu)?;

    // WHY: decode_entry and verify_chain both reject payload_len > MAX_ENTRY_BYTES,
    // but nothing enforced it here — the SAFETY note below asserted a validation
    // that did not exist. An oversized entry was written happily and then made the
    // whole log unverifiable; past 4 GiB the `as u32` cast also truncated the
    // length prefix, corrupting every following entry's framing.
    let payload_len = cbor_bytes.len() as u64; // SAFETY: usize->u64 is lossless on all supported targets
    if payload_len > MAX_ENTRY_BYTES {
        return Err(TamperLogError::EntryTooLarge {
            size: payload_len,
            max: MAX_ENTRY_BYTES,
        });
    }

    let mut hasher = blake3::Hasher::new_keyed(chain_key.as_bytes());
    hasher.update(&cbor_bytes);
    hasher.update(prev_hash);
    let hash: [u8; 32] = hasher.finalize().into();

    let len = cbor_bytes.len() as u32; // SAFETY: checked above against MAX_ENTRY_BYTES (16 MiB), so it fits u32
    let mut wire = Vec::with_capacity(4 + cbor_bytes.len() + 32);
    wire.extend_from_slice(&len.to_le_bytes());
    wire.extend_from_slice(&cbor_bytes);
    wire.extend_from_slice(&hash);

    Ok((wire, hash))
}

/// Parses a wire-format entry FROM `bytes`, returning `(LogEntry, stored_hash)`.
///
/// Does **not** verify the hash chain  -  use [`verify_chain`] for that.
///
/// # Errors
///
/// - [`TamperLogError::EntryTooLarge`] if the declared payload size exceeds 16 MiB.
/// - [`TamperLogError::Corrupted`] if `bytes` is too short for the declared payload.
/// - [`TamperLogError::CborDecode`] if the CBOR payload cannot be deserialized.
pub fn decode_entry(bytes: &[u8]) -> Result<(LogEntry, [u8; 32]), TamperLogError> {
    let mut cursor = Cursor::new(bytes);

    let mut len_buf = [0u8; 4];
    cursor
        .read_exact(&mut len_buf)
        .map_err(|_| TamperLogError::Corrupted { offset: 0 })?;
    let payload_len = u64::from(u32::from_le_bytes(len_buf));

    if payload_len > MAX_ENTRY_BYTES {
        return Err(TamperLogError::EntryTooLarge {
            size: payload_len,
            max: MAX_ENTRY_BYTES,
        });
    }

    // WHY: payload_len is already bounded by MAX_ENTRY_BYTES above, but the
    // usize conversion is still fallible on 32-bit-usize targets — treat
    // that as the same corruption failure rather than silently allocating a
    // zero-length buffer and reading a truncated entry.
    let payload_len_usize =
        usize::try_from(payload_len).map_err(|_| TamperLogError::Corrupted { offset: 0 })?;
    let mut cbor_bytes = vec![0u8; payload_len_usize];
    cursor
        .read_exact(&mut cbor_bytes)
        .map_err(|_| TamperLogError::Corrupted { offset: 4 })?;

    let entry: LogEntry = ciborium::from_reader(cbor_bytes.as_slice()).context(CborDecodeSnafu)?;

    let mut hash = [0u8; 32];
    cursor
        .read_exact(&mut hash)
        .map_err(|_| TamperLogError::Corrupted {
            offset: 4 + payload_len,
        })?;

    Ok((entry, hash))
}

// ---------------------------------------------------------------------------
// TamperLog writer
// ---------------------------------------------------------------------------

/// Append-only tamper-evident log writer.
///
/// Each call to [`TamperLog::append`] writes a length-prefixed CBOR entry
/// followed by a keyed BLAKE3 hash that chains FROM the previous entry,
/// then refreshes the truncation seal. Holds an exclusive advisory lock on
/// `{path}.lock` for its entire lifetime (akroasis#226); the OS releases it
/// automatically if the process dies, so a crashed writer never wedges the
/// next opener.
pub struct TamperLog {
    writer: BufWriter<File>,
    path: PathBuf,
    prev_hash: [u8; 32],
    /// The hash this segment's own chain was linked from: the keyed genesis
    /// root for a never-rotated log, or the outgoing segment's terminal
    /// hash for one created by rotation. Fixed for the file's lifetime;
    /// persisted in the seal so a restart on an empty post-rotation file
    /// recovers it (akroasis#211).
    segment_start_hash: [u8; 32],
    sequence: u64,
    bytes_written: u64,
    max_file_bytes: u64,
    chain_key: ChainKey,
    // WHY: held for its Drop impl, which releases the OS advisory lock —
    // never read, only kept alive. Mirrors kryphos::Vault's `_lock` field.
    _lock: File,
}

impl TamperLog {
    /// Opens or creates a log file at `path`, keyed with `chain_key`.
    ///
    /// If the file already exists, it is fully verified first (see
    /// [`verify_chain`]). [`ChainStatus::Intact`] and [`ChainStatus::Empty`]
    /// resume directly; [`ChainStatus::Unsealed`] also resumes — it is only
    /// reachable by the chain-key holder (akroasis#285) — and this call
    /// re-seals to the true, now-recovered count before returning. Anything
    /// else is refused with [`TamperLogError::ChainCompromised`] rather
    /// than resumed, which would launder the tampering into an
    /// apparently-shorter-but-valid chain. Otherwise reads to the end to
    /// recover `prev_hash` and `sequence` so new appends continue the
    /// chain.
    ///
    /// Acquires and holds an exclusive lock on `{path}.lock` for the
    /// returned handle's lifetime (akroasis#226); a concurrent `open` on
    /// the same path fails with [`TamperLogError::Locked`] instead of
    /// racing this call's state recovery.
    ///
    /// # Errors
    ///
    /// Returns [`TamperLogError::Locked`] if another writer already holds
    /// the path. Returns [`TamperLogError::ChainCompromised`] if an
    /// existing file fails pre-resume verification. Returns
    /// [`TamperLogError::Io`] on filesystem errors, or
    /// [`TamperLogError::CborDecode`] / [`TamperLogError::Corrupted`] if an
    /// existing file cannot be parsed.
    pub fn open(path: impl AsRef<Path>, chain_key: ChainKey) -> Result<Self, TamperLogError> {
        Self::open_with_config(path, chain_key, &TamperLogConfig::default())
    }

    /// Opens or creates a log file at `path` with the supplied tuning.
    ///
    /// Equivalent to [`Self::open`] but reads all behavioral knobs from
    /// the [`TamperLogConfig`]. This is the agent-preferred entry point:
    /// given a deserialized config, everything follows.
    ///
    /// # Errors
    ///
    /// Returns [`TamperLogError::Locked`] if another writer already holds
    /// the path. Returns [`TamperLogError::ChainCompromised`] if an
    /// existing file fails pre-resume verification. Returns
    /// [`TamperLogError::Io`] on filesystem errors, or
    /// [`TamperLogError::CborDecode`] / [`TamperLogError::Corrupted`] if an
    /// existing file cannot be parsed.
    pub fn open_with_config(
        path: impl AsRef<Path>,
        chain_key: ChainKey,
        config: &TamperLogConfig,
    ) -> Result<Self, TamperLogError> {
        let path = path.as_ref().to_owned();

        // WHY (#226): acquire the exclusive single-writer lock before any
        // state recovery touches `path` — a second writer that loses this
        // race fails here, rather than independently recovering the same
        // stale tail and forking the chain onto the same
        // prev_hash/sequence. Held for the returned handle's lifetime; the
        // OS drops it automatically if this process dies (see
        // `tamper_log_lock`).
        let held_lock = lock::acquire(&path)?;

        if path.exists() {
            let verification = verify_chain(&path, &chain_key)?;
            match verification.status {
                // WHY (#285) on the Unsealed arm: the stream verified MORE
                // entries than a validly-authenticated seal claims —
                // reachable only by whoever holds `chain_key`, so this is a
                // safe-to-resume seal-refresh failure, not tampering. Fall
                // through: recover_state below reads the TRUE (larger) tail
                // straight from the verified content, and the refresh_seal
                // call at the end of this function re-seals to that count
                // before any further append is accepted. Intact and Empty
                // resume directly, needing no recovery commentary.
                ChainStatus::Intact | ChainStatus::Empty | ChainStatus::Unsealed { .. } => {}
                status => return ChainCompromisedSnafu { path, status }.fail(),
            }
        }

        // WHY: gated on `path.exists()`, matching the compromise check
        // above — a log file that does not exist starts at genesis
        // unconditionally, never from a leftover `.seal` sidecar surviving
        // a deleted `.log` (an admin/attacker action that removes both
        // together in the normal case). Reading a stale seal here would be
        // harmless — still cryptographically self-consistent, forgeable by
        // nobody without `chain_key` — but would silently make this fresh
        // log fail closed under `verify_segment_chain`'s genesis-anchored
        // walk later, for no benefit.
        let segment_start_hash = if path.exists() {
            verify::resolve_start_hash(seal::read_seal(&path, &chain_key), &chain_key)
        } else {
            seal::genesis_hash(&chain_key)
        };
        let (prev_hash, sequence, bytes_written) = Self::recover_state(&path, segment_start_hash)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context(IoSnafu { path: &path })?;

        let log = Self {
            writer: BufWriter::new(file),
            path,
            prev_hash,
            segment_start_hash,
            sequence,
            bytes_written,
            max_file_bytes: config.max_file_bytes,
            chain_key,
            _lock: held_lock,
        };
        log.refresh_seal()?;

        Ok(log)
    }

    /// Sets the rotation threshold in bytes (default: 100 MiB).
    #[must_use]
    pub const fn with_max_file_bytes(mut self, max: u64) -> Self {
        self.max_file_bytes = max;
        self
    }

    /// Returns the hash of the last written entry, or this segment's start
    /// hash if none has been written yet — the keyed genesis root for a
    /// never-rotated log, or the outgoing segment's terminal hash for one
    /// just created by rotation (akroasis#211).
    pub const fn last_hash(&self) -> &[u8; 32] {
        &self.prev_hash
    }

    /// Returns the number of entries written to the current file.
    pub const fn entry_count(&self) -> u64 {
        self.sequence
    }

    /// Appends an event entry, flushes, refreshes the truncation seal,
    /// and returns the sequence number assigned.
    ///
    /// Triggers file rotation if `bytes_written` exceeds `max_file_bytes`.
    ///
    /// # Errors
    ///
    /// Returns [`TamperLogError::Io`] on write failures or
    /// [`TamperLogError::CborEncode`] on serialization failures.
    pub fn append(&mut self, kind: LogEntryKind) -> Result<u64, TamperLogError> {
        let entry = LogEntry {
            sequence: self.sequence,
            timestamp_ms: jiff::Timestamp::now().as_millisecond(),
            kind,
        };

        let (wire, hash) = encode_entry(&entry, &self.prev_hash, &self.chain_key)?;
        self.writer
            .write_all(&wire)
            .context(IoSnafu { path: &self.path })?;
        self.writer.flush().context(IoSnafu { path: &self.path })?;

        let seq = self.sequence;
        self.prev_hash = hash;
        self.sequence += 1;
        self.bytes_written += wire.len() as u64; // SAFETY: wire.len() <= 4 + MAX_ENTRY_BYTES + 32; fits u64 trivially

        self.refresh_seal()?;

        if self.bytes_written > self.max_file_bytes {
            self.rotate()?;
        }

        Ok(seq)
    }

    /// Flushes any buffered data to the underlying file.
    ///
    /// # Errors
    ///
    /// Returns [`TamperLogError::Io`] if the flush fails.
    pub fn flush(&mut self) -> Result<(), TamperLogError> {
        self.writer.flush().context(IoSnafu { path: &self.path })
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Rewrites the sidecar seal to authenticate the log's current state.
    fn refresh_seal(&self) -> Result<(), TamperLogError> {
        seal::write_seal(
            &self.path,
            &self.chain_key,
            self.sequence,
            &self.segment_start_hash,
        )
    }

    /// Reads an existing log file to recover `(prev_hash, next_sequence, bytes_written)`.
    ///
    /// Trusts stored hashes structurally (no re-verification) — callers
    /// that need cryptographic assurance the file was not tampered with
    /// must call [`verify_chain`] first, as [`Self::open_with_config`] does.
    ///
    /// `segment_start_hash` seeds `prev_hash` for a file with zero entries
    /// (nonexistent, or empty because it is a freshly-rotated segment) —
    /// the caller resolves it from the seal (or genesis, for a brand new
    /// file) via `verify::resolve_start_hash`, since an empty file carries
    /// no information of its own to recover it from (akroasis#211).
    fn recover_state(
        path: &Path,
        segment_start_hash: [u8; 32],
    ) -> Result<([u8; 32], u64, u64), TamperLogError> {
        if !path.exists() {
            return Ok((segment_start_hash, 0, 0));
        }

        let file = File::open(path).context(IoSnafu { path })?;
        let file_len = file.metadata().context(IoSnafu { path })?.len();
        if file_len == 0 {
            return Ok((segment_start_hash, 0, 0));
        }

        let mut reader = BufReader::new(file);
        let mut prev_hash = segment_start_hash;
        let mut sequence: u64 = 0;
        let mut bytes_read: u64 = 0;

        loop {
            let mut len_buf = [0u8; 4];
            match reader.read_exact(&mut len_buf) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    return Err(TamperLogError::Io {
                        path: path.to_owned(),
                        source: e,
                    });
                }
            }

            let payload_len = u64::from(u32::from_le_bytes(len_buf));
            if payload_len > MAX_ENTRY_BYTES {
                break;
            }

            let seek_offset = i64::try_from(payload_len).map_err(|e| TamperLogError::Io {
                path: path.to_owned(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
            })?;
            reader
                .seek(SeekFrom::Current(seek_offset))
                .context(IoSnafu { path })?;

            let mut stored_hash = [0u8; 32];
            if reader.read_exact(&mut stored_hash).is_err() {
                break;
            }

            prev_hash = stored_hash;
            sequence += 1;
            bytes_read += 4 + payload_len + 32;
        }

        Ok((prev_hash, sequence, bytes_read))
    }

    /// Renames the current log file to `{stem}.{n}.log` (and its seal
    /// sidecar alongside it) and opens a fresh one, keyed the same as the
    /// original.
    ///
    /// The new segment's chain is NOT reset to genesis: it is seeded from
    /// the outgoing segment's terminal hash, and that seed is authenticated
    /// in the new segment's own seal as `segment_start_hash` so the link
    /// survives both a later restart and cross-segment verification (see
    /// [`verify_segment_chain`], akroasis#211). The single-writer lock (on
    /// `{self.path}.lock`, a fixed sidecar untouched by rotation) is held
    /// throughout without any extra work here.
    fn rotate(&mut self) -> Result<(), TamperLogError> {
        self.writer.flush().context(IoSnafu { path: &self.path })?;

        let n = rotation::next_rotation_number(&self.path)?;
        let rotated = rotation_path(&self.path, n);

        std::fs::rename(&self.path, &rotated).context(IoSnafu { path: &self.path })?;
        seal::rename_seal(&self.path, &rotated)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context(IoSnafu { path: &self.path })?;

        self.writer = BufWriter::new(file);
        self.segment_start_hash = self.prev_hash;
        self.sequence = 0;
        self.bytes_written = 0;

        self.refresh_seal()?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "tamper_log_tests.rs"]
mod tests;

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "tamper_log_codec_tests.rs"]
mod codec_tests;

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "tamper_log_recovery_tests.rs"]
mod recovery_tests;
