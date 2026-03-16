//! Error types for the kryphos crate.

use snafu::Snafu;
use std::path::PathBuf;

/// Crypto operation error.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum CryptoError {
    /// Encryption failed.
    #[snafu(display("encryption failed"))]
    Encrypt,

    /// Decryption failed (wrong key or tampered ciphertext).
    #[snafu(display("decryption failed — wrong key or tampered ciphertext"))]
    Decrypt,
}

/// Vault operation error.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum VaultError {
    /// Failed to create the vault directory.
    #[snafu(display("failed to create vault directory at {path}", path = path.display()))]
    CreateDir {
        /// Directory path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// Vault already exists at the given path.
    #[snafu(display("vault already exists at {path}", path = path.display()))]
    AlreadyExists {
        /// Vault path.
        path: PathBuf,
    },

    /// No vault found at the given path.
    #[snafu(display("vault not found at {path}", path = path.display()))]
    NotFound {
        /// Vault path.
        path: PathBuf,
    },

    /// Failed to write the vault header.
    #[snafu(display("failed to write vault header at {path}", path = path.display()))]
    WriteHeader {
        /// Header file path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// Failed to read the vault header.
    #[snafu(display("failed to read vault header at {path}", path = path.display()))]
    ReadHeader {
        /// Header file path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// Vault header is corrupted or incompatible.
    #[snafu(display("invalid vault header — corrupted or incompatible version"))]
    InvalidHeader,

    /// Passphrase does not match the vault.
    #[snafu(display("wrong passphrase"))]
    WrongPassphrase,

    /// Failed to open the fjall database.
    #[snafu(display("failed to open fjall database at {path}", path = path.display()))]
    OpenDatabase {
        /// Database path.
        path: PathBuf,
        /// Underlying fjall error.
        source: fjall::Error,
    },

    /// Failed to open a fjall keyspace.
    #[snafu(display("failed to open fjall keyspace"))]
    OpenKeyspace {
        /// Underlying fjall error.
        source: fjall::Error,
    },

    /// Fjall storage operation failed.
    #[snafu(display("fjall storage operation failed"))]
    Storage {
        /// Underlying fjall error.
        source: fjall::Error,
    },

    /// Encryption failed for a specific entry.
    #[snafu(display("encryption failed for entry '{name}'"))]
    EntryEncrypt {
        /// Entry name.
        name: String,
        /// Underlying crypto error.
        source: CryptoError,
    },

    /// Decryption failed for a specific entry.
    #[snafu(display("decryption failed for entry '{name}'"))]
    EntryDecrypt {
        /// Entry name.
        name: String,
        /// Underlying crypto error.
        source: CryptoError,
    },

    /// CBOR serialization failed.
    #[snafu(display("serialization failed"))]
    Serialize {
        /// Underlying ciborium error.
        source: ciborium::ser::Error<std::io::Error>,
    },

    /// CBOR deserialization failed.
    #[snafu(display("deserialization failed"))]
    Deserialize {
        /// Underlying ciborium error.
        source: ciborium::de::Error<std::io::Error>,
    },

    /// Entry not found in the vault.
    #[snafu(display("entry '{name}' not found in vault"))]
    EntryNotFound {
        /// Entry name.
        name: String,
    },

    /// Vault is locked by another process.
    #[snafu(display("vault is locked by another process at {path}", path = path.display()))]
    Locked {
        /// Vault path.
        path: PathBuf,
    },

    /// Failed to acquire the vault lock.
    #[snafu(display("failed to acquire vault lock at {path}", path = path.display()))]
    LockAcquire {
        /// Lock file path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
}
