//! [`Frequency`] newtype over `u64` (Hz) with unit-aware constructors and display.

use std::{
    fmt,
    ops::{Add, Sub},
};

use serde::{Deserialize, Serialize};

/// A radio frequency stored as a raw count of hertz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct Frequency(u64);

impl Frequency {
    /// Construct FROM a raw hertz value.
    #[must_use]
    pub const fn hz(value: u64) -> Self {
        Self(value)
    }

    /// Construct FROM kilohertz (1 kHz = 1 000 Hz).
    #[must_use]
    pub const fn khz(value: u64) -> Self {
        Self(value * 1_000)
    }

    /// Construct FROM megahertz (1 MHz = 1 000 000 Hz).
    #[must_use]
    pub const fn mhz(value: u64) -> Self {
        Self(value * 1_000_000)
    }

    /// Construct FROM gigahertz (1 GHz = 1 000 000 000 Hz).
    #[must_use]
    pub const fn ghz(value: u64) -> Self {
        Self(value * 1_000_000_000)
    }

    /// Return the raw hertz value.
    #[must_use]
    pub const fn as_hz(&self) -> u64 {
        self.0
    }

    /// Return the frequency as kilohertz.
    #[must_use]
    pub fn as_khz_f64(&self) -> f64 {
        (self.0 as f64) / 1_000.0
    }

    /// Return the frequency as megahertz.
    #[must_use]
    pub fn as_mhz_f64(&self) -> f64 {
        (self.0 as f64) / 1_000_000.0
    }

    /// Return the frequency as gigahertz.
    #[must_use]
    pub fn as_ghz_f64(&self) -> f64 {
        (self.0 as f64) / 1_000_000_000.0
    }
}

impl fmt::Display for Frequency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 >= 1_000_000_000 {
            write!(f, "{:.3} GHz", self.as_ghz_f64())
        } else if self.0 >= 1_000_000 {
            write!(f, "{:.3} MHz", self.as_mhz_f64())
        } else if self.0 >= 1_000 {
            write!(f, "{:.3} kHz", self.as_khz_f64())
        } else {
            write!(f, "{} Hz", self.0)
        }
    }
}

impl Add for Frequency {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }
}

impl Sub for Frequency {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn mhz_displays_correctly() {
        assert_eq!(Frequency::mhz(146).to_string(), "146.000 MHz");
    }

    #[test]
    fn ghz_displays_correctly() {
        assert_eq!(Frequency::ghz(2).to_string(), "2.000 GHz");
    }

    #[test]
    fn khz_displays_correctly() {
        // 145 kHz = 145_000 Hz, below the 1 MHz threshold
        assert_eq!(Frequency::khz(145).to_string(), "145.000 kHz");
    }

    #[test]
    fn hz_displays_correctly() {
        assert_eq!(Frequency::hz(440).to_string(), "440 Hz");
    }

    #[test]
    fn as_hz_round_trips() {
        let f = Frequency::mhz(146);
        assert_eq!(f.as_hz(), 146_000_000);
    }

    #[test]
    fn as_mhz_f64() {
        let f = Frequency::mhz(146);
        assert!((f.as_mhz_f64() - 146.0).abs() < f64::EPSILON);
    }

    #[test]
    fn add_frequencies() {
        let a = Frequency::mhz(100);
        let b = Frequency::mhz(46);
        assert_eq!((a + b).as_hz(), 146_000_000);
    }

    #[test]
    fn sub_frequencies() {
        let a = Frequency::mhz(146);
        let b = Frequency::mhz(100);
        assert_eq!((a - b).as_hz(), 46_000_000);
    }

    #[test]
    fn serde_round_trip() {
        let f = Frequency::mhz(146);
        let json = serde_json::to_string(&f).unwrap();
        let back: Frequency = serde_json::from_str(&json).unwrap();
        assert_eq!(f, back);
    }

    // --- Behavioral tests ---

    /// `Frequency::mhz(146)` → `as_mhz_f64()` must round-trip to exactly 146.0.
    #[test]
    fn frequency_mhz_round_trip() {
        let f = Frequency::mhz(146);
        assert!(
            (f.as_mhz_f64() - 146.0).abs() < f64::EPSILON,
            "expected 146.0 MHz, got {}",
            f.as_mhz_f64()
        );
    }

    /// 144 MHz < 146 MHz < 430 MHz — ordering must reflect raw Hz value.
    #[test]
    fn frequency_ordering() {
        let low = Frequency::mhz(144);
        let mid = Frequency::mhz(146);
        let high = Frequency::mhz(430);
        assert!(low < mid, "144 MHz must be less than 146 MHz");
        assert!(mid < high, "146 MHz must be less than 430 MHz");
        assert!(low < high, "144 MHz must be less than 430 MHz");
    }

    /// VHF (146 MHz) displays as MHz; microwave (2.4 GHz) displays as GHz.
    #[test]
    fn frequency_display_uses_appropriate_unit() {
        let vhf = Frequency::mhz(146);
        let microwave = Frequency::ghz(2);
        let vhf_str = vhf.to_string();
        let mw_str = microwave.to_string();
        assert!(
            vhf_str.contains("MHz"),
            "VHF 146 MHz should display with MHz unit, got: {vhf_str}"
        );
        assert!(
            mw_str.contains("GHz"),
            "2 GHz should display with GHz unit, got: {mw_str}"
        );
    }
}
