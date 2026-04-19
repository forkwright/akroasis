//! [`Power`] newtype over `f64` (dBm) for signal-strength arithmetic.

use std::{
    fmt,
    ops::{Add, Sub},
};

use serde::{Deserialize, Serialize};

/// Signal power expressed in decibel-milliwatts (dBm).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Power(f64);

impl Power {
    /// Construct from a dBm value.
    #[must_use]
    pub const fn dbm(value: f64) -> Self {
        Self(value)
    }

    /// Return the raw dBm value.
    #[must_use]
    pub const fn as_dbm(&self) -> f64 {
        self.0
    }

    /// Convert dBm to watts.
    ///
    /// Formula: `P(W) = 10^(P(dBm) / 10) / 1000`
    #[must_use]
    pub fn to_watts(&self) -> f64 {
        10_f64.powf(self.0 / 10.0) / 1_000.0
    }

    /// Construct from a watt value.
    ///
    /// Formula: `P(dBm) = 10 × log₁₀(P(mW))`
    #[must_use]
    pub fn from_watts(watts: f64) -> Self {
        Self(10.0 * (watts * 1_000.0).log10())
    }
}

impl fmt::Display for Power {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} dBm", self.0)
    }
}

impl Add for Power {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Power {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn display_negative_dbm() {
        assert_eq!(Power::dbm(-42.0).to_string(), "-42.0 dBm");
    }

    #[test]
    fn display_positive_dbm() {
        assert_eq!(Power::dbm(10.0).to_string(), "10.0 dBm");
    }

    #[test]
    fn as_dbm_round_trips() {
        let p = Power::dbm(-30.5);
        assert!((p.as_dbm() - (-30.5)).abs() < f64::EPSILON);
    }

    #[test]
    fn to_watts_zero_dbm() {
        // 0 dBm = 1 mW = 0.001 W
        let watts = Power::dbm(0.0).to_watts();
        assert!((watts - 0.001).abs() < 1e-10);
    }

    #[test]
    fn from_watts_round_trip() {
        let original = Power::dbm(-10.0);
        let back = Power::from_watts(original.to_watts());
        assert!((original.as_dbm() - back.as_dbm()).abs() < 1e-10);
    }

    #[test]
    fn serde_round_trip() {
        let p = Power::dbm(-42.0);
        let json = serde_json::to_string(&p).expect("serialize");
        let back: Power = serde_json::from_str(&json).expect("deserialize");
        assert!((p.as_dbm() - back.as_dbm()).abs() < f64::EPSILON);
    }
}
