//! Error types for hybrid sealing.

use snafu::Snafu;

/// Errors produced by sealing, unsealing, and key handling.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum SealError {
    /// A key or ciphertext byte slice had the wrong length.
    #[snafu(display("wrong length for {what}: expected {expected}, got {actual}"))]
    WrongLength {
        /// What was being decoded (e.g. "encapsulation key").
        what: &'static str,
        /// Expected byte length.
        expected: usize,
        /// Actual byte length.
        actual: usize,
    },

    /// An ML-KEM key or ciphertext failed structural validation.
    #[snafu(display("invalid ML-KEM material: {reason}"))]
    InvalidMlKem {
        /// Failure detail.
        reason: String,
    },

    /// HKDF expansion failed (invalid output length request).
    #[snafu(display("HKDF expand failed"))]
    HkdfExpand,

    /// AEAD sealing of the content key failed.
    #[snafu(display("content-key AEAD seal failed"))]
    AeadSeal,

    /// AEAD opening failed: wrong recipient, tampered ciphertext, or wrong key.
    #[snafu(display("content-key AEAD open failed"))]
    AeadOpen,

    /// The wrapped content key declared a version this build cannot decode.
    #[snafu(display("unsupported seal version: {version}"))]
    UnsupportedVersion {
        /// The version byte found on the wire.
        version: u8,
    },

    /// CBOR (de)serialization of a wrapped content key failed.
    #[snafu(display("wrapped-key serialization failed: {reason}"))]
    Serialization {
        /// Failure detail.
        reason: String,
    },
}
