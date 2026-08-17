//! Cryptographic key types for vault encryption and installation identity.

use std::fmt;

use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use ed25519_dalek::Signer;

use crate::error::{CryptoError, KeyError};

/// ChaCha20-Poly1305 symmetric key length in bytes.
pub(crate) const VAULT_KEY_LEN: usize = 32;

/// Ed25519 secret key length in bytes.
pub(crate) const SIGNING_KEY_LEN: usize = 32;

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
        let arr = zeroizing_key_array(bytes)?;
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

/// Copies `bytes` into a fixed-size array wrapped for zero-on-drop.
///
/// WHY: `TryInto<[u8; N]>` makes an unavoidable copy crossing from the
/// caller's slice into a stack array; `SigningKey::from_bytes` is the
/// call-frame directly above `unseal_signing_key` (vault.rs), whose own
/// decrypt-output copy is `Zeroizing`-wrapped (RUST/#218) — this closes the
/// next frame so that coverage does not stop one call short.
///
/// # Errors
///
/// Returns [`KeyError::WrongKeyLength`] if `bytes` is not `SIGNING_KEY_LEN`
/// bytes.
fn zeroizing_key_array(bytes: &[u8]) -> Result<Zeroizing<[u8; SIGNING_KEY_LEN]>, KeyError> {
    let arr: [u8; SIGNING_KEY_LEN] = bytes.try_into().map_err(|_| KeyError::WrongKeyLength {
        expected: SIGNING_KEY_LEN,
        actual: bytes.len(),
    })?;
    Ok(Zeroizing::new(arr))
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
        let _ = write!(s, "{b:02x}"); // WHY: fmt::Write for String is infallible; the Result exists only for the trait's generality over fallible writers.
        s
    })
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
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

    /// Dispositive by construction, same mechanism as
    /// `decrypted_secret_is_zeroized_on_drop_by_type` (kryphos storage
    /// tests): `[u8; N]` alone does not implement `ZeroizeOnDrop` (only
    /// `Zeroize`), so this specific bound fails to compile against a bare
    /// array and passes only because `zeroizing_key_array` returns
    /// `Zeroizing<[u8; N]>`.
    #[test]
    fn signing_key_from_bytes_intermediate_array_is_zeroize_on_drop_by_type() {
        fn assert_zeroizes_on_drop<T: ZeroizeOnDrop>(_: &T) {}

        let bytes = [0x11; SIGNING_KEY_LEN];
        let arr = zeroizing_key_array(&bytes).unwrap();
        assert_zeroizes_on_drop(&arr);
        assert_eq!(
            *arr, bytes,
            "wrapped array must carry the same bytes through"
        );
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

    // -----------------------------------------------------------------
    // IETF known-answer vector tests
    // -----------------------------------------------------------------

    /// RFC 8032 Section 7.1 Test Vector 1 for Ed25519.
    ///
    /// Proves the underlying `ed25519-dalek` crate signs the empty message
    /// with the RFC private key and produces the exact RFC signature, and
    /// that verification succeeds against the RFC public key.
    ///
    /// Reference: <https://www.rfc-editor.org/rfc/rfc8032#section-7.1>
    #[test]
    fn ed25519_rfc8032_test_vector_1() {
        use std::convert::TryFrom;

        // RFC 8032 Section 7.1, Test Vector 1.
        const RFC_PRIVATE_KEY: [u8; 32] = [
            0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec,
            0x2c, 0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03,
            0x1c, 0xae, 0x7f, 0x60,
        ];
        const RFC_PUBLIC_KEY: [u8; 32] = [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ];
        // Signature of the empty message with RFC_PRIVATE_KEY per RFC 8032.
        const RFC_SIGNATURE: [u8; 64] = [
            0xe5, 0x56, 0x43, 0x00, 0xc3, 0x60, 0xac, 0x72, 0x90, 0x86, 0xe2, 0xcc, 0x80, 0x6e,
            0x82, 0x8a, 0x84, 0x87, 0x7f, 0x1e, 0xb8, 0xe5, 0xd9, 0x74, 0xd8, 0x73, 0xe0, 0x65,
            0x22, 0x49, 0x01, 0x55, 0x5f, 0xb8, 0x82, 0x15, 0x90, 0xa3, 0x3b, 0xac, 0xc6, 0x1e,
            0x39, 0x70, 0x1c, 0xf9, 0xb4, 0x6b, 0xd2, 0x5b, 0xf5, 0xf0, 0x59, 0x5b, 0xbe, 0x24,
            0x65, 0x51, 0x41, 0x43, 0x8e, 0x7a, 0x10, 0x0b,
        ];

        let signing_key = SigningKey::from_bytes(&RFC_PRIVATE_KEY).unwrap();

        // Verify the derived public key matches the RFC public key.
        assert_eq!(
            signing_key.verifying_key().as_bytes(),
            &RFC_PUBLIC_KEY,
            "public key derived from RFC private key must match RFC public key"
        );

        // Sign the empty message and verify the signature matches the RFC vector.
        let signature = signing_key.sign(b"");
        let expected = ed25519_dalek::Signature::try_from(RFC_SIGNATURE.as_slice()).unwrap();
        assert_eq!(
            signature, expected,
            "signature of empty message must match RFC 8032 Section 7.1 Test Vector 1"
        );

        // Verify the signature round-trips through the VerifyingKey wrapper.
        let verifying_key = VerifyingKey::from_bytes(&RFC_PUBLIC_KEY).unwrap();
        assert!(
            verifying_key.verify(b"", &signature).is_ok(),
            "RFC signature must verify against RFC public key"
        );
    }
}
