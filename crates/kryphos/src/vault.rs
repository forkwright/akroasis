//! Vault storage backend using fjall.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use compact_str::CompactString;
use fs2::FileExt;
use snafu::ResultExt;

use crate::crypto::{self, VaultKey};
use crate::error::{self, VaultError};
use crate::model::{
    CredentialType, EntryMetadata, HEADER_MAGIC, HEADER_VERSION, KdfParams, VERIFY_PLAINTEXT,
    VaultEntry, VaultEntryInner, VaultHeader,
};

/// File name for the vault header within the vault directory.
const HEADER_FILE: &str = "vault.header";

/// File name for the advisory lock.
const LOCK_FILE: &str = "vault.lock";

/// Fjall subdirectory within the vault directory.
const FJALL_DIR: &str = "data";

/// Fjall partition name for vault entries.
const ENTRIES_PARTITION: &str = "entries";

/// An opened credential vault backed by fjall.
pub struct Vault {
    _lock_file: File,
    key: VaultKey,
    db: fjall::Database,
    entries: fjall::Keyspace,
    path: PathBuf,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Create a new vault at `path` with the given passphrase.
    ///
    /// The directory must not already exist as a vault (contain a header file).
    ///
    /// # Errors
    ///
    /// Returns an error if the vault already exists, the directory cannot be created,
    /// or the storage backend fails to initialize.
    pub fn create(path: impl AsRef<Path>, passphrase: &[u8]) -> Result<Self, VaultError> {
        Self::create_with_params(path, passphrase, &KdfParams::default())
    }

    /// Create with explicit KDF parameters (useful for testing with fast params).
    pub(crate) fn create_with_params(
        path: impl AsRef<Path>,
        passphrase: &[u8],
        kdf_params: &KdfParams,
    ) -> Result<Self, VaultError> {
        let path = path.as_ref();

        let header_path = path.join(HEADER_FILE);
        if header_path.exists() {
            return Err(VaultError::AlreadyExists {
                path: path.to_path_buf(),
            });
        }

        fs::create_dir_all(path).context(error::CreateDirSnafu {
            path: path.to_path_buf(),
        })?;

        let lock = acquire_lock(path)?;
        let salt = crypto::generate_salt();
        let key = crypto::derive_key(passphrase, &salt, kdf_params);

        let verify_tag =
            crypto::encrypt(&key, VERIFY_PLAINTEXT).map_err(|source| VaultError::EntryEncrypt {
                name: "<verify>".to_string(),
                source,
            })?;

        let header = VaultHeader {
            version: HEADER_VERSION,
            salt: salt.to_vec(),
            kdf_params: kdf_params.clone(),
            verify_tag,
        };

        write_header(&header_path, &header)?;

        let fjall_path = path.join(FJALL_DIR);
        let db = fjall::Database::builder(&fjall_path)
            .open()
            .context(error::OpenDatabaseSnafu { path: fjall_path })?;
        let entries = db
            .keyspace(ENTRIES_PARTITION, fjall::KeyspaceCreateOptions::default)
            .context(error::OpenKeyspaceSnafu)?;

        Ok(Self {
            _lock_file: lock,
            key,
            db,
            entries,
            path: path.to_path_buf(),
        })
    }

    /// Open an existing vault at `path` with the given passphrase.
    ///
    /// # Errors
    ///
    /// Returns an error if the vault does not exist, the passphrase is wrong,
    /// the vault is locked by another process, or the storage backend fails.
    pub fn open(path: impl AsRef<Path>, passphrase: &[u8]) -> Result<Self, VaultError> {
        let path = path.as_ref();

        let header_path = path.join(HEADER_FILE);
        if !header_path.exists() {
            return Err(VaultError::NotFound {
                path: path.to_path_buf(),
            });
        }

        let lock = acquire_lock(path)?;
        let header = read_header(&header_path)?;

        let key = crypto::derive_key(passphrase, &header.salt, &header.kdf_params);

        crypto::decrypt(&key, &header.verify_tag).map_err(|_| VaultError::WrongPassphrase)?;

        let fjall_path = path.join(FJALL_DIR);
        let db = fjall::Database::builder(&fjall_path)
            .open()
            .context(error::OpenDatabaseSnafu { path: fjall_path })?;
        let entries = db
            .keyspace(ENTRIES_PARTITION, fjall::KeyspaceCreateOptions::default)
            .context(error::OpenKeyspaceSnafu)?;

        Ok(Self {
            _lock_file: lock,
            key,
            db,
            entries,
            path: path.to_path_buf(),
        })
    }

