//! Error types for the kryphos crate.

use snafu::Snafu;

/// Errors from vault operations (open, read, write, seal/unseal).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum VaultError {
    /// The vault file header is malformed or uses an unsupported version.
    #[snafu(display("invalid vault header: {reason}"))]
    InvalidHeader {
        /// Human-readable explanation.
        reason: String,
    },

    /// A requested entry was not found in the vault.
    #[snafu(display("entry not found: {name}"))]
    EntryNotFound {
        /// Name of the missing entry.
        name: String,
    },

    /// An entry with this name already exists.
    #[snafu(display("duplicate entry: {name}"))]
    DuplicateEntry {
        /// Name of the conflicting entry.
        name: String,
    },

    /// Serialization or deserialization of vault data failed.
    #[snafu(display("vault serialization error: {source}"))]
    Serialization {
        /// Underlying JSON error.
        source: serde_json::Error,
    },

    /// I/O error accessing the vault file.
    #[snafu(display("vault I/O error on {}: {source}", path.display()))]
    Io {
        /// Path of the file that triggered the error.
        path: std::path::PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// The passphrase is incorrect (key check decryption failed).
    #[snafu(display("wrong passphrase: decryption of key check failed"))]
    WrongPassphrase,

    /// The vault is already locked by another process.
    #[snafu(display("vault is locked by another process: {path}", path = path.display()))]
    Locked {
        /// Path to the vault directory.
        path: std::path::PathBuf,
    },

    /// The vault already exists at this path.
    #[snafu(display("vault already exists at {path}", path = path.display()))]
    AlreadyExists {
        /// Path to the existing vault directory.
        path: std::path::PathBuf,
    },

    /// The storage backend (fjall) returned an error.
    #[snafu(display("storage backend error: {message}"))]
    StorageBackend {
        /// Human-readable explanation.
        message: String,
    },

    /// A cryptographic operation on a vault entry failed.
    #[snafu(display("entry crypto error: {source}"))]
    EntryCrypto {
        /// Underlying crypto error.
        source: CryptoError,
    },

    /// The entry has been revoked and cannot be retrieved.
    #[snafu(display("entry revoked: {name}"))]
    EntryRevoked {
        /// Name of the revoked entry.
        name: String,
    },

    /// A revoked entry cannot be deleted (audit trail).
    #[snafu(display("cannot DELETE revoked entry: {name} (audit trail)"))]
    EntryNotDeletable {
        /// Name of the revoked entry.
        name: String,
    },

    /// Writing the tamper-evident vault audit log failed.
    #[snafu(display("tamper log error: {source}"))]
    TamperLog {
        /// Underlying tamper-log failure.
        source: koinon::TamperLogError,
    },
}

/// Errors from key generation, derivation, or loading.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum KeyError {
    /// Ed25519 key material is invalid (wrong length or not on curve).
    #[snafu(display("invalid ed25519 key: {reason}"))]
    InvalidEd25519Key {
        /// Human-readable explanation.
        reason: String,
    },

    /// Key derivation via Argon2id failed.
    #[snafu(display("key derivation failed: {reason}"))]
    DerivationFailed {
        /// Human-readable explanation.
        reason: String,
    },

    /// The provided key material has the wrong length.
    #[snafu(display("wrong key length: expected {expected}, got {actual}"))]
    WrongKeyLength {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        actual: usize,
    },
}

/// Low-level cryptographic operation errors.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum CryptoError {
    /// Authenticated decryption failed (wrong key or tampered ciphertext).
    #[snafu(display("decryption failed: ciphertext is invalid or key is wrong"))]
    DecryptionFailed,

    /// Encryption failed.
    #[snafu(display("encryption failed: {reason}"))]
    EncryptionFailed {
        /// Human-readable explanation.
        reason: String,
    },

    /// Signature verification failed.
    #[snafu(display("signature verification failed"))]
    SignatureInvalid,

    /// The provided nonce has the wrong length.
    #[snafu(display("invalid nonce length: expected {expected}, got {actual}"))]
    InvalidNonceLength {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        actual: usize,
    },
}
