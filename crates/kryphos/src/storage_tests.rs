//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use super::*;

const TEST_PASSPHRASE: &[u8] = b"correct horse battery staple";

#[test]
fn create_and_open_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("test-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    drop(vault);

    let vault = Vault::open(&vault_path, TEST_PASSPHRASE).unwrap();
    assert_eq!(vault.path(), vault_path);
}

#[test]
fn create_fails_if_path_exists() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("existing-vault");

    Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    // Drop implicitly releases the lock.

    let result = Vault::create(&vault_path, TEST_PASSPHRASE);
    assert!(
        result.is_err(),
        "creating a vault at an existing path must fail"
    );
}

#[test]
fn open_with_wrong_passphrase_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("wrong-pass-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    drop(vault);

    let result = Vault::open(&vault_path, b"wrong passphrase");
    assert!(
        result.is_err(),
        "opening with wrong passphrase must return an error"
    );
}

#[test]
fn add_and_get_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("add-get-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let secret = b"sk-1234567890abcdef";
    vault
        .add("openai-key", CredentialType::ApiKey, secret)
        .unwrap();

    let entry = vault.get("openai-key").unwrap();
    assert_eq!(entry.name, "openai-key");
    assert_eq!(entry.credential_type, CredentialType::ApiKey);
    assert_eq!(entry.secret.as_slice(), secret.as_slice());
}

#[test]
fn add_duplicate_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("dup-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("key-1", CredentialType::Psk, b"secret-a")
        .unwrap();

    let result = vault.add("key-1", CredentialType::Psk, b"secret-b");
    assert!(result.is_err(), "adding a duplicate entry name must fail");
}

#[test]
fn get_missing_entry_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("missing-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let result = vault.get("nonexistent");
    assert!(result.is_err(), "getting a nonexistent entry must fail");
}

#[test]
fn list_returns_metadata_without_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("list-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("cred-a", CredentialType::ApiKey, b"secret-a")
        .unwrap();
    vault
        .add("cred-b", CredentialType::Psk, b"secret-b")
        .unwrap();

    let entries = vault.list().unwrap();
    assert_eq!(entries.len(), 2, "list must return all entries");

    for info in &entries {
        assert!(
            info.name == "cred-a" || info.name == "cred-b",
            "list must contain expected entry names"
        );
    }
}

#[test]
fn remove_deletes_entry() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("remove-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("disposable", CredentialType::RadioKey, b"key-data")
        .unwrap();

    vault.remove("disposable").unwrap();

    let result = vault.get("disposable");
    assert!(result.is_err(), "get after remove must fail");

    let entries = vault.list().unwrap();
    assert!(entries.is_empty(), "list after remove must be empty");
}

#[test]
fn remove_missing_entry_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("remove-missing-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let result = vault.remove("ghost");
    assert!(result.is_err(), "removing a nonexistent entry must fail");
}

#[test]
fn entries_persist_across_open_close() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("persist-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("persistent", CredentialType::Certificate, b"cert-pem")
        .unwrap();
    drop(vault);

    let vault = Vault::open(&vault_path, TEST_PASSPHRASE).unwrap();
    let entry = vault.get("persistent").unwrap();
    assert_eq!(
        entry.secret.as_slice(),
        b"cert-pem".as_slice(),
        "secret must survive close/open"
    );
}

#[test]
fn concurrent_open_fails_with_lock_error() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("lock-vault");

    let _vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let result = Vault::open(&vault_path, TEST_PASSPHRASE);
    assert!(result.is_err(), "concurrent open must fail with lock error");
}

// -----------------------------------------------------------------
// Rotation
// -----------------------------------------------------------------

#[test]
fn rotate_updates_secret_and_preserves_name() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("rotate-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("api-key", CredentialType::ApiKey, b"old-secret")
        .unwrap();

    vault.rotate("api-key", b"new-secret").unwrap();

    let entry = vault.get("api-key").unwrap();
    assert_eq!(entry.name, "api-key", "name must be preserved after rotate");
    assert_eq!(
        entry.secret.as_slice(),
        b"new-secret".as_slice(),
        "secret must be updated after rotate"
    );
    assert_eq!(
        entry.credential_type,
        CredentialType::ApiKey,
        "credential type must be preserved after rotate"
    );
}

