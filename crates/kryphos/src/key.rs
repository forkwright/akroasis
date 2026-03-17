//! Cryptographic key types for vault encryption and installation identity.

use std::fmt;

use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use ed25519_dalek::Signer;

use crate::error::{CryptoError, KeyError};

/// ChaCha20-Poly1305 symmetric key length in bytes.
pub const VAULT_KEY_LEN: usize = 32;

/// Ed25519 secret key length in bytes.
pub const SIGNING_KEY_LEN: usize = 32;

/// Ed25519 public key length in bytes.
pub const VERIFYING_KEY_LEN: usize = 32;

/// Wrapper around an Ed25519 signing (secret) key.
///
/// The inner `ed25519_dalek::SigningKey` implements [`ZeroizeOnDrop`],
/// so secret material is automatically zeroized when this value is dropped.
/// Debug output is redacted.
pub struct SigningKey {
    inner: ed25519_dalek::SigningKey,
}

impl SigningKey {
    /// Generates a new random signing key.
    #[must_use]
    pub fn generate() -> Self {
        let mut csprng = rand_core::OsRng;
        Self {
            inner: ed25519_dalek::SigningKey::generate(&mut csprng),
        }
    }

    /// Constructs a signing key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::WrongKeyLength`] if `bytes` is not 32 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        let arr: [u8; SIGNING_KEY_LEN] =
            bytes.try_into().map_err(|_| KeyError::WrongKeyLength {
                expected: SIGNING_KEY_LEN,
                actual: bytes.len(),
            })?;
        Ok(Self {
            inner: ed25519_dalek::SigningKey::from_bytes(&arr),
        })
    }

    /// Returns the corresponding public verifying key.
    #[must_use]
    pub fn verifying_key(&self) -> VerifyingKey {
        VerifyingKey {
            inner: self.inner.verifying_key(),
        }
    }

    /// Signs a message and returns the signature.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> ed25519_dalek::Signature {
        self.inner.sign(message)
    }

    /// Returns the raw 32-byte secret key material.
    ///
    /// Caller is responsible for zeroizing the returned bytes.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; SIGNING_KEY_LEN] {
        self.inner.to_bytes()
    }

    /// Borrows the underlying `ed25519_dalek::SigningKey`.
    #[must_use]
    pub const fn as_inner(&self) -> &ed25519_dalek::SigningKey {
        &self.inner
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SigningKey([REDACTED])")
    }
}

/// Wrapper around an Ed25519 verifying (public) key.
///
/// This is public data and can be freely serialized and displayed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyingKey {
    inner: ed25519_dalek::VerifyingKey,
}

impl VerifyingKey {
    /// Constructs a verifying key from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::WrongKeyLength`] if `bytes` is not 32 bytes,
    /// or [`KeyError::InvalidEd25519Key`] if the bytes are not a valid
    /// curve point.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeyError> {
        let arr: [u8; VERIFYING_KEY_LEN] =
            bytes.try_into().map_err(|_| KeyError::WrongKeyLength {
                expected: VERIFYING_KEY_LEN,
                actual: bytes.len(),
            })?;
        let inner = ed25519_dalek::VerifyingKey::from_bytes(&arr).map_err(|e| {
            KeyError::InvalidEd25519Key {
                reason: e.to_string(),
            }
        })?;
        Ok(Self { inner })
    }

    /// Returns the raw 32-byte public key.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; VERIFYING_KEY_LEN] {
        self.inner.as_bytes()
    }

    /// Verifies a signature against a message.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::SignatureInvalid`] if the signature does not match.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &ed25519_dalek::Signature,
    ) -> Result<(), CryptoError> {
        use ed25519_dalek::Verifier;
        self.inner
            .verify(message, signature)
            .map_err(|_| CryptoError::SignatureInvalid)
    }

    /// Borrows the underlying `ed25519_dalek::VerifyingKey`.
    #[must_use]
    pub const fn as_inner(&self) -> &ed25519_dalek::VerifyingKey {
        &self.inner
    }
}

impl fmt::Debug for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VerifyingKey({})", hex(self.inner.as_bytes()))
    }
}

