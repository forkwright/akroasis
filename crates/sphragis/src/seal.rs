//! Multi-recipient content-key sealing.
//!
//! Wraps one content key separately for each recipient device. Revoking a device
//! means re-sealing the (optionally rotated) content key for the remaining
//! recipients only.

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::envelope::{NONCE_LEN, derive_wrap_key, open, seal};
use crate::error::SealError;
use crate::hybrid::{DecapsulationKey, EncapsulationKey};
use crate::{SEAL_VERSION_V1, WRAP_DOMAIN_V1};

use rand_core::{OsRng, RngCore};

/// Content-key length (the symmetric key the consuming store uses for payloads).
pub const CONTENT_KEY_LEN: usize = 32;

/// A recipient identifier: BLAKE3 of the recipient's X-Wing encapsulation key.
///
/// Stable, public, and collision-resistant; used to select the right
/// [`WrappedContentKey`] for a device and bound as AEAD associated data.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientId(pub [u8; 32]);

impl RecipientId {
    /// Computes the id for an encapsulation key.
    #[must_use]
    pub fn of(ek: &EncapsulationKey) -> Self {
        Self(blake3::hash(&ek.to_bytes()).into())
    }
}

impl core::fmt::Debug for RecipientId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RecipientId({})", blake3::Hash::from(self.0).to_hex())
    }
}

/// A content key wrapped for exactly one recipient device.
///
/// Wire form (CBOR). `version` gates the construction; decoders reject unknown
/// versions. The recipient id and version are bound as AEAD associated data, so
/// a wrap for one device cannot be replayed against another.
#[derive(Clone, Serialize, Deserialize)]
pub struct WrappedContentKey {
    /// Construction version (1 = X-Wing + HKDF-SHA256 + ChaCha20-Poly1305).
    pub version: u8,
    /// Which device this wrap is for.
    pub recipient_id: RecipientId,
    /// X-Wing ciphertext (`ML-KEM ct || X25519 ct`). Length `CIPHERTEXT_LEN`.
    pub kem_ciphertext: Vec<u8>,
    /// ChaCha20-Poly1305 nonce.
    pub aead_nonce: [u8; NONCE_LEN],
    /// Sealed content key: `ciphertext || tag` (48 bytes for a 32-byte key).
    pub sealed_key: Vec<u8>,
}

impl core::fmt::Debug for WrappedContentKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WrappedContentKey")
            .field("version", &self.version)
            .field("recipient_id", &self.recipient_id)
            .finish_non_exhaustive()
    }
}

impl WrappedContentKey {
    /// Encodes to CBOR.
    ///
    /// # Errors
    ///
    /// Returns [`SealError::Serialization`] on encoding failure.
    pub fn to_cbor(&self) -> Result<Vec<u8>, SealError> {
        let mut buf = Vec::new();
        ciborium::into_writer(self, &mut buf).map_err(|e| SealError::Serialization {
            reason: e.to_string(),
        })?;
        Ok(buf)
    }

    /// Decodes from CBOR.
    ///
    /// # Errors
    ///
    /// Returns [`SealError::Serialization`] on decoding failure.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, SealError> {
        ciborium::from_reader(bytes).map_err(|e| SealError::Serialization {
            reason: e.to_string(),
        })
    }

    /// Associated data bound into the AEAD: `version || recipient_id`.
    fn aad(&self) -> [u8; 1 + 32] {
        let mut aad = [0u8; 33];
        aad[0] = self.version;
        aad[1..].copy_from_slice(&self.recipient_id.0);
        aad
    }
}

/// Seals a content key for each recipient device.
///
/// Returns one [`WrappedContentKey`] per recipient; all unseal to the same
/// `content_key`. The order of the output matches `recipients`.
///
/// # Errors
///
/// Returns a [`SealError`] if HKDF or the AEAD seal fails for any recipient.
pub fn seal_for(
    content_key: &[u8; CONTENT_KEY_LEN],
    recipients: &[EncapsulationKey],
) -> Result<Vec<WrappedContentKey>, SealError> {
    let mut out = Vec::with_capacity(recipients.len());
    for ek in recipients {
        let recipient_id = RecipientId::of(ek);
        let (kem_ciphertext, ss) = ek.encapsulate();

        let wrap_key = derive_wrap_key(ss.as_slice(), WRAP_DOMAIN_V1)?;

        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);

        let mut wck = WrappedContentKey {
            version: SEAL_VERSION_V1,
            recipient_id,
            kem_ciphertext,
            aead_nonce: nonce,
            sealed_key: Vec::new(),
        };
        wck.sealed_key = seal(&wrap_key, &nonce, content_key, &wck.aad())?;
        out.push(wck);
    }
    Ok(out)
}

/// Unseals a wrapped content key with this device's decapsulation key.
///
/// # Errors
///
/// Returns [`SealError::UnsupportedVersion`] for an unknown version,
/// [`SealError::AeadOpen`] for the wrong recipient / tampered ciphertext, or a
/// KEM error for a corrupted ciphertext.
pub fn unseal(
    dk: &DecapsulationKey,
    wck: &WrappedContentKey,
) -> Result<Zeroizing<Vec<u8>>, SealError> {
    if wck.version != SEAL_VERSION_V1 {
        return Err(SealError::UnsupportedVersion {
            version: wck.version,
        });
    }
    let ss = dk.decapsulate(&wck.kem_ciphertext)?;
    let wrap_key = derive_wrap_key(ss.as_slice(), WRAP_DOMAIN_V1)?;
    open(&wrap_key, &wck.aead_nonce, &wck.sealed_key, &wck.aad())
}