#[test]
fn rotate_increments_rotation_count() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("rotate-count-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::Psk, b"secret-v1").unwrap();

    vault.rotate("key", b"secret-v2").unwrap();
    vault.rotate("key", b"secret-v3").unwrap();

    let history = vault.history("key").unwrap();
    assert_eq!(
        history.metadata.rotation_count, 2,
        "rotation count must reflect number of rotations"
    );
    assert!(
        history.metadata.rotated_at.is_some(),
        "rotated_at must be SET after rotation"
    );
}

#[test]
fn rotate_missing_entry_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("rotate-missing-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let result = vault.rotate("ghost", b"new-secret");
    assert!(result.is_err(), "rotating a nonexistent entry must fail");
}

#[test]
fn rotate_revoked_entry_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("rotate-revoked-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::ApiKey, b"secret").unwrap();
    vault.revoke("key").unwrap();

    let result = vault.rotate("key", b"new-secret");
    assert!(result.is_err(), "rotating a revoked entry must fail");
}

// -----------------------------------------------------------------
// Revocation
// -----------------------------------------------------------------

#[test]
fn revoke_prevents_get() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("revoke-get-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::ApiKey, b"secret").unwrap();

    vault.revoke("key").unwrap();

    let result = vault.get("key");
    assert!(
        result.is_err(),
        "get on a revoked entry must return an error"
    );
}

#[test]
fn revoke_sets_revoked_at() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("revoke-timestamp-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::Psk, b"secret").unwrap();

    vault.revoke("key").unwrap();

    let history = vault.history("key").unwrap();
    assert_eq!(
        history.status,
        EntryStatus::Revoked,
        "status must be Revoked after revocation"
    );
    assert!(
        history.metadata.revoked_at.is_some(),
        "revoked_at must be SET after revocation"
    );
}

#[test]
fn revoke_already_revoked_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("revoke-twice-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::ApiKey, b"secret").unwrap();
    vault.revoke("key").unwrap();

    let result = vault.revoke("key");
    assert!(
        result.is_err(),
        "revoking an already revoked entry must fail"
    );
}

#[test]
fn revoke_missing_entry_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("revoke-missing-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let result = vault.revoke("ghost");
    assert!(result.is_err(), "revoking a nonexistent entry must fail");
}

#[test]
fn revoked_entry_not_deletable() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("revoke-DELETE-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("audit-key", CredentialType::ApiKey, b"secret")
        .unwrap();
    vault.revoke("audit-key").unwrap();

    let result = vault.remove("audit-key");
    assert!(
        result.is_err(),
        "removing a revoked entry must fail for audit trail"
    );

    let entries = vault.list().unwrap();
    assert_eq!(entries.len(), 1, "revoked entry must remain in the vault");
}

// -----------------------------------------------------------------
// History
// -----------------------------------------------------------------

#[test]
fn history_tracks_creation_event() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("history-CREATE-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::ApiKey, b"secret").unwrap();

    let history = vault.history("key").unwrap();
    assert_eq!(history.name, "key");
    assert_eq!(history.status, EntryStatus::Active);
    assert_eq!(
        history.events.len(),
        1,
        "new entry must have exactly one history event"
    );
    assert_eq!(history.events[0].kind, HistoryEventKind::Created);
}

#[test]
fn history_tracks_rotation_events() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("history-rotate-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::Psk, b"v1").unwrap();

    vault.rotate("key", b"v2").unwrap();
    vault.rotate("key", b"v3").unwrap();

    let history = vault.history("key").unwrap();
    assert_eq!(
        history.events.len(),
        3,
        "history must have created + 2 rotations"
    );
    assert_eq!(history.events[0].kind, HistoryEventKind::Created);
    assert_eq!(history.events[1].kind, HistoryEventKind::Rotated);
    assert_eq!(history.events[2].kind, HistoryEventKind::Rotated);
}

