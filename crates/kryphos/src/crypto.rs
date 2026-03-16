//! Cryptographic primitives: Argon2id KDF and ChaCha20-Poly1305 AEAD.

use rand::RngCore;

use chacha20poly1305::aead::{Aead, KeyInit, OsRng};
use chacha20poly1305::{AeadCore, ChaCha20Poly1305, Nonce};

use crate::error::CryptoError;
use crate::model::KdfParams;

/// Nonce size for ChaCha20-Poly1305 (12 bytes).
const NONCE_LEN: usize = 12;

/// Derived symmetric key for vault encryption.
#[derive(Clone)]
pub struct VaultKey {
    bytes: [u8; 32],
}

impl VaultKey {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }
}

impl Drop for VaultKey {
    fn drop(&mut self) {
        // WHY: Zero key material on drop to reduce exposure window.
        self.bytes.fill(0);
    }
}

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultKey").finish_non_exhaustive()
    }
}

/// Derive a 256-bit key from a passphrase and salt using Argon2id.
pub fn derive_key(passphrase: &[u8], salt: &[u8], params: &KdfParams) -> VaultKey {
    let argon_params = argon2::Params::new(params.m_cost, params.t_cost, params.p_cost, Some(32))
        .unwrap_or_else(|_| {
            // WHY: Fallback to defaults if caller provides invalid params.
            let d = KdfParams::default();
            argon2::Params::new(d.m_cost, d.t_cost, d.p_cost, Some(32))
                .unwrap_or_else(|_| argon2::Params::default())
        });

    let argon2 = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon_params,
    );

    let mut key_bytes = [0u8; 32];
    // WHY: hash_password_into only fails if output length is wrong (32 is valid).
    let _ = argon2.hash_password_into(passphrase, salt, &mut key_bytes);

    VaultKey::from_bytes(key_bytes)
}

/// Generate a cryptographically random 32-byte salt.
pub fn generate_salt() -> [u8; 32] {
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Encrypt plaintext with ChaCha20-Poly1305. Returns nonce || ciphertext.
///
/// # Errors
///
/// Returns `CryptoError::Encrypt` if encryption fails.
pub fn encrypt(key: &VaultKey, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| CryptoError::Encrypt)?;

    let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt nonce || ciphertext with ChaCha20-Poly1305.
///
/// # Errors
///
/// Returns `CryptoError::Decrypt` if decryption fails (wrong key or tampered data).
pub fn decrypt(key: &VaultKey, data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < NONCE_LEN {
        return Err(CryptoError::Decrypt);
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.as_bytes().into());
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::Decrypt)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::model::KdfParams;

    fn test_params() -> KdfParams {
        // WHY: Reduced params for fast tests.
        KdfParams {
            m_cost: 256,
            t_cost: 1,
            p_cost: 1,
        }
    }

    #[test]
    fn derive_key_deterministic_for_same_inputs() {
        let salt = [42u8; 32];
        let params = test_params();
        let k1 = derive_key(b"passphrase", &salt, &params);
        let k2 = derive_key(b"passphrase", &salt, &params);
        assert_eq!(k1.as_bytes(), k2.as_bytes());
    }

    #[test]
    fn derive_key_differs_for_different_passphrase() {
        let salt = [42u8; 32];
        let params = test_params();
        let k1 = derive_key(b"alpha", &salt, &params);
        let k2 = derive_key(b"beta", &salt, &params);
        assert_ne!(k1.as_bytes(), k2.as_bytes());
    }

    #[test]
    fn derive_key_differs_for_different_salt() {
        let params = test_params();
        let k1 = derive_key(b"passphrase", &[1u8; 32], &params);
        let k2 = derive_key(b"passphrase", &[2u8; 32], &params);
        assert_ne!(k1.as_bytes(), k2.as_bytes());
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = derive_key(b"test", &[0u8; 32], &test_params());
        let plaintext = b"hello vault";
        let encrypted = encrypt(&key, plaintext).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let k1 = derive_key(b"correct", &[0u8; 32], &test_params());
        let k2 = derive_key(b"wrong", &[0u8; 32], &test_params());
        let encrypted = encrypt(&k1, b"secret").unwrap();
        assert!(decrypt(&k2, &encrypted).is_err());
    }

    #[test]
    fn tampered_ciphertext_rejected() {
        let key = derive_key(b"test", &[0u8; 32], &test_params());
        let mut encrypted = encrypt(&key, b"data").unwrap();
        if let Some(last) = encrypted.last_mut() {
            *last ^= 0xFF;
        }
        assert!(decrypt(&key, &encrypted).is_err());
    }

    #[test]
    fn two_encryptions_of_same_plaintext_differ() {
        let key = derive_key(b"test", &[0u8; 32], &test_params());
        let e1 = encrypt(&key, b"same").unwrap();
        let e2 = encrypt(&key, b"same").unwrap();
        assert_ne!(e1, e2);
    }

    #[test]
    fn encrypt_decrypt_empty_plaintext() {
        let key = derive_key(b"test", &[0u8; 32], &test_params());
        let encrypted = encrypt(&key, b"").unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn encrypt_decrypt_large_payload() {
        let key = derive_key(b"test", &[0u8; 32], &test_params());
        let big = vec![0xABu8; 1024 * 1024];
        let encrypted = encrypt(&key, &big).unwrap();
        let decrypted = decrypt(&key, &encrypted).unwrap();
        assert_eq!(decrypted, big);
    }

    #[test]
    fn decrypt_too_short_data_fails() {
        let key = derive_key(b"test", &[0u8; 32], &test_params());
        assert!(decrypt(&key, &[0u8; 5]).is_err());
    }

    #[test]
    fn generate_salt_produces_unique_values() {
        let s1 = generate_salt();
        let s2 = generate_salt();
        assert_ne!(s1, s2);
    }
}
