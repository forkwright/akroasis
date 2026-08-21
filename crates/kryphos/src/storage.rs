//! Vault storage backend: encrypted credential CRUD over fjall.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use compact_str::CompactString;
use fs2::FileExt;
use jiff::Timestamp;
use koinon::{ChainKey, LogEntryKind, TamperLog, VerificationResult};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use zeroize::Zeroizing;

use crate::crypto::{self, ENTRY_ENVELOPE_VERSION, decrypt, encrypt, entry_aad};
use crate::error::{
    AlreadyExistsSnafu, CryptoError, EmptyPassphraseSnafu, EntryCryptoSnafu,
    EntryNotDeletableSnafu, EntryRevokedSnafu, IoSnafu, NotInitializedSnafu, SerializationSnafu,
    TamperLogSnafu, VaultError, WrongPassphraseSnafu,
};
use crate::key::VaultKey;
use crate::vault::{
    CredentialType, EntryMetadata, EntryStatus, HistoryEvent, HistoryEventKind, KdfParams,
    MIN_SUPPORTED_VAULT_VERSION, VAULT_VERSION,
};

/// Sentinel `envelope_version` marking a pre-#283 entry: no `envelope_version`
/// key existed in that on-disk shape at all, so `#[serde(default)]` fills
/// this in on read. Never written by `add`/`rotate` — see
/// [`crate::crypto::ENTRY_ENVELOPE_VERSION`].
const LEGACY_ENVELOPE_VERSION: u8 = 0;

/// Well-known plaintext used to verify the passphrase on open.
const KEY_CHECK_PLAINTEXT: &[u8] = b"kryphos-vault-key-check-v1";

/// Name of the header file within the vault directory.
const HEADER_FILE: &str = "header.json";

/// Name of the lock file within the vault directory.
const LOCK_FILE: &str = "vault.lock";

/// Subdirectory for the fjall keyspace.
const DATA_DIR: &str = "data";

/// Name of the tamper-evident vault audit log within the vault directory.
const TAMPER_LOG_FILE: &str = "tamper.log";

/// Domain-separation tag for deriving the tamper log's [`ChainKey`] from
/// the vault's [`VaultKey`].
///
/// Reuses the vault's existing secret rather than requiring a second one
/// to manage: the derivation is a one-way keyed hash, so recovering the
/// vault key from a leaked chain key is infeasible.
const CHAIN_KEY_DOMAIN: &[u8] = b"kryphos/tamper-log/chain-key/v1";

/// Domain-separation tag for deriving the fjall lookup-key subkey from
/// the vault's [`VaultKey`] (see [`Vault::lookup_key`]).
///
/// Reuses the vault's existing secret rather than requiring a second one
/// to manage, mirroring [`CHAIN_KEY_DOMAIN`].
const LOOKUP_KEY_DOMAIN: &[u8] = b"kryphos/vault/lookup-key/v1";

/// On-disk vault header stored as JSON.
#[derive(Debug, Serialize, Deserialize)]
struct StoredHeader {
    version: u32,
    salt: Vec<u8>,
    kdf_params: KdfParams,
    key_check: Vec<u8>,
}

/// Entry as stored in fjall (JSON-serialized value).
///
/// `encrypted_secret` and `encrypted_metadata` are independently-nonced
/// ChaCha20-Poly1305 ciphertexts. `encrypted_metadata` decrypts to an
/// [`EntryMetadataRecord`] carrying the name, type, metadata, status, and
/// history — none of it readable from the fjall data directory without the
/// vault key. Keeping it separate from `encrypted_secret` means listing
/// entries (which needs only the metadata) never touches secret ciphertext.
///
/// `envelope_version` is the one field that stays plaintext: it has to be
/// readable before `encrypted_secret` is decrypted, since it selects which
/// AAD that ciphertext was bound under (forkwright/akroasis#283, see
/// [`entry_aad`]). It carries no secret information itself.
#[derive(Debug, Serialize, Deserialize)]
struct StoredEntry {
    /// AEAD associated-data envelope version this entry's
    /// `encrypted_secret` was bound under. Bound into the AAD itself
    /// ([`entry_aad`]), so a value tampered independently of the ciphertext
    /// fails authentication rather than silently taking effect.
    #[serde(default)]
    envelope_version: u8,
    encrypted_secret: Vec<u8>,
    encrypted_metadata: Vec<u8>,
}

