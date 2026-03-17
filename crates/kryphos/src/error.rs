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
