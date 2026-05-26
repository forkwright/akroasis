//! Integration test: signing tamper log entries with an installation identity.

#![expect(
    clippy::unwrap_used,
    reason = "integration test — panics are the correct failure mode"
)]

use compact_str::CompactString;

use koinon::tamper_log::{LogEntry, LogEntryKind, TamperLog, encode_entry};
use kryphos::InstallationIdentity;

#[test]
fn sign_tamper_log_entry_and_verify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("signed.log");

    let identity = InstallationIdentity::generate();
    let mut log = TamperLog::open(&path).unwrap();

    let kind = LogEntryKind::ActionTaken {
        actor: CompactString::from("operator"),
        action: CompactString::from("deploy"),
        target: Some(CompactString::from("node-1")),
    };

    let seq = log.append(kind.clone()).unwrap();
    assert_eq!(seq, 0, "first entry should be sequence 0");

    // Reconstruct the entry to get its hash (simulates what a caller would do).
    let entry = LogEntry {
        sequence: 0,
        timestamp_ms: 0, // hash depends on CBOR bytes, but we use last_hash() instead
        kind,
    };
    let _ = entry; // entry reconstruction shown for documentation

    // Use the log's last_hash, which is the actual on-disk entry hash.
    let entry_hash = log.last_hash();
    let signature = identity.sign_entry(entry_hash);

    assert!(
        identity.verify(entry_hash, &signature).is_ok(),
        "signature on entry hash must verify with the same identity"
    );

    // A different identity must not verify.
    let other = InstallationIdentity::generate();
    assert!(
        other.verify(entry_hash, &signature).is_err(),
        "signature must not verify with a different identity"
    );
}

#[test]
fn sign_multiple_entries_each_verifiable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multi_signed.log");

    let identity = InstallationIdentity::generate();
    let mut log = TamperLog::open(&path).unwrap();

    let mut signatures = Vec::new();
    let mut hashes = Vec::new();

    for i in 0..5_u64 {
        let kind = LogEntryKind::ConfigChanged {
            key: CompactString::from(format!("key-{i}")),
            old_value: None,
            new_value: CompactString::from(format!("val-{i}")),
        };
        log.append(kind).unwrap();

        let hash = *log.last_hash();
        let sig = identity.sign_entry(&hash);
        hashes.push(hash);
        signatures.push(sig);
    }

    for (hash, sig) in hashes.iter().zip(&signatures) {
        assert!(
            identity.verify(hash, sig).is_ok(),
            "each signed entry hash must verify"
        );
    }
}

#[test]
fn encode_entry_hash_matches_sign_entry_input() {
    let identity = InstallationIdentity::generate();
    let prev_hash = [0u8; 32];

    let entry = LogEntry {
        sequence: 0,
        timestamp_ms: 1_000_000,
        kind: LogEntryKind::AlertRaised {
            alert_id: CompactString::from("ALT-001"),
            severity: CompactString::from("warning"),
            message: CompactString::from("test alert"),
        },
    };

    let (_wire, entry_hash) = encode_entry(&entry, &prev_hash).unwrap();
    let signature = identity.sign_entry(&entry_hash);

    assert!(
        identity.verify(&entry_hash, &signature).is_ok(),
        "sign_entry on encode_entry hash must verify"
    );
}
