//! Content-key envelope: HKDF-SHA256 expansion + ChaCha20-Poly1305 sealing.
//!
//! The X-Wing shared secret is expanded under a versioned domain tag into a
//! 32-byte wrapping key, which seals the content key with ChaCha20-Poly1305. The
//! recipient id and version are bound as AEAD associated data.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::SealError;

/// AEAD nonce length (ChaCha20-Poly1305).
pub const NONCE_LEN: usize = 12;
/// Wrapping-key length derived from HKDF.
pub const WRAP_KEY_LEN: usize = 32;

/// Derives the 32-byte wrapping key from a hybrid shared secret.
///
/// `HKDF-SHA256(salt = 32 zero bytes, ikm = shared_secret, info = domain)`.
/// A null (zero-filled) salt is used per the PQXDH/SP 800-56C convention for a
/// uniformly-random IKM.
///
/// # Errors
///
/// Returns [`SealError::HkdfExpand`] if expansion fails (cannot occur for a
/// 32-byte output, but surfaced rather than panicking).
pub fn derive_wrap_key(
    shared_secret: &[u8],
    domain: &[u8],
) -> Result<Zeroizing<[u8; WRAP_KEY_LEN]>, SealError> {
    let salt = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared_secret);
    let mut okm = Zeroizing::new([0u8; WRAP_KEY_LEN]);
    hk.expand(domain, okm.as_mut_slice())
        .map_err(|_| SealError::HkdfExpand)?;
    Ok(okm)
}

/// Seals `content_key` under `wrap_key`, binding `aad`. Returns
/// `nonce || ciphertext || tag`.
///
/// # Errors
///
/// Returns [`SealError::AeadSeal`] if the AEAD operation fails.
pub fn seal(
    wrap_key: &[u8; WRAP_KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    content_key: &[u8],
    aad: &[u8],
) -> Result<Vec<u8>, SealError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(wrap_key));
    cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: content_key,
                aad,
            },
        )
        .map_err(|_| SealError::AeadSeal)
}

/// Opens a sealed content key produced by [`seal`].
///
/// # Errors
///
/// Returns [`SealError::AeadOpen`] on a wrong key, wrong recipient, tampered
/// ciphertext, or wrong associated data.
pub fn open(
    wrap_key: &[u8; WRAP_KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    sealed: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, SealError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(wrap_key));
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad })
        .map(Zeroizing::new)
        .map_err(|_| SealError::AeadOpen)
}
