//! Tests for single-writer locking (#226), safe seal-refresh-failure
//! recovery (#285), and cross-segment chain linkage (#211); split out from
//! `tamper_log_tests.rs` so neither file grows past the RUST/file-too-long
//! 800-line threshold.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Barrier};
use std::thread;

use compact_str::CompactString;

use super::*;

fn test_key() -> ChainKey {
    ChainKey::from_bytes([0x91; CHAIN_KEY_LEN])
}

fn vault_kind() -> LogEntryKind {
    LogEntryKind::VaultMutation {
        credential_name: CompactString::from("recovery-test-cred"),
        operation: CompactString::from("test"),
    }
}

// -----------------------------------------------------------------------
// #226: single-writer lock
// -----------------------------------------------------------------------

#[test]
fn second_open_while_writer_live_is_locked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.log");

    let _held = TamperLog::open(&path, test_key()).unwrap();

    let second = TamperLog::open(&path, test_key());
    assert!(
        matches!(second, Err(TamperLogError::Locked { .. })),
        "a second open while the first writer is still live must fail with \
         Locked instead of independently recovering the same tail"
    );
}

#[test]
fn lock_is_released_when_writer_drops() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("reopen.log");

    let first = TamperLog::open(&path, test_key()).unwrap();
    drop(first);

    let second = TamperLog::open(&path, test_key());
    assert!(
        second.is_ok(),
        "dropping the first writer must release the OS advisory lock for \
         the next opener — this also models process death: the OS \
         releases an flock the moment every fd referencing it closes, \
         whether by explicit drop, unwind, or process exit"
    );
}

#[test]
fn concurrent_opens_on_the_same_path_never_both_succeed() {
    // WHY: a purely sequential open/drop/open test cannot prove mutual
    // exclusion under real contention — two threads racing to open the
    // SAME path, released together by a Barrier, must still see exactly
    // one winner. flock() is atomic at the kernel level, so this is a
    // deterministic guarantee, not a probabilistic one.
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("race.log"));
    let barrier = Arc::new(Barrier::new(2));

    // NOTE: `array::from_fn` eagerly spawns BOTH threads before any join
    // below — a lazy iterator chain would spawn-join-spawn-join and never
    // actually race, silently gutting the test.
    let handles: [_; 2] = std::array::from_fn(|_| {
        let path = Arc::clone(&path);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            // WHY return the `Result` instead of evaluating `is_ok()`
            // here: an `Ok` handle dropped at this point releases the
            // flock immediately, so a winner scheduled well before the
            // loser would free the lock before the loser even attempts
            // its open — both would succeed under load skew. Keeping the
            // handle alive inside the join result holds the winner's lock
            // until BOTH threads have made their attempt, which is what
            // makes exactly-one-winner deterministic rather than
            // probabilistic.
            TamperLog::open(path.as_path(), test_key())
        })
    });

    // WHY join both before counting: each `Ok` handle's flock is held
    // until its `Result` drops, so both attempts must complete while the
    // winner still holds the lock.
    let results = handles.map(|h| h.join().unwrap());
    let successes: usize = results.into_iter().flatten().count();
    assert_eq!(
        successes, 1,
        "exactly one of two concurrently-racing opens on the same path must succeed"
    );
}

// -----------------------------------------------------------------------
// #285: safe recovery from a seal refresh that failed after an append
// -----------------------------------------------------------------------

#[test]
fn log_ahead_of_a_valid_seal_classifies_as_unsealed_not_truncated() {
    // WHY: manufactures the exact on-disk state a failed refresh_seal
    // leaves behind, using the real encode_entry/write_seal building
    // blocks (not a re-implementation), without depending on OS-level
    // failure-injection timing. See
    // `seal_refresh_failure_is_recoverable_after_the_fs_error_clears`
    // below for the same state reached via a genuine injected I/O
    // failure through the shipped `TamperLog::append` path.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stale-seal.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..5 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    seal::write_seal(
        &path,
        &test_key(),
        3,
        &seal::genesis_hash(&test_key()),
        None,
    )
    .unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(
        result.entries_verified, 5,
        "the stream itself is untouched and must still verify all 5 entries"
    );
    assert_eq!(
        result.status,
        ChainStatus::Unsealed {
            verified_entries: 5,
            sealed_entries: 3,
        },
        "a valid seal claiming FEWER entries than the stream must classify as \
         Unsealed (safe, resumable), not Truncated (refused) — only the \
         chain-key holder can produce entries that verify past a valid seal's \
         count"
    );
}