#[test]
fn history_tracks_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("history-revoke-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::ApiKey, b"secret").unwrap();

    vault.rotate("key", b"rotated-secret").unwrap();
    vault.revoke("key").unwrap();

    let history = vault.history("key").unwrap();
    assert_eq!(
        history.events.len(),
        3,
        "history must have created + rotated + revoked"
    );
    assert_eq!(history.events[0].kind, HistoryEventKind::Created);
    assert_eq!(history.events[1].kind, HistoryEventKind::Rotated);
    assert_eq!(history.events[2].kind, HistoryEventKind::Revoked);
    assert_eq!(history.status, EntryStatus::Revoked);
}

#[test]
fn history_events_are_chronological() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("history-chrono-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault.add("key", CredentialType::Psk, b"v1").unwrap();
    vault.rotate("key", b"v2").unwrap();
    vault.revoke("key").unwrap();

    let history = vault.history("key").unwrap();
    for pair in history.events.windows(2) {
        assert!(
            pair[0].timestamp <= pair[1].timestamp,
            "history events must be in chronological ORDER"
        );
    }
}

#[test]
fn history_missing_entry_fails() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("history-missing-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let result = vault.history("ghost");
    assert!(result.is_err(), "history for a nonexistent entry must fail");
}

// -----------------------------------------------------------------
// List with status
// -----------------------------------------------------------------

#[test]
fn list_shows_entry_status() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("list-status-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("active-key", CredentialType::ApiKey, b"secret-a")
        .unwrap();
    vault
        .add("revoked-key", CredentialType::Psk, b"secret-b")
        .unwrap();
    vault.revoke("revoked-key").unwrap();

    let entries = vault.list().unwrap();
    assert_eq!(entries.len(), 2, "list must return all entries");

    for info in &entries {
        if info.name == "active-key" {
            assert_eq!(
                info.status,
                EntryStatus::Active,
                "active entry must show Active status"
            );
        } else if info.name == "revoked-key" {
            assert_eq!(
                info.status,
                EntryStatus::Revoked,
                "revoked entry must show Revoked status"
            );
        }
    }
}

// -----------------------------------------------------------------
// Filesystem permissions (akroasis#217)
// -----------------------------------------------------------------

#[cfg(unix)]
#[test]
fn create_restricts_vault_dir_to_owner_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("perms-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let vault_mode = std::fs::metadata(&vault_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        vault_mode, 0o700,
        "vault directory must be owner-only (0700)"
    );

    drop(vault);
}

#[cfg(unix)]
#[test]
fn create_restricts_header_file_to_owner_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("perms-header-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let header_mode = std::fs::metadata(vault_path.join(HEADER_FILE))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        header_mode, 0o600,
        "header.json (the passphrase key-check oracle) must be owner-only (0600)"
    );

    drop(vault);
}

#[cfg(unix)]
#[test]
fn create_restricts_data_dir_to_owner_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("perms-data-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let data_mode = std::fs::metadata(vault_path.join(DATA_DIR))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        data_mode, 0o700,
        "fjall data directory must be owner-only (0700)"
    );

    drop(vault);
}

#[cfg(unix)]
#[test]
fn create_restricts_lock_file_to_owner_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("perms-lock-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let lock_mode = std::fs::metadata(vault_path.join(LOCK_FILE))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(lock_mode, 0o600, "vault.lock must be owner-only (0600)");

    drop(vault);
}

#[test]
fn open_on_a_missing_vault_leaves_the_path_untouched() {
    // Regression for forkwright/akroasis#286: `open` acquired the lock
    // before checking for a header, and `acquire_lock` created the vault
    // directory plus the lock file. A `list`/`get` before initialization —
    // or one mistyped path — therefore poisoned the path against `create`.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("never-initialized");

    let result = Vault::open(&vault_path, TEST_PASSPHRASE);

    assert!(
        matches!(result, Err(VaultError::NotInitialized { .. })),
        "opening a path with no vault must report typed absence, got: {result:?}"
    );
    assert!(
        !vault_path.exists(),
        "a failed open must not create the vault directory"
    );
}

#[test]
fn create_succeeds_after_a_failed_open_at_the_same_path() {
    // The acceptance condition of forkwright/akroasis#286: the failed open
    // must leave the path usable for the initialization it preceded.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("open-then-create");

    let failed = Vault::open(&vault_path, TEST_PASSPHRASE);
    assert!(failed.is_err(), "vault does not exist yet");

    let created = Vault::create(&vault_path, TEST_PASSPHRASE);
    assert!(
        created.is_ok(),
        "create must succeed at a path a failed open touched, got: {created:?}"
    );
    assert_eq!(created.unwrap().path(), vault_path);
}

