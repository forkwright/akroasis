//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use compact_str::CompactString;
use ulid::Ulid;

use super::*;
use crate::{EntityId, SignalId};

fn test_key() -> ChainKey {
    ChainKey::from_bytes([0x5A; CHAIN_KEY_LEN])
}

fn other_key() -> ChainKey {
    ChainKey::from_bytes([0xC3; CHAIN_KEY_LEN])
}

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

fn vault_mutation_kind() -> LogEntryKind {
    LogEntryKind::VaultMutation {
        credential_name: CompactString::from("incident-radio-key"),
        operation: CompactString::from("rotate"),
    }
}

// -----------------------------------------------------------------------
// Core functionality
// -----------------------------------------------------------------------

#[test]
fn append_single_entry_and_verify_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    let seq = log.append(signal_kind()).unwrap();
    assert_eq!(seq, 0);
    assert_eq!(log.entry_count(), 1);

    let result = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(result.entries_verified, 1);
    assert_eq!(result.status, ChainStatus::Intact);
}

#[test]
fn append_100_entries_chain_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for i in 0..100_u64 {
        let seq = log.append(alert_kind()).unwrap();
        assert_eq!(seq, i);
    }

    let result = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(result.entries_verified, 100);
    assert_eq!(result.status, ChainStatus::Intact);
}

#[test]
fn empty_file_returns_empty_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.log");
    File::create(&path).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert_eq!(result.status, ChainStatus::Empty);
    assert_eq!(result.entries_verified, 0);
}

#[test]
fn recovery_continues_chain_correctly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recover.log");

    {
        let mut log = TamperLog::open(&path, test_key()).unwrap();
        for _ in 0..5 {
            log.append(action_kind()).unwrap();
        }
    }

    {
        let mut log = TamperLog::open(&path, test_key()).unwrap();
        assert_eq!(log.entry_count(), 5);
        for _ in 0..5 {
            log.append(config_kind()).unwrap();
        }
    }

    let result = verify_chain(&path, &test_key()).unwrap();
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

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..10 {
        log.append(signal_kind()).unwrap();
    }
    drop(log);

    let mut data = std::fs::read(&path).unwrap();
    let off = entry_offset(&data, 5);
    // Flip a byte inside the CBOR payload (byte 4 = first CBOR byte).
    data[off + 4] ^= 0xFF;
    std::fs::write(&path, &data).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(matches!(
        result.status,
        ChainStatus::Broken { sequence: 5, .. }
    ));
}

#[test]
fn flip_byte_in_hash_detected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tamper_hash.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
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

    let result = verify_chain(&path, &test_key()).unwrap();
    // Entry 3's stored hash is wrong → broken at entry 3.
    assert!(matches!(result.status, ChainStatus::Broken { .. }));
}

#[test]
fn truncated_file_returns_corrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("truncate.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..10 {
        log.append(config_kind()).unwrap();
    }
    drop(log);

    let data = std::fs::read(&path).unwrap();
    let truncated = &data[..data.len() - 20];
    std::fs::write(&path, truncated).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(matches!(result.status, ChainStatus::Corrupted { .. }));
}

#[test]
fn zero_out_last_hash_detected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("zerohash.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
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

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(matches!(result.status, ChainStatus::Broken { .. }));
}

// -----------------------------------------------------------------------
// Truncation & keying (akroasis#213)
// -----------------------------------------------------------------------

#[test]
fn removing_final_entry_reports_non_intact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("truncate_tail.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..10 {
        log.append(alert_kind()).unwrap();
    }
    drop(log);

    let data = std::fs::read(&path).unwrap();
    let cutoff = entry_offset(&data, 9);
    std::fs::write(&path, &data[..cutoff]).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(
        !matches!(result.status, ChainStatus::Intact),
        "removing the final entry must not verify as Intact"
    );
    assert!(
        matches!(
            result.status,
            ChainStatus::Truncated {
                sealed_entries: Some(10)
            }
        ),
        "expected Truncated{{sealed_entries: Some(10)}}, got {:?}",
        result.status
    );
}

#[test]
fn wiping_all_entries_is_not_reported_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wipe_all.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..5 {
        log.append(config_kind()).unwrap();
    }
    drop(log);

    std::fs::write(&path, []).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert_ne!(
        result.status,
        ChainStatus::Empty,
        "wiping all entries while the seal still claims 5 must not read as Empty"
    );
    assert!(matches!(
        result.status,
        ChainStatus::Truncated {
            sealed_entries: Some(5)
        }
    ));
}

