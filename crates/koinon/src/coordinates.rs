//! Geographic coordinates with validation and distance calculation.

use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Geodetic datum used for coordinate interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum Datum {
    /// World Geodetic System 1984 — the GPS standard.
    #[default]
    Wgs84,
}

/// A geographic point with optional altitude.
// WHY: the fields are pub, so a derived Deserialize rebuilt the struct field by
// field and skipped new()'s range check entirely — a plan file could carry
// latitude 900.0 or a NaN altitude straight into haversine_distance_m. Every
// deserialized value now goes through the same validation as new().
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CoordinatesRepr")]
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
#[non_exhaustive]
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
    /// Altitude was NaN or infinite.
    #[snafu(display("altitude {value} is not finite"))]
    AltitudeNotFinite {
        /// The offending value.
        value: f64,
    },
}

/// Wire form of [`Coordinates`], validated on the way in.
///
/// WHY: `try_from` needs a shape serde can build without validation; this is it.
/// It must stay field-identical to `Coordinates`.
#[derive(Deserialize)]
struct CoordinatesRepr {
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    altitude: Option<f64>,
    #[serde(default)]
    datum: Datum,
}

impl TryFrom<CoordinatesRepr> for Coordinates {
    type Error = CoordinatesError;

    fn try_from(repr: CoordinatesRepr) -> Result<Self, Self::Error> {
        let mut coords = Self::new(repr.latitude, repr.longitude, repr.altitude)?;
        coords.datum = repr.datum;
        Ok(coords)
    }
}

impl Coordinates {
    /// Construct validated WGS-84 coordinates.
    ///
    /// # Errors
    ///
    /// Returns [`CoordinatesError::LatitudeOutOfRange`] if `latitude` is not in `[-90, 90]`.
    /// Returns [`CoordinatesError::LongitudeOutOfRange`] if `longitude` is not in `[-180, 180]`.
    /// Returns [`CoordinatesError::AltitudeNotFinite`] if `altitude` is present and not finite.
    ///
    /// NaN latitude or longitude is rejected: `RangeInclusive::contains` is false
    /// for NaN, so the range checks below fail closed.
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
        if let Some(alt) = altitude
            && !alt.is_finite()
        {
            return Err(CoordinatesError::AltitudeNotFinite { value: alt });
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

    #[test]
    fn deserializing_coordinates_cannot_bypass_the_range_check() {
        // WHY: the fields are pub and the derived Deserialize rebuilt the struct
        // field by field, so a plan file could carry latitude 900 straight into
        // haversine_distance_m and produce a silently meaningless distance.
        let bad_lat = serde_json::from_str::<Coordinates>(
            r#"{"latitude":900.0,"longitude":0.0,"altitude":null,"datum":"Wgs84"}"#,
        );
        assert!(bad_lat.is_err(), "latitude 900 deserialized: {bad_lat:?}");

        let bad_lon = serde_json::from_str::<Coordinates>(
            r#"{"latitude":0.0,"longitude":-400.0,"altitude":null,"datum":"Wgs84"}"#,
        );
        assert!(bad_lon.is_err(), "longitude -400 deserialized: {bad_lon:?}");
    }

    #[test]
    fn a_valid_coordinate_still_round_trips_through_serde() {
        // The guard above must not reject legitimate values.
        let original = Coordinates::new(51.5, -0.12, Some(35.0)).unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let back: Coordinates = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn a_non_finite_altitude_is_rejected() {
        assert!(Coordinates::new(0.0, 0.0, Some(f64::NAN)).is_err());
        assert!(Coordinates::new(0.0, 0.0, Some(f64::INFINITY)).is_err());
        assert!(Coordinates::new(0.0, 0.0, Some(35.0)).is_ok());
        assert!(Coordinates::new(0.0, 0.0, None).is_ok());
    }

    #[test]
    fn a_nan_latitude_or_longitude_is_rejected() {
        // RangeInclusive::contains is false for NaN, so the checks fail closed.
        assert!(Coordinates::new(f64::NAN, 0.0, None).is_err());
        assert!(Coordinates::new(0.0, f64::NAN, None).is_err());
    }
}
