//! Encrypted vault storage using ChaCha20-Poly1305 AEAD.

use std::path::{Path, PathBuf};

use chacha20poly1305::{
    ChaCha20Poly1305,
    aead::{Aead, KeyInit},
};
use rand_core::{OsRng, RngCore};
use snafu::{ResultExt, Snafu};

/// Nonce length for ChaCha20-Poly1305 (96 bits).
const NONCE_LEN: usize = 12;

/// Errors from vault operations.
#[derive(Debug, Snafu)]
pub enum VaultError {
    /// I/O error accessing vault file.
    #[snafu(display("vault I/O error on {}: {source}", path.display()))]
    Io {
        /// Path of the file that triggered the error.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// AEAD encryption failed.
    #[snafu(display("encryption failed"))]
    Encrypt,

    /// AEAD decryption failed (wrong key or corrupted data).
    #[snafu(display("decryption failed: wrong key or corrupted data"))]
    Decrypt,

    /// Vault file too short to contain nonce and ciphertext.
    #[snafu(display("vault file corrupted: too short ({size} bytes)"))]
    TooShort {
        /// Actual file size.
        size: usize,
    },
}

/// A 256-bit key used to encrypt and decrypt vault contents.
#[derive(Clone)]
pub struct VaultKey([u8; 32]);

impl VaultKey {
    /// Creates a vault key from raw bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the raw key bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for VaultKey {
    fn drop(&mut self) {
        // WHY: Zeroize key material on drop to minimize exposure window.
        // black_box prevents the compiler from optimizing away the fill.
        self.0.fill(0);
        std::hint::black_box(&self.0);
    }
}

/// File-based encrypted storage for sensitive key material.
///
/// Stored format: `[12-byte nonce][ciphertext + 16-byte Poly1305 tag]`.
pub struct VaultStorage {
    path: PathBuf,
}

impl VaultStorage {
    /// Creates a vault storage backed by the given file path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_owned(),
        }
    }

    /// Returns the vault file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Encrypts `plaintext` with `key` and writes to the vault file.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Encrypt`] if AEAD encryption fails, or
    /// [`VaultError::Io`] on filesystem errors.
    pub fn store(&self, plaintext: &[u8], key: &VaultKey) -> Result<(), VaultError> {
        let cipher = ChaCha20Poly1305::new(key.0.as_ref().into());

        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = chacha20poly1305::Nonce::from(nonce_bytes);

        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| VaultError::Encrypt)?;

        let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        output.extend_from_slice(&nonce_bytes);
        output.extend_from_slice(&ciphertext);

        std::fs::write(&self.path, &output).context(IoSnafu { path: &self.path })
    }

    /// Reads the vault file, decrypts with `key`, and returns the plaintext.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Io`] on filesystem errors,
    /// [`VaultError::TooShort`] if the file is malformed, or
    /// [`VaultError::Decrypt`] if decryption fails.
    pub fn load(&self, key: &VaultKey) -> Result<Vec<u8>, VaultError> {
        let data = std::fs::read(&self.path).context(IoSnafu { path: &self.path })?;

        if data.len() < NONCE_LEN + 1 {
            return Err(VaultError::TooShort { size: data.len() });
        }

        let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
        let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
        let cipher = ChaCha20Poly1305::new(key.0.as_ref().into());

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| VaultError::Decrypt)
    }
}