    /// Add a new entry to the vault.
    ///
    /// # Errors
    ///
    /// Returns an error if encryption or storage fails.
    pub fn add(
        &self,
        name: &str,
        credential_type: CredentialType,
        secret: &[u8],
    ) -> Result<(), VaultError> {
        let now_ms = jiff::Timestamp::now().as_millisecond();

        let inner = VaultEntryInner {
            metadata: EntryMetadata {
                name: CompactString::new(name),
                credential_type,
                created_at_ms: now_ms,
                rotated_at_ms: None,
                tags: Vec::new(),
            },
            secret: secret.to_vec(),
        };

        let serialized = cbor_encode(&inner)?;
        let encrypted =
            crypto::encrypt(&self.key, &serialized).map_err(|source| VaultError::EntryEncrypt {
                name: name.to_string(),
                source,
            })?;

        self.entries
            .insert(name.as_bytes(), encrypted)
            .context(error::StorageSnafu)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .context(error::StorageSnafu)?;

        Ok(())
    }

    /// Get a decrypted entry by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry is not found, decryption fails, or storage fails.
    pub fn get(&self, name: &str) -> Result<VaultEntry, VaultError> {
        let raw = self
            .entries
            .get(name.as_bytes())
            .context(error::StorageSnafu)?
            .ok_or_else(|| VaultError::EntryNotFound {
                name: name.to_string(),
            })?;

        let decrypted =
            crypto::decrypt(&self.key, &raw).map_err(|source| VaultError::EntryDecrypt {
                name: name.to_string(),
                source,
            })?;

        let inner: VaultEntryInner = cbor_decode(&decrypted)?;

        Ok(VaultEntry {
            metadata: inner.metadata,
            secret: inner.secret,
        })
    }

    /// List all entry names and metadata (without decrypted secrets).
    ///
    /// # Errors
    ///
    /// Returns an error if iteration or decryption fails.
    pub fn list(&self) -> Result<Vec<EntryMetadata>, VaultError> {
        let mut result = Vec::new();

        for guard in self.entries.iter() {
            let (key, value) = guard.into_inner().context(error::StorageSnafu)?;
            let decrypted =
                crypto::decrypt(&self.key, &value).map_err(|source| VaultError::EntryDecrypt {
                    name: String::from_utf8_lossy(&key).to_string(),
                    source,
                })?;
            let inner: VaultEntryInner = cbor_decode(&decrypted)?;
            result.push(inner.metadata);
        }

        Ok(result)
    }

    /// Remove an entry by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry is not found or storage fails.
    pub fn remove(&self, name: &str) -> Result<(), VaultError> {
        if self
            .entries
            .get(name.as_bytes())
            .context(error::StorageSnafu)?
            .is_none()
        {
            return Err(VaultError::EntryNotFound {
                name: name.to_string(),
            });
        }

        self.entries
            .remove(name.as_bytes())
            .context(error::StorageSnafu)?;
        self.db
            .persist(fjall::PersistMode::SyncAll)
            .context(error::StorageSnafu)?;

        Ok(())
    }

    /// Path to the vault directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn acquire_lock(vault_path: &Path) -> Result<File, VaultError> {
    let lock_path = vault_path.join(LOCK_FILE);

    // WHY: Create parent dir first — lock file creation needs the directory.
    fs::create_dir_all(vault_path).context(error::CreateDirSnafu {
        path: vault_path.to_path_buf(),
    })?;

    let file = File::create(&lock_path).context(error::LockAcquireSnafu { path: lock_path })?;

    file.try_lock_exclusive().map_err(|_| VaultError::Locked {
        path: vault_path.to_path_buf(),
    })?;

    Ok(file)
}

fn write_header(path: &Path, header: &VaultHeader) -> Result<(), VaultError> {
    let mut buf = Vec::new();
    buf.extend_from_slice(HEADER_MAGIC);

    let mut cbor_buf = Vec::new();
    ciborium::into_writer(header, &mut cbor_buf).map_err(|e| VaultError::WriteHeader {
        path: path.to_path_buf(),
        source: std::io::Error::other(e.to_string()),
    })?;

    buf.extend_from_slice(&(cbor_buf.len() as u32).to_le_bytes());
    buf.extend_from_slice(&cbor_buf);

    fs::write(path, &buf).context(error::WriteHeaderSnafu {
        path: path.to_path_buf(),
    })
}

