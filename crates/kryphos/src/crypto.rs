//! Argon2id key derivation and ChaCha20-Poly1305 authenticated encryption.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, aead::Aead};
use snafu::{ResultExt, Snafu};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

const ARGON2_M_COST_KIB: u32 = 65_536;
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Errors from cryptographic operations.
#[derive(Debug, Snafu)]
#[expect(missing_docs, reason = "snafu display messages document each variant")]
pub enum CryptoError {
    #[snafu(display("key derivation failed"))]
    KeyDerivation {
        source: argon2::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("random number generation failed"))]
    RandomGeneration {
        source: getrandom::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("encryption failed"))]
    Encryption {
        source: chacha20poly1305::aead::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("ciphertext too short: expected at least {expected} bytes, got {actual}"))]
    CiphertextTooShort {
        expected: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("decryption failed"))]
    Decryption {
        source: chacha20poly1305::aead::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// 256-bit symmetric key derived from a passphrase.
///
/// Zeroed on drop. Constant-time equality comparison.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey {
    bytes: [u8; KEY_LEN],
}

impl VaultKey {
    /// Returns the raw key bytes.
    pub const fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.bytes
    }
}

impl PartialEq for VaultKey {
    fn eq(&self, other: &Self) -> bool {
        self.bytes.ct_eq(&other.bytes).into()
    }
}

impl Eq for VaultKey {}

impl fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultKey").finish_non_exhaustive()
    }
}

/// Generates a 32-byte cryptographic salt from OS randomness.
///
/// # Errors
///
/// Returns [`CryptoError::RandomGeneration`] if the OS RNG is unavailable.
pub fn generate_salt() -> Result<[u8; KEY_LEN], CryptoError> {
    let mut salt = [0u8; KEY_LEN];
    getrandom::getrandom(&mut salt).context(RandomGenerationSnafu)?;
    Ok(salt)
}

/// Derives a 256-bit key from a passphrase and salt using Argon2id.
///
/// Parameters: m=64 MiB, t=3, p=4.
///
/// # Errors
///
/// Returns [`CryptoError::KeyDerivation`] if the salt is too short (minimum 8 bytes)
/// or if the Argon2id computation fails.
pub fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<VaultKey, CryptoError> {
    let params = Params::new(
        ARGON2_M_COST_KIB,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(KEY_LEN),
    )
    .context(KeyDerivationSnafu)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key_bytes = [0u8; KEY_LEN];
    argon2
        .hash_password_into(passphrase, salt, &mut key_bytes)
        .context(KeyDerivationSnafu)?;
    Ok(VaultKey { bytes: key_bytes })
}

/// Encrypts plaintext using ChaCha20-Poly1305 with a random nonce.
///
/// Returns `nonce (12 bytes) || ciphertext || tag (16 bytes)`.
///
/// # Errors
///
/// Returns [`CryptoError::RandomGeneration`] if nonce generation fails, or
/// [`CryptoError::Encryption`] if the AEAD operation fails.
pub fn encrypt(key: &VaultKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.bytes.as_ref().into());

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce_bytes).context(RandomGenerationSnafu)?;
    let nonce = chacha20poly1305::Nonce::from(nonce_bytes);

    let ciphertext = cipher.encrypt(&nonce, plaintext).context(EncryptionSnafu)?;

    let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypts ciphertext produced by [`encrypt`].
///
/// Expects `nonce (12 bytes) || ciphertext || tag (16 bytes)`.
///
/// # Errors
///
/// Returns [`CryptoError::CiphertextTooShort`] if the input is shorter than the nonce,
/// or [`CryptoError::Decryption`] if authentication fails (wrong key or tampered data).
pub fn decrypt(key: &VaultKey, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    snafu::ensure!(
        ciphertext.len() > NONCE_LEN,
        CiphertextTooShortSnafu {
            expected: NONCE_LEN + 1,
            actual: ciphertext.len(),
        }
    );

    let (nonce_bytes, encrypted) = ciphertext.split_at(NONCE_LEN);
    let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.bytes.as_ref().into());

    cipher.decrypt(nonce, encrypted).context(DecryptionSnafu)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "tests use expect/unwrap for clarity and indexing for nonce extraction"
)]
mod tests {
    use super::*;

