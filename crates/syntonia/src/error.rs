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

    /// EEPROM image is too small.
    #[snafu(display("image too small: {actual} bytes, need at least {required}"))]
    ImageTooSmall {
        /// Actual size provided.
        actual: usize,
        /// Minimum required size.
        required: usize,
    },

    /// Slice access out of bounds.
    #[snafu(display("slice out of bounds: offset {offset}, len {len}, image size {size}"))]
    SliceOutOfBounds {
        /// Requested offset.
        offset: usize,
        /// Requested length.
        len: usize,
        /// Total image size.
        size: usize,
    },

    /// Invalid BCD nibble (value > 9).
    #[snafu(display("invalid BCD nibble: {value}"))]
    InvalidBcdNibble {
        /// The invalid nibble value.
        value: u8,
    },

    /// Frequency not aligned to required step size.
    #[snafu(display("frequency {freq_hz} Hz not aligned to {step} Hz steps"))]
    FrequencyNotAligned {
        /// The unaligned frequency in Hz.
        freq_hz: u64,
        /// Required step size in Hz.
        step: u64,
    },

    /// Tone raw value does not map to a known encoding.
    #[snafu(display("invalid raw tone value: {raw}"))]
    InvalidToneRaw {
        /// The unrecognized raw value.
        raw: u16,
    },

    /// Tone index out of range.
    #[snafu(display("tone index {index} out of range (max {max})"))]
    ToneIndexOutOfRange {
        /// The invalid index.
        index: u16,
        /// Maximum valid index.
        max: u16,
    },

    /// Channel index out of range.
    #[snafu(display("channel index {index} out of range (max {max})"))]
    ChannelIndexOutOfRange {
        /// The invalid index.
        index: u8,
        /// Maximum valid index.
        max: u8,
    },

    /// Attempted to decode a frequency from an empty slot.
    #[snafu(display("empty frequency field WHERE a value was expected"))]
    EmptyFrequency,
}

/// A specialized `Result` type for syntonia operations.
pub type Result<T> = std::result::Result<T, Error>;
