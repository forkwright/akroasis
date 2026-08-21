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

    /// The passphrase is empty, so the derived key carries no entropy.
    #[snafu(display("passphrase must not be empty"))]
    EmptyPassphrase,

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

    /// No vault has been initialized at this path.
    ///
    /// Distinct from [`VaultError::Io`]: absence is the expected outcome of
    /// opening a path before `create`, not a filesystem failure, and callers
    /// branch on it to offer initialization.
    #[snafu(display("no vault initialized at {path}", path = path.display()))]
    NotInitialized {
        /// Path that holds no vault.
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

    /// The entry has expired and cannot be retrieved.
    #[snafu(display("entry expired: {name}"))]
    EntryExpired {
        /// Name of the expired entry.
        name: String,
    },

    /// A revoked entry cannot be deleted (audit trail).
    #[snafu(display("cannot DELETE revoked entry: {name} (audit trail)"))]
    EntryNotDeletable {
        /// Name of the revoked entry.
        name: String,
    },

    /// The audit entry for a vault mutation was written, but the mutation
    /// itself then failed.
    ///
    /// WHY this is a distinct variant rather than the underlying error alone:
    /// the two failure directions carry opposite consequences and a caller
    /// must be able to tell them apart. A plain mutation error means nothing
    /// happened. This one means the tamper-evident log now carries an entry
    /// for an operation that did not take effect, so a later reader
    /// reconciling the log against the vault will find a record with no
    /// corresponding change and must not read that as tampering.
    #[snafu(display(
        "vault audit for '{operation}' on '{name}' was recorded but the mutation failed: {source}"
    ))]
    AuditedMutationFailed {
        /// Name of the entry the mutation targeted.
        name: String,
        /// The operation recorded in the audit log.
        operation: &'static str,
        /// Why the mutation failed after its audit entry was written.
        source: Box<VaultError>,
    },

    /// Writing the tamper-evident vault audit log failed.
    #[snafu(display("tamper log error: {source}"))]
    TamperLog {
        /// Underlying tamper-log failure.
        source: koinon::TamperLogError,
    },

    /// A field passed to [`crate::crypto::entry_aad`] is too long to encode
    /// under its 4-byte length prefix.
    ///
    /// INVARIANT guard, not a realistic runtime case: every current caller
    /// passes a fixed-size salt, a vault entry name, or a JSON-serialized
    /// `CredentialType`, none of which approach `u32::MAX` bytes. Erroring
    /// here is what keeps the AAD's length-prefix encoding canonical —
    /// silently truncating the length instead would let two different
    /// (field, length) pairs collide on the same encoded bytes.
    #[snafu(display(
        "AAD field '{field}' is {len} bytes, which exceeds the u32 length-prefix limit"
    ))]
    FieldTooLarge {
        /// Which field overflowed the length prefix.
        field: &'static str,
        /// The field's actual byte length.
        len: usize,
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

    /// The decrypted plaintext could not be parsed as a valid signing key.
    #[snafu(display("unseal produced invalid key material: {source}"))]
    KeyParse {
        /// Underlying key-parse error.
        source: KeyError,
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

    /// The KDF parameters are outside the accepted range.
    #[snafu(display("unacceptable KDF parameters: {reason}"))]
    InvalidKdfParams {
        /// Which bound was violated, and by what.
        reason: String,
    },

    /// The provided salt has the wrong length.
    #[snafu(display("invalid salt length: expected {expected}, got {actual}"))]
    InvalidSaltLength {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        actual: usize,
    },
}