#[test]
fn open_resumes_an_unsealed_log_and_reseals_to_the_true_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resume-unsealed.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..5 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    seal::write_seal(
        &path,
        &test_key(),
        3,
        &seal::genesis_hash(&test_key()),
        None,
    )
    .unwrap();

    // Before the fix this was refused as ChainCompromised(Truncated); the
    // fix must resume it instead.
    let mut resumed = TamperLog::open(&path, test_key()).unwrap();
    assert_eq!(
        resumed.entry_count(),
        5,
        "must recover the TRUE tail from the log content, not the stale seal"
    );

    let after_open = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(after_open.status, ChainStatus::Intact);
    assert_eq!(after_open.entries_verified, 5);

    // The resumed handle must genuinely be usable, correctly continuing
    // the chain rather than just reporting a recovered count.
    let seq = resumed.append(vault_kind()).unwrap();
    assert_eq!(seq, 5);
    drop(resumed);

    let final_result = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(final_result.entries_verified, 6);
    assert_eq!(final_result.status, ChainStatus::Intact);
}

#[cfg(unix)]
#[test]
fn seal_refresh_failure_is_recoverable_after_the_fs_error_clears() {
    // WHY: a REAL, non-mocked I/O failure — the log's directory is made
    // non-writable so `File::create` on the seal's `.tmp` sibling fails
    // with EACCES — injected through the SHIPPED `TamperLog::append` path,
    // not a re-implementation.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("injected.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..3 {
        log.append(vault_kind()).unwrap();
    }
    assert_eq!(log.entry_count(), 3);

    let writable = fs::metadata(dir.path()).unwrap().permissions();
    let mut readonly = writable.clone();
    readonly.set_mode(0o555);
    fs::set_permissions(dir.path(), readonly).unwrap();

    // The write to the ALREADY-OPEN log fd succeeds (append-mode writes
    // don't re-check directory permissions), but refresh_seal's
    // `File::create` on a NEW `.tmp` sibling fails: directory write
    // permission is what gates creating a new directory entry.
    let append_result = log.append(vault_kind());
    assert!(
        matches!(append_result, Err(TamperLogError::Io { .. })),
        "append must surface the seal-write failure rather than silently \
         swallowing it, got {append_result:?}"
    );
    drop(log);

    fs::set_permissions(dir.path(), writable).unwrap();

    // While the directory was still read-only, the on-disk state was
    // exactly the log-ahead-of-seal shape — confirm it classifies as
    // Unsealed even reached via a genuine injected failure, not just the
    // manufactured seal in the tests above.
    let mid_state = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(mid_state.entries_verified, 4);
    assert_eq!(
        mid_state.status,
        ChainStatus::Unsealed {
            verified_entries: 4,
            sealed_entries: 3,
        }
    );

    // Reopening after the transient fs error clears must resume, not
    // refuse, and must re-seal to the true count.
    let resumed = TamperLog::open(&path, test_key()).unwrap();
    assert_eq!(resumed.entry_count(), 4);
    drop(resumed);

    let after = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(after.status, ChainStatus::Intact);
    assert_eq!(after.entries_verified, 4);
}

