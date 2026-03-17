//! κρυφός — credential vault and installation identity.
//!
//! Provides encrypted credential storage with a fjall-backed vault,
//! Argon2id key derivation, ChaCha20-Poly1305 encryption,
//! and Ed25519-based installation identity.

pub mod config;
pub mod crypto;
pub mod error;
pub mod key;
pub mod storage;
pub mod vault;

pub use config::VaultProvider;
pub use crypto::{decrypt, derive_key, encrypt, generate_salt};
pub use error::{CryptoError, KeyError, VaultError};
pub use key::{InstallationIdentity, SigningKey, VaultKey, VerifyingKey};
pub use storage::{DecryptedEntry, EntryInfo, Vault};
pub use vault::{
    CredentialType, EntryMetadata, KdfParams, NONCE_LEN, SALT_LEN, VAULT_VERSION, VaultEntry,
    VaultHeader, seal_signing_key, unseal_signing_key,
};
