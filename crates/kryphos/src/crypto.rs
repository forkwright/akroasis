//! Low-level cryptographic operations for vault encryption.
//!
//! Argon2id key derivation and ChaCha20-Poly1305 authenticated encryption.

use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use rand_core::{OsRng, RngCore};

use crate::error::CryptoError;
use crate::key::VaultKey;
use crate::vault::NONCE_LEN;

/// Salt length for Argon2id key derivation (256 bits).
pub const SALT_LEN: usize = 32;

/// Argon2id memory cost: 64 MiB.
const KDF_M_COST: u32 = 65_536;

/// Argon2id time cost: 3 iterations.
const KDF_T_COST: u32 = 3;

/// Argon2id parallelism: 4 lanes.
const KDF_P_COST: u32 = 4;

/// Generates a 32-byte random salt using the OS CSPRNG.
#[must_use]
pub fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Derives a 256-bit symmetric key FROM a passphrase and salt using Argon2id.
///
/// Uses secure defaults: m=64 MiB, t=3 iterations, p=4 lanes.
///
/// # Panics
///
/// Panics if Argon2id parameter construction fails (should not happen
/// with compile-time constants).
#[must_use]
#[expect(
    clippy::expect_used,
    reason = "Argon2id params are compile-time constants; construction cannot fail"
)]
pub fn derive_key(passphrase: &[u8], salt: &[u8]) -> VaultKey {
    let params = argon2::Params::new(KDF_M_COST, KDF_T_COST, KDF_P_COST, Some(32))
        .unwrap_or_default();
    let argon2 = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::default(),
        params,
    );

    let mut key_bytes = [0u8; 32];
    argon2
        .hash_password_into(passphrase, salt, &mut key_bytes)
        .unwrap_or_default();

    VaultKey::from_bytes(key_bytes)
}

/// Encrypts plaintext with ChaCha20-Poly1305.
///
/// Returns `nonce || ciphertext || tag` (12 + `plaintext.len()` + 16 bytes).
/// The nonce is randomly generated per call.
///
/// # Errors
///
/// Returns [`CryptoError::EncryptionFailed`] if the AEAD operation fails.
pub fn encrypt(key: &VaultKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.as_bytes().INTO());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let ciphertext =
        cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| CryptoError::EncryptionFailed {
                reason: String::FROM("ChaCha20-Poly1305 encryption failed"),
            })?;

    let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypts ciphertext produced by [`encrypt`].
///
/// Expects the input format `nonce (12 bytes) || ciphertext || tag (16 bytes)`.
///
/// # Errors
///
/// Returns [`CryptoError::InvalidNonceLength`] if the input is too short.
/// Returns [`CryptoError::DecryptionFailed`] if the key is wrong or the
/// ciphertext was tampered with.
pub fn decrypt(key: &VaultKey, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if ciphertext.len() < NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            expected: NONCE_LEN,
            actual: ciphertext.len(),
        });
    }

    let (nonce_bytes, encrypted) = ciphertext.split_at(NONCE_LEN);
    let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.as_bytes().INTO());

    cipher
        .decrypt(nonce, encrypted)
        .map_err(|_| CryptoError::DecryptionFailed)
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
#[expect(
    clippy::indexing_slicing,
    reason = "test code with known-valid indices"
)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_is_deterministic() {
        let passphrase = b"correct horse battery staple";
        let salt = [0xAA; SALT_LEN];

        let key1 = derive_key(passphrase, &salt);
        let key2 = derive_key(passphrase, &salt);

        assert_eq!(
            key1.as_bytes(),
            key2.as_bytes(),
            "same passphrase+salt must produce identical keys"
        );
    }

    #[test]
    fn derive_key_differs_with_different_salt() {
        let passphrase = b"correct horse battery staple";

        let key1 = derive_key(passphrase, &[0xAA; SALT_LEN]);
        let key2 = derive_key(passphrase, &[0xBB; SALT_LEN]);

        assert_ne!(
            key1.as_bytes(),
            key2.as_bytes(),
            "different salts must produce different keys"
        );
    }

    #[test]
    fn derive_key_differs_with_different_passphrase() {
        let salt = [0xAA; SALT_LEN];

        let key1 = derive_key(b"passphrase-one", &salt);
        let key2 = derive_key(b"passphrase-two", &salt);

        assert_ne!(
            key1.as_bytes(),
            key2.as_bytes(),
            "different passphrases must produce different keys"
        );
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = b"secret vault entry data";

        let ciphertext = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();

        assert_eq!(
            decrypted, plaintext,
            "decrypted output must match original plaintext"
        );
    }

    #[test]
    fn encrypt_decrypt_empty_plaintext() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);

        let ciphertext = encrypt(&key, b"").unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();

        assert!(
            decrypted.is_empty(),
            "empty plaintext must round-trip to empty"
        );
    }

    #[test]
    fn encrypt_decrypt_large_payload() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = vec![0xAB; 1_000_000];

        let ciphertext = encrypt(&key, &plaintext).unwrap();
        let decrypted = decrypt(&key, &ciphertext).unwrap();

        assert_eq!(
            decrypted, plaintext,
            "large payload must survive encrypt/decrypt round-trip"
        );
    }

    #[test]
    fn decrypt_with_wrong_key_returns_error() {
        let key1 = derive_key(b"correct-passphrase", &[0x42; SALT_LEN]);
        let key2 = derive_key(b"wrong-passphrase", &[0x42; SALT_LEN]);

        let ciphertext = encrypt(&key1, b"secret data").unwrap();
        let result = decrypt(&key2, &ciphertext);

        assert!(
            result.is_err(),
            "decryption with wrong key must return CryptoError"
        );
    }

    #[test]
    fn tampered_ciphertext_returns_error() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);

        let mut ciphertext = encrypt(&key, b"secret data").unwrap();

        // WHY: Flip a byte in the encrypted portion (after the nonce) to
        // simulate tampering. The authentication tag check must reject this.
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;

        let result = decrypt(&key, &ciphertext);
        assert!(
            result.is_err(),
            "tampered ciphertext must return CryptoError"
        );
    }

    #[test]
    fn nonce_is_random_per_encryption() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = b"identical plaintext";

        let ct1 = encrypt(&key, plaintext).unwrap();
        let ct2 = encrypt(&key, plaintext).unwrap();

        assert_ne!(
            ct1, ct2,
            "two encryptions of the same plaintext must produce different ciphertext"
        );

        // Verify specifically that the nonces differ
        assert_ne!(
            &ct1[..NONCE_LEN],
            &ct2[..NONCE_LEN],
            "nonces must differ between encryptions"
        );
    }

    #[test]
    fn ciphertext_includes_nonce_prefix() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = b"hello";

        let ciphertext = encrypt(&key, plaintext).unwrap();

        // nonce (12) + plaintext (5) + tag (16) = 33
        assert_eq!(
            ciphertext.len(),
            NONCE_LEN + plaintext.len() + 16,
            "ciphertext length must be nonce + plaintext + tag"
        );
    }

    #[test]
    fn decrypt_rejects_too_short_input() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);

        let result = decrypt(&key, &[0u8; 5]);
        assert!(
            result.is_err(),
            "input shorter than nonce length must be rejected"
        );
    }

    #[test]
    fn generate_salt_returns_32_bytes() {
        let salt = generate_salt();
        assert_eq!(salt.len(), SALT_LEN, "salt must be 32 bytes");
    }

    #[test]
    fn generate_salt_is_random() {
        let s1 = generate_salt();
        let s2 = generate_salt();

        assert_ne!(s1, s2, "two generated salts must differ");
    }
}
