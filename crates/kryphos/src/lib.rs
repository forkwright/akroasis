//! κρυφός — credential vault and installation identity.

pub mod crypto;
pub mod error;
pub mod model;
pub mod vault;

pub use crypto::VaultKey;
pub use error::{CryptoError, VaultError};
pub use model::{CredentialType, EntryMetadata, KdfParams, VaultEntry, VaultHeader};
pub use vault::Vault;
