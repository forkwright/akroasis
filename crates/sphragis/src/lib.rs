//! σφραγίς — post-quantum hybrid sealing for multi-device content-key distribution.
//!
//! Seals a 32-byte content key for one or more recipient devices so that only a
//! holder of the matching secret key can recover it, with security resting on
//! **both** a classical (X25519) and a post-quantum (ML-KEM-768) assumption.
//!
//! # Construction
//!
//! - KEM: **X-Wing** (`draft-connolly-cfrg-xwing-kem`) — X25519 + ML-KEM-768,
//!   combined via `SHA3-256(ss_M || ss_X || ct_X || pk_X || label)`.
//! - Envelope: **HKDF-SHA256** (null salt) expands the X-Wing shared secret under
//!   a versioned domain tag, then **ChaCha20-Poly1305** seals the content key.
//! - Wire format: versioned, per-recipient [`WrappedContentKey`] (CBOR).
//!
//! # Status — UNAUDITED PREVIEW
//!
//! WARNING: this is unaudited cryptography behind the `preview-pq` feature. The
//! known-answer tests prove the construction matches the published standards;
//! they do not substitute for a cryptographic review. Do not use on the default
//! binary path. See `DECISION.md` and akroasis#131.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]

#[cfg(feature = "preview-pq")]
pub mod envelope;
#[cfg(feature = "preview-pq")]
pub mod error;
#[cfg(feature = "preview-pq")]
pub mod hybrid;
#[cfg(feature = "preview-pq")]
pub mod seal;

#[cfg(feature = "preview-pq")]
pub use error::SealError;
#[cfg(feature = "preview-pq")]
pub use hybrid::{DecapsulationKey, EncapsulationKey, HybridKem, SharedSecret};
#[cfg(feature = "preview-pq")]
pub use seal::{CONTENT_KEY_LEN, RecipientId, WrappedContentKey, seal_for, unseal};

/// Wire-format version for the v1 sealing construction.
///
/// v1 is X-Wing + HKDF-SHA256 + ChaCha20-Poly1305. Future primitive changes
/// increment this; decoders reject unknown versions rather than reinterpreting
/// bytes.
pub const SEAL_VERSION_V1: u8 = 1;

/// HKDF `info` domain-separation tag for the v1 content-key wrap.
///
/// INVARIANT: changing the construction requires a new version + a new tag.
pub const WRAP_DOMAIN_V1: &[u8] = b"akroasis-sphragis-ck-wrap-v1";
