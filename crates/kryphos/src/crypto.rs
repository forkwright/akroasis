//! Low-level cryptographic operations for vault encryption.
//!
//! Argon2id key derivation and ChaCha20-Poly1305 authenticated encryption.

use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, Payload};
use rand_core::{OsRng, RngCore};

use snafu::ResultExt;

use crate::error::{CryptoError, SerializationSnafu, VaultError};
use crate::key::VaultKey;
use crate::vault::{CredentialType, NONCE_LEN};

/// Salt length for Argon2id key derivation (256 bits).
pub const SALT_LEN: usize = 32;

/// Format version of the per-entry AEAD associated-data binding built by
/// [`entry_aad`].
///
/// Distinct from [`crate::vault::VAULT_VERSION`] (the vault header/on-disk
/// format): this versions only the identity binding baked into each
/// entry's ciphertext (forkwright/akroasis#283), so a future change to the
/// binding scheme does not force every unrelated header field to bump.
pub(crate) const ENTRY_ENVELOPE_VERSION: u8 = 1;

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
        .expect("Argon2id params are compile-time constants"); // SAFETY: KDF_M_COST/T_COST/P_COST are compile-time constants within valid ranges per argon2 docs
    let argon2 = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::default(),
        params,
    );

    let mut key_bytes = [0u8; 32];
    argon2
        .hash_password_into(passphrase, salt, &mut key_bytes)
        .expect("Argon2id KDF should not fail with valid inputs"); // SAFETY: key_bytes length (32) is within the OUTPUT_SIZE range documented by argon2::Params

    VaultKey::from_bytes(key_bytes)
}

/// Encrypts plaintext with ChaCha20-Poly1305.
///
/// `aad` is authenticated but not encrypted or stored in the output — the
/// caller must supply the identical bytes to [`decrypt`], or authentication
/// fails. Pass `b""` when there is nothing to bind.
///
/// Returns `nonce || ciphertext || tag` (12 + `plaintext.len()` + 16 bytes).
/// The nonce is randomly generated per call.
///
/// # Errors
///
/// Returns [`CryptoError::EncryptionFailed`] if the AEAD operation fails.
pub fn encrypt(key: &VaultKey, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = ChaCha20Poly1305::new(key.as_bytes().into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::EncryptionFailed {
            reason: String::from("ChaCha20-Poly1305 encryption failed"),
        })?;

    let mut output = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypts ciphertext produced by [`encrypt`].
///
/// `aad` must be byte-identical to the value passed to the original
/// [`encrypt`] call. A mismatch — a different entry's binding, a tampered
/// bound field, or simply the wrong bytes — fails authentication exactly
/// like a wrong key or a corrupted ciphertext; the caller cannot distinguish
/// which. This is the property that binds a ciphertext to its identity
/// rather than to the key alone.
///
/// Expects the input format `nonce (12 bytes) || ciphertext || tag (16 bytes)`.
///
/// # Errors
///
/// Returns [`CryptoError::InvalidNonceLength`] if the input is too short.
/// Returns [`CryptoError::DecryptionFailed`] if the key is wrong, `aad`
/// does not match what was used to encrypt, or the ciphertext was
/// tampered with.
pub fn decrypt(key: &VaultKey, ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if ciphertext.len() < NONCE_LEN {
        return Err(CryptoError::InvalidNonceLength {
            expected: NONCE_LEN,
            actual: ciphertext.len(),
        });
    }

    let (nonce_bytes, encrypted) = ciphertext.split_at(NONCE_LEN);
    let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.as_bytes().into());

    cipher
        .decrypt(
            nonce,
            Payload {
                msg: encrypted,
                aad,
            },
        )
        .map_err(|_| CryptoError::DecryptionFailed)
}

