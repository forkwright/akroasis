//! Integration test: vault mutations append to the tamper-evident audit log.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "integration test — panics are the correct failure mode"
)]

use koinon::ChainStatus;
use kryphos::{CredentialType, Vault};

const TEST_PASSPHRASE: &[u8] = b"correct horse battery staple";

/// Walks wire-format bytes and returns the byte offset of entry `target_idx`.
///
/// Mirrors `koinon::tamper_log`'s own private test helper — kryphos has no
/// access to it, and the wire format (`[4-byte LE len][cbor][32-byte hash]`)
/// is part of the documented on-disk contract, not an implementation detail.
fn entry_offset(data: &[u8], target_idx: usize) -> usize {
    let mut offset = 0usize;
    for _ in 0..target_idx {
        let len = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4 + len + 32;
    }
    offset
}

#[test]
fn vault_mutations_append_intact_tamper_log() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("audited-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    vault
        .add("incident-radio-key", CredentialType::RadioKey, b"secret-v1")
        .unwrap();
    vault.rotate("incident-radio-key", b"secret-v2").unwrap();
    vault.revoke("incident-radio-key").unwrap();

    let result = vault.verify_tamper_log().unwrap();
    assert_eq!(result.status, ChainStatus::Intact);
    assert_eq!(result.entries_verified, 3);
}

#[test]
fn truncated_vault_audit_log_is_detected() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("truncate-audit-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("k1", CredentialType::ApiKey, b"v1").unwrap();
    vault.add("k2", CredentialType::ApiKey, b"v2").unwrap();
    vault.rotate("k1", b"v1-new").unwrap();

    let before = vault.verify_tamper_log().unwrap();
    assert_eq!(before.status, ChainStatus::Intact);
    assert_eq!(before.entries_verified, 3);

    // Simulate an adversary with host/log write access deleting the final
    // (most recent) entry — the canonical attack against an incident
    // audit trail.
    let log_path = vault.tamper_log_path();
    let mut data = std::fs::read(&log_path).unwrap();
    let cutoff = entry_offset(&data, 2);
    data.truncate(cutoff);
    std::fs::write(&log_path, &data).unwrap();

    let after = vault.verify_tamper_log().unwrap();
    assert!(
        !matches!(after.status, ChainStatus::Intact),
        "a truncated vault audit log must not verify as Intact, got {:?}",
        after.status
    );
}
