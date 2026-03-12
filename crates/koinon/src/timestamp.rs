//! UTC [`Timestamp`] wrapper over [`jiff::Timestamp`] for storage and transmission.

use std::fmt;

use jiff::Timestamp as JiffTimestamp;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// A UTC instant with nanosecond precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(JiffTimestamp);

/// Errors returned when constructing a [`Timestamp`].
#[derive(Debug, Snafu)]
pub enum TimestampError {
    /// The millisecond value was outside the representable range.
    #[snafu(display("invalid unix milliseconds {millis}: {source}"))]
    InvalidMillis {
        /// The offending value.
        millis: i64,
        /// Underlying jiff error.
        source: jiff::Error,
    },
}

impl Timestamp {
    /// Return the current UTC time.
    #[must_use]
    pub fn now() -> Self {
        Self(JiffTimestamp::now())
    }

    /// Construct from unix epoch milliseconds.
    ///
    /// # Errors
    ///
    /// Returns [`TimestampError::InvalidMillis`] if `millis` is outside the valid range.
    pub fn from_unix_millis(millis: i64) -> Result<Self, TimestampError> {
        JiffTimestamp::from_millisecond(millis)
            .map(Self)
            .map_err(|source| TimestampError::InvalidMillis { millis, source })
    }

    /// Return the unix epoch millisecond representation.
    #[must_use]
    pub fn as_unix_millis(&self) -> i64 {
        self.0.as_millisecond()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn now_returns_valid_timestamp() {
        let ts = Timestamp::now();
        assert!(ts.as_unix_millis() > 0);
    }

    #[test]
    fn from_unix_millis_round_trip() {
        let ms: i64 = 1_700_000_000_000;
        let ts = Timestamp::from_unix_millis(ms).unwrap();
        assert_eq!(ts.as_unix_millis(), ms);
    }

    #[test]
    fn display_is_nonempty() {
        let ts = Timestamp::now();
        assert!(!ts.to_string().is_empty());
    }

    #[test]
    fn serde_round_trip() {
        let ts = Timestamp::now();
        let json = serde_json::to_string(&ts).expect("serialize");
        let back: Timestamp = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(ts, back);
    }
}
