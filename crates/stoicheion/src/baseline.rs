//! Online statistical baseline engine using Welford's algorithm.
//!
//! Provides running mean, variance, and standard deviation in O(1) memory per
//! baseline, temporal bucketing across 168 time slots (7 days × 24 hours), and
//! [`AnomalyScore`] classification for anomaly detection across every domain.

use serde::{Deserialize, Serialize};

/// Single-pass online mean, variance, and standard deviation computation.
///
/// Implements Welford's algorithm (1962) for numerically stable running statistics
/// in O(1) memory. Suitable for embedded and field deployments WHERE storing raw
/// observations is not feasible.
///
/// Reference: Welford, B. P. (1962). "Note on a method for calculating corrected
/// sums of squares and products." Technometrics 4(3):419–420.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    count: u64,
    mean: f64,
    /// Sum of squared deviations FROM the running mean.
    m2: f64,
    min: f64,
    max: f64,
}

impl Default for Baseline {
    fn default() -> Self {
        Self::new()
    }
}

impl Baseline {
    /// Creates an empty baseline with no observations.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
            // WHY: INFINITY / NEG_INFINITY sentinel VALUES let the first real
            // observation correctly initialise min/max without a special case.
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
        }
    }

    /// Incorporates a new observation using Welford's online update rule.
    ///
    /// A non-finite `value` (NaN or ±∞) is rejected and logged rather than
    /// applied: Welford's update never recovers from a NaN `mean`/`m2`, so a
    /// single bad observation would otherwise pin every future
    /// [`Baseline::score`] call to `Anomalous(±∞)` permanently. Mirrors the
    /// same guard on `topology.rs::update_link`.
    pub fn observe(&mut self, value: f64) {
        if !value.is_finite() {
            tracing::warn!(value, "rejecting non-finite baseline observation");
            return;
        }
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / (self.count as f64); // SAFETY: u64→f64 precision loss only matters beyond 2^53 observations; statistical accumulation
        let delta2 = value - self.mean;
        self.m2 = delta.mul_add(delta2, self.m2);
        if value < self.min {
            self.min = value;
        }
        if value > self.max {
            self.max = value;
        }
    }

    /// Returns the total number of observations recorded.
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    /// Returns the running mean, or [`None`] if no observations have been recorded.
    #[must_use]
    pub const fn mean(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.mean)
        }
    }

    /// Returns the sample variance (`m2 / (count − 1)`), or [`None`] if fewer than 2 observations.
    #[must_use]
    pub const fn variance(&self) -> Option<f64> {
        if self.count < 2 {
            None
        } else {
            Some(self.m2 / (self.count - 1) as f64) // SAFETY: u64→f64 precision loss only matters beyond 2^53 observations
        }
    }

    /// Returns the population variance (`m2 / count`), or [`None`] if no observations.
    #[must_use]
    pub const fn population_variance(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.m2 / (self.count as f64)) // SAFETY: u64→f64 precision loss only matters beyond 2^53 observations
        }
    }

    /// Returns the sample standard deviation, or [`None`] if fewer than 2 observations.
    #[must_use]
    pub fn stddev(&self) -> Option<f64> {
        self.variance().map(f64::sqrt)
    }

    /// Returns the minimum observed value, or [`None`] if no observations.
    #[must_use]
    pub const fn min(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.min)
        }
    }

    /// Returns the maximum observed value, or [`None`] if no observations.
    #[must_use]
    pub const fn max(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.max)
        }
    }

    /// Returns the z-score of `value`, or [`None`] if the standard deviation is zero
    /// or fewer than 2 observations have been recorded.
    #[must_use]
    pub fn z_score(&self, value: f64) -> Option<f64> {
        let mean = self.mean()?;
        let stddev = self.stddev()?;
        // WHY: using > avoids float equality comparison; stddev is always ≥ 0
        // (it is sqrt of a non-negative variance), so > 0.0 is equivalent to ≠ 0.0.
        if stddev > 0.0 {
            Some((value - mean) / stddev)
        } else {
            None
        }
    }

    /// Merges `other` INTO this baseline using the parallel Welford algorithm.
    ///
    /// After merging, this baseline represents all observations FROM both baselines.
    /// Merge ORDER does not affect the result.
    pub fn merge(&mut self, other: &Self) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = other.clone();
            return;
        }
        let combined_count = self.count + other.count;
        let delta = other.mean - self.mean;
        let self_weight = self.count as f64; // SAFETY: u64→f64 precision loss only matters beyond 2^53 observations
        let other_weight = other.count as f64; // SAFETY: u64→f64 precision loss only matters beyond 2^53 observations
        let combined_weight = combined_count as f64; // SAFETY: u64→f64 precision loss only matters beyond 2^53 observations
        self.mean = delta.mul_add(other_weight / combined_weight, self.mean);
        self.m2 += (delta * delta).mul_add(self_weight * other_weight / combined_weight, other.m2);
        self.count = combined_count;
        if other.min < self.min {
            self.min = other.min;
        }
        if other.max > self.max {
            self.max = other.max;
        }
    }

    /// Scores `value` against this baseline using the default [`ScoringConfig`] thresholds.
    ///
    /// Returns [`AnomalyScore::InsufficientData`] when fewer than
    /// [`ScoringConfig::min_observations`] (default: 10) observations have been recorded.
    #[must_use]
    pub fn score(&self, value: f64) -> AnomalyScore {
        let config = ScoringConfig::default();
        if self.count < config.min_observations {
            return AnomalyScore::InsufficientData;
        }
        // WHY: z_score is None exactly when stddev is not > 0.0 (a zero-variance
        // baseline), which is not the same as insufficient data — the count gate
        // above already guarantees min_observations were recorded. Any deviation
        // FROM a perfectly stable baseline is maximally significant.
        self.z_score(value).map_or_else(
            || {
                let mean = self.mean().unwrap_or(value);
                // WHY: exact equality is intentional — a zero-variance baseline means
                // every recorded observation equals `mean` bit-for-bit (Welford), so
                // this is the precise "no deviation" test, not an approximate one.
                #[expect(
                    clippy::float_cmp,
                    reason = "zero-variance baseline: exact equality to a bit-stable mean is the intended zero-deviation check"
                )]
                let at_baseline = value == mean;
                if at_baseline {
                    AnomalyScore::Normal
                } else {
                    AnomalyScore::Anomalous(f64::INFINITY.copysign(value - mean))
                }
            },
            |z| {
                let abs_z = z.abs();
                if abs_z >= config.anomalous_threshold {
                    AnomalyScore::Anomalous(z)
                } else if abs_z >= config.elevated_threshold {
                    AnomalyScore::Elevated(z)
                } else {
                    AnomalyScore::Normal
                }
            },
        )
    }
}