    fn test_salt() -> [u8; KEY_LEN] {
        let mut salt = [0u8; KEY_LEN];
        for (i, byte) in salt.iter_mut().enumerate() {
            *byte = i as u8;
        }
        salt
    }

    #[test]
    fn derive_key_deterministic_for_same_inputs() {
        let salt = test_salt();
        let key_a = derive_key(b"correct horse battery staple", &salt)
            .expect("key derivation should succeed");
        let key_b = derive_key(b"correct horse battery staple", &salt)
            .expect("key derivation should succeed");
        assert_eq!(key_a, key_b);
    }

    #[test]
    fn derive_key_differs_for_different_passphrases() {
        let salt = test_salt();
        let key_a = derive_key(b"passphrase-one", &salt).expect("key derivation should succeed");
        let key_b = derive_key(b"passphrase-two", &salt).expect("key derivation should succeed");
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn derive_key_differs_for_different_salts() {
        let salt_a = test_salt();
        let mut salt_b = test_salt();
        salt_b[0] = 0xFF;
        let key_a = derive_key(b"same-passphrase", &salt_a).expect("key derivation should succeed");
        let key_b = derive_key(b"same-passphrase", &salt_b).expect("key derivation should succeed");
        assert_ne!(key_a, key_b);
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");
        let plaintext = b"secret vault entry";

        let ciphertext = encrypt(&key, plaintext).expect("encryption should succeed");
        let decrypted = decrypt(&key, &ciphertext).expect("decryption should succeed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn encrypt_decrypt_empty_plaintext() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");

        let ciphertext = encrypt(&key, b"").expect("encryption should succeed");
        let decrypted = decrypt(&key, &ciphertext).expect("decryption should succeed");

        assert!(decrypted.is_empty());
    }

    #[test]
    fn encrypt_decrypt_large_payload() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");
        let plaintext = vec![0xAB_u8; 1_000_000];

        let ciphertext = encrypt(&key, &plaintext).expect("encryption should succeed");
        let decrypted = decrypt(&key, &ciphertext).expect("decryption should succeed");

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let salt = test_salt();
        let key = derive_key(b"correct-key", &salt).expect("key derivation should succeed");
        let wrong_key = derive_key(b"wrong-key", &salt).expect("key derivation should succeed");

        let ciphertext = encrypt(&key, b"secret").expect("encryption should succeed");
        let result = decrypt(&wrong_key, &ciphertext);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CryptoError::Decryption { .. }
        ));
    }

    #[test]
    fn decrypt_tampered_ciphertext_fails() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");

        let mut ciphertext = encrypt(&key, b"secret").expect("encryption should succeed");
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;

        let result = decrypt(&key, &ciphertext);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CryptoError::Decryption { .. }
        ));
    }

    #[test]
    fn decrypt_too_short_ciphertext_fails() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");

        let result = decrypt(&key, &[0u8; NONCE_LEN]);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CryptoError::CiphertextTooShort { .. }
        ));
    }

    #[test]
    fn nonce_is_random_per_encryption() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");
        let plaintext = b"same plaintext";

        let ct_a = encrypt(&key, plaintext).expect("encryption should succeed");
        let ct_b = encrypt(&key, plaintext).expect("encryption should succeed");

        assert_ne!(
            ct_a, ct_b,
            "two encryptions of the same plaintext must differ"
        );

        let nonce_a = &ct_a[..NONCE_LEN];
        let nonce_b = &ct_b[..NONCE_LEN];
        assert_ne!(nonce_a, nonce_b, "nonces must differ between encryptions");
    }

    #[test]
    fn generate_salt_returns_random_bytes() {
        let salt_a = generate_salt().expect("salt generation should succeed");
        let salt_b = generate_salt().expect("salt generation should succeed");
        assert_ne!(salt_a, salt_b);
    }

    #[test]
    fn vault_key_debug_does_not_leak_bytes() {
        let salt = test_salt();
        let key = derive_key(b"test-passphrase", &salt).expect("key derivation should succeed");
        let debug = format!("{key:?}");
        assert!(!debug.contains('['));
        assert!(debug.contains("VaultKey"));
    }
}
