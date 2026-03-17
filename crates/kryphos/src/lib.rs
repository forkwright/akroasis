//! κρυφός — credential vault and installation identity.
//!
//! Provides the data model for encrypted credential storage
//! and Ed25519-based installation identity.

pub mod crypto;
pub mod error;
pub mod key;
pub mod vault;

pub use crypto::{decrypt, derive_key, encrypt, generate_salt};
pub use error::{CryptoError, KeyError, VaultError};
pub use key::{InstallationIdentity, SigningKey, VaultKey, VerifyingKey};
pub use vault::{
    CredentialType, EntryMetadata, KdfParams, NONCE_LEN, SALT_LEN, VAULT_VERSION, VaultEntry,
    VaultHeader,
};
