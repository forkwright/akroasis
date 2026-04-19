//! Geographic coordinates with validation and distance calculation.

use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Geodetic datum used for coordinate interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Datum {
    /// World Geodetic System 1984 — the GPS standard.
    #[default]
    Wgs84,
}

/// A geographic point with optional altitude.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinates {
    /// Latitude in decimal degrees, range \[-90, 90\].
    pub latitude: f64,
    /// Longitude in decimal degrees, range \[-180, 180\].
    pub longitude: f64,
    /// Altitude above the datum ellipsoid in metres, if known.
    pub altitude: Option<f64>,
    /// Geodetic datum for this coordinate.
    pub datum: Datum,
}

/// Errors returned when constructing [`Coordinates`].
#[derive(Debug, Snafu)]
pub enum CoordinatesError {
    /// Latitude was outside the valid range.
    #[snafu(display("latitude {value} is out of range [-90, 90]"))]
    LatitudeOutOfRange {
        /// The offending value.
        value: f64,
    },
    /// Longitude was outside the valid range.
    #[snafu(display("longitude {value} is out of range [-180, 180]"))]
    LongitudeOutOfRange {
        /// The offending value.
        value: f64,
    },
}

impl Coordinates {
    /// Construct validated WGS-84 coordinates.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinatesError::LatitudeOutOfRange`] if `latitude` is not in `[-90, 90]`.
    /// Returns [`CoordinatesError::LongitudeOutOfRange`] if `longitude` is not in `[-180, 180]`.
    pub fn new(
        latitude: f64,
        longitude: f64,
        altitude: Option<f64>,
    ) -> Result<Self, CoordinatesError> {
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(CoordinatesError::LatitudeOutOfRange { value: latitude });
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(CoordinatesError::LongitudeOutOfRange { value: longitude });
        }
        Ok(Self {
            latitude,
            longitude,
            altitude,
            datum: Datum::default(),
        })
    }

    /// Compute the great-circle distance in metres using the Haversine formula.
    #[must_use]
    pub fn haversine_distance_m(&self, other: &Self) -> f64 {
        const EARTH_RADIUS_M: f64 = 6_371_000.0;

        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let dlat = (other.latitude - self.latitude).to_radians();
        let dlon = (other.longitude - self.longitude).to_radians();

        let a = (lat1.cos() * lat2.cos())
            .mul_add((dlon / 2.0).sin().powi(2), (dlat / 2.0).sin().powi(2));
        let c = 2.0 * a.sqrt().asin();

        EARTH_RADIUS_M * c
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn valid_coordinates_accepted() {
        let c = Coordinates::new(51.5074, -0.1278, Some(11.0));
        assert!(c.is_ok());
    }

    #[test]
    fn latitude_out_of_range_rejected() {
        let err = Coordinates::new(91.0, 0.0, None).unwrap_err();
        assert!(matches!(err, CoordinatesError::LatitudeOutOfRange { .. }));
    }

    #[test]
    fn longitude_out_of_range_rejected() {
        let err = Coordinates::new(0.0, 181.0, None).unwrap_err();
        assert!(matches!(err, CoordinatesError::LongitudeOutOfRange { .. }));
    }

    #[test]
    fn haversine_same_point_is_zero() {
        let c = Coordinates::new(48.8566, 2.3522, None).unwrap();
        assert!(c.haversine_distance_m(&c) < f64::EPSILON);
    }

    #[test]
    fn haversine_known_distance() {
        // London to Paris is roughly 340 km
        let london = Coordinates::new(51.5074, -0.1278, None).unwrap();
        let paris = Coordinates::new(48.8566, 2.3522, None).unwrap();
        let dist = london.haversine_distance_m(&paris);
        assert!((dist - 343_556.0).abs() < 500.0, "dist={dist}");
    }

    #[test]
    fn serde_round_trip() {
        let c = Coordinates::new(40.7128, -74.0060, Some(10.0)).unwrap();
        let json = serde_json::to_string(&c).expect("serialize");
        let back: Coordinates = serde_json::from_str(&json).expect("deserialize");
        assert!((c.latitude - back.latitude).abs() < f64::EPSILON);
        assert!((c.longitude - back.longitude).abs() < f64::EPSILON);
    }

    // --- Behavioral tests ---

    /// London (51.5074°N, 0.1278°W) to Paris (48.8566°N, 2.3522°E) is ~343 km.
    /// Tolerance ±500 m accounts for differing Earth-radius conventions.
    #[test]
    fn haversine_distance_known_pair() {
        let london = Coordinates::new(51.5074, -0.1278, None).unwrap();
        let paris = Coordinates::new(48.8566, 2.3522, None).unwrap();
        let dist_m = london.haversine_distance_m(&paris);
        // Expected ~343 556 m; accept ±500 m.
        assert!(
            (dist_m - 343_556.0).abs() < 500.0,
            "London→Paris distance {dist_m:.0} m is outside expected range"
        );
    }

    #[test]
    fn haversine_distance_same_point_is_zero() {
        let c = Coordinates::new(35.6895, 139.6917, None).unwrap(); // Tokyo
        assert!(
            c.haversine_distance_m(&c) < f64::EPSILON,
            "distance to self must be zero"
        );
    }

    #[test]
    fn haversine_distance_is_symmetric() {
        let london = Coordinates::new(51.5074, -0.1278, None).unwrap();
        let paris = Coordinates::new(48.8566, 2.3522, None).unwrap();
        let a_to_b = london.haversine_distance_m(&paris);
        let b_to_a = paris.haversine_distance_m(&london);
        assert!(
            (a_to_b - b_to_a).abs() < 1e-6,
            "dist(A,B)={a_to_b} ≠ dist(B,A)={b_to_a}"
        );
    }
}
