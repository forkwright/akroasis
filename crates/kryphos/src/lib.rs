//! Vault cryptography: key derivation, authenticated encryption, and secret storage.

mod crypto;

pub use crypto::{CryptoError, VaultKey, decrypt, derive_key, encrypt, generate_salt};