/// Builds the AEAD associated data binding a vault entry's ciphertext to
/// its identity: this vault instance, the entry's name (its fjall key),
/// its declared credential type, and the envelope version.
///
/// Verified on every [`decrypt`] call site alongside the ciphertext itself
/// (forkwright/akroasis#283) — moving a valid ciphertext beneath a
/// different name, or editing its stored `credential_type` /
/// `envelope_version` independently of the secret, changes this binding
/// and fails authentication instead of decrypting into the wrong slot.
///
/// # Errors
///
/// Returns [`VaultError::Serialization`] if `credential_type` cannot be
/// encoded — `CredentialType` derives `Serialize` over plain data, so this
/// does not fail in practice.
///
/// INVARIANT: every variable-length field is 4-byte-length-prefixed before
/// concatenation. Without this, e.g. `(name="ab", type="c")` and
/// `(name="a", type="bc")` would produce identical AAD bytes, letting a
/// relocated ciphertext smuggle a different name/type split through
/// authentication.
pub(crate) fn entry_aad(
    vault_salt: &[u8],
    name: &str,
    credential_type: &CredentialType,
    envelope_version: u8,
) -> Result<Vec<u8>, VaultError> {
    let type_bytes = serde_json::to_vec(credential_type).context(SerializationSnafu)?;

    let mut aad =
        Vec::with_capacity(1 + 4 + vault_salt.len() + 4 + name.len() + 4 + type_bytes.len());
    aad.push(envelope_version);
    aad.extend_from_slice(&checked_len_prefix("vault_salt", vault_salt.len())?);
    aad.extend_from_slice(vault_salt);
    aad.extend_from_slice(&checked_len_prefix("name", name.len())?);
    aad.extend_from_slice(name.as_bytes());
    aad.extend_from_slice(&checked_len_prefix("credential_type", type_bytes.len())?);
    aad.extend_from_slice(&type_bytes);
    Ok(aad)
}

