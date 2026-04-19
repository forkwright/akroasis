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
const MAX_ENTRY_BYTES: u64 = 16 * 1024 * 1024;
/// Default rotation threshold (100 MiB).
const DEFAULT_MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced by [`TamperLog`] and [`verify_chain`].
#[derive(Debug, Snafu)]
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

    let len = cbor_bytes.len() as u32;
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
                .map(|e| e.sequence)
                .unwrap_or(entries_verified);

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
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
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
        self.bytes_written += wire.len() as u64;

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
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items
)]
mod tests {
    use super::*;
    use compact_str::CompactString;
    use ulid::Ulid;

    fn signal_kind() -> LogEntryKind {
        LogEntryKind::SignalObserved {
            signal_id: SignalId::new(),
            kind_tag: CompactString::from("rf"),
        }
    }

    fn entity_kind() -> LogEntryKind {
        LogEntryKind::EntityCreated {
            entity_id: EntityId::new(),
            kind_tag: CompactString::from("drone"),
        }
    }

    fn config_kind() -> LogEntryKind {
        LogEntryKind::ConfigChanged {
            key: CompactString::from("threshold"),
            old_value: Some(CompactString::from("10")),
            new_value: CompactString::from("20"),
        }
    }

    fn alert_kind() -> LogEntryKind {
        LogEntryKind::AlertRaised {
            alert_id: CompactString::from("ALT-001"),
            severity: CompactString::from("critical"),
            message: CompactString::from("signal strength exceeded LIMIT"),
        }
    }

    fn action_kind() -> LogEntryKind {
        LogEntryKind::ActionTaken {
            actor: CompactString::from("operator"),
            action: CompactString::from("acknowledge"),
            target: Some(CompactString::from("ALT-001")),
        }
    }

    // -----------------------------------------------------------------------
    // Core functionality
    // -----------------------------------------------------------------------

    #[test]
    fn append_single_entry_and_verify_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");

        let mut log = TamperLog::open(&path).unwrap();
        let seq = log.append(signal_kind()).unwrap();
        assert_eq!(seq, 0);
        assert_eq!(log.entry_count(), 1);

