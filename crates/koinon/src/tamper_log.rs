//! Tamper-evident append-only log with BLAKE3 hash chaining.
//!
//! # Binary format (per entry)
//!
//! ```text
//! [4 bytes: payload length (little-endian u32)]
//! [N bytes: CBOR-encoded LogEntry]
//! [32 bytes: BLAKE3 hash = BLAKE3(cbor_bytes || prev_hash)]
//! ```
//!
//! The first entry uses `[0u8; 32]` as `prev_hash`.

use std::{
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use crate::{EntityId, SignalId};

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
#[serde(default)]
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
}

// ---------------------------------------------------------------------------
// Log entry types
// ---------------------------------------------------------------------------

/// The kind of event recorded in a [`LogEntry`].
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LogEntryKind {
    /// A signal was observed by a collector.
    SignalObserved {
        /// Identifier of the observed signal.
        signal_id: SignalId,
        /// Short tag describing the signal kind.
        kind_tag: CompactString,
    },
    /// A new entity was created in the system.
    EntityCreated {
        /// Identifier of the created entity.
        entity_id: EntityId,
        /// Short tag describing the entity kind.
        kind_tag: CompactString,
    },
    /// A configuration parameter was changed.
    ConfigChanged {
        /// Configuration key that changed.
        key: CompactString,
        /// Previous value, if any.
        old_value: Option<CompactString>,
        /// New value after the change.
        new_value: CompactString,
    },
    /// An alert was raised by the analysis pipeline.
    AlertRaised {
        /// Unique identifier for this alert.
        alert_id: CompactString,
        /// Severity level (e.g. `"critical"`, `"warning"`).
        severity: CompactString,
        /// Human-readable alert message.
        message: CompactString,
    },
    /// An operator or automation took an action.
    ActionTaken {
        /// Identity of the actor (user or system).
        actor: CompactString,
        /// Description of the action performed.
        action: CompactString,
        /// Target of the action, if applicable.
        target: Option<CompactString>,
    },
    /// A credential vault entry lifecycle mutation was committed.
    VaultMutation {
        /// Human-readable credential name affected by the mutation.
        credential_name: CompactString,
        /// Mutation operation, e.g. `"add"`, `"rotate"`, `"revoke"`, or `"remove"`.
        operation: CompactString,
    },
}

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

/// Serializes `entry` to CBOR, computes its hash, and returns `(wire_bytes, entry_hash)`.
///
/// `wire_bytes` is the complete on-disk representation:
/// `[4-byte LE length][CBOR payload][32-byte BLAKE3 hash]`.
///
/// # Errors
///
/// Returns [`TamperLogError::CborEncode`] if serialization fails.
pub fn encode_entry(
    entry: &LogEntry,
    prev_hash: &[u8; 32],
) -> Result<(Vec<u8>, [u8; 32]), TamperLogError> {
    let mut cbor_bytes: Vec<u8> = Vec::new();
    ciborium::into_writer(entry, &mut cbor_bytes).context(CborEncodeSnafu)?;

    let mut hasher = blake3::Hasher::new();
    hasher.update(&cbor_bytes);
    hasher.update(prev_hash);
    let hash: [u8; 32] = hasher.finalize().into();

    let len = cbor_bytes.len() as u32; // SAFETY: CBOR payload size validated <= MAX_ENTRY_BYTES (16 MiB), fits u32
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

    let mut cbor_bytes = vec![0u8; usize::try_from(payload_len).unwrap_or_default()];
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
// Chain verification
// ---------------------------------------------------------------------------

/// Status of a chain verification pass.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChainStatus {
    /// All entries verified successfully.
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
    /// File is empty (no entries).
    Empty,
    /// File is truncated or otherwise unreadable at `byte_offset`.
    Corrupted {
        /// Byte OFFSET WHERE the problem was detected.
        byte_offset: u64,
    },
}

/// Result of a chain verification pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationResult {
    /// Number of entries that were successfully parsed and verified.
    pub entries_verified: u64,
    /// Overall chain status.
    pub status: ChainStatus,
}

