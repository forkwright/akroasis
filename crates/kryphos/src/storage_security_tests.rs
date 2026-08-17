//! Tests for [`super`]; split out from `storage_tests.rs` to keep both
//! under the RUST/file-too-long 800-line threshold. Covers the three
//! security defects fixed together: empty passphrases (akroasis#287),
//! ciphertext identity binding (akroasis#283), and concurrent mutation
//! atomicity (akroasis#214).

use super::*;

const TEST_PASSPHRASE: &[u8] = b"correct horse battery staple";

// -----------------------------------------------------------------
// Empty passphrase (akroasis#287)
// -----------------------------------------------------------------

#[test]
fn create_rejects_empty_passphrase() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("empty-passphrase-vault");

    let result = Vault::create(&vault_path, b"");

    assert!(
        matches!(result, Err(VaultError::EmptyPassphrase)),
        "Vault::create with an empty passphrase must return a typed \
         validation error, got {result:?}"
    );
}

#[test]
fn create_with_empty_passphrase_leaves_no_filesystem_state() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("empty-passphrase-no-state-vault");

    let result = Vault::create(&vault_path, b"");
    assert!(result.is_err());

    assert!(
        !vault_path.exists(),
        "a rejected empty-passphrase create must not create the vault path"
    );
}

#[test]
fn create_succeeds_with_nonempty_passphrase_after_a_rejected_empty_one() {
    // A path that was correctly refused for an empty passphrase must remain
    // usable — the rejection must not itself leave a poisoned path (the
    // same class of concern as `create_succeeds_after_a_failed_open_at_the_same_path`
    // in storage_tests.rs, applied to the new #287 boundary).
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("empty-then-real-passphrase-vault");

    let rejected = Vault::create(&vault_path, b"");
    assert!(rejected.is_err());

    let created = Vault::create(&vault_path, TEST_PASSPHRASE);
    assert!(
        created.is_ok(),
        "create must succeed at a path a rejected empty-passphrase call touched, got: {created:?}"
    );
}

// -----------------------------------------------------------------
// Ciphertext identity binding (akroasis#283)
// -----------------------------------------------------------------

#[test]
fn moved_ciphertext_between_entries_fails_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("aad-relocate-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("entry-a", CredentialType::ApiKey, b"secret-a")
        .unwrap();
    vault
        .add("entry-b", CredentialType::ApiKey, b"secret-b")
        .unwrap();

    // Simulate a write-capable attacker (or store corruption): take the raw
    // stored value for `entry-a` — a valid, correctly-authenticated
    // ciphertext — and place it under `entry-b`'s fjall key.
    let raw_a = vault.keyspace.get("entry-a").unwrap().unwrap();
    vault.keyspace.insert("entry-b", raw_a).unwrap();

    let result = vault.get("entry-b");
    assert!(
        result.is_err(),
        "a ciphertext relocated from a different entry must fail \
         authentication rather than decrypt as a misidentified credential, \
         got {result:?}"
    );

    // The untouched original must still be exactly retrievable.
    let entry_a = vault.get("entry-a").unwrap();
    assert_eq!(entry_a.secret, b"secret-a");
}

#[test]
fn mutated_credential_type_field_fails_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("aad-type-tamper-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("entry", CredentialType::ApiKey, b"secret")
        .unwrap();

    let raw = vault.keyspace.get("entry").unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["credential_type"] = serde_json::to_value(CredentialType::Psk).unwrap();
    let tampered = serde_json::to_vec(&value).unwrap();
    vault.keyspace.insert("entry", tampered).unwrap();

    let result = vault.get("entry");
    assert!(
        result.is_err(),
        "a credential_type edited independently of encrypted_secret must \
         fail authentication, got {result:?}"
    );
}

#[test]
fn mutated_envelope_version_field_fails_authentication() {
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("aad-version-tamper-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("entry", CredentialType::ApiKey, b"secret")
        .unwrap();

    let raw = vault.keyspace.get("entry").unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["envelope_version"] = serde_json::json!(99);
    let tampered = serde_json::to_vec(&value).unwrap();
    vault.keyspace.insert("entry", tampered).unwrap();

    let result = vault.get("entry");
    assert!(
        result.is_err(),
        "an envelope_version edited independently of encrypted_secret must \
         fail authentication, got {result:?}"
    );
}

// -----------------------------------------------------------------
// Concurrent mutation atomicity (akroasis#214)
// -----------------------------------------------------------------

#[test]
fn concurrent_add_same_name_yields_one_winner_and_duplicate_losers() {
    const THREADS: usize = 16;
    const ITERATIONS: usize = 10;

    for iteration in 0..ITERATIONS {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join(format!("concurrent-add-vault-{iteration}"));
        let vault = std::sync::Arc::new(Vault::create(&vault_path, TEST_PASSPHRASE).unwrap());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(THREADS));

        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let vault = std::sync::Arc::clone(&vault);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    vault.add(
                        "race-key",
                        CredentialType::ApiKey,
                        format!("secret-{i}").as_bytes(),
                    )
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        let successes = results.iter().filter(|r| r.is_ok()).count();
        let duplicates = results
            .iter()
            .filter(|r| matches!(r, Err(VaultError::DuplicateEntry { .. })))
            .count();

        assert_eq!(
            successes, 1,
            "iteration {iteration}: exactly one concurrent add must win, got {successes} of {THREADS}"
        );
        assert_eq!(
            duplicates,
            THREADS - 1,
            "iteration {iteration}: every losing add must see DuplicateEntry, got {duplicates}"
        );

        let entries = vault.list().unwrap();
        assert_eq!(
            entries.len(),
            1,
            "iteration {iteration}: exactly one entry must be stored after the race, got {}",
            entries.len()
        );
    }
}

#[test]
fn concurrent_rotate_never_loses_a_rotation_count_increment() {
    const THREADS: usize = 16;
    const ITERATIONS: usize = 10;

    for iteration in 0..ITERATIONS {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir
            .path()
            .join(format!("concurrent-rotate-vault-{iteration}"));
        let vault = std::sync::Arc::new(Vault::create(&vault_path, TEST_PASSPHRASE).unwrap());
        vault
            .add("rotate-race", CredentialType::ApiKey, b"v0")
            .unwrap();

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(THREADS));

        let handles: Vec<_> = (0..THREADS)
            .map(|i| {
                let vault = std::sync::Arc::clone(&vault);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    vault
                        .rotate("rotate-race", format!("v{i}").as_bytes())
                        .unwrap();
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let history = vault.history("rotate-race").unwrap();
        assert_eq!(
            history.metadata.rotation_count, THREADS as u32,
            "iteration {iteration}: every concurrent rotation must be \
             counted with no lost update, got {} of {THREADS}",
            history.metadata.rotation_count
        );
        assert_eq!(
            history
                .events
                .iter()
                .filter(|e| e.kind == HistoryEventKind::Rotated)
                .count(),
            THREADS,
            "iteration {iteration}: every concurrent rotation must append \
             its own history event"
        );
    }
}