#[test]
fn a_seal_destroyed_by_a_failed_rename_stays_fail_closed() {
    // WHY: rename() and create() share the same directory-write
    // precondition on POSIX, so a permission-based injection cannot
    // isolate the rename stage from create without also blocking create.
    // Isolating rename specifically requires replacing the seal's target
    // with something rename can't overwrite (a directory) — which
    // necessarily destroys whatever valid seal was there first. The
    // correct behavior for THAT injected failure is refusal, not
    // resumption: an absent/unreadable seal is indistinguishable from one
    // an attacker removed, so it stays fail-closed exactly like
    // `Truncated` — Unsealed's leniency applies only to a seal that is
    // still validly authenticated, just behind.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rename-destroyed.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..2 {
        log.append(vault_kind()).unwrap();
    }

    let seal_target = seal::seal_path(&path);
    fs::remove_file(&seal_target).unwrap();
    fs::create_dir(&seal_target).unwrap();

    let append_result = log.append(vault_kind());
    assert!(
        matches!(append_result, Err(TamperLogError::Io { .. })),
        "the rename-stage failure must surface as an Io error, got {append_result:?}"
    );
    drop(log);
    fs::remove_dir(&seal_target).unwrap();

    let reopened = TamperLog::open(&path, test_key());
    // WHY not `{reopened:?}`: `TamperLog` intentionally has no `Debug`
    // impl (it holds the `ChainKey`; koinon follows
    // RUST/no-debug-derive-on-public-types), so format the outcome without
    // naming the Ok payload.
    let got = match &reopened {
        Ok(_) => "Ok(TamperLog)".to_owned(),
        Err(e) => format!("Err({e:?})"),
    };
    assert!(
        matches!(reopened, Err(TamperLogError::ChainCompromised { .. })),
        "a log whose seal is absent must stay refused even though the \
         content itself is fully valid — an absent seal cannot be told \
         apart from one an attacker deleted, got {got}"
    );
}

// -----------------------------------------------------------------------
// #285: every seal stage fails deterministically, and recovers
// -----------------------------------------------------------------------

/// Appends one entry with `stage` armed to fail, and returns the error the
/// shipped `append` path surfaced.
///
/// Clears the injection unconditionally, so a failure to reach the
/// assertion cannot leave a later test in this thread armed.
fn append_failing_at(log: &mut TamperLog, stage: seal::inject::SealStage) -> TamperLogError {
    seal::inject::fail_at(stage);
    let result = log.append(vault_kind());
    seal::inject::clear();
    match result {
        Err(error) => error,
        Ok(_) => panic!("append must surface the injected {stage:?} failure, got Ok"),
    }
}

/// Drives a log to `entries` appends, each verified.
fn seeded_log(path: &std::path::Path, entries: usize) -> TamperLog {
    let mut log = TamperLog::open(path, test_key()).unwrap();
    for _ in 0..entries {
        log.append(vault_kind()).unwrap();
    }
    log
}

/// The three stages that run before the rename all leave the previous seal
/// in place, so each produces the same recoverable log-ahead state. Covering
/// them individually is the point: they are separate failure sites with
/// separate error paths, and a test that only ever exercises `create` cannot
/// tell whether `write` and `sync` propagate at all or silently succeed.
#[test]
fn every_pre_rename_seal_stage_failure_recovers_on_reopen() {
    for (index, stage) in [
        seal::inject::SealStage::Create,
        seal::inject::SealStage::Write,
        seal::inject::SealStage::Sync,
    ]
    .into_iter()
    .enumerate()
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("stage-{index}.log"));

        let mut log = seeded_log(&path, 3);
        let error = append_failing_at(&mut log, stage);
        assert!(
            matches!(error, TamperLogError::Io { .. }),
            "{stage:?} must surface as an Io error, got {error:?}"
        );
        drop(log);

        // The seal that was already on disk is untouched and still
        // authenticates — this is what separates a torn commit from an
        // attack, and it is the precondition for recovering rather than
        // refusing.
        assert_eq!(
            seal::read_seal(&path, &test_key()),
            seal::SealState::Valid {
                entry_count: 3,
                segment_start_hash: seal::genesis_hash(&test_key()),
                provenance: None,
            },
            "{stage:?} must leave the previous seal intact and valid"
        );

        let mid = verify_chain(&path, &test_key()).unwrap();
        assert_eq!(
            mid.status,
            ChainStatus::Unsealed {
                verified_entries: 4,
                sealed_entries: 3,
            },
            "{stage:?} must leave a log ahead of its seal, not a truncated one"
        );

        let resumed = TamperLog::open(&path, test_key()).unwrap();
        assert_eq!(
            resumed.entry_count(),
            4,
            "{stage:?} must resume at the true count"
        );
        drop(resumed);

        let after = verify_chain(&path, &test_key()).unwrap();
        assert_eq!(after.status, ChainStatus::Intact, "{stage:?} must reseal");
        assert_eq!(after.entries_verified, 4);
    }
}