        let result = verify_chain(&path).unwrap();
        assert_eq!(result.entries_verified, 1);
        assert_eq!(result.status, ChainStatus::Intact);
    }

    #[test]
    fn append_100_entries_chain_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.log");

        let mut log = TamperLog::open(&path).unwrap();
        for i in 0..100_u64 {
            let seq = log.append(alert_kind()).unwrap();
            assert_eq!(seq, i);
        }

        let result = verify_chain(&path).unwrap();
        assert_eq!(result.entries_verified, 100);
        assert_eq!(result.status, ChainStatus::Intact);
    }

    #[test]
    fn empty_file_returns_empty_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.log");
        File::create(&path).unwrap();

        let result = verify_chain(&path).unwrap();
        assert_eq!(result.status, ChainStatus::Empty);
        assert_eq!(result.entries_verified, 0);
    }

    #[test]
    fn recovery_continues_chain_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recover.log");

        {
            let mut log = TamperLog::open(&path).unwrap();
            for _ in 0..5 {
                log.append(action_kind()).unwrap();
            }
        }

        {
            let mut log = TamperLog::open(&path).unwrap();
            assert_eq!(log.entry_count(), 5);
            for _ in 0..5 {
                log.append(config_kind()).unwrap();
            }
        }

        let result = verify_chain(&path).unwrap();
        assert_eq!(result.entries_verified, 10);
        assert_eq!(result.status, ChainStatus::Intact);
    }

    // -----------------------------------------------------------------------
    // Tampering detection
    // -----------------------------------------------------------------------

    /// Walks wire-format bytes and returns the byte offset of entry `target_idx`.
    fn entry_offset(data: &[u8], target_idx: usize) -> usize {
        let mut offset = 0usize;
        for i in 0..target_idx {
            let len = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            // Only advance if not at target.
            if i < target_idx {
                offset += 4 + len + 32;
            }
        }
        offset
    }

    #[test]
    fn flip_byte_in_cbor_payload_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tamper.log");

        let mut log = TamperLog::open(&path).unwrap();
        for _ in 0..10 {
            log.append(signal_kind()).unwrap();
        }
        drop(log);

        let mut data = std::fs::read(&path).unwrap();
        let off = entry_offset(&data, 5);
        // Flip a byte inside the CBOR payload (byte 4 = first CBOR byte).
        data[off + 4] ^= 0xFF;
        std::fs::write(&path, &data).unwrap();

        let result = verify_chain(&path).unwrap();
        assert!(matches!(
            result.status,
            ChainStatus::Broken { sequence: 5, .. }
        ));
    }

    #[test]
    fn flip_byte_in_hash_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tamper_hash.log");

        let mut log = TamperLog::open(&path).unwrap();
        for _ in 0..10 {
            log.append(entity_kind()).unwrap();
        }
        drop(log);

        let mut data = std::fs::read(&path).unwrap();
        let off = entry_offset(&data, 3);
        let payload_len =
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as usize;
        // Flip the first byte of entry 3's stored hash.
        data[off + 4 + payload_len] ^= 0x01;
        std::fs::write(&path, &data).unwrap();

        let result = verify_chain(&path).unwrap();
        // Entry 3's stored hash is wrong → broken at entry 3.
        assert!(matches!(result.status, ChainStatus::Broken { .. }));
    }

    #[test]
    fn truncated_file_returns_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("truncate.log");

        let mut log = TamperLog::open(&path).unwrap();
        for _ in 0..10 {
            log.append(config_kind()).unwrap();
        }
        drop(log);

        let data = std::fs::read(&path).unwrap();
        let truncated = &data[..data.len() - 20];
        std::fs::write(&path, truncated).unwrap();

        let result = verify_chain(&path).unwrap();
        assert!(matches!(result.status, ChainStatus::Corrupted { .. }));
    }

    #[test]
    fn zero_out_last_hash_detected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zerohash.log");

        let mut log = TamperLog::open(&path).unwrap();
        for _ in 0..10 {
            log.append(alert_kind()).unwrap();
        }
        drop(log);

        let mut data = std::fs::read(&path).unwrap();
        let hash_start = data.len() - 32;
        for b in &mut data[hash_start..] {
            *b = 0;
        }
        std::fs::write(&path, &data).unwrap();

        let result = verify_chain(&path).unwrap();
        assert!(matches!(result.status, ChainStatus::Broken { .. }));
    }

    // -----------------------------------------------------------------------
    // Rotation
    // -----------------------------------------------------------------------

    #[test]
    fn rotation_triggers_at_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rotate.log");

        let mut log = TamperLog::open(&path).unwrap().with_max_file_bytes(500);
        for _ in 0..20 {
            log.append(alert_kind()).unwrap();
        }
        drop(log);

        let rotated = dir.path().join("rotate.1.log");
        assert!(rotated.exists(), "rotated file should exist");
    }

    #[test]
    fn rotated_file_named_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mylog.log");

        let mut log = TamperLog::open(&path).unwrap().with_max_file_bytes(200);
        for _ in 0..15 {
            log.append(action_kind()).unwrap();
        }
        drop(log);

        assert!(dir.path().join("mylog.1.log").exists());
    }

    #[test]
    fn new_file_after_rotation_has_fresh_chain() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chain.log");

        let mut log = TamperLog::open(&path).unwrap().with_max_file_bytes(300);
        for _ in 0..20 {
            log.append(signal_kind()).unwrap();
        }
        drop(log);

        let result = verify_chain(&path).unwrap();
        assert!(
            matches!(result.status, ChainStatus::Intact | ChainStatus::Empty),
            "new file must be intact or empty"
        );
    }

    #[test]
    fn pre_rotation_file_verifies_independently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pre.log");

        let mut log = TamperLog::open(&path).unwrap().with_max_file_bytes(300);
        for _ in 0..20 {
            log.append(entity_kind()).unwrap();
        }
        drop(log);

        let rotated = dir.path().join("pre.1.log");
        if rotated.exists() {
            let result = verify_chain(&rotated).unwrap();
            assert_eq!(result.status, ChainStatus::Intact);
        }
    }

    #[test]
    fn multiple_rotations_sequential_numbering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.log");

        let mut log = TamperLog::open(&path).unwrap().with_max_file_bytes(150);
        for _ in 0..60 {
            log.append(config_kind()).unwrap();
        }
        drop(log);

        assert!(
            dir.path().join("multi.1.log").exists(),
            "multi.1.log missing"
        );
        assert!(
            dir.path().join("multi.2.log").exists(),
            "multi.2.log missing"
        );
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn single_entry_chain_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("single.log");

        let mut log = TamperLog::open(&path).unwrap();
        log.append(signal_kind()).unwrap();
        drop(log);

        let result = verify_chain(&path).unwrap();
        assert_eq!(result.status, ChainStatus::Intact);
        assert_eq!(result.entries_verified, 1);
    }

    #[test]
    fn cbor_roundtrip_signal_observed() {
        let entry = LogEntry {
            sequence: 0,
            timestamp_ms: 1_000_000,
            kind: signal_kind(),
        };
        let prev = [0u8; 32];
        let (wire, _) = encode_entry(&entry, &prev).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn cbor_roundtrip_entity_created() {
        let entry = LogEntry {
            sequence: 1,
            timestamp_ms: 2_000_000,
            kind: entity_kind(),
        };
        let (wire, _) = encode_entry(&entry, &[0u8; 32]).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn cbor_roundtrip_config_changed() {
        let entry = LogEntry {
            sequence: 2,
            timestamp_ms: 3_000_000,
            kind: config_kind(),
        };
        let (wire, _) = encode_entry(&entry, &[0u8; 32]).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn cbor_roundtrip_alert_raised() {
        let entry = LogEntry {
            sequence: 3,
            timestamp_ms: 4_000_000,
            kind: alert_kind(),
        };
        let (wire, _) = encode_entry(&entry, &[0u8; 32]).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn cbor_roundtrip_action_taken() {
        let entry = LogEntry {
            sequence: 4,
            timestamp_ms: 5_000_000,
            kind: action_kind(),
        };
        let (wire, _) = encode_entry(&entry, &[0u8; 32]).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn large_metadata_no_truncation() {
        // 512 bytes  -  well above compact_str's 24-byte inline capacity, tests
        // that heap-allocated string content survives a CBOR encode/decode
        // round-trip without truncation.
        let big = "x".repeat(512);
        let kind = LogEntryKind::AlertRaised {
            alert_id: CompactString::from("BIG"),
            severity: CompactString::from("info"),
            message: CompactString::from(big.as_str()),
        };
        let entry = LogEntry {
            sequence: 0,
            timestamp_ms: 0,
            kind,
        };
        let (wire, _) = encode_entry(&entry, &[0u8; 32]).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded);
    }

    #[test]
    fn id_types_usable_in_entry_kinds() {
        let sid = SignalId::from_ulid(Ulid::new());
        let eid = EntityId::from_ulid(Ulid::new());
        let _ = LogEntryKind::SignalObserved {
            signal_id: sid,
            kind_tag: CompactString::from("t"),
        };
        let _ = LogEntryKind::EntityCreated {
            entity_id: eid,
            kind_tag: CompactString::from("t"),
        };
    }
}
