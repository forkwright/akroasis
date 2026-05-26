//! Integration test: vault mutations append to the tamper-evident audit log.

#![expect(
    clippy::unwrap_used,
    reason = "integration test — panics are the correct failure mode"
)]

use koinon::{ChainStatus, verify_chain};
use kryphos::{CredentialType, Vault};

const TEST_PASSPHRASE: &[u8] = b"correct horse battery staple";

#[test]
fn vault_mutations_append_intact_tamper_log() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("audited-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    let log_path = vault.tamper_log_path();

    vault
        .add("incident-radio-key", CredentialType::RadioKey, b"secret-v1")
        .unwrap();
    vault.rotate("incident-radio-key", b"secret-v2").unwrap();
    vault.revoke("incident-radio-key").unwrap();

    let result = verify_chain(&log_path).unwrap();
    assert_eq!(result.status, ChainStatus::Intact);
    assert_eq!(result.entries_verified, 3);
}