/// The case `a_seal_destroyed_by_a_failed_rename_stays_fail_closed` cannot
/// reach: that fixture has to replace the seal with a directory to make
/// `rename` fail from outside, which destroys the stale seal along with it,
/// so it proves the fail-closed branch and says nothing about the
/// recoverable one. A rename that simply does not happen leaves the previous
/// seal exactly where it was, which is the realistic crash shape and must
/// recover.
#[test]
fn a_failed_rename_preserves_the_previous_seal_and_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rename-preserved.log");

    let mut log = seeded_log(&path, 2);
    let error = append_failing_at(&mut log, seal::inject::SealStage::Rename);
    assert!(
        matches!(error, TamperLogError::Io { .. }),
        "a rename-stage failure must surface as an Io error, got {error:?}"
    );
    drop(log);

    assert!(
        seal::seal_path(&path).exists(),
        "the previous seal must survive a rename that never ran"
    );
    assert_eq!(
        seal::read_seal(&path, &test_key()),
        seal::SealState::Valid {
            entry_count: 2,
            segment_start_hash: seal::genesis_hash(&test_key()),
            provenance: None,
        },
        "and must still authenticate at its old count"
    );

    let resumed = TamperLog::open(&path, test_key()).unwrap();
    assert_eq!(resumed.entry_count(), 3);
    drop(resumed);

    assert_eq!(
        verify_chain(&path, &test_key()).unwrap().status,
        ChainStatus::Intact
    );
}

/// The directory sync runs after the rename has already published the new
/// seal, so this failure reports an error over a state that is in fact fully
/// sealed. The log must not be left worse off by the error it reported.
#[test]
fn a_failed_directory_sync_leaves_a_fully_sealed_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dirsync.log");

    let mut log = seeded_log(&path, 2);
    let error = append_failing_at(&mut log, seal::inject::SealStage::DirSync);
    assert!(
        matches!(error, TamperLogError::Io { .. }),
        "a directory-sync failure must surface as an Io error, got {error:?}"
    );
    drop(log);

    let state = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(
        state.status,
        ChainStatus::Intact,
        "the rename had already published the new seal, so the log is sealed"
    );
    assert_eq!(state.entries_verified, 3);

    let resumed = TamperLog::open(&path, test_key()).unwrap();
    assert_eq!(resumed.entry_count(), 3);
}

/// The security bound on `Unsealed`'s leniency. Resuming a log that is ahead
/// of a valid seal is safe only because every extra entry is verified first;
/// if that check were dropped, the same leniency would launder an appended
/// forgery. A stale-but-valid seal with a corrupted entry beyond it must be
/// refused, exactly like a truncated log.
#[test]
fn a_stale_seal_with_a_corrupted_extra_link_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt-suffix.log");

    let mut log = seeded_log(&path, 3);
    let sealed_len = fs::metadata(&path).unwrap().len();
    append_failing_at(&mut log, seal::inject::SealStage::Create);
    drop(log);

    // Precondition: without the corruption this state recovers. Asserting it
    // here is what makes the refusal below attributable to the corrupted
    // link rather than to the log-ahead state itself.
    assert!(
        matches!(
            verify_chain(&path, &test_key()).unwrap().status,
            ChainStatus::Unsealed { .. }
        ),
        "the uncorrupted log-ahead state must be the recoverable one"
    );

    // Corrupt a byte inside the fourth entry — the one beyond the seal.
    let mut bytes = fs::read(&path).unwrap();
    let target = usize::try_from(sealed_len).unwrap() + 8;
    bytes[target] ^= 0xFF;
    fs::write(&path, &bytes).unwrap();

    let reopened = TamperLog::open(&path, test_key());
    let got = match &reopened {
        Ok(_) => "Ok(TamperLog)".to_owned(),
        Err(error) => format!("Err({error:?})"),
    };
    assert!(
        matches!(reopened, Err(TamperLogError::ChainCompromised { .. })),
        "an unverifiable entry beyond a valid seal must be refused, not \
         resumed — otherwise the log-ahead allowance launders a forged \
         append, got {got}"
    );
}

