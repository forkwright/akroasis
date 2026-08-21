//! Tests for [`super`]; split out from `storage_tests.rs` to keep both
//! under the RUST/file-too-long 800-line threshold. Covers the security
//! defects fixed together: empty passphrases (akroasis#287), ciphertext
//! identity binding (akroasis#283), concurrent mutation atomicity
//! (akroasis#214), and transparent legacy-entry migration (akroasis#283
//! Desired Correction, akroasis#215).

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
    // ciphertext — and place it under `entry-b`'s fjall key. Keys are
    // looked up via `lookup_key` (forkwright/akroasis#215): the fjall
    // record key is a keyed hash of the name, not the name itself.
    let key_a = vault.lookup_key("entry-a");
    let key_b = vault.lookup_key("entry-b");
    let raw_a = vault.keyspace.get(key_a).unwrap().unwrap();
    vault.keyspace.insert(key_b, raw_a).unwrap();

    let result = vault.get("entry-b");
    assert!(
        result.is_err(),
        "a ciphertext relocated from a different entry must fail \
         authentication rather than decrypt as a misidentified credential, \
         got {result:?}"
    );

    // The untouched original must still be exactly retrievable.
    let entry_a = vault.get("entry-a").unwrap();
    assert_eq!(entry_a.secret.as_slice(), b"secret-a".as_slice());
}

#[test]
fn secret_ciphertext_paired_with_a_different_entrys_metadata_fails_authentication() {
    // `credential_type` moved from a plaintext top-level field into the
    // encrypted `encrypted_metadata` blob (forkwright/akroasis#215), so an
    // attacker without the vault key can no longer flip it as a bare JSON
    // field the way #283's original review scenario assumed — editing
    // `encrypted_metadata` without the key just breaks its own AEAD tag.
    // What remains reachable without the key: splicing one entry's
    // `encrypted_secret` into another entry's `StoredEntry`, pairing it with
    // THAT entry's own (validly-decrypting) metadata — including a
    // different `credential_type`. `entry_aad` still binds `credential_type`
    // (sourced from the decrypted metadata) into what `encrypted_secret`
    // authenticates, so the spliced pair must fail exactly like a
    // whole-entry relocation does.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("aad-type-splice-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("api-entry", CredentialType::ApiKey, b"secret-api")
        .unwrap();
    vault
        .add("psk-entry", CredentialType::Psk, b"secret-psk")
        .unwrap();

    let key_api = vault.lookup_key("api-entry");
    let key_psk = vault.lookup_key("psk-entry");

    let raw_api = vault.keyspace.get(key_api).unwrap().unwrap();
    let raw_psk = vault.keyspace.get(key_psk).unwrap().unwrap();

    let mut value_api: serde_json::Value = serde_json::from_slice(&raw_api).unwrap();
    let value_psk: serde_json::Value = serde_json::from_slice(&raw_psk).unwrap();

    // Splice psk-entry's encrypted_secret into api-entry's StoredEntry,
    // keeping api-entry's own encrypted_metadata/envelope_version — so
    // decrypt_metadata still succeeds (it decrypts api-entry's own,
    // untouched blob) and reports credential_type == ApiKey, while the
    // secret ciphertext was actually bound under credential_type == Psk at
    // encrypt time.
    value_api["encrypted_secret"] = value_psk["encrypted_secret"].clone();
    let spliced = serde_json::to_vec(&value_api).unwrap();
    vault.keyspace.insert(key_api, spliced).unwrap();

    let result = vault.get("api-entry");
    assert!(
        result.is_err(),
        "a secret ciphertext bound under a different entry's credential_type \
         must fail authentication even when paired with metadata that \
         decrypts cleanly on its own, got {result:?}"
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

    let key = vault.lookup_key("entry");
    let raw = vault.keyspace.get(key).unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["envelope_version"] = serde_json::json!(99);
    let tampered = serde_json::to_vec(&value).unwrap();
    vault.keyspace.insert(key, tampered).unwrap();

    let result = vault.get("entry");
    assert!(
        result.is_err(),
        "an envelope_version edited independently of encrypted_secret must \
         fail authentication, got {result:?}"
    );
}

#[test]
fn envelope_version_downgraded_to_legacy_fails_authentication() {
    // WHY: the migration branch (LEGACY_ENVELOPE_VERSION => empty AAD) is
    // itself a new attack surface if it can be reached for an entry that was
    // NOT actually sealed under empty AAD. Tamper a genuinely #283-bound
    // entry's `envelope_version` DOWN to the legacy sentinel and confirm
    // `get` still fails — its ciphertext was authenticated under a non-empty
    // AAD at encrypt time, so decrypting with `b""` cannot succeed either.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("aad-downgrade-tamper-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();
    vault
        .add("entry", CredentialType::ApiKey, b"secret")
        .unwrap();

    let key = vault.lookup_key("entry");
    let raw = vault.keyspace.get(key).unwrap().unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        value["envelope_version"],
        serde_json::json!(1),
        "precondition: a freshly-added entry is bound under the current envelope"
    );
    value["envelope_version"] = serde_json::json!(0);
    let tampered = serde_json::to_vec(&value).unwrap();
    vault.keyspace.insert(key, tampered).unwrap();

    let result = vault.get("entry");
    assert!(
        result.is_err(),
        "downgrading a #283-bound entry's envelope_version to the legacy \
         sentinel must not let it decrypt under empty AAD, got {result:?}"
    );
}