#[test]
fn open_reports_absence_rather_than_io_failure_for_a_bare_directory() {
    // A directory with no header is not a vault. Callers branch on
    // `NotInitialized` to offer initialization, so this must not surface as
    // a generic I/O error.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("empty-dir");
    std::fs::create_dir(&vault_path).unwrap();

    let result = Vault::open(&vault_path, TEST_PASSPHRASE);

    assert!(
        matches!(result, Err(VaultError::NotInitialized { .. })),
        "a headerless directory must report typed absence, got: {result:?}"
    );
}

// -----------------------------------------------------------------
// Secret zeroization (akroasis#218)
// -----------------------------------------------------------------

#[test]
fn decrypted_secret_is_zeroized_on_drop_by_type() {
    // Regression for forkwright/akroasis#218: `DecryptedEntry.secret` used
    // to be a bare `Vec<u8>`, which does not implement `ZeroizeOnDrop` —
    // this assertion would FAIL TO COMPILE against that field type. Wrapping
    // it in `Zeroizing<Vec<u8>>` (at the point `decrypt` returns, with no
    // intermediate unwrapped copy) makes the guarantee type-enforced rather
    // than a claim: `Zeroizing<Z>: ZeroizeOnDrop` for any `Z: Zeroize`, so
    // this now compiles and holds for every `DecryptedEntry` the shipped
    // `Vault::get` returns.
    fn assert_zeroizes_on_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("zeroize-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("zeroize-key", CredentialType::ApiKey, b"scrub-me-on-drop")
        .unwrap();

    let entry = vault.get("zeroize-key").unwrap();
    assert_zeroizes_on_drop(&entry.secret);
}

// -----------------------------------------------------------------
// At-rest metadata encryption (akroasis#215)
// -----------------------------------------------------------------

#[test]
fn on_disk_fjall_contents_do_not_reveal_credential_name() {
    // Regression for forkwright/akroasis#215: `StoredEntry` used to keep
    // credential_type/metadata/status/history as plaintext JSON, and the
    // fjall record KEY was the credential name itself in cleartext — so the
    // name appeared on disk twice over, once as the key and (implicitly, via
    // being findable) as an index into the plaintext value. This reads
    // every file fjall actually wrote under the vault's `data/` directory
    // and asserts the name is not a byte-for-byte substring of any of them.
    //
    // Before the fix this failed: the name was the literal fjall key, so it
    // appears verbatim in the LSM tree's persisted pages. After the fix the
    // fjall key is a keyed BLAKE3 hash of the name and the value is two
    // ChaCha20-Poly1305 ciphertexts, neither of which can contain the
    // plaintext name as a substring without breaking either primitive.
    const DISTINCTIVE_NAME: &str = "surveillance-counter-mesh-psk-zzyzx9182";

    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("metadata-at-rest-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add(DISTINCTIVE_NAME, CredentialType::Psk, b"mesh-secret-value")
        .unwrap();
    drop(vault);

    let mut on_disk = Vec::new();
    collect_file_bytes(&vault_path.join(DATA_DIR), &mut on_disk);

    assert!(
        !on_disk.is_empty(),
        "validation check: fjall must have written SOMETHING to the data directory"
    );
    assert!(
        !on_disk
            .windows(DISTINCTIVE_NAME.len())
            .any(|window| window == DISTINCTIVE_NAME.as_bytes()),
        "credential name must not appear in plaintext anywhere under the fjall data directory"
    );
}

/// Recursively reads every regular file under `dir`, appending its bytes to
/// `out`. Test-only: used to scan fjall's actual on-disk output for
/// plaintext leakage rather than trusting the in-process API surface.
fn collect_file_bytes(dir: &std::path::Path, out: &mut Vec<u8>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_file_bytes(&path, out);
        } else if let Ok(bytes) = std::fs::read(&path) {
            out.extend_from_slice(&bytes);
        }
    }
}
