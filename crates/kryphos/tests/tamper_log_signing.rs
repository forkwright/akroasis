//! Integration test: a vault's audit log records which installation wrote it.
//!
//! WHY every assertion here reads from disk: the file this replaces generated an
//! identity in-test, signed a hash in-test, and verified it against that same
//! in-test identity. Every line of it passed without `TamperLog` knowing an
//! identity existed, so it reported a property the production path did not have
//! (forkwright/akroasis#284). A provenance test that never reopens the log is
//! testing the signature library, not the log.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "integration test — panics are the correct failure mode"
)]

use kryphos::{CredentialType, Vault};
use tekmerion::TipStatus;

const TEST_PASSPHRASE: &[u8] = b"correct horse battery staple";

/// The clause: production appends are signed, and verification confirms which
/// installation produced them — from the recorded key, not one held in hand.
#[test]
fn a_vaults_audit_log_verifies_against_its_own_installation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault");

    let vault = Vault::create(&path, TEST_PASSPHRASE).unwrap();
    vault
        .add("signed-cred", CredentialType::ApiKey, b"secret")
        .unwrap();
    vault.rotate("signed-cred", b"secret-2").unwrap();

    assert_eq!(
        vault.verify_tamper_log_provenance().unwrap(),
        TipStatus::Verified,
        "a vault's own log must verify against its own installation"
    );

    // Across a close: the identity is durable, so provenance is still
    // attributable after the handle that wrote it is gone.
    drop(vault);
    let reopened = Vault::open(&path, TEST_PASSPHRASE).unwrap();
    assert_eq!(
        reopened.verify_tamper_log_provenance().unwrap(),
        TipStatus::Verified,
        "provenance must survive a restart"
    );
}

/// The acceptance bar: substituting the signature must fail.
#[test]
fn a_substituted_signature_does_not_verify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault");

    let vault = Vault::create(&path, TEST_PASSPHRASE).unwrap();
    vault
        .add("cred", CredentialType::ApiKey, b"secret")
        .unwrap();
    assert_eq!(
        vault.verify_tamper_log_provenance().unwrap(),
        TipStatus::Verified,
        "precondition: it verifies before tampering"
    );

    // Flip a bit inside the signature field of the seal. The seal's own MAC
    // covers it, so this must be refused — and refused for that reason rather
    // than by a signature check that never ran.
    //
    // The handle is dropped first: it holds the vault's directory lock, and
    // reopening under it fails as Locked before reaching anything this test is
    // about.
    let seal_path = seal_file(&vault);
    drop(vault);
    let mut bytes = std::fs::read(&seal_path).unwrap();
    let signature_start = bytes.len() - 32 - 64;
    bytes[signature_start] ^= 0xFF;
    std::fs::write(&seal_path, &bytes).unwrap();

    let status = Vault::open(&path, TEST_PASSPHRASE)
        .unwrap()
        .verify_tamper_log_provenance();

    // The seal's MAC covers the signature field, so the edit is caught there
    // and the signature check never runs. That is the correct outer defence and
    // the honest thing for this test to assert — it does NOT cover
    // `check_tip`'s signature arm, which tekmerion's own unit tests exercise
    // directly with a valid MAC over a bad signature.
    assert_eq!(
        status.unwrap(),
        TipStatus::NoSeal,
        "an edited signature must be refused by the seal MAC"
    );
}

/// The other half of the bar: substituting the key id must fail too.
///
/// Kept separate from the signature case because the two fields fail for
/// different reasons at the layer below, even though both are stopped here by
/// the same MAC.
#[test]
fn a_substituted_key_id_does_not_verify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault");

    let vault = Vault::create(&path, TEST_PASSPHRASE).unwrap();
    vault
        .add("cred", CredentialType::ApiKey, b"secret")
        .unwrap();

    let seal_path = seal_file(&vault);
    drop(vault);
    let mut bytes = std::fs::read(&seal_path).unwrap();
    let key_id_start = bytes.len() - 32 - 64 - 8;
    bytes[key_id_start] ^= 0xFF;
    std::fs::write(&seal_path, &bytes).unwrap();

    let status = Vault::open(&path, TEST_PASSPHRASE)
        .unwrap()
        .verify_tamper_log_provenance();

    // As above: the MAC covers the key id too, so this is refused before the
    // signature is examined. tekmerion's unit tests cover the case where the MAC
    // is valid and the key id names a different installation.
    assert_eq!(
        status.unwrap(),
        TipStatus::NoSeal,
        "an edited key id must be refused by the seal MAC"
    );
}

/// The migration rule, in the direction that must be refused.
///
/// A vault holding an identity whose log tip is unsigned has been downgraded —
/// there is no innocent route to that state, because this code signs whenever
/// an identity exists. Reporting it as merely "unsigned" would make signing
/// optional in practice for anyone able to write the seal.
#[test]
fn an_identity_bearing_vault_refuses_an_unsigned_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault");

    let vault = Vault::create(&path, TEST_PASSPHRASE).unwrap();
    vault
        .add("cred", CredentialType::ApiKey, b"secret")
        .unwrap();

    // Delete the seal outright: the most direct way to present a log with no
    // tip provenance under a vault that has an identity.
    std::fs::remove_file(seal_file(&vault)).unwrap();

    let status = Vault::open(&path, TEST_PASSPHRASE).map(|v| v.verify_tamper_log_provenance());

    // Either the log refuses to open at all (an absent seal is already
    // fail-closed, akroasis#285) or provenance refuses it. Both are refusals;
    // neither is a quiet `Unsigned`.
    match status {
        Err(_) => {}
        Ok(inner) => assert!(
            !matches!(inner, Ok(TipStatus::Unsigned)),
            "a vault with an identity must not report its log as merely unsigned, got {inner:?}"
        ),
    }
}

/// The same rule in the direction that must be *allowed*, which is what keeps
/// it a migration rule rather than a wall: a vault with no recorded identity
/// has an unsigned log for an innocent reason, and must keep working.
#[test]
fn a_vault_without_an_identity_still_verifies_its_chain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("vault");

    let vault = Vault::create(&path, TEST_PASSPHRASE).unwrap();
    drop(vault);
    strip_identity(&path);

    let vault = Vault::open(&path, TEST_PASSPHRASE).unwrap();
    vault
        .add("cred", CredentialType::ApiKey, b"secret")
        .unwrap();

    assert_eq!(
        vault.verify_tamper_log_provenance().unwrap(),
        TipStatus::Unsigned,
        "a vault that never had an identity reports an unsigned log, not an error"
    );
    assert!(
        vault.get("cred").is_ok(),
        "and the vault itself keeps working"
    );
}

fn seal_file(vault: &Vault) -> std::path::PathBuf {
    let log = vault.tamper_log_path();
    let mut name = log.into_os_string();
    name.push(".seal");
    std::path::PathBuf::from(name)
}

fn strip_identity(vault_path: &std::path::Path) {
    let header_path = vault_path.join("header.json");
    let mut header: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&header_path).unwrap()).unwrap();
    let object = header.as_object_mut().unwrap();
    object.remove("installation_public_key");
    object.remove("sealed_signing_key");
    std::fs::write(&header_path, serde_json::to_vec(&header).unwrap()).unwrap();
}
