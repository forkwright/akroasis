//! Vault storage backend: encrypted credential CRUD over fjall.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use compact_str::CompactString;
use fs2::FileExt;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::crypto::{self, decrypt, encrypt};
use crate::error::{
    AlreadyExistsSnafu, EntryCryptoSnafu, IoSnafu, SerializationSnafu, VaultError,
    WrongPassphraseSnafu,
};
use crate::key::VaultKey;
use crate::vault::{CredentialType, EntryMetadata, KdfParams, VAULT_VERSION};

/// Well-known plaintext used to verify the passphrase on open.
const KEY_CHECK_PLAINTEXT: &[u8] = b"kryphos-vault-key-check-v1";

/// Name of the header file within the vault directory.
const HEADER_FILE: &str = "header.json";

/// Name of the lock file within the vault directory.
const LOCK_FILE: &str = "vault.lock";

/// Subdirectory for the fjall keyspace.
const DATA_DIR: &str = "data";

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
    /// Associated metadata.
    pub metadata: EntryMetadata,
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
    /// writes the header, and initializes the fjall keyspace.
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

        fs::create_dir_all(path).context(IoSnafu { path })?;

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
        fs::write(path.join(HEADER_FILE), header_json).context(IoSnafu { path })?;

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
    /// Returns [`VaultError::WrongPassphrase`] if the passphrase is incorrect.
    /// Returns [`VaultError::Locked`] if another process holds the lock.
    /// Returns [`VaultError::InvalidHeader`] if the header is malformed.
    pub fn open(path: impl AsRef<Path>, passphrase: &[u8]) -> Result<Self, VaultError> {
        let path = path.as_ref();
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

        let entry = StoredEntry {
            credential_type,
            encrypted_secret,
            metadata: EntryMetadata {
                created_at: Timestamp::now(),
                rotated_at: None,
                tags: Vec::new(),
            },
        };

        let value = serde_json::to_vec(&entry).context(SerializationSnafu)?;
        self.keyspace.insert(name, value).map_err(fjall_err)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_err)?;

        Ok(())
    }

    /// Retrieves and decrypts a credential by name.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    /// Returns [`VaultError::EntryCrypto`] if decryption fails.
    pub fn get(&self, name: &str) -> Result<DecryptedEntry, VaultError> {
        let raw = self.keyspace.get(name).map_err(fjall_err)?.ok_or_else(|| {
            VaultError::EntryNotFound {
                name: name.to_owned(),
            }
        })?;

        let entry: StoredEntry = serde_json::from_slice(&raw).context(SerializationSnafu)?;

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
                metadata: entry.metadata,
            });
        }

        Ok(entries)
    }

    /// Removes a credential from the vault.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::EntryNotFound`] if no entry with this name exists.
    pub fn remove(&self, name: &str) -> Result<(), VaultError> {
        if self.keyspace.get(name).map_err(fjall_err)?.is_none() {
            return Err(VaultError::EntryNotFound {
                name: name.to_owned(),
            });
        }

        self.keyspace.remove(name).map_err(fjall_err)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .map_err(fjall_err)?;

        Ok(())
    }

    /// Returns the filesystem path of this vault.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
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
fn acquire_lock(vault_path: &Path) -> Result<File, VaultError> {
    let lock_path = vault_path.join(LOCK_FILE);

    // WHY: create_dir_all is idempotent and needed when called from open()
    // before the lock file exists.
    fs::create_dir_all(vault_path).context(IoSnafu { path: vault_path })?;

    let file = File::create(&lock_path).context(IoSnafu { path: vault_path })?;

    file.try_lock_exclusive().map_err(|_| VaultError::Locked {
        path: vault_path.to_path_buf(),
    })?;

    Ok(file)
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
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
mod tests {
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
        assert_eq!(entry.secret, secret);
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
        assert_eq!(entry.secret, b"cert-pem", "secret must survive close/open");
    }

    #[test]
    fn concurrent_open_fails_with_lock_error() {
        let dir = tempfile::tempdir().unwrap();
        let vault_path = dir.path().join("lock-vault");

        let _vault = Vault::create(&vault_path, TEST_PASSPHRASE).unwrap();

        let result = Vault::open(&vault_path, TEST_PASSPHRASE);
        assert!(result.is_err(), "concurrent open must fail with lock error");
    }
}
