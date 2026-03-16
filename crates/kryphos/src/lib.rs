//! κρυφός — encryption, key management, credential vault, identity.

pub mod identity;
pub mod vault;

pub use ed25519_dalek::Signature;
pub use identity::{IdentityError, InstallationIdentity, verify_with_public_key};
pub use vault::{VaultError, VaultKey, VaultStorage};
