//! Error types for the syntonia crate.

use snafu::Snafu;

/// Errors that can occur in syntonia operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// An invalid CTCSS tone frequency was provided.
    #[snafu(display("invalid CTCSS tone: {value} Hz"))]
    InvalidCtcssTone {
        /// The invalid tone value.
        value: f32,
    },

    /// An invalid DCS code was provided.
    #[snafu(display("invalid DCS code: {value}"))]
    InvalidDcsCode {
        /// The invalid code value.
        value: u16,
    },

    /// JSON serialization or deserialization failed.
    #[snafu(display("JSON error: {source}"))]
    Json {
        /// The underlying `serde_json` error.
        source: serde_json::Error,
    },

    /// TOML serialization failed.
    #[snafu(display("TOML serialization error: {source}"))]
    TomlSerialize {
        /// The underlying toml serialization error.
        source: toml::ser::Error,
    },

    /// TOML deserialization failed.
    #[snafu(display("TOML deserialization error: {source}"))]
    TomlDeserialize {
        /// The underlying toml deserialization error.
        source: toml::de::Error,
    },
}

/// A specialized `Result` type for syntonia operations.
pub type Result<T> = std::result::Result<T, Error>;