impl fmt::Display for VerifyingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(self.inner.as_bytes()))
    }
}

/// Ed25519 keypair representing a unique installation.
///
/// Used to sign tamper log entries, proving they originated from
/// this specific installation. The signing key is secret and zeroized
/// on drop. Debug output is redacted.
pub struct InstallationIdentity {
    signing: SigningKey,
    verifying: VerifyingKey,
}

impl InstallationIdentity {
    /// Generates a new random installation identity.
    #[must_use]
    pub fn generate() -> Self {
        let signing = SigningKey::generate();
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// Constructs an identity from an existing signing key.
    #[must_use]
    pub fn from_signing_key(signing: SigningKey) -> Self {
        let verifying = signing.verifying_key();
        Self { signing, verifying }
    }

    /// Returns a reference to the signing (secret) key.
    #[must_use]
    pub const fn signing_key(&self) -> &SigningKey {
        &self.signing
    }

    /// Returns a reference to the verifying (public) key.
    #[must_use]
    pub const fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying
    }

    /// Signs a message with this installation's private key.
    #[must_use]
    pub fn sign(&self, message: &[u8]) -> ed25519_dalek::Signature {
        self.signing.sign(message)
    }

    /// Verifies a signature against a message using this installation's public key.
    ///
    /// # Errors
    ///
    /// Returns [`CryptoError::SignatureInvalid`] if the signature does not match.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &ed25519_dalek::Signature,
    ) -> Result<(), CryptoError> {
        self.verifying.verify(message, signature)
    }

    /// Returns the raw 32-byte public key for identity fingerprinting.
    #[must_use]
    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.verifying.as_bytes()
    }

    /// Signs a tamper log entry hash, proving this installation produced it.
    ///
    /// `entry_hash` is the 32-byte BLAKE3 hash from `koinon::tamper_log::encode_entry`.
    #[must_use]
    pub fn sign_entry(&self, entry_hash: &[u8; 32]) -> ed25519_dalek::Signature {
        self.signing.sign(entry_hash)
    }
}

impl fmt::Debug for InstallationIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstallationIdentity")
            .field("signing", &"[REDACTED]")
            .field("verifying", &self.verifying)
            .finish()
    }
}

/// Symmetric key derived from a passphrase via Argon2id.
///
/// Used with ChaCha20-Poly1305 to encrypt/decrypt vault entries.
/// Zeroized on drop. Debug output is redacted.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey {
    bytes: [u8; VAULT_KEY_LEN],
}

impl VaultKey {
    /// Wraps raw key bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; VAULT_KEY_LEN]) -> Self {
        Self { bytes }
    }

    /// Returns the raw key material.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; VAULT_KEY_LEN] {
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
        f.write_str("VaultKey([REDACTED])")
    }
}