fn read_header(path: &Path) -> Result<VaultHeader, VaultError> {
    let data = fs::read(path).context(error::ReadHeaderSnafu {
        path: path.to_path_buf(),
    })?;

    if data.len() < HEADER_MAGIC.len() + 4 {
        return Err(VaultError::InvalidHeader);
    }

    let (magic, rest) = data.split_at(HEADER_MAGIC.len());
    if magic != HEADER_MAGIC {
        return Err(VaultError::InvalidHeader);
    }

    let len_bytes: [u8; 4] = rest
        .get(..4)
        .ok_or(VaultError::InvalidHeader)?
        .try_into()
        .map_err(|_| VaultError::InvalidHeader)?;
    let cbor_len = u32::from_le_bytes(len_bytes) as usize;

    let cbor_data = rest.get(4..4 + cbor_len).ok_or(VaultError::InvalidHeader)?;
    ciborium::from_reader(cbor_data).map_err(|_| VaultError::InvalidHeader)
}

fn cbor_encode(value: &impl serde::Serialize) -> Result<Vec<u8>, VaultError> {
    let mut buf = Vec::new();
    ciborium::into_writer(value, &mut buf).context(error::SerializeSnafu)?;
    Ok(buf)
}

fn cbor_decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, VaultError> {
    ciborium::from_reader(data).context(error::DeserializeSnafu)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fast_params() -> KdfParams {
        KdfParams {
            m_cost: 256,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn create_and_open_round_trip() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");

        {
            let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();
            vault
                .add("test-key", CredentialType::ApiKey, b"secret123")
                .unwrap();
        }

        let vault = Vault::open(&vault_path, b"pass").unwrap();
        let entry = vault.get("test-key").unwrap();
        assert_eq!(entry.secret, b"secret123");
    }

    #[test]
    fn add_get_returns_original_secret() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        vault
            .add("api", CredentialType::ApiKey, b"key-abc")
            .unwrap();
        vault.add("psk", CredentialType::Psk, b"psk-xyz").unwrap();
        vault
            .add("cert", CredentialType::Certificate, b"-----BEGIN CERT-----")
            .unwrap();

        let api = vault.get("api").unwrap();
        assert_eq!(api.secret, b"key-abc");
        assert_eq!(api.metadata.credential_type, CredentialType::ApiKey);

        let psk = vault.get("psk").unwrap();
        assert_eq!(psk.secret, b"psk-xyz");

        let cert = vault.get("cert").unwrap();
        assert_eq!(cert.secret, b"-----BEGIN CERT-----");
    }

    #[test]
    fn list_returns_metadata_without_secrets() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        vault.add("alpha", CredentialType::ApiKey, b"s1").unwrap();
        vault.add("beta", CredentialType::Psk, b"s2").unwrap();

        let list = vault.list().unwrap();
        assert_eq!(list.len(), 2);

        let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[test]
    fn remove_deletes_entry() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        vault.add("temp", CredentialType::ApiKey, b"data").unwrap();
        assert!(vault.get("temp").is_ok());

        vault.remove("temp").unwrap();

        let err = vault.get("temp").unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "expected 'not found', got: {err}"
        );
    }

    #[test]
    fn remove_nonexistent_returns_error() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        let err = vault.remove("ghost").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn wrong_passphrase_on_open_returns_error() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");

        {
            Vault::create_with_params(&vault_path, b"correct", &fast_params()).unwrap();
        }

        let err = Vault::open(&vault_path, b"wrong").unwrap_err();
        assert!(
            err.to_string().contains("passphrase"),
            "expected passphrase error, got: {err}"
        );
    }

    #[test]
    fn concurrent_open_fails_with_lock_error() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");

        let _v1 = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        let err = Vault::open(&vault_path, b"pass").unwrap_err();
        assert!(
            err.to_string().contains("locked") || err.to_string().contains("lock"),
            "expected lock error, got: {err}"
        );
    }

    #[test]
    fn create_existing_vault_fails() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");

        {
            Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();
        }

        let err = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn open_nonexistent_vault_fails() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("nope");

        let err = Vault::open(&vault_path, b"pass").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn add_get_binary_secret() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        let binary = (0..=255).collect::<Vec<u8>>();
        vault
            .add(
                "bin",
                CredentialType::Custom(CompactString::new("binary-blob")),
                &binary,
            )
            .unwrap();

        let entry = vault.get("bin").unwrap();
        assert_eq!(entry.secret, binary);
        assert_eq!(
            entry.metadata.credential_type,
            CredentialType::Custom(CompactString::new("binary-blob"))
        );
    }

    #[test]
    fn list_empty_vault_returns_empty() {
        let dir = TempDir::new().unwrap();
        let vault_path = dir.path().join("vault");
        let vault = Vault::create_with_params(&vault_path, b"pass", &fast_params()).unwrap();

        let list = vault.list().unwrap();
        assert!(list.is_empty());
    }
}
