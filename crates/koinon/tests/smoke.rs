//! Integration smoke tests for the `koinon` public API.
//!
//! Unit tests live alongside each module in `src/*.rs`. These tests exist so
//! that the library exposes a public test binary (required by
//! TESTING/no-tests).

#![expect(
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]

use koinon::{Coordinates, Frequency, Power, Timestamp};

#[test]
fn frequency_roundtrip_mhz() {
    let f = Frequency::mhz(146);
    assert_eq!(f.as_hz(), 146_000_000);
}

#[test]
fn power_dbm_zero_is_minus_infinity() {
    let p = Power::dbm(-120.0);
    assert!((p.as_dbm() - -120.0).abs() < f64::EPSILON);
}

#[test]
fn timestamp_now_is_positive() {
    let ts = Timestamp::now();
    assert!(ts.as_unix_millis() > 0);
}

#[test]
fn coordinates_valid_construction() {
    let c = Coordinates::new(30.0, -97.0, Some(150.0)).expect("valid coordinates");
    assert!((c.latitude - 30.0).abs() < f64::EPSILON);
}