/// Classification of an observed value relative to a statistical baseline.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AnomalyScore {
    /// Within 2 standard deviations of the mean.
    Normal,
    /// Between 2 and 3 standard deviations. Contains the z-score.
    Elevated(f64),
    /// Beyond 3 standard deviations. Contains the z-score.
    Anomalous(f64),
    /// Insufficient data for scoring (fewer than `min_observations`).
    InsufficientData,
}

impl AnomalyScore {
    /// Returns `true` if this score indicates an anomalous observation.
    #[must_use]
    pub const fn is_anomalous(&self) -> bool {
        matches!(self, Self::Anomalous(_))
    }

    /// Returns the z-score for [`AnomalyScore::Elevated`] and [`AnomalyScore::Anomalous`]
    /// variants, or [`None`] for [`AnomalyScore::Normal`] and [`AnomalyScore::InsufficientData`].
    #[must_use]
    pub const fn z_score(&self) -> Option<f64> {
        match self {
            Self::Elevated(z) | Self::Anomalous(z) => Some(*z),
            _ => None,
        }
    }
}

/// Thresholds and minimum observation count for [`Baseline::score`].
#[derive(Debug, Clone)]
pub struct ScoringConfig {
    /// Z-score magnitude at or above which a value is classified as [`AnomalyScore::Elevated`].
    pub elevated_threshold: f64,
    /// Z-score magnitude at or above which a value is classified as [`AnomalyScore::Anomalous`].
    pub anomalous_threshold: f64,
    /// Minimum observations required before scoring returns a meaningful result.
    pub min_observations: u64,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            elevated_threshold: 2.0,
            anomalous_threshold: 3.0,
            min_observations: 10,
        }
    }
}

/// A SET of 168 independent baselines, one per (day-of-week, hour-of-day) slot.
///
/// Maintains separate statistics for each of the 7 days × 24 hours = 168 temporal
/// buckets, allowing the system to distinguish "normal for Tuesday at 03:00" FROM
/// "normal for Saturday at noon." Day 0 is Monday; day 6 is Sunday.
#[derive(Debug, Clone)]
pub struct TemporalBucketedBaseline {
    /// Layout: `buckets[day_of_week][hour]`, day 0 = Monday, hour 0–23.
    buckets: [[Baseline; 24]; 7],
}

impl Default for TemporalBucketedBaseline {
    fn default() -> Self {
        Self::new()
    }
}

impl TemporalBucketedBaseline {
    /// Creates a new bucketed baseline with 168 empty [`Baseline`] instances.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buckets: std::array::from_fn(|_| std::array::from_fn(|_| Baseline::new())),
        }
    }

    /// Routes an observation to the `(day_of_week, hour)` bucket.
    ///
    /// `day_of_week` must be 0–6 (0 = Monday). `hour` must be 0–23.
    /// Out-of-range VALUES are silently ignored.
    pub fn observe(&mut self, day_of_week: u8, hour: u8, value: f64) {
        if let Some(day) = self.buckets.get_mut(usize::from(day_of_week)) {
            if let Some(bucket) = day.get_mut(usize::from(hour)) {
                bucket.observe(value);
            }
        }
    }

    /// Scores `value` against the `(day_of_week, hour)` bucket.
    ///
    /// Returns [`AnomalyScore::InsufficientData`] for out-of-range indices.
    #[must_use]
    pub fn score(&self, day_of_week: u8, hour: u8, value: f64) -> AnomalyScore {
        self.buckets
            .get(usize::from(day_of_week))
            .and_then(|day| day.get(usize::from(hour)))
            .map_or(AnomalyScore::InsufficientData, |b| b.score(value))
    }

    /// Returns a reference to the `(day_of_week, hour)` bucket, or [`None`] if out of range.
    #[must_use]
    pub fn bucket(&self, day_of_week: u8, hour: u8) -> Option<&Baseline> {
        self.buckets
            .get(usize::from(day_of_week))
            .and_then(|day| day.get(usize::from(hour)))
    }

    /// Merges all 168 buckets INTO a single global [`Baseline`].
    #[must_use]
    pub fn global_baseline(&self) -> Baseline {
        let mut merged = Baseline::new();
        for day in &self.buckets {
            for bucket in day {
                merged.merge(bucket);
            }
        }
        merged
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "baseline_tests.rs"]
mod tests;