/// Encodes `len` as a big-endian 4-byte length prefix for [`entry_aad`].
///
/// # Errors
///
/// Returns [`VaultError::FieldTooLarge`] if `len` exceeds `u32::MAX`.
// PIN(akroasis#383 review): behaviorally identical to the inline
// `.unwrap_or(u32::MAX)` this replaces — still clamps rather than errors.
// This is the exact defect the follow-up commit fixes.
#[expect(
    clippy::unnecessary_wraps,
    reason = "PIN akroasis#383 review: this pinned pre-fix body is intentionally infallible (clamps instead of erroring); the fix commit adds the Err path and this attribute goes with it"
)]
fn checked_len_prefix(_field: &'static str, len: usize) -> Result<[u8; 4], VaultError> {
    Ok(u32::try_from(len).unwrap_or(u32::MAX).to_be_bytes())
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

        let ciphertext = encrypt(&key, plaintext, b"").unwrap();
        let decrypted = decrypt(&key, &ciphertext, b"").unwrap();

        assert_eq!(
            decrypted, plaintext,
            "decrypted output must match original plaintext"
        );
    }

    #[test]
    fn encrypt_decrypt_empty_plaintext() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);

        let ciphertext = encrypt(&key, b"", b"").unwrap();
        let decrypted = decrypt(&key, &ciphertext, b"").unwrap();

        assert!(
            decrypted.is_empty(),
            "empty plaintext must round-trip to empty"
        );
    }

    #[test]
    fn encrypt_decrypt_large_payload() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = vec![0xAB; 1_000_000];

        let ciphertext = encrypt(&key, &plaintext, b"").unwrap();
        let decrypted = decrypt(&key, &ciphertext, b"").unwrap();

        assert_eq!(
            decrypted, plaintext,
            "large payload must survive encrypt/decrypt round-trip"
        );
    }

    #[test]
    fn decrypt_with_wrong_key_returns_error() {
        let key1 = derive_key(b"correct-passphrase", &[0x42; SALT_LEN]);
        let key2 = derive_key(b"wrong-passphrase", &[0x42; SALT_LEN]);

        let ciphertext = encrypt(&key1, b"secret data", b"").unwrap();
        let result = decrypt(&key2, &ciphertext, b"");

        assert!(
            result.is_err(),
            "decryption with wrong key must return CryptoError"
        );
    }

    #[test]
    fn tampered_ciphertext_returns_error() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);

        let mut ciphertext = encrypt(&key, b"secret data", b"").unwrap();

        // WHY: Flip a byte in the encrypted portion (after the nonce) to
        // simulate tampering. The authentication tag check must reject this.
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0xFF;

        let result = decrypt(&key, &ciphertext, b"");
        assert!(
            result.is_err(),
            "tampered ciphertext must return CryptoError"
        );
    }

    #[test]
    fn encrypt_decrypt_round_trip_with_associated_data() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = b"secret vault entry data";
        let aad = b"entry-identity-binding";

        let ciphertext = encrypt(&key, plaintext, aad).unwrap();
        let decrypted = decrypt(&key, &ciphertext, aad).unwrap();

        assert_eq!(
            decrypted, plaintext,
            "decrypted output must match original plaintext under matching AAD"
        );
    }

    #[test]
    fn decrypt_with_mismatched_associated_data_fails() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = b"secret vault entry data";

        let ciphertext = encrypt(&key, plaintext, b"entry-a").unwrap();
        let result = decrypt(&key, &ciphertext, b"entry-b");

        assert!(
            result.is_err(),
            "decryption with mismatched associated data must fail — this is the \
             AEAD property forkwright/akroasis#283 relies on to bind ciphertext \
             to its entry identity"
        );
    }

    #[test]
    fn decrypt_with_empty_aad_against_bound_ciphertext_fails() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);

        let ciphertext = encrypt(&key, b"secret data", b"entry-a").unwrap();
        let result = decrypt(&key, &ciphertext, b"");

        assert!(
            result.is_err(),
            "omitting AAD on decrypt must not silently succeed against a \
             ciphertext that was bound at encrypt time"
        );
    }

    #[test]
    fn nonce_is_random_per_encryption() {
        let key = derive_key(b"test-passphrase", &[0x42; SALT_LEN]);
        let plaintext = b"identical plaintext";

        let ct1 = encrypt(&key, plaintext, b"").unwrap();
        let ct2 = encrypt(&key, plaintext, b"").unwrap();

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

        let ciphertext = encrypt(&key, plaintext, b"").unwrap();

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

        let result = decrypt(&key, &[0u8; 5], b"");
        assert!(
            result.is_err(),
            "input shorter than nonce length must be rejected"
        );
    }

    // -----------------------------------------------------------------
    // AAD length-prefix guard (checked_len_prefix)
    // -----------------------------------------------------------------

    #[test]
    fn checked_len_prefix_accepts_u32_max() {
        let result = checked_len_prefix("field", u32::MAX as usize);
        assert_eq!(
            result.unwrap(),
            u32::MAX.to_be_bytes(),
            "the largest representable length must encode, not error"
        );
    }

    #[test]
    fn checked_len_prefix_rejects_one_past_u32_max() {
        // WHY exercised via the helper directly rather than a real
        // >4GiB-length `entry_aad` call: allocating gigabytes in a unit test
        // is impractical, and this arithmetic boundary needs no allocation
        // to exercise — `checked_len_prefix` IS the shipped guard `entry_aad`
        // calls, not a re-implementation of it.
        let result = checked_len_prefix("field", u32::MAX as usize + 1);
        assert!(
            matches!(
                result,
                Err(VaultError::FieldTooLarge { field: "field", len }) if len == u32::MAX as usize + 1
            ),
            "a length that cannot be represented in 4 bytes must error \
             rather than silently clamp to u32::MAX (which would collide \
             two different lengths onto the same AAD prefix), got {result:?}"
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

    // -----------------------------------------------------------------
    // IETF known-answer vector tests
    // -----------------------------------------------------------------

    /// RFC 8439 Section 2.8.2 known-answer vector for ChaCha20-Poly1305.
    ///
    /// Proves the underlying `chacha20poly1305` crate produces the correct
    /// ciphertext and authentication tag for the authoritative IETF test
    /// vector, including AAD authentication. Uses the raw AEAD API with a
    /// fixed nonce so the output is deterministic and comparable.
    ///
    /// Reference: <https://www.rfc-editor.org/rfc/rfc8439#section-2.8.2>
    #[expect(
        clippy::expect_used,
        reason = "RFC vector test — construction cannot fail"
    )]
    #[test]
    fn chacha20poly1305_rfc8439_test_vector() {
        use chacha20poly1305::ChaCha20Poly1305;
        use chacha20poly1305::aead::{Aead, KeyInit, Payload};

        // RFC 8439 Section 2.8.2 test vector.
        const RFC_KEY: [u8; 32] = [
            0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d,
            0x8e, 0x8f, 0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b,
            0x9c, 0x9d, 0x9e, 0x9f,
        ];
        const RFC_NONCE: [u8; 12] = [
            0x07, 0x00, 0x00, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47,
        ];
        const RFC_AAD: [u8; 12] = [
            0x50, 0x51, 0x52, 0x53, 0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7,
        ];
        const RFC_PLAINTEXT: &[u8] = b"Ladies and Gentlemen of the class of '99: \
            If I could offer you only one tip for the future, sunscreen would be it.";
        // Expected ciphertext (without tag) from RFC 8439 Section 2.8.2.
        const RFC_CIPHERTEXT: [u8; 114] = [
            0xd3, 0x1a, 0x8d, 0x34, 0x64, 0x8e, 0x60, 0xdb, 0x7b, 0x86, 0xaf, 0xbc, 0x53, 0xef,
            0x7e, 0xc2, 0xa4, 0xad, 0xed, 0x51, 0x29, 0x6e, 0x08, 0xfe, 0xa9, 0xe2, 0xb5, 0xa7,
            0x36, 0xee, 0x62, 0xd6, 0x3d, 0xbe, 0xa4, 0x5e, 0x8c, 0xa9, 0x67, 0x12, 0x82, 0xfa,
            0xfb, 0x69, 0xda, 0x92, 0x72, 0x8b, 0x1a, 0x71, 0xde, 0x0a, 0x9e, 0x06, 0x0b, 0x29,
            0x05, 0xd6, 0xa5, 0xb6, 0x7e, 0xcd, 0x3b, 0x36, 0x92, 0xdd, 0xbd, 0x7f, 0x2d, 0x77,
            0x8b, 0x8c, 0x98, 0x03, 0xae, 0xe3, 0x28, 0x09, 0x1b, 0x58, 0xfa, 0xb3, 0x24, 0xe4,
            0xfa, 0xd6, 0x75, 0x94, 0x55, 0x85, 0x80, 0x8b, 0x48, 0x31, 0xd7, 0xbc, 0x3f, 0xf4,
            0xde, 0xf0, 0x8e, 0x4b, 0x7a, 0x9d, 0xe5, 0x76, 0xd2, 0x65, 0x86, 0xce, 0xc6, 0x4b,
            0x61, 0x16,
        ];
        // Poly1305 authentication tag from RFC 8439 Section 2.8.2.
        const RFC_TAG: [u8; 16] = [
            0x1a, 0xe1, 0x0b, 0x59, 0x4f, 0x09, 0xe2, 0x6a, 0x7e, 0x90, 0x2e, 0xcb, 0xd0, 0x60,
            0x06, 0x91,
        ];

        let key = chacha20poly1305::Key::from(RFC_KEY);
        let nonce = chacha20poly1305::Nonce::from(RFC_NONCE);
        let cipher = ChaCha20Poly1305::new(&key);

        // Encrypt with AAD — the output is ciphertext || tag (postfix tag).
        let output = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: RFC_PLAINTEXT,
                    aad: &RFC_AAD,
                },
            )
            .expect("RFC 8439 test vector encryption must succeed");

        assert_eq!(
            output.len(),
            RFC_PLAINTEXT.len() + 16,
            "encrypted output must be plaintext length + 16-byte tag"
        );
        assert_eq!(
            &output[..RFC_PLAINTEXT.len()],
            RFC_CIPHERTEXT,
            "ciphertext bytes must match RFC 8439 Section 2.8.2 vector"
        );
        assert_eq!(
            &output[RFC_PLAINTEXT.len()..],
            RFC_TAG,
            "Poly1305 tag must match RFC 8439 Section 2.8.2 vector"
        );

        // Verify decryption recovers the original plaintext.
        let mut ct_with_tag = Vec::from(RFC_CIPHERTEXT.as_slice());
        ct_with_tag.extend_from_slice(&RFC_TAG);
        let decrypted = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: &ct_with_tag,
                    aad: &RFC_AAD,
                },
            )
            .expect("RFC 8439 test vector decryption must succeed");
        assert_eq!(
            decrypted, RFC_PLAINTEXT,
            "decrypted RFC vector must equal original plaintext"
        );
    }

    /// RFC 9106 known-answer vector for Argon2id version 0x13.
    ///
    /// Proves the underlying `argon2` crate produces the correct output for
    /// the authoritative IETF test vector (Argon2id, m=32 KiB, t=3, p=4,
    /// with secret and associated data). Bypasses the `derive_key` wrapper
    /// to exercise the primitive directly.
    ///
    /// Reference: <https://www.rfc-editor.org/rfc/rfc9106#appendix-B>
    #[expect(
        clippy::expect_used,
        reason = "RFC vector test — construction cannot fail"
    )]
    #[test]
    fn argon2id_rfc9106_test_vector() {
        // RFC 9106 Appendix B, Argon2id version 0x13 test vector.
        // Memory: 32 KiB, Iterations: 3, Parallelism: 4 lanes, Tag: 32 bytes.
        const RFC_PASSWORD: [u8; 32] = [0x01; 32];
        const RFC_SALT: [u8; 16] = [0x02; 16];
        const RFC_SECRET: [u8; 8] = [0x03; 8];
        const RFC_AD: [u8; 12] = [0x04; 12];
        // Expected 32-byte output tag from RFC 9106 Appendix B.
        const RFC_TAG: [u8; 32] = [
            0x0d, 0x64, 0x0d, 0xf5, 0x8d, 0x78, 0x76, 0x6c, 0x08, 0xc0, 0x37, 0xa3, 0x4a, 0x8b,
            0x53, 0xc9, 0xd0, 0x1e, 0xf0, 0x45, 0x2d, 0x75, 0xb6, 0x5e, 0xb5, 0x25, 0x20, 0xe9,
            0x6b, 0x01, 0xe6, 0x59,
        ];

        let ad = argon2::AssociatedData::new(&RFC_AD).expect("valid AD length");
        let mut builder = argon2::ParamsBuilder::new();
        builder.m_cost(32);
        builder.t_cost(3);
        builder.p_cost(4);
        builder.data(ad);
        builder.output_len(32);
        let params = builder
            .build()
            .expect("RFC 9106 Argon2id params must be valid");

        let ctx = argon2::Argon2::new_with_secret(
            &RFC_SECRET,
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            params,
        )
        .expect("RFC 9106 Argon2id context construction must succeed");

        let mut out = [0u8; 32];
        ctx.hash_password_into(&RFC_PASSWORD, &RFC_SALT, &mut out)
            .expect("RFC 9106 Argon2id KDF must not fail with valid inputs");

        assert_eq!(
            out, RFC_TAG,
            "Argon2id output must match RFC 9106 Appendix B test vector"
        );
    }
}