#[test]
fn open_refuses_to_resume_a_truncated_chain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resume_truncated.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..6 {
        log.append(action_kind()).unwrap();
    }
    drop(log);

    let data = std::fs::read(&path).unwrap();
    let cutoff = entry_offset(&data, 5);
    std::fs::write(&path, &data[..cutoff]).unwrap();

    let result = TamperLog::open(&path, test_key());
    assert!(
        matches!(result, Err(TamperLogError::ChainCompromised { .. })),
        "opening a chain whose tail was truncated must refuse to resume, not silently launder it"
    );
}

#[test]
fn verify_with_wrong_key_rejects_chain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wrong_key.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..5 {
        log.append(signal_kind()).unwrap();
    }
    drop(log);

    let result = verify_chain(&path, &other_key()).unwrap();
    assert!(
        !matches!(result.status, ChainStatus::Intact),
        "verifying with the wrong key must not report Intact"
    );
}

#[test]
fn forged_unkeyed_chain_rejected_by_keyed_verification() {
    // WHY: simulates an attacker without the chain key, who can only
    // recompute the OLD unkeyed-BLAKE3-over-a-public-genesis scheme this
    // module used before akroasis#213.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forged_unkeyed.log");

    let entry = LogEntry {
        sequence: 0,
        timestamp_ms: 0,
        kind: signal_kind(),
    };
    let mut cbor_bytes = Vec::new();
    ciborium::into_writer(&entry, &mut cbor_bytes).unwrap();

    let prev_hash = [0u8; 32]; // the old public genesis constant
    let mut hasher = blake3::Hasher::new(); // the old UNKEYED hasher
    hasher.update(&cbor_bytes);
    hasher.update(&prev_hash);
    let forged_hash: [u8; 32] = hasher.finalize().into();

    let mut wire = Vec::new();
    wire.extend_from_slice(&(u32::try_from(cbor_bytes.len()).unwrap()).to_le_bytes());
    wire.extend_from_slice(&cbor_bytes);
    wire.extend_from_slice(&forged_hash);
    std::fs::write(&path, &wire).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(
        matches!(result.status, ChainStatus::Broken { .. }),
        "a chain forged with the old unkeyed scheme must not validate under keyed verification"
    );
}

#[test]
fn forged_seal_without_key_is_not_trusted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("forged_seal.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    for _ in 0..8 {
        log.append(entity_kind()).unwrap();
    }
    drop(log);

    // Attacker truncates off the last 3 entries...
    let data = std::fs::read(&path).unwrap();
    let cutoff = entry_offset(&data, 5);
    std::fs::write(&path, &data[..cutoff]).unwrap();

    // ...and, lacking the real chain key, forges a replacement seal
    // claiming the new (truncated) count of 5 under a key they guessed.
    let forged_key = other_key();
    let mut mac_hasher = blake3::Hasher::new_keyed(forged_key.as_bytes());
    mac_hasher.update(b"koinon/tamper-log/seal/v1");
    mac_hasher.update(&5_u64.to_le_bytes());
    let forged_mac: [u8; 32] = mac_hasher.finalize().into();

    let mut forged_seal = Vec::new();
    forged_seal.extend_from_slice(&5_u64.to_le_bytes());
    forged_seal.extend_from_slice(&forged_mac);
    std::fs::write(seal::seal_path(&path), &forged_seal).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(
        !matches!(result.status, ChainStatus::Intact),
        "a forged seal claiming a matching count under the wrong key must not be trusted as Intact"
    );
}

// -----------------------------------------------------------------------
// Rotation
// -----------------------------------------------------------------------

#[test]
fn rotation_triggers_at_threshold() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rotate.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(500);
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

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(200);
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

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(300);
    for _ in 0..20 {
        log.append(signal_kind()).unwrap();
    }
    drop(log);

    let result = verify_chain(&path, &test_key()).unwrap();
    assert!(
        matches!(result.status, ChainStatus::Intact | ChainStatus::Empty),
        "new file must be intact or empty"
    );
}

#[test]
fn pre_rotation_file_verifies_independently() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pre.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(300);
    for _ in 0..20 {
        log.append(entity_kind()).unwrap();
    }
    drop(log);

    let rotated = dir.path().join("pre.1.log");
    if rotated.exists() {
        let result = verify_chain(&rotated, &test_key()).unwrap();
        assert_eq!(result.status, ChainStatus::Intact);
    }
}