/// Reads `path` FROM the beginning, recomputes every hash link, and returns
/// the first break found.
///
/// This is O(n) in file size and streams the file; it never loads the whole
/// file INTO memory.
///
/// # Errors
///
/// Returns [`TamperLogError::Io`] if the file cannot be opened or read.
pub fn verify_chain(path: impl AsRef<Path>) -> Result<VerificationResult, TamperLogError> {
    let path = path.as_ref();
    let file = File::open(path).context(IoSnafu { path })?;
    let mut reader = BufReader::new(file);

    let mut prev_hash = [0u8; 32];
    let mut entries_verified: u64 = 0;
    let mut byte_offset: u64 = 0;

    loop {
        // Read length prefix.
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                if entries_verified == 0 {
                    return Ok(VerificationResult {
                        entries_verified: 0,
                        status: ChainStatus::Empty,
                    });
                }
                return Ok(VerificationResult {
                    entries_verified,
                    status: ChainStatus::Intact,
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
            return Ok(VerificationResult {
                entries_verified,
                status: ChainStatus::Corrupted { byte_offset },
            });
        }

        // Read CBOR payload.
        let mut cbor_bytes = vec![0u8; usize::try_from(payload_len).unwrap_or_default()];
        if reader.read_exact(&mut cbor_bytes).is_err() {
            return Ok(VerificationResult {
                entries_verified,
                status: ChainStatus::Corrupted {
                    byte_offset: byte_offset + 4,
                },
            });
        }

        // Read stored hash.
        let mut stored_hash = [0u8; 32];
        if reader.read_exact(&mut stored_hash).is_err() {
            return Ok(VerificationResult {
                entries_verified,
                status: ChainStatus::Corrupted {
                    byte_offset: byte_offset + 4 + payload_len,
                },
            });
        }

        // Recompute hash.
        let mut hasher = blake3::Hasher::new();
        hasher.update(&cbor_bytes);
        hasher.update(&prev_hash);
        let expected_hash: [u8; 32] = hasher.finalize().into();

        if expected_hash != stored_hash {
            let sequence = ciborium::from_reader::<LogEntry, _>(cbor_bytes.as_slice())
                .map_or(entries_verified, |e| e.sequence);

            return Ok(VerificationResult {
                entries_verified,
                status: ChainStatus::Broken {
                    sequence,
                    expected_hash,
                    actual_hash: stored_hash,
                },
            });
        }

        prev_hash = expected_hash;
        entries_verified += 1;
        byte_offset += 4 + payload_len + 32;
    }
}

// ---------------------------------------------------------------------------
// TamperLog writer
// ---------------------------------------------------------------------------

/// Append-only tamper-evident log writer.
///
/// Each call to [`TamperLog::append`] writes a length-prefixed CBOR entry
/// followed by a BLAKE3 hash that chains FROM the previous entry.
pub struct TamperLog {
    writer: BufWriter<File>,
    path: PathBuf,
    prev_hash: [u8; 32],
    sequence: u64,
    bytes_written: u64,
    max_file_bytes: u64,
}

impl TamperLog {
    /// Opens or creates a log file at `path`.
    ///
    /// If the file already exists and contains entries, reads to the end to
    /// recover `prev_hash` and `sequence` so new appends continue the chain.
    ///
    /// # Errors
    ///
    /// Returns [`TamperLogError::Io`] on filesystem errors, or
    /// [`TamperLogError::CborDecode`] / [`TamperLogError::Corrupted`] if an
    /// existing file cannot be parsed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, TamperLogError> {
        Self::open_with_config(path, &TamperLogConfig::default())
    }

    /// Opens or creates a log file at `path` with the supplied tuning.
    ///
    /// Equivalent to [`Self::open`] but reads all behavioral knobs from
    /// the [`TamperLogConfig`]. This is the agent-preferred entry point:
    /// given a deserialized config, everything follows.
    ///
    /// # Errors
    ///
    /// Returns [`TamperLogError::Io`] on filesystem errors, or
    /// [`TamperLogError::CborDecode`] / [`TamperLogError::Corrupted`] if an
    /// existing file cannot be parsed.
    pub fn open_with_config(
        path: impl AsRef<Path>,
        config: &TamperLogConfig,
    ) -> Result<Self, TamperLogError> {
        let path = path.as_ref().to_owned();

        let (prev_hash, sequence, bytes_written) = Self::recover_state(&path)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .context(IoSnafu { path: &path })?;

        Ok(Self {
            writer: BufWriter::new(file),
            path,
            prev_hash,
            sequence,
            bytes_written,
            max_file_bytes: config.max_file_bytes,
        })
    }

    /// Sets the rotation threshold in bytes (default: 100 MiB).
    #[must_use]
    pub const fn with_max_file_bytes(mut self, max: u64) -> Self {
        self.max_file_bytes = max;
        self
    }

    /// Returns the hash of the last written entry (or `[0u8; 32]` for a fresh log).
    pub const fn last_hash(&self) -> &[u8; 32] {
        &self.prev_hash
    }

    /// Returns the number of entries written to the current file.
    pub const fn entry_count(&self) -> u64 {
        self.sequence
    }

    /// Appends an event entry, flushes, and returns the sequence number assigned.
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

        let (wire, hash) = encode_entry(&entry, &self.prev_hash)?;
        self.writer
            .write_all(&wire)
            .context(IoSnafu { path: &self.path })?;
        self.writer.flush().context(IoSnafu { path: &self.path })?;

        let seq = self.sequence;
        self.prev_hash = hash;
        self.sequence += 1;
        self.bytes_written += wire.len() as u64; // SAFETY: wire.len() <= 4 + MAX_ENTRY_BYTES + 32; fits u64 trivially

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

    /// Reads an existing log file to recover `(prev_hash, next_sequence, bytes_written)`.
    fn recover_state(path: &Path) -> Result<([u8; 32], u64, u64), TamperLogError> {
        if !path.exists() {
            return Ok(([0u8; 32], 0, 0));
        }

        let file = File::open(path).context(IoSnafu { path })?;
        let file_len = file.metadata().context(IoSnafu { path })?.len();
        if file_len == 0 {
            return Ok(([0u8; 32], 0, 0));
        }

        let mut reader = BufReader::new(file);
        let mut prev_hash = [0u8; 32];
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

    /// Renames the current log file to `{stem}.{n}.log` and opens a fresh one.
    fn rotate(&mut self) -> Result<(), TamperLogError> {
        self.writer.flush().context(IoSnafu { path: &self.path })?;

        let n = Self::next_rotation_number(&self.path);
        let rotated = rotation_path(&self.path, n);

        std::fs::rename(&self.path, &rotated).context(IoSnafu { path: &self.path })?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .context(IoSnafu { path: &self.path })?;

        self.writer = BufWriter::new(file);
        self.prev_hash = [0u8; 32];
        self.sequence = 0;
        self.bytes_written = 0;

        Ok(())
    }

    /// Scans sibling files to find the next rotation number.
    fn next_rotation_number(path: &Path) -> u32 {
        let stem = log_stem(path);
        let dir = path.parent().unwrap_or_else(|| Path::new("."));

        let mut max_n: u32 = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(n) = parse_rotation_number(&name, &stem) {
                    if n > max_n {
                        max_n = n;
                    }
                }
            }
        }
        max_n + 1
    }
}

// ---------------------------------------------------------------------------
// Rotation helpers
// ---------------------------------------------------------------------------

/// Returns the stem of a log path, e.g. `"audit"` FROM `"audit.log"`.
fn log_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Builds `{dir}/{stem}.{n}.log`.
fn rotation_path(path: &Path, n: u32) -> PathBuf {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = log_stem(path);
    dir.join(format!("{stem}.{n}.log"))
}

/// Parses `{stem}.{n}.log` → `Some(n)`, returns `None` otherwise.
fn parse_rotation_number(name: &str, stem: &str) -> Option<u32> {
    let prefix = format!("{stem}.");
    let suffix = ".log";
    let inner = name.strip_prefix(&prefix)?.strip_suffix(suffix)?;
    inner.parse::<u32>().ok()
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