/// Plaintext form of everything but the secret value; JSON-serialized
/// and encrypted as `StoredEntry::encrypted_metadata`.
#[derive(Debug, Serialize, Deserialize)]
struct EntryMetadataRecord {
    name: CompactString,
    credential_type: CredentialType,
    metadata: EntryMetadata,
    #[serde(default)]
    status: EntryStatus,
    #[serde(default)]
    history: Vec<HistoryEvent>,
}

/// A decrypted credential retrieved from the vault.
#[derive(Clone)]
pub struct DecryptedEntry {
    /// Human-readable name for this credential.
    pub name: CompactString,
    /// What kind of credential this is.
    pub credential_type: CredentialType,
    /// The decrypted secret bytes.
    ///
    /// Wrapped in [`Zeroizing`] at the point of allocation (the return
    /// of [`decrypt`], moved straight in — no unwrapped copy exists in
    /// between) so the plaintext is scrubbed on drop rather than left in
    /// freed heap memory.
    pub secret: Zeroizing<Vec<u8>>,
    /// Associated metadata.
    pub metadata: EntryMetadata,
}

// WHY: manual Debug instead of #[derive(Debug)] — `secret` holds the
// decrypted plaintext credential; redact it so an accidental `{:?}` log
// never prints it (RUST/no-debug-derive-on-public-types).
impl std::fmt::Debug for DecryptedEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecryptedEntry")
            .field("name", &self.name)
            .field("credential_type", &self.credential_type)
            .field("secret", &"<redacted>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// Summary of a vault entry (no secret material).
#[derive(Clone)]
pub struct EntryInfo {
    /// Human-readable name for this credential.
    pub name: CompactString,
    /// What kind of credential this is.
    pub credential_type: CredentialType,
    /// Lifecycle status.
    pub status: EntryStatus,
    /// Associated metadata.
    pub metadata: EntryMetadata,
}

// WHY: manual Debug instead of #[derive(Debug)] — the type touches
// `credential_type` (RUST/no-debug-derive-on-public-types matches on the
// "credential" token). None of these fields are secret material, so this
// mirrors the derived output exactly.
impl std::fmt::Debug for EntryInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryInfo")
            .field("name", &self.name)
            .field("credential_type", &self.credential_type)
            .field("status", &self.status)
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// Lifecycle history of a vault entry.
// WHY: pure data — a query result bag with no derived invariant.
#[derive(Debug, Clone)]
pub struct EntryHistory {
    /// Human-readable name for this credential.
    pub name: CompactString,
    /// Current lifecycle status.
    pub status: EntryStatus,
    /// Associated metadata.
    pub metadata: EntryMetadata,
    /// Chronological lifecycle events.
    pub events: Vec<HistoryEvent>,
}

/// Encrypted credential vault backed by fjall.
///
/// Each entry is individually encrypted with ChaCha20-Poly1305 using a key
/// derived from the user's passphrase via Argon2id, with `encrypted_secret`'s
/// AEAD associated data binding it to its entry identity
/// (forkwright/akroasis#283) and `encrypted_metadata` protecting the name,
/// type, status, and history at rest (forkwright/akroasis#215) under a fjall
/// record key that is itself a keyed hash of the name, not the name itself.
/// The vault directory is advisory-locked to prevent concurrent access from
/// OTHER PROCESSES; `write_lock` is the separate in-process guard that
/// serializes this handle's own mutating calls (forkwright/akroasis#214) —
/// see its field doc. Appends to the tamper-evident audit log are further
/// serialized across their open-through-append critical section by
/// `tamper_log_guard` (akroasis#226) — see `append_vault_audit`.
pub struct Vault {
    db: fjall::Database,
    keyspace: fjall::Keyspace,
    key: VaultKey,
    /// Copy of the header's salt, used only as this vault instance's
    /// identity component in [`entry_aad`] — never as key material.
    salt: Vec<u8>,
    /// Serializes `add`/`remove`/`rotate`/`revoke` against each other
    /// within THIS process.
    ///
    /// INVARIANT: held across the full duplicate-check-then-write (`add`)
    /// or read-modify-write (`rotate`/`revoke`) region of each of those
    /// methods, never released partway through. `Vault` is `Send + Sync`
    /// (fjall handles + a fixed key + a lock file), so a multithreaded
    /// caller holding `Arc<Vault>` can otherwise interleave two calls
    /// between the duplicate check and the write, both observing the
    /// pre-write state — forkwright/akroasis#214. The directory lock
    /// above does not help here: it guards a different boundary
    /// (concurrent processes), not concurrent threads inside one.
    ///
    /// WHY recover from poison rather than propagate it: this crate denies
    /// `panic`/`unwrap_used`/`expect_used`, so a panic while the guard is
    /// held can only come from an allocation failure or similar, not from
    /// a `.unwrap()` in this code path. Refusing every subsequent vault
    /// operation over a panic that was never this mutex's own fault would
    /// turn one incident into a stuck vault; the recovered guard still
    /// serializes correctly because fjall's own per-key operations stay
    /// individually atomic regardless.
    write_lock: std::sync::Mutex<()>,
    _lock: File,
    path: PathBuf,
    // WHY (akroasis#226): `TamperLog::open` re-verifies and recovers the
    // on-disk tail on every call — that's what lets a concurrently- or
    // externally-truncated log be caught on the very next append (see
    // `corrupted_tamper_log_blocks_further_vault_mutations`). Holding one
    // long-lived `TamperLog` handle instead would skip that re-verification
    // and let a stale in-memory tail launder an externally-truncated file.
    // So concurrency is guarded here instead: this mutex serializes the
    // open-through-append critical section, so two threads sharing one
    // `Arc<Vault>` block on each other rather than each independently
    // opening the log, recovering the same tail, and forking the chain.
    tamper_log_guard: std::sync::Mutex<()>,
}

impl Vault {
    /// Creates a new vault at the given path.
    ///
    /// Generates a random salt, derives a key from the passphrase,
    /// writes the header, and initializes the fjall keyspace. On Unix
    /// the vault directory, its `data/` subdirectory, and every file
    /// written are created owner-only (`0700`/`0600`) so a local
    /// unprivileged user cannot read the key-check oracle or ciphertext.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EmptyPassphrase`] if `passphrase` is empty.
    /// Returns [`VaultError::AlreadyExists`] if the path already exists.
    /// Returns [`VaultError::Io`] on filesystem errors.
    /// Returns [`VaultError::StorageBackend`] if fjall initialization fails.
    pub fn create(path: impl AsRef<Path>, passphrase: &[u8]) -> Result<Self, VaultError> {
        // WHY checked first, before any filesystem access: an empty
        // passphrase carries no entropy, so `Vault::create(path, b"")` must
        // reject at the library boundary rather than only in the CLI's
        // interactive confirmation (forkwright/akroasis#287) — and it must
        // leave no filesystem state behind, per the issue's Done-when.
        if passphrase.is_empty() {
            return EmptyPassphraseSnafu.fail();
        }

        let path = path.as_ref();
        if path.exists() {
            return AlreadyExistsSnafu { path }.fail();
        }

        create_owner_only_dir(path)?;

        let lock = acquire_lock(path)?;
        let salt = crypto::generate_salt();
        // The salt is freshly generated at SALT_LEN and the parameters are this
        // crate's own defaults, so neither check can fail here; both are mapped
        // rather than unwrapped so no future change to either can turn that
        // assumption into a panic. `kdf_default_is_within_the_accepted_range`
        // holds the parameter half of it.
        let key =
            crypto::derive_key(passphrase, &salt, &KdfParams::default()).map_err(|source| {
                VaultError::InvalidHeader {
                    reason: source.to_string(),
                }
            })?;

        let key_check = encrypt(&key, KEY_CHECK_PLAINTEXT, b"").context(EntryCryptoSnafu)?;

        let header = StoredHeader {
            version: VAULT_VERSION,
            salt: salt.to_vec(),
            kdf_params: KdfParams::default(),
            key_check,
        };

        let header_json = serde_json::to_string_pretty(&header).context(SerializationSnafu)?;
        write_owner_only_file(&path.join(HEADER_FILE), header_json.as_bytes())?;

        // WHY: pre-create the data dir owner-only before fjall populates it —
        // fjall's own `create_dir_all` uses process-default (umask) perms and
        // would otherwise leave the keyspace directory world-readable.
        create_owner_only_dir(&path.join(DATA_DIR))?;
        let (db, keyspace) = open_fjall(&path.join(DATA_DIR))?;

        Ok(Self {
            db,
            keyspace,
            key,
            salt: salt.to_vec(),
            write_lock: std::sync::Mutex::new(()),
            _lock: lock,
            path: path.to_path_buf(),
            tamper_log_guard: std::sync::Mutex::new(()),
        })
    }

    /// Opens an existing vault.
    ///
    /// Reads the header, derives the key from the passphrase, and verifies
    /// the key check value before opening the fjall keyspace.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::NotInitialized`] if no vault exists at `path`.
    /// Returns [`VaultError::WrongPassphrase`] if the passphrase is incorrect.
    /// Returns [`VaultError::Locked`] if another process holds the lock.
    /// Returns [`VaultError::InvalidHeader`] if the header is malformed, or
    /// its version is outside
    /// `MIN_SUPPORTED_VAULT_VERSION..=VAULT_VERSION`.
    pub fn open(path: impl AsRef<Path>, passphrase: &[u8]) -> Result<Self, VaultError> {
        let path = path.as_ref();

        // WHY: establish that a vault is present before anything touches the
        // filesystem. Locking first used to create the vault directory and the
        // lock file on a path that held no vault, after which `create` rejected
        // that same path as `AlreadyExists` — so a mistyped path, or any read
        // attempted before initialization, poisoned the path until someone
        // cleaned it up by hand.
        //
        // This ordering also gives `acquire_lock` its precondition: the vault
        // directory exists by the time it runs.
        //
        // Checking before locking introduces no race the old order avoided. If
        // another process creates the vault in between, we lock and read it
        // normally; if one removes it, the header read fails exactly as before.
        if !path.join(HEADER_FILE).is_file() {
            return NotInitializedSnafu { path }.fail();
        }

        let lock = acquire_lock(path)?;

        let header_bytes = fs::read(path.join(HEADER_FILE)).context(IoSnafu { path })?;
        let header: StoredHeader =
            serde_json::from_slice(&header_bytes).context(SerializationSnafu)?;

        // WHY a range, not an exact match against VAULT_VERSION: the header
        // shape is unchanged between MIN_SUPPORTED_VAULT_VERSION and
        // VAULT_VERSION (see VAULT_VERSION's doc) — a v1 vault opens exactly
        // like a v2 one. What differs is per-entry: `get` below selects the
        // AAD from each entry's own `envelope_version` rather than assuming
        // one scheme for the whole vault. Below the floor is a version this
        // crate never wrote; above the ceiling is a newer format this build
        // predates. Both remain hard rejections (forkwright/akroasis#283's
        // Desired Correction is an in-place migration, not "accept anything").
        if !(MIN_SUPPORTED_VAULT_VERSION..=VAULT_VERSION).contains(&header.version) {
            return Err(VaultError::InvalidHeader {
                reason: format!(
                    "unsupported version {}, expected {MIN_SUPPORTED_VAULT_VERSION}..={VAULT_VERSION}",
                    header.version
                ),
            });
        }

        // WHY(#231) this is the reachable one: `header.salt` is a
        // variable-length field read from the stored JSON, so a corrupt or
        // tampered vault can carry a salt Argon2 will not accept. A bad file
        // must open as an invalid header, not crash the process.
        let key =
            crypto::derive_key(passphrase, &header.salt, &header.kdf_params).map_err(|source| {
                VaultError::InvalidHeader {
                    reason: source.to_string(),
                }
            })?;

        // WHY(#231) the two failures are separated: any decrypt error used to
        // become WrongPassphrase, so a truncated or corrupt key-check told the
        // operator their correct passphrase was wrong. They would retype it
        // forever. A ciphertext shorter than a nonce is not a failed
        // decryption — it is not a ciphertext.
        let plaintext = decrypt(&key, &header.key_check, b"").map_err(|source| match source {
            CryptoError::InvalidNonceLength { expected, actual } => VaultError::InvalidHeader {
                reason: format!(
                    "key check is {actual} bytes, too short to carry a {expected}-byte nonce"
                ),
            },
            _ => VaultError::WrongPassphrase,
        })?;
        if plaintext != KEY_CHECK_PLAINTEXT {
            return WrongPassphraseSnafu.fail();
        }

        let (db, keyspace) = open_fjall(&path.join(DATA_DIR))?;

        Ok(Self {
            db,
            keyspace,
            key,
            salt: header.salt,
            write_lock: std::sync::Mutex::new(()),
            _lock: lock,
            path: path.to_path_buf(),
            tamper_log_guard: std::sync::Mutex::new(()),
        })
    }

    /// Stores a new credential in the vault.
    ///
    /// The secret is encrypted before writing to the backend.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::DuplicateEntry`] if an entry with this name exists.
    /// Returns [`VaultError::EntryCrypto`] if encryption fails.
    pub fn add(
        &self,
        name: &str,
        credential_type: CredentialType,
        secret: &[u8],
    ) -> Result<(), VaultError> {
        // INVARIANT: held across the duplicate check AND the write below.
        // See `write_lock`'s field doc — this is what makes two concurrent
        // `add` calls for the same name resolve to exactly one winner
        // (forkwright/akroasis#214) instead of both observing `None` and
        // both inserting.
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let key = self.lookup_key(name);
        if self.keyspace.get(key).map_err(fjall_err)?.is_some() {
            return Err(VaultError::DuplicateEntry {
                name: name.to_owned(),
            });
        }

        let aad = entry_aad(&self.salt, name, &credential_type, ENTRY_ENVELOPE_VERSION)?;
        let encrypted_secret = encrypt(&self.key, secret, &aad).context(EntryCryptoSnafu)?;

        let now = Timestamp::now();
        let record = EntryMetadataRecord {
            name: CompactString::from(name),
            credential_type,
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
        let encrypted_metadata = self.encrypt_metadata(&record)?;

        let entry = StoredEntry {
            envelope_version: ENTRY_ENVELOPE_VERSION,
            encrypted_secret,
            encrypted_metadata,
        };

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(key, value).map_err(fjall_err)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_err)?;
        self.append_vault_audit(name, "add")?;

        Ok(())
    }

    /// Retrieves and decrypts a credential by name.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    /// Returns [`VaultError::EntryRevoked`] if the entry has been revoked.
    /// Returns [`VaultError::EntryCrypto`] if decryption fails.
    pub fn get(&self, name: &str) -> Result<DecryptedEntry, VaultError> {
        let key = self.lookup_key(name);
        let raw = self.keyspace.get(key).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;
        let record = self.decrypt_metadata(&entry)?;

        if record.status == EntryStatus::Revoked {
            return EntryRevokedSnafu { name }.fail();
        }

        // WHY branch on envelope_version rather than always building an AAD:
        // a LEGACY_ENVELOPE_VERSION (0) entry is one written before
        // forkwright/akroasis#283's AAD binding existed — its ciphertext was
        // sealed with `crypto::encrypt(key, secret, b"")` (empty AAD; see
        // VAULT_VERSION's doc). Building a non-empty `entry_aad` for it
        // would authenticate against bytes the original encryption never
        // used, permanently failing every `get` on such an entry — exactly
        // the access loss #283's Desired Correction asked to avoid. Any
        // OTHER version (the current ENTRY_ENVELOPE_VERSION, or a tampered
        // value) goes through the full identity-bound AAD: rebuilt from the
        // CALLER's `name` plus this entry's OWN decrypted `credential_type`
        // (from `encrypted_metadata`) and stored `envelope_version`, never
        // trusted verbatim. A ciphertext relocated from a different entry
        // (or with its `envelope_version` edited independently of
        // `encrypted_secret`) was bound under a different AAD at encrypt
        // time, so it fails authentication here instead of decrypting into
        // this slot (forkwright/akroasis#283) — including a downgrade
        // attempt that tampers a bound entry's `envelope_version` DOWN to
        // 0, since its ciphertext was never sealed under empty AAD in the
        // first place.
        //
        // WHY: wrap at the point of allocation in BOTH branches —
        // `decrypt`'s return is moved straight into `Zeroizing::new` with no
        // intermediate unwrapped binding, so there is no plaintext copy that
        // this fix leaves unscrubbed on drop.
        let secret = if entry.envelope_version == LEGACY_ENVELOPE_VERSION {
            Zeroizing::new(
                decrypt(&self.key, &entry.encrypted_secret, b"").context(EntryCryptoSnafu)?,
            )
        } else {
            let aad = entry_aad(
                &self.salt,
                name,
                &record.credential_type,
                entry.envelope_version,
            )?;
            Zeroizing::new(
                decrypt(&self.key, &entry.encrypted_secret, &aad).context(EntryCryptoSnafu)?,
            )
        };

        Ok(DecryptedEntry {
            name: record.name,
            credential_type: record.credential_type,
            secret,
            metadata: record.metadata,
        })
    }

    /// Lists all entries in the vault (names and metadata only).
    ///
    /// No secrets are decrypted or returned: only `encrypted_metadata` is
    /// touched, never `encrypted_secret`.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::StorageBackend`] on iteration errors.
    /// Returns [`VaultError::EntryCrypto`] if metadata decryption fails.
    pub fn list(&self) -> Result<Vec<EntryInfo>, VaultError> {
        let mut entries = Vec::new();

        for guard in self.keyspace.iter() {
            let (_key, value) = guard.into_inner().map_err(fjall_err)?;
            // WHY(#231) an unreadable entry is skipped rather than fatal: a
            // single corrupt record used to abort the whole listing, so one bad
            // row hid every good one and the operator could not see the
            // credentials they still had.
            let Ok(entry) = serde_json::from_slice::<StoredEntry>(&value) else {
                tracing::warn!("skipping a vault entry whose stored record does not parse");
                continue;
            };
            let Ok(record) = self.decrypt_metadata(&entry) else {
                tracing::warn!("skipping a vault entry whose metadata does not decrypt");
                continue;
            };

            entries.push(EntryInfo {
                name: record.name,
                credential_type: record.credential_type,
                status: record.status,
                metadata: record.metadata,
            });
        }

        Ok(entries)
    }

    /// Removes a credential from the vault.
    ///
    /// Revoked entries cannot be removed (audit trail preservation).
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    /// Returns [`VaultError::EntryNotDeletable`] if the entry is revoked.
    pub fn remove(&self, name: &str) -> Result<(), VaultError> {
        // INVARIANT: see `write_lock`'s field doc. `remove` is a
        // read-modify-write too (read status, conditionally remove); without
        // this, a concurrent `revoke` racing this call could both read
        // `Active` before either write lands — the revoke's write would then
        // resurrect a name this call just believed it had deleted.
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let key = self.lookup_key(name);
        let raw = self.keyspace.get(key).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;
        let record = self.decrypt_metadata(&entry)?;

        if record.status == EntryStatus::Revoked {
            return EntryNotDeletableSnafu { name }.fail();
        }

        self.keyspace.remove(key).map_err(fjall_err)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_err)?;
        self.append_vault_audit(name, "remove")?;

        Ok(())
    }

    /// Rotates a credential's secret, preserving name and metadata.
    ///
    /// Re-encrypts the entry with the new secret, updates the
    /// `rotated_at` timestamp, and increments the rotation count.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    /// Returns [`VaultError::EntryRevoked`] if the entry has been revoked.
    /// Returns [`VaultError::EntryCrypto`] if encryption fails.
    pub fn rotate(&self, name: &str, new_secret: &[u8]) -> Result<(), VaultError> {
        // INVARIANT: see `write_lock`'s field doc. Held across this whole
        // read-modify-write so two concurrent `rotate` calls serialize
        // instead of each reading the same starting `rotation_count` /
        // history and one's increment/event silently overwriting the
        // other's (forkwright/akroasis#214).
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let key = self.lookup_key(name);
        let raw = self.keyspace.get(key).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;
        let mut record = self.decrypt_metadata(&entry)?;

        if record.status == EntryStatus::Revoked {
            return EntryRevokedSnafu { name }.fail();
        }

        // WHY stamp the current envelope version on every rewrite, not just
        // preserve whatever was there: `rotate` re-encrypts the secret
        // anyway, so it is the natural opportunistic upgrade point for any
        // entry still carrying a pre-#283 envelope.
        let aad = entry_aad(
            &self.salt,
            name,
            &record.credential_type,
            ENTRY_ENVELOPE_VERSION,
        )?;
        let encrypted_secret = encrypt(&self.key, new_secret, &aad).context(EntryCryptoSnafu)?;

        let now = Timestamp::now();
        record.metadata.rotated_at = Some(now);
        record.metadata.rotation_count += 1;
        record.history.push(HistoryEvent {
            timestamp: now,
            kind: HistoryEventKind::Rotated,
        });
        let encrypted_metadata = self.encrypt_metadata(&record)?;

        let entry = StoredEntry {
            envelope_version: ENTRY_ENVELOPE_VERSION,
            encrypted_secret,
            encrypted_metadata,
        };

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(key, value).map_err(fjall_err)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_err)?;
        self.append_vault_audit(name, "rotate")?;

        Ok(())
    }

    /// Revokes a credential, preventing future retrieval.
    ///
    /// The encrypted data is preserved for audit purposes. Revoked
    /// entries cannot be deleted or rotated.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    /// Returns [`VaultError::EntryRevoked`] if the entry is already revoked.
    pub fn revoke(&self, name: &str) -> Result<(), VaultError> {
        // INVARIANT: see `write_lock`'s field doc. Same read-modify-write
        // shape as `rotate` — without this, two concurrent `revoke` calls
        // (or a `revoke` racing a `rotate`) can both read `Active` and one
        // write silently loses the other's status/history change
        // (forkwright/akroasis#214).
        let _write_guard = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let key = self.lookup_key(name);
        let raw = self.keyspace.get(key).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;
        let mut record = self.decrypt_metadata(&entry)?;

        if record.status == EntryStatus::Revoked {
            return EntryRevokedSnafu { name }.fail();
        }

        let now = Timestamp::now();
        record.status = EntryStatus::Revoked;
        record.metadata.revoked_at = Some(now);
        record.history.push(HistoryEvent {
            timestamp: now,
            kind: HistoryEventKind::Revoked,
        });
        let encrypted_metadata = self.encrypt_metadata(&record)?;

        let entry = StoredEntry {
            envelope_version: entry.envelope_version,
            encrypted_secret: entry.encrypted_secret,
            encrypted_metadata,
        };

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(key, value).map_err(fjall_err)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_err)?;
        self.append_vault_audit(name, "revoke")?;

        Ok(())
    }

    /// Returns the lifecycle history for a credential.
    ///
    /// Includes creation, rotation, and revocation events in
    /// chronological order.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    pub fn history(&self, name: &str) -> Result<EntryHistory, VaultError> {
        let key = self.lookup_key(name);
        let raw = self.keyspace.get(key).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;
        let record = self.decrypt_metadata(&entry)?;

        Ok(EntryHistory {
            name: record.name,
            status: record.status,
            metadata: record.metadata,
            events: record.history,
        })
    }

    /// Returns the filesystem path of this vault.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the filesystem path of the vault mutation audit log.
    #[must_use]
    pub fn tamper_log_path(&self) -> PathBuf {
        self.path.join(TAMPER_LOG_FILE)
    }

    /// Verifies the vault's tamper-evident mutation audit log.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::TamperLog`] if the log file cannot be read.
    pub fn verify_tamper_log(&self) -> Result<VerificationResult, VaultError> {
        koinon::verify_chain(self.tamper_log_path(), &self.chain_key()).context(TamperLogSnafu)
    }

    /// Derives this vault's tamper-log chain key from its [`VaultKey`].
    ///
    /// A fresh derivation on every call, not a stored copy: the vault
    /// holds only the `VaultKey`, and every caller that needs to open or
    /// verify the tamper log derives the chain key from it on demand —
    /// no second secret to generate, store, or rotate.
    fn chain_key(&self) -> ChainKey {
        ChainKey::from_bytes(blake3::keyed_hash(self.key.as_bytes(), CHAIN_KEY_DOMAIN).into())
    }

    /// Derives the fjall record key for `name` via a two-step keyed BLAKE3
    /// hash of the vault key, so credential names never appear as fjall
    /// keys on disk.
    ///
    /// Deterministic (same name -> same key), so `get`/`add`/`remove` stay
    /// O(1) keyspace lookups without ever storing the name itself. A fresh
    /// derivation on every call, mirroring [`Self::chain_key`].
    fn lookup_key(&self, name: &str) -> [u8; 32] {
        let subkey = blake3::keyed_hash(self.key.as_bytes(), LOOKUP_KEY_DOMAIN);
        blake3::keyed_hash(subkey.as_bytes(), name.as_bytes()).into()
    }

    /// Decrypts and parses an entry's `encrypted_metadata` field.
    fn decrypt_metadata(&self, entry: &StoredEntry) -> Result<EntryMetadataRecord, VaultError> {
        let metadata_bytes =
            decrypt(&self.key, &entry.encrypted_metadata, b"").context(EntryCryptoSnafu)?;
        serde_json::from_slice(&metadata_bytes).context(SerializationSnafu)
    }

    /// Serializes and encrypts an [`EntryMetadataRecord`] for storage as
    /// `StoredEntry::encrypted_metadata`.
    fn encrypt_metadata(&self, record: &EntryMetadataRecord) -> Result<Vec<u8>, VaultError> {
        let metadata_bytes = serde_json::to_vec(record).context(SerializationSnafu)?;
        encrypt(&self.key, &metadata_bytes, b"").context(EntryCryptoSnafu)
    }

    fn append_vault_audit(&self, name: &str, operation: &str) -> Result<(), VaultError> {
        // WHY (akroasis#226): held across the ENTIRE open-through-append
        // sequence, not just the open — a guard that unlocked between open
        // and append would let a second thread's open interleave with this
        // one's still-in-flight append, recovering the same pre-append tail.
        // A poisoned mutex (a prior holder panicked mid-critical-section)
        // still recovers the guard: `TamperLog::open`'s own re-verification
        // is what actually protects correctness here, not the poison flag.
        let _guard = self
            .tamper_log_guard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // INVARIANT: `log` must drop before `_guard` — Rust drops function
        // locals in reverse declaration order, so declaring `_guard` first
        // guarantees `log`'s koinon-level OS advisory lock (`TamperLog`'s
        // `_lock` field) is released before this mutex is, so a thread that
        // was waiting on `_guard` never sees a spurious `TamperLogError::
        // Locked` from koinon's own (fail-fast, non-blocking) lock.
        let mut log =
            TamperLog::open(self.tamper_log_path(), self.chain_key()).context(TamperLogSnafu)?;
        log.append(LogEntryKind::VaultMutation {
            credential_name: CompactString::from(name),
            operation: CompactString::from(operation),
        })
        .context(TamperLogSnafu)?;
        Ok(())
    }
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Acquires an advisory lock on the vault directory.
///
/// INVARIANT: `vault_path` already exists as a vault directory. `create`
/// builds it before locking, and `open` refuses a path with no header before
/// it gets here. The directory-creating call this function used to make was
/// what let `open` leave artifacts on a non-existent vault, so the caller
/// owns directory creation now and this function only ever locks.
fn acquire_lock(vault_path: &Path) -> Result<File, VaultError> {
    let lock_path = vault_path.join(LOCK_FILE);

    let file = create_owner_only_file(&lock_path)?;

    // WHY(#231) the error kind is inspected: every lock failure used to report
    // "locked by another process", so a permission or filesystem problem sent
    // the operator hunting for a process that was never there. Only WouldBlock
    // means contention.
    file.try_lock_exclusive().map_err(|source| {
        if source.kind() == std::io::ErrorKind::WouldBlock {
            VaultError::Locked {
                path: vault_path.to_path_buf(),
            }
        } else {
            VaultError::Io {
                path: lock_path.clone(),
                source,
            }
        }
    })?;

    Ok(file)
}