// -----------------------------------------------------------------
// Legacy entry migration (akroasis#283 Desired Correction)
// -----------------------------------------------------------------

#[test]
fn legacy_pre_envelope_entry_opens_and_decrypts_under_the_current_vault_format() {
    // WHY hand-assembled rather than produced by `Vault::add`: no code in
    // this binary writes an envelope_version-0 entry anymore (`add` always
    // stamps `ENTRY_ENVELOPE_VERSION`), so an entry sealed before
    // forkwright/akroasis#283's AAD binding existed can only be
    // reconstructed by replicating what that OLDER code actually wrote:
    // `encrypted_secret` sealed with `crypto::encrypt(key, secret, b"")`
    // (empty AAD). VAULT_VERSION itself never changed for the AAD-binding
    // fix alone — see its doc — so this vault's HEADER is the CURRENT
    // format (forkwright/akroasis#215's encrypted-metadata + hashed-lookup
    // shape) throughout; only this one entry predates entry_aad. Built with
    // the vault's own `key`/`lookup_key`/`encrypt_metadata` (accessible
    // here as a descendant module of `storage`), not a reimplementation of
    // them.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("legacy-pre-envelope-vault");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let now = Timestamp::now();
    let record = EntryMetadataRecord {
        name: CompactString::from("legacy-entry"),
        credential_type: CredentialType::ApiKey,
        metadata: EntryMetadata {
            created_at: now,
            rotated_at: None,
            revoked_at: None,
            rotation_count: 0,
            tags: Vec::new(),
        },
        status: EntryStatus::Active,
        history: vec![HistoryEvent {
            timestamp: now,
            kind: HistoryEventKind::Created,
        }],
    };
    let encrypted_metadata = vault.encrypt_metadata(&record).unwrap();
    let encrypted_secret = encrypt(&vault.key, b"legacy-secret", b"").unwrap();
    let legacy_entry = StoredEntry {
        envelope_version: 0,
        encrypted_secret,
        encrypted_metadata,
    };
    vault
        .keyspace
        .insert(
            vault.lookup_key("legacy-entry"),
            serde_json::to_vec(&legacy_entry).unwrap(),
        )
        .unwrap();
    vault.db.persist(fjall::PersistMode::SyncAll).unwrap();

    // The production get path: must transparently decrypt a pre-AAD-binding
    // (envelope_version 0, empty-AAD) entry with no migrate command and no
    // operator round-trip through an old binary.
    let decrypted = vault.get("legacy-entry").unwrap();
    assert_eq!(decrypted.secret.as_slice(), b"legacy-secret".as_slice());
    assert_eq!(decrypted.credential_type, CredentialType::ApiKey);
}

#[test]
fn legacy_pre_envelope_entry_rotate_opportunistically_upgrades_the_envelope() {
    // A legacy (pre-AAD-binding) entry that gets rotated must come out
    // bound under the current envelope, so it stops depending on the
    // legacy branch on every subsequent read.
    let dir = tempfile::tempdir().unwrap();
    let vault_path = dir.path().join("legacy-pre-envelope-vault-rotate");

    let vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

    let now = Timestamp::now();
    let record = EntryMetadataRecord {
        name: CompactString::from("legacy-entry"),
        credential_type: CredentialType::ApiKey,
        metadata: EntryMetadata {
            created_at: now,
            rotated_at: None,
            revoked_at: None,
            rotation_count: 0,
            tags: Vec::new(),
        },
        status: EntryStatus::Active,
        history: vec![HistoryEvent {
            timestamp: now,
            kind: HistoryEventKind::Created,
        }],
    };
    let encrypted_metadata = vault.encrypt_metadata(&record).unwrap();
    let encrypted_secret = encrypt(&vault.key, b"v0", b"").unwrap();
    let legacy_entry = StoredEntry {
        envelope_version: 0,
        encrypted_secret,
        encrypted_metadata,
    };
    let key = vault.lookup_key("legacy-entry");
    vault
        .keyspace
        .insert(key, serde_json::to_vec(&legacy_entry).unwrap())
        .unwrap();
    vault.db.persist(fjall::PersistMode::SyncAll).unwrap();

    vault.rotate("legacy-entry", b"v1").unwrap();

    let raw = vault.keyspace.get(key).unwrap().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        value["envelope_version"],
        serde_json::json!(1),
        "rotate must stamp the current envelope on a legacy entry it rewrites"
    );

    let decrypted = vault.get("legacy-entry").unwrap();
    assert_eq!(decrypted.secret.as_slice(), b"v1".as_slice());
}

// -----------------------------------------------------------------
// Concurrent mutation atomicity (akroasis#214)
// -----------------------------------------------------------------

#[test]
// WHY expect not allow, and why the collect is NOT needless despite the
// lint: `.collect()` into `handles` is what forces every `thread::spawn`
// to run before any `.join()` starts. Taking clippy's suggested fix —
// chaining spawn and join in one lazy iterator — would join thread 0
// before thread 1 ever spawns, serializing the race this test exists to
// observe. A test that always passes because it never actually
// contends is decoration, not a fixture.
#[expect(
    clippy::needless_collect,
    reason = "eager collect forces every thread to spawn (and reach the barrier) before any is joined — required for a real race, not needless"
)]
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

        let entries = vault.list().unwrap().entries;
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
