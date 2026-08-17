//! Cross-segment (post-rotation) chain verification for [`super`]; split
//! out to keep the parent file under the RUST/file-too-long 800-line
//! threshold.
//!
//! [`verify_chain`] proves one segment file in isolation. A rotated log is
//! several files (`audit.log` plus `audit.1.log`, `audit.2.log`, ...,
//! oldest to newest by rotation number), and proving the WHOLE history
//! requires walking them together: [`verify_segment_chain`] verifies each
//! segment's own content, seeded from the terminal hash the PREVIOUS
//! segment actually produced — not from what a segment's own seal merely
//! claims — so a segment whose true predecessor is missing or was swapped
//! fails to verify at its very first entry, rather than being trusted on
//! the strength of a self-consistent internal chain. Each segment's own
//! seal is still cross-checked for trailing truncation exactly as
//! [`verify_chain`] does standalone: truncating a non-final segment is
//! already caught indirectly (its terminal hash changes, so its successor's
//! link check fails), but the LIVE file has no successor to catch a
//! truncation of its own tail, so its local seal-count check is the only
//! thing that would (akroasis#211).

use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::rotation::{log_stem, parse_rotation_number, rotation_path};
use super::seal::{genesis_hash, read_seal};
use super::verify::{classify_terminal, stream_verify};
use super::{ChainKey, ChainStatus, IoSnafu, TamperLogError};

/// Whether a segment's own verification status is acceptable for it to
/// count toward an [`SegmentChainStatus::Intact`] result.
///
/// Only the live (final, unrotated) file may be [`ChainStatus::Empty`] or
/// [`ChainStatus::Unsealed`] — a rotated-out segment is never appended to
/// again after rotation, so its seal is expected to exactly match its
/// content; seeing either there is treated as anomalous rather than
/// silently accepted, even though `Unsealed` alone would be safe under the
/// same reasoning as the live file (akroasis#285's leniency is scoped to
/// the writer's own resume path, not this read-only auditor).
fn segment_ok(status: &ChainStatus, is_live: bool) -> bool {
    match status {
        ChainStatus::Intact => true,
        ChainStatus::Empty | ChainStatus::Unsealed { .. } => is_live,
        ChainStatus::Broken { .. }
        | ChainStatus::Corrupted { .. }
        | ChainStatus::Truncated { .. } => false,
    }
}

/// Result of verifying an entire rotated log's segment set together.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentChainStatus {
    /// Every segment verified, oldest to newest, with no gap in the
    /// rotation numbering and each segment's content correctly chained
    /// from the terminal hash of the one before it.
    Intact {
        /// Total entries across every segment, oldest to newest.
        total_entries: u64,
        /// Number of segment files verified, including the live file.
        segments: u32,
    },
    /// A rotation number is missing from an otherwise-numbered run — the
    /// directory-visible signature of a deleted whole segment.
    MissingSegment {
        /// The lowest rotation number absent from the sequence.
        number: u32,
    },
    /// A segment (identified by path) failed to verify against the
    /// terminal hash its predecessor actually produced — either its own
    /// content is broken/corrupted, or it does not truly continue the
    /// segment before it (the signature of a deleted-and-silently-skipped
    /// segment, or one swapped for an unrelated file).
    SegmentBroken {
        /// Path of the segment that failed to verify.
        path: PathBuf,
        /// Its verification status against the expected predecessor link.
        status: ChainStatus,
    },
}

/// Verifies an entire rotated log's segment set: every `{stem}.N.log`
/// sibling of `path`, oldest to newest, followed by the live file at
/// `path` itself.
///
/// Each segment is streamed seeded from the terminal hash the PREVIOUS
/// segment's own content actually produced (genesis for the oldest), so
/// cross-segment continuity is proved by the hash chain itself rather than
/// by trusting any segment's self-reported link. A single-file,
/// never-rotated log (no `{stem}.N.log` siblings) verifies exactly as
/// [`verify_chain`](super::verify_chain) alone would.
///
/// Only the live (final, unrotated) file may legitimately be empty — a
/// rotated-out segment always has at least one entry, since
/// [`super::TamperLog`] only ever rotates after an append, so an empty
/// numbered segment is treated as broken rather than skipped.
///
/// # Errors
///
/// Returns [`TamperLogError::Io`] if the log's directory cannot be scanned
/// or a listed segment cannot be opened.
pub fn verify_segment_chain(
    path: impl AsRef<Path>,
    chain_key: &ChainKey,
) -> Result<SegmentChainStatus, TamperLogError> {
    let path = path.as_ref();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = log_stem(path);

    // WHY: swallowing an unreadable directory here would report whatever
    // partial segment set happened to be scannable as the WHOLE set,
    // silently downgrading a real "can't see the evidence" failure into an
    // apparent pass or an under-counted gap. Refuse instead of guessing —
    // mirrors `rotation::next_rotation_number`.
    let entries = std::fs::read_dir(dir).context(IoSnafu { path: dir })?;

    let mut numbers: Vec<u32> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            parse_rotation_number(&name.to_string_lossy(), &stem)
        })
        .collect();
    numbers.sort_unstable();
    numbers.dedup();

    let max = numbers.last().copied().unwrap_or(0);
    for expected in 1..=max {
        if numbers.binary_search(&expected).is_err() {
            return Ok(SegmentChainStatus::MissingSegment { number: expected });
        }
    }

    let mut expected_start = genesis_hash(chain_key);
    let mut total_entries: u64 = 0;
    let mut segments: u32 = 0;

    for n in 1..=max {
        let seg_path = rotation_path(path, n);
        let stream = stream_verify(&seg_path, chain_key, expected_start)?;
        let status = match stream.break_status {
            Some(status) => status,
            None => classify_terminal(read_seal(&seg_path, chain_key), stream.entries_verified),
        };

        if !segment_ok(&status, false) {
            return Ok(SegmentChainStatus::SegmentBroken {
                path: seg_path,
                status,
            });
        }

        total_entries += stream.entries_verified;
        segments += 1;
        expected_start = stream.terminal_hash;
    }

    let live = stream_verify(path, chain_key, expected_start)?;
    let live_status = match live.break_status {
        Some(status) => status,
        None => classify_terminal(read_seal(path, chain_key), live.entries_verified),
    };

    if !segment_ok(&live_status, true) {
        return Ok(SegmentChainStatus::SegmentBroken {
            path: path.to_owned(),
            status: live_status,
        });
    }
    total_entries += live.entries_verified;
    segments += 1;

    Ok(SegmentChainStatus::Intact {
        total_entries,
        segments,
    })
}
