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

use crate::crypto::{self, decrypt, encrypt};
use crate::error::{
    AlreadyExistsSnafu, EntryCryptoSnafu, EntryNotDeletableSnafu, EntryRevokedSnafu, IoSnafu,
    NotInitializedSnafu, SerializationSnafu, TamperLogSnafu, VaultError, WrongPassphraseSnafu,
};
use crate::key::VaultKey;
use crate::vault::{
    CredentialType, EntryMetadata, EntryStatus, HistoryEvent, HistoryEventKind, KdfParams,
    VAULT_VERSION,
};

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

/// On-disk vault header stored as JSON.
#[derive(Debug, Serialize, Deserialize)]
struct StoredHeader {
    version: u32,
    salt: Vec<u8>,
    kdf_params: KdfParams,
    key_check: Vec<u8>,
}

/// Entry as stored in fjall (JSON-serialized value).
#[derive(Debug, Serialize, Deserialize)]
struct StoredEntry {
    credential_type: CredentialType,
    encrypted_secret: Vec<u8>,
    metadata: EntryMetadata,
    #[serde(default)]
    status: EntryStatus,
    #[serde(default)]
    history: Vec<HistoryEvent>,
}

/// A decrypted credential retrieved from the vault.
#[derive(Debug, Clone)]
pub struct DecryptedEntry {
    /// Human-readable name for this credential.
    pub name: CompactString,
    /// What kind of credential this is.
    pub credential_type: CredentialType,
    /// The decrypted secret bytes.
    pub secret: Vec<u8>,
    /// Associated metadata.
    pub metadata: EntryMetadata,
}

/// Summary of a vault entry (no secret material).
#[derive(Debug, Clone)]
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

/// Lifecycle history of a vault entry.
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
/// Each entry is individually encrypted with ChaCha20-Poly1305 using a
/// key derived from the user's passphrase via Argon2id. The vault
/// directory is advisory-locked to prevent concurrent access.
pub struct Vault {
    db: fjall::Database,
    keyspace: fjall::Keyspace,
    key: VaultKey,
    _lock: File,
    path: PathBuf,
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
    /// Returns [`VaultError::AlreadyExists`] if the path already exists.
    /// Returns [`VaultError::Io`] on filesystem errors.
    /// Returns [`VaultError::StorageBackend`] if fjall initialization fails.
    pub fn create(path: impl AsRef<Path>, passphrase: &[u8]) -> Result<Self, VaultError> {
        let path = path.as_ref();
        if path.exists() {
            return AlreadyExistsSnafu { path }.fail();
        }

        create_owner_only_dir(path)?;

        let lock = acquire_lock(path)?;
        let salt = crypto::generate_salt();
        let key = crypto::derive_key(passphrase, &salt);

        let key_check = encrypt(&key, KEY_CHECK_PLAINTEXT).context(EntryCryptoSnafu)?;

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
            _lock: lock,
            path: path.to_path_buf(),
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
    /// Returns [`VaultError::InvalidHeader`] if the header is malformed.
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

        if header.version != VAULT_VERSION {
            return Err(VaultError::InvalidHeader {
                reason: format!(
                    "unsupported version {}, expected {VAULT_VERSION}",
                    header.version
                ),
            });
        }

        let key = crypto::derive_key(passphrase, &header.salt);

        let plaintext =
            decrypt(&key, &header.key_check).map_err(|_| VaultError::WrongPassphrase)?;
        if plaintext != KEY_CHECK_PLAINTEXT {
            return WrongPassphraseSnafu.fail();
        }

        let (db, keyspace) = open_fjall(&path.join(DATA_DIR))?;

        Ok(Self {
            db,
            keyspace,
            key,
            _lock: lock,
            path: path.to_path_buf(),
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
        if self.keyspace.get(name).map_err(fjall_err)?.is_some() {
            return Err(VaultError::DuplicateEntry {
                name: name.to_owned(),
            });
        }

        let encrypted_secret = encrypt(&self.key, secret).context(EntryCryptoSnafu)?;

        let now = Timestamp::now();
        let entry = StoredEntry {
            credential_type,
            encrypted_secret,
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

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(name, value).map_err(fjall_err)?;
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
        let raw = self.keyspace.get(name).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;

        if entry.status == EntryStatus::Revoked {
            return EntryRevokedSnafu { name }.fail();
        }

        let secret = decrypt(&self.key, &entry.encrypted_secret).context(EntryCryptoSnafu)?;

        Ok(DecryptedEntry {
            name: CompactString::from(name),
            credential_type: entry.credential_type,
            secret,
            metadata: entry.metadata,
        })
    }

    /// Lists all entries in the vault (names and metadata only).
    ///
    /// No secrets are decrypted or returned.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::StorageBackend`] on iteration errors.
    pub fn list(&self) -> Result<Vec<EntryInfo>, VaultError> {
        let mut entries = Vec::new();

        for guard in self.keyspace.iter() {
            let (key, value) = guard.into_inner().map_err(fjall_err)?;
            let name = std::str::from_utf8(&key).map_err(|e| VaultError::StorageBackend {
                message: format!("invalid UTF-8 key: {e}"),
            })?;
            let entry: StoredEntry = serde_json::from_slice(&value).context(SerializationSnafu)?;

            entries.push(EntryInfo {
                name: CompactString::from(name),
                credential_type: entry.credential_type,
                status: entry.status,
                metadata: entry.metadata,
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
        let raw = self.keyspace.get(name).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;

        if entry.status == EntryStatus::Revoked {
            return EntryNotDeletableSnafu { name }.fail();
        }

        self.keyspace.remove(name).map_err(fjall_err)?;
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
        let raw = self.keyspace.get(name).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let mut entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;

        if entry.status == EntryStatus::Revoked {
            return EntryRevokedSnafu { name }.fail();
        }

        entry.encrypted_secret = encrypt(&self.key, new_secret).context(EntryCryptoSnafu)?;

        let now = Timestamp::now();
        entry.metadata.rotated_at = Some(now);
        entry.metadata.rotation_count += 1;
        entry.history.push(HistoryEvent {
            timestamp: now,
            kind: HistoryEventKind::Rotated,
        });

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(name, value).map_err(fjall_err)?;
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
        let raw = self.keyspace.get(name).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let mut entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;

        if entry.status == EntryStatus::Revoked {
            return EntryRevokedSnafu { name }.fail();
        }

        let now = Timestamp::now();
        entry.status = EntryStatus::Revoked;
        entry.metadata.revoked_at = Some(now);
        entry.history.push(HistoryEvent {
            timestamp: now,
            kind: HistoryEventKind::Revoked,
        });

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(name, value).map_err(fjall_err)?;
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
        let raw = self.keyspace.get(name).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;

        Ok(EntryHistory {
            name: CompactString::from(name),
            status: entry.status,
            metadata: entry.metadata,
            events: entry.history,
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

    fn append_vault_audit(&self, name: &str, operation: &str) -> Result<(), VaultError> {
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

    file.try_lock_exclusive().map_err(|_| VaultError::Locked {
        path: vault_path.to_path_buf(),
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