/// Creates `path` as a directory restricted to the owner on Unix (`0700`).
///
/// Idempotent: succeeds without error if `path` already exists as a
/// directory (matching the prior `create_dir_all` behavior). On non-Unix
/// platforms this is equivalent to `create_dir_all` — no portable
/// owner-only directory API exists in `std`.
fn create_owner_only_dir(path: &Path) -> Result<(), VaultError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }

    builder.create(path).context(IoSnafu { path })
}

/// Opens `path` for writing, creating (or truncating) it, restricted to the
/// owner on Unix (`0600`). On non-Unix platforms this is equivalent to
/// `File::create` — no portable owner-only file-creation API exists in
/// `std`.
fn create_owner_only_file(path: &Path) -> Result<File, VaultError> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options.open(path).context(IoSnafu { path })
}

/// Writes `contents` to a fresh owner-only file at `path` (`0600` on Unix).
fn write_owner_only_file(path: &Path, contents: &[u8]) -> Result<(), VaultError> {
    create_owner_only_file(path)?
        .write_all(contents)
        .context(IoSnafu { path })
}

/// Opens or creates a fjall database and keyspace at the given path.
fn open_fjall(data_path: &Path) -> Result<(fjall::Database, fjall::Keyspace), VaultError> {
    let db =
        fjall::Database::builder(data_path)
            .open()
            .map_err(|e| VaultError::StorageBackend {
                message: format!("fjall open: {e}"),
            })?;

    let keyspace = db
        .keyspace("entries", fjall::KeyspaceCreateOptions::default)
        .map_err(|e| VaultError::StorageBackend {
            message: format!("fjall keyspace: {e}"),
        })?;

    Ok((db, keyspace))
}

/// Converts a fjall error into a `VaultError::StorageBackend`.
fn fjall_err(e: impl std::fmt::Display) -> VaultError {
    VaultError::StorageBackend {
        message: e.to_string(),
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "storage_tests.rs"]
mod tests;

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "storage_security_tests.rs"]
mod security_tests;