#[test]
fn multiple_rotations_sequential_numbering() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multi.log");

    let mut log = TamperLog::open(&path, test_key())
        .unwrap()
        .with_max_file_bytes(150);
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

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    log.append(signal_kind()).unwrap();
    drop(log);

    let result = verify_chain(&path, &test_key()).unwrap();
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
    let (wire, _) = encode_entry(&entry, &prev, &test_key()).unwrap();
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
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
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
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
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
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
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
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
    let (decoded, _) = decode_entry(&wire).unwrap();
    assert_eq!(entry, decoded);
}

#[test]
fn cbor_roundtrip_vault_mutation() {
    let entry = LogEntry {
        sequence: 5,
        timestamp_ms: 6_000_000,
        kind: vault_mutation_kind(),
    };
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
    let (decoded, _) = decode_entry(&wire).unwrap();
    assert_eq!(entry, decoded);
}

#[test]
fn entry_too_large_guard_rejects_oversized_length_prefix() {
    // WHY: a length prefix claiming u32::MAX bytes must be rejected by the
    // MAX_ENTRY_BYTES sanity guard BEFORE decode_entry attempts the
    // corresponding `vec![0u8; payload_len]` allocation. If the guard were
    // removed, this would instead surface as a Corrupted (short-read) or
    // an out-of-memory abort, not EntryTooLarge.
    let bytes = u32::MAX.to_le_bytes();

    let result = decode_entry(&bytes);
    assert!(
        matches!(
            result,
            Err(TamperLogError::EntryTooLarge { max, .. }) if max == MAX_ENTRY_BYTES
        ),
        "expected EntryTooLarge{{max: {MAX_ENTRY_BYTES}, ..}}, got {result:?}"
    );
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
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
    let (decoded, _) = decode_entry(&wire).unwrap();
    assert_eq!(entry, decoded);
}

#[test]
fn id_types_survive_a_round_trip_through_entry_kinds() {
    // WHY: this only constructed the two kinds and dropped them with `let _`,
    // so it asserted nothing and passed even if the ids were silently replaced.
    // Assert the ids come back out of a full encode/decode instead.
    let sid = SignalId::from_ulid(Ulid::generate());
    let eid = EntityId::from_ulid(Ulid::generate());

    for (kind, label) in [
        (
            LogEntryKind::SignalObserved {
                signal_id: sid,
                kind_tag: CompactString::from("t"),
            },
            "signal",
        ),
        (
            LogEntryKind::EntityCreated {
                entity_id: eid,
                kind_tag: CompactString::from("t"),
            },
            "entity",
        ),
    ] {
        let entry = LogEntry {
            sequence: 0,
            timestamp_ms: 0,
            kind,
        };
        let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
        let (decoded, _) = decode_entry(&wire).unwrap();
        assert_eq!(entry, decoded, "{label} id did not survive the round trip");
    }
}

#[test]
fn configured_rotation_observably_changes_trigger() {
    // WHY: parameterization-observability test — open_with_config(max=300)
    // must trigger rotation after fewer entries than the 100 MiB default
    // would.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.log");

    let cfg = TamperLogConfig {
        max_file_bytes: 300,
    };
    let mut log = TamperLog::open_with_config(&path, test_key(), &cfg).unwrap();
    for _ in 0..30 {
        log.append(alert_kind()).unwrap();
    }
    drop(log);

    assert!(
        dir.path().join("cfg.1.log").exists(),
        "300-byte threshold must force rotation; default 100 MiB would not"
    );
}

#[test]
fn tamper_log_config_toml_roundtrip() {
    // WHY: the config must survive TOML round-trip so operators and
    // agents can express the tuning in the same file that configures
    // the rest of the service.
    let cfg = TamperLogConfig {
        max_file_bytes: 1_048_576,
    };
    let toml_str = toml::to_string(&cfg).unwrap();
    let parsed: TamperLogConfig = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.max_file_bytes, 1_048_576);
}

#[test]
fn tamper_log_config_empty_toml_uses_default() {
    // WHY: an empty or missing [tamper_log] section must fall through
    // to the default so bootstrap-from-nothing is possible.
    let parsed: TamperLogConfig = toml::from_str("").unwrap();
    assert_eq!(parsed.max_file_bytes, DEFAULT_MAX_FILE_BYTES);
}
