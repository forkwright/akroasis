//! Codec and framing tests for [`super`]; split out to keep
//! `tamper_log_tests.rs` under the RUST/file-too-long 800-line threshold.

use compact_str::CompactString;

use super::*;

fn test_key() -> ChainKey {
    ChainKey::from_bytes([0x5A; CHAIN_KEY_LEN])
}

fn config_kind() -> LogEntryKind {
    LogEntryKind::ConfigChanged {
        key: CompactString::from("threshold"),
        old_value: Some(CompactString::from("10")),
        new_value: CompactString::from("20"),
    }
}

#[test]
fn verify_chain_reports_corrupted_on_an_oversized_length_prefix() {
    // WHY: decode_entry's MAX_ENTRY_BYTES guard is covered above, but
    // verify_chain carries its OWN copy of that bound and walks the file
    // itself. Without this, removing verify_chain's guard is invisible.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("oversized.log");

    let mut log = TamperLog::open(&path, test_key()).unwrap();
    log.append(config_kind()).unwrap();
    drop(log);

    // Overwrite the first entry's 4-byte LE length prefix with u32::MAX.
    let mut data = std::fs::read(&path).unwrap();
    data.splice(0..4, u32::MAX.to_le_bytes());
    std::fs::write(&path, &data).unwrap();

    let result = verify_chain(&path, &test_key()).unwrap();

    // WARNING: assert the OFFSET, not just the variant. Without the guard,
    // verify_chain still reports Corrupted — but only after attempting a 4 GiB
    // `vec![0u8; payload_len]` and hitting EOF, which reports byte_offset 4.
    // The guard reports byte_offset 0, so only this distinguishes them.
    assert!(
        matches!(result.status, ChainStatus::Corrupted { byte_offset: 0 }),
        "expected Corrupted at byte_offset 0 (the guard), got {:?}",
        result.status
    );
}

#[test]
fn decode_entry_reports_corrupted_when_the_payload_is_short() {
    // A length prefix promising 64 bytes with only 8 present.
    let mut bytes = 64_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0xAA; 8]);

    let result = decode_entry(&bytes);

    assert!(
        matches!(result, Err(TamperLogError::Corrupted { offset: 4 })),
        "expected Corrupted at the payload offset, got {result:?}"
    );
}

#[test]
fn decode_entry_reports_corrupted_when_the_hash_is_missing() {
    // Well-formed length + payload, but the trailing 32-byte hash is absent.
    let entry = LogEntry {
        sequence: 0,
        timestamp_ms: 0,
        kind: config_kind(),
    };
    let (wire, _) = encode_entry(&entry, &[0u8; 32], &test_key()).unwrap();
    let without_hash = wire.get(..wire.len() - 32).unwrap();

    let result = decode_entry(without_hash);

    assert!(
        matches!(result, Err(TamperLogError::Corrupted { .. })),
        "expected Corrupted for a missing hash, got {result:?}"
    );
}

#[test]
fn decode_entry_reports_cbor_decode_on_an_undecodable_payload() {
    // Correct framing, but the payload is not valid CBOR for a LogEntry.
    let payload = [0xFFu8; 16];
    let mut bytes = 16_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&payload);
    bytes.extend_from_slice(&[0u8; 32]);

    let result = decode_entry(&bytes);

    assert!(
        matches!(result, Err(TamperLogError::CborDecode { .. })),
        "expected CborDecode for a non-CBOR payload, got {result:?}"
    );
}

#[test]
fn encode_entry_refuses_a_payload_over_the_decoder_bound() {
    // WHY: encode_entry's SAFETY note claimed the payload was "validated <=
    // MAX_ENTRY_BYTES" while nothing checked it, so the writer could emit an
    // entry that decode_entry and verify_chain then rejected — a log made
    // permanently unverifiable by its own writer.
    let oversized = "x".repeat(usize::try_from(MAX_ENTRY_BYTES).unwrap() + 1);
    let entry = LogEntry {
        sequence: 0,
        timestamp_ms: 0,
        kind: LogEntryKind::AlertRaised {
            alert_id: CompactString::from("BIG"),
            severity: CompactString::from("info"),
            message: CompactString::from(oversized.as_str()),
        },
    };

    let result = encode_entry(&entry, &[0u8; 32], &test_key());

    assert!(
        matches!(
            result,
            Err(TamperLogError::EntryTooLarge { max, .. }) if max == MAX_ENTRY_BYTES
        ),
        "expected EntryTooLarge, got {:?}",
        result.map(|(w, _)| w.len())
    );
}
