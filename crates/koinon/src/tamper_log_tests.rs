//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use compact_str::CompactString;
use ulid::Ulid;

use super::*;

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
    let mut log = TamperLog::open_with_config(&path, &cfg).unwrap();
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
