//! Integration test: vault mutations append to the tamper-evident audit log.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "integration test — panics are the correct failure mode"
)]

use koinon::{ChainStatus, LogEntryKind};
use kryphos::{CredentialType, Vault, VaultError};

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
    // Separate credential: `remove` refuses a revoked entry (audit trail
    // preservation, see storage.rs), so exercising it needs a credential
    // that was never revoked.
    vault
        .add(
            "incident-backup-key",
            CredentialType::RadioKey,
            b"secret-v3",
        )
        .unwrap();
    vault.remove("incident-backup-key").unwrap();

    let result = vault.verify_tamper_log().unwrap();
    assert_eq!(result.status, ChainStatus::Intact);
    assert_eq!(result.entries_verified, 5);

    let data = std::fs::read(vault.tamper_log_path()).unwrap();
    let expected = [
        ("incident-radio-key", "add"),
        ("incident-radio-key", "rotate"),
        ("incident-radio-key", "revoke"),
        ("incident-backup-key", "add"),
        ("incident-backup-key", "remove"),
    ];
    for (idx, (name, operation)) in expected.into_iter().enumerate() {
        let offset = entry_offset(&data, idx);
        let (entry, _hash) = koinon::tamper_log::decode_entry(&data[offset..]).unwrap();
        match entry.kind {
            LogEntryKind::VaultMutation {
                credential_name,
                operation: logged_operation,
            } => {
                assert_eq!(
                    credential_name, name,
                    "entry {idx} credential_name mismatch"
                );
                assert_eq!(
                    logged_operation, operation,
                    "entry {idx} operation mismatch"
                );
            }
            other => panic!("entry {idx}: expected VaultMutation, got {other:?}"),
        }
    }
}

#[test]
fn corrupted_tamper_log_blocks_further_vault_mutations() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("failure-path-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("setup-cred", CredentialType::ApiKey, b"v0")
        .unwrap();
    vault
        .add("setup-cred-2", CredentialType::ApiKey, b"v1")
        .unwrap();

    // Delete the most recent entry (same attack shape as
    // `truncated_vault_audit_log_is_detected` above): the sidecar seal
    // still authenticates 2 entries, so re-opening the log for a further
    // append sees a streamed-but-short chain and must refuse to resume it
    // rather than silently laundering the tampering.
    let log_path = vault.tamper_log_path();
    let mut data = std::fs::read(&log_path).unwrap();
    let cutoff = entry_offset(&data, 1);
    data.truncate(cutoff);
    std::fs::write(&log_path, &data).unwrap();

    let result = vault.add("failure-path-cred", CredentialType::ApiKey, b"v2");
    assert!(
        matches!(result, Err(VaultError::TamperLog { .. })),
        "a vault mutation on top of a compromised tamper log must surface \
         VaultError::TamperLog, got {result:?}"
    );

    // WHY this second assertion (forkwright/akroasis#231): the error type
    // alone is what this test used to check, and it held just as well when
    // the mutation was applied and durably persisted BEFORE the audit
    // append that failed — so the entry existed and this test's own name was
    // aspirational. Auditing ahead of the mutation is what makes the block
    // real, and only an absence check can witness it.
    assert!(
        matches!(
            vault.get("failure-path-cred"),
            Err(VaultError::EntryNotFound { .. })
        ),
        "a mutation refused by the tamper log must leave no entry behind"
    );

    // The acceptance partner: the entries written before the corruption are
    // untouched, so the assertion above is reporting a blocked write rather
    // than a vault that has simply stopped answering.
    assert!(
        vault.get("setup-cred").is_ok(),
        "entries committed before the tamper-log corruption must still read"
    );
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

#[test]
fn concurrent_vault_mutations_produce_a_single_non_forked_chain() {
    // WHY (akroasis#226): before the fix, `append_vault_audit` opened a
    // fresh `TamperLog` per call with no synchronization — two threads
    // sharing this `Arc<Vault>` could both recover the same tail and each
    // append an entry chained from it, forking the chain (`verify_chain`
    // reports `Broken`) or losing one writer's entry outright. The
    // in-process mutex in `append_vault_audit` plus koinon's own
    // single-writer lock must make every mutation land, in some order, as
    // one strictly-serial, verifiable chain.
    const WRITERS: usize = 8;

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("concurrent-vault");
    let vault = std::sync::Arc::new(Vault::create(&vault_path, TEST_PASSPHRASE).unwrap());

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(WRITERS));

    let handles: Vec<_> = (0..WRITERS)
        .map(|i| {
            let vault = std::sync::Arc::clone(&vault);
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                vault
                    .add(
                        &format!("concurrent-cred-{i}"),
                        CredentialType::ApiKey,
                        b"v",
                    )
                    .unwrap();
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    let result = vault.verify_tamper_log().unwrap();
    assert_eq!(
        result.status,
        ChainStatus::Intact,
        "concurrent vault mutations on a shared Arc<Vault> must produce a \
         single, non-forked, verifiable chain, got {:?}",
        result.status
    );
    assert_eq!(result.entries_verified, WRITERS as u64);
}