// -----------------------------------------------------------------------
// #211: cross-segment chain linkage survives rotation
// -----------------------------------------------------------------------

#[test]
fn rotation_carries_the_chain_and_segment_set_verifies_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("linked.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(200);
    for _ in 0..20 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    assert!(
        dir.path().join("linked.1.log").exists(),
        "test setup: rotation must have actually triggered"
    );

    let result = verify_segment_chain(&path, &test_key()).unwrap();
    match result {
        SegmentChainStatus::Intact {
            total_entries,
            segments,
        } => {
            assert_eq!(
                total_entries, 20,
                "every entry across every segment must be counted"
            );
            assert!(
                segments >= 2,
                "must have walked at least one rotated segment plus the live file"
            );
        }
        other => panic!("expected Intact, got {other:?}"),
    }
}

#[test]
fn deleting_a_rotated_segment_breaks_the_segment_chain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete-mid.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(150);
    for _ in 0..40 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    assert!(
        dir.path().join("delete-mid.2.log").exists(),
        "test setup: needs at least two rotations"
    );

    // Delete the first rotated segment (and its seal) outright — the
    // canonical attack: an adversary erases a whole segment file to
    // remove a window of history.
    let victim = dir.path().join("delete-mid.1.log");
    fs::remove_file(&victim).unwrap();
    let _ = fs::remove_file(seal::seal_path(&victim));

    let result = verify_segment_chain(&path, &test_key()).unwrap();
    assert!(
        !matches!(result, SegmentChainStatus::Intact { .. }),
        "deleting a rotated segment must not verify as Intact, got {result:?}"
    );
}

#[test]
fn deleting_the_most_recent_rotated_segment_is_still_caught() {
    // WHY: the segment right before the live file leaves NO numeric gap
    // when deleted (the remaining numbers stay contiguous 1..=max-1) —
    // the case a pure directory-listing gap check would miss. Only the
    // cross-segment hash link catches it: the live file's real first
    // entry was chained from the deleted segment's TRUE terminal hash,
    // which no surviving segment can supply.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("delete-last.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(150);
    for _ in 0..40 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    let mut n = 1u32;
    while dir
        .path()
        .join(format!("delete-last.{}.log", n + 1))
        .exists()
    {
        n += 1;
    }
    let highest = dir.path().join(format!("delete-last.{n}.log"));
    assert!(
        highest.exists(),
        "test setup: expected at least one rotated segment"
    );
    fs::remove_file(&highest).unwrap();
    let _ = fs::remove_file(seal::seal_path(&highest));

    let result = verify_segment_chain(&path, &test_key()).unwrap();
    assert!(
        !matches!(result, SegmentChainStatus::Intact { .. }),
        "deleting the segment immediately before the live file must still be \
         caught via the broken cross-segment link, got {result:?}"
    );
}

#[test]
fn truncating_the_live_files_tail_is_caught_by_segment_verification() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("truncate-live.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(150);
    for _ in 0..25 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    let data = fs::read(&path).unwrap();
    assert!(
        !data.is_empty(),
        "test setup: live file must have its own entries"
    );
    fs::write(&path, &data[..data.len().saturating_sub(20)]).unwrap();

    let result = verify_segment_chain(&path, &test_key()).unwrap();
    assert!(
        !matches!(result, SegmentChainStatus::Intact { .. }),
        "truncating the live file's own tail must be caught even though \
         cross-segment linkage into it is untouched, got {result:?}"
    );
}

#[test]
fn never_rotated_log_verifies_via_segment_chain_same_as_verify_chain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("never-rotated.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..4 {
        log.append(vault_kind()).unwrap();
    }
    drop(log);

    let result = verify_segment_chain(&path, &test_key()).unwrap();
    assert_eq!(
        result,
        SegmentChainStatus::Intact {
            total_entries: 4,
            segments: 1,
        },
        "a single-file, never-rotated log must verify exactly like verify_chain alone"
    );
}