/// Formats a byte slice as lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        use fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // SigningKey
    // -----------------------------------------------------------------

    #[test]
    fn signing_key_generate_produces_valid_key() {
        let sk = SigningKey::generate();
        let vk = sk.verifying_key();
        assert_eq!(vk.as_bytes().len(), VERIFYING_KEY_LEN);
    }

    #[test]
    fn signing_key_from_bytes_accepts_32_bytes() {
        let bytes = [0x42; SIGNING_KEY_LEN];
        let sk = SigningKey::from_bytes(&bytes).unwrap();
        assert_eq!(sk.as_inner().to_bytes(), bytes);
    }

    #[test]
    fn signing_key_from_bytes_rejects_wrong_length() {
        let result = SigningKey::from_bytes(&[0u8; 16]);
        assert!(result.is_err());
    }

    #[test]
    fn signing_key_debug_is_redacted() {
        let sk = SigningKey::generate();
        let dbg = format!("{sk:?}");
        assert_eq!(dbg, "SigningKey([REDACTED])");
    }

    // -----------------------------------------------------------------
    // VerifyingKey
    // -----------------------------------------------------------------

    #[test]
    fn verifying_key_from_signing_key_round_trips() {
        let sk = SigningKey::generate();
        let vk = sk.verifying_key();
        let bytes = vk.as_bytes();
        let vk2 = VerifyingKey::from_bytes(bytes).unwrap();
        assert_eq!(vk, vk2);
    }

    #[test]
    fn verifying_key_from_bytes_rejects_wrong_length() {
        let result = VerifyingKey::from_bytes(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn verifying_key_serde_round_trip() {
        let sk = SigningKey::generate();
        let vk = sk.verifying_key();
        let json = serde_json::to_string(&vk).unwrap();
        let back: VerifyingKey = serde_json::from_str(&json).unwrap();
        assert_eq!(vk, back);
    }

    #[test]
    fn verifying_key_display_is_hex() {
        let sk = SigningKey::generate();
        let vk = sk.verifying_key();
        let display = vk.to_string();
        assert_eq!(display.len(), 64, "hex of 32 bytes should be 64 chars");
        assert!(
            display.chars().all(|c| c.is_ascii_hexdigit()),
            "display should be hex"
        );
    }

    // -----------------------------------------------------------------
    // InstallationIdentity
    // -----------------------------------------------------------------

    #[test]
    fn installation_identity_generate_has_matching_keys() {
        let id = InstallationIdentity::generate();
        let derived_vk = id.signing_key().verifying_key();
        assert_eq!(id.verifying_key(), &derived_vk);
    }

    #[test]
    fn installation_identity_from_signing_key_preserves_keypair() {
        let sk = SigningKey::generate();
        let expected_vk = sk.verifying_key();
        let id = InstallationIdentity::from_signing_key(sk);
        assert_eq!(id.verifying_key(), &expected_vk);
    }

    #[test]
    fn installation_identity_debug_redacts_signing_key() {
        let id = InstallationIdentity::generate();
        let dbg = format!("{id:?}");
        assert!(dbg.contains("[REDACTED]"), "debug must redact signing key");
        assert!(
            !dbg.contains("SigningKey("),
            "debug must not expose signing key innards"
        );
    }

    #[test]
    fn sign_verify_round_trip_succeeds() {
        let id = InstallationIdentity::generate();
        let message = b"tamper log entry hash";
        let signature = id.sign(message);
        assert!(
            id.verify(message, &signature).is_ok(),
            "verification must succeed for matching key"
        );
    }

    #[test]
    fn verify_with_wrong_key_fails() {
        let id1 = InstallationIdentity::generate();
        let id2 = InstallationIdentity::generate();
        let message = b"some message";
        let signature = id1.sign(message);
        assert!(
            id2.verify(message, &signature).is_err(),
            "verification must fail with a different key"
        );
    }

    #[test]
    fn verify_with_wrong_message_fails() {
        let id = InstallationIdentity::generate();
        let signature = id.sign(b"original");
        assert!(
            id.verify(b"tampered", &signature).is_err(),
            "verification must fail with a different message"
        );
    }

    #[test]
    fn public_key_bytes_returns_32_bytes() {
        let id = InstallationIdentity::generate();
        let bytes = id.public_key_bytes();
        assert_eq!(bytes.len(), 32, "public key must be 32 bytes");
        assert_eq!(
            &bytes,
            id.verifying_key().as_bytes(),
            "public_key_bytes must match verifying key"
        );
    }

    #[test]
    fn sign_entry_signs_32_byte_hash() {
        let id = InstallationIdentity::generate();
        let entry_hash = [0xAB_u8; 32];
        let signature = id.sign_entry(&entry_hash);
        assert!(
            id.verify(&entry_hash, &signature).is_ok(),
            "sign_entry signature must verify against the same hash"
        );
    }

    // -----------------------------------------------------------------
    // VaultKey
    // -----------------------------------------------------------------

    #[test]
    fn vault_key_from_bytes_round_trips() {
        let bytes = [0xAB; VAULT_KEY_LEN];
        let key = VaultKey::from_bytes(bytes);
        assert_eq!(key.as_bytes(), &bytes);
    }

    #[test]
    fn vault_key_debug_is_redacted() {
        let key = VaultKey::from_bytes([0; VAULT_KEY_LEN]);
        let dbg = format!("{key:?}");
        assert_eq!(dbg, "VaultKey([REDACTED])");
    }
}
