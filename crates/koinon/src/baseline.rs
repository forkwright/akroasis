//! Online statistical baseline engine using Welford's algorithm.
//!
//! Provides running mean, variance, and standard deviation in O(1) memory per
//! baseline, temporal bucketing across 168 time slots (7 days × 24 hours), and
//! [`AnomalyScore`] classification for anomaly detection across every domain.

use std::collections::VecDeque;

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
    pub fn observe(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / (self.count as f64);
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
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
            Some(self.m2 / (self.count - 1) as f64)
        }
    }

    /// Returns the population variance (`m2 / count`), or [`None`] if no observations.
    #[must_use]
    pub const fn population_variance(&self) -> Option<f64> {
        if self.count == 0 {
            None
        } else {
            Some(self.m2 / (self.count as f64))
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
        let self_weight = self.count as f64;
        let other_weight = other.count as f64;
        let combined_weight = combined_count as f64;
        self.mean += delta * (other_weight / combined_weight);
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
        self.z_score(value)
            .map_or(AnomalyScore::InsufficientData, |z| {
                let abs_z = z.abs();
                if abs_z >= config.anomalous_threshold {
                    AnomalyScore::Anomalous(z)
                } else if abs_z >= config.elevated_threshold {
                    AnomalyScore::Elevated(z)
                } else {
                    AnomalyScore::Normal
                }
            })
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

/// A sliding-window baseline that evicts observations by count or age.
///
/// Maintains a ring buffer of `(timestamp_ms, value)` pairs. On each call to
/// [`observe`](Self::observe), stale entries are evicted and the INNER [`Baseline`]
/// is rebuilt FROM the surviving observations.
#[derive(Debug, Clone)]
pub struct TimeWindowedBaseline {
    /// Ring buffer of `(timestamp_millis, value)` pairs.
    observations: VecDeque<(i64, f64)>,
    /// Maximum number of observations to retain.
    max_observations: usize,
    /// Maximum age of observations in milliseconds. [`None`] means no time LIMIT.
    max_age_ms: Option<i64>,
    /// Current computed baseline, rebuilt after each eviction.
    baseline: Baseline,
}

impl TimeWindowedBaseline {
    /// Creates a new windowed baseline that retains at most `max_observations` entries.
    #[must_use]
    pub const fn new(max_observations: usize) -> Self {
        Self {
            observations: VecDeque::new(),
            max_observations,
            max_age_ms: None,
            baseline: Baseline::new(),
        }
    }

    /// Limits retained observations to those no older than `duration_ms` milliseconds.
    #[must_use]
    pub const fn with_max_age(mut self, duration_ms: i64) -> Self {
        self.max_age_ms = Some(duration_ms);
        self
    }

    /// Records an observation at `timestamp_ms`, evicting stale entries as needed.
    ///
    /// After eviction the INNER baseline is rebuilt FROM all surviving observations.
    pub fn observe(&mut self, timestamp_ms: i64, value: f64) {
        self.observations.push_back((timestamp_ms, value));

        // Evict by count.
        while self.observations.len() > self.max_observations {
            self.observations.pop_front();
        }

        // Evict by age.
        if let Some(max_age) = self.max_age_ms {
            while self
                .observations
                .front()
                .is_some_and(|&(ts, _)| timestamp_ms - ts > max_age)
            {
                self.observations.pop_front();
            }
        }

        // Rebuild FROM surviving observations.
        self.baseline = Baseline::new();
        for &(_, v) in &self.observations {
            self.baseline.observe(v);
        }
    }

    /// Scores `value` against the current windowed baseline.
    #[must_use]
    pub fn score(&self, value: f64) -> AnomalyScore {
        self.baseline.score(value)
    }

    /// Returns a read-only reference to the current INNER baseline.
    #[must_use]
    pub const fn baseline(&self) -> &Baseline {
        &self.baseline
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
mod tests {
    use super::*;

    // ── Baseline unit tests ───────────────────────────────────────────────────

    #[test]
    fn empty_baseline_returns_none_for_all_statistics() {
        let b = Baseline::new();
        assert_eq!(b.count(), 0);
        assert_eq!(b.mean(), None);
        assert_eq!(b.variance(), None);
        assert_eq!(b.population_variance(), None);
        assert_eq!(b.stddev(), None);
        assert_eq!(b.min(), None);
        assert_eq!(b.max(), None);
        assert_eq!(b.z_score(0.0), None);
    }

    #[test]
    fn single_observation_mean_equals_value() {
        let mut b = Baseline::new();
        b.observe(42.0);
        assert_eq!(b.count(), 1);
        assert_eq!(b.mean(), Some(42.0));
        assert_eq!(b.variance(), None);
        assert_eq!(b.min(), Some(42.0));
        assert_eq!(b.max(), Some(42.0));
    }

    #[test]
    fn known_sequence_mean_5_variance_4571_stddev_2138() {
        let mut b = Baseline::new();
        for v in [2.0_f64, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
            b.observe(v);
        }
        assert_eq!(b.count(), 8);
        assert_eq!(b.mean(), Some(5.0));
        assert!(
            b.variance()
                .is_some_and(|v| (v - 4.571_428_571_428_571).abs() < 1e-9),
            "expected variance ≈ 4.571, got {:?}",
            b.variance()
        );
        assert!(
            b.stddev()
                .is_some_and(|s| (s - 2.138_089_935_618_152).abs() < 1e-9),
            "expected stddev ≈ 2.138, got {:?}",
            b.stddev()
        );
    }

    #[test]
    fn z_score_correctness_against_known_data() {
        let mut b = Baseline::new();
        for v in [2.0_f64, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0] {
            b.observe(v);
        }
        // mean=5.0, stddev≈2.138; z_score(5.0) ≈ 0.0
        assert!(b.z_score(5.0).is_some_and(|z| z.abs() < 1e-10));
        // z_score(7.138) ≈ 1.0
        let expected_z = (7.138_089_935_618_152 - 5.0) / 2.138_089_935_618_152;
        assert!(
            b.z_score(7.138_089_935_618_152)
                .is_some_and(|z| (z - expected_z).abs() < 1e-9)
        );
    }

    #[test]
    fn score_returns_normal_for_values_within_two_sigma() {
        let mut b = Baseline::new();
        // 20 observations with known mean ≈ 5.5, stddev ≈ 1.29 for uniform 1..=10 sequence
        for v in (1..=20).map(f64::from) {
            b.observe(v);
        }
        // mean ≈ 10.5, well within range
        assert_eq!(b.score(10.5), AnomalyScore::Normal);
    }

    #[test]
    fn score_returns_elevated_for_values_between_two_and_three_sigma() {
        let b = build_baseline_with_known_stats(0.0, 1.0, 500);
        let score = b.score(2.5);
        assert!(matches!(score, AnomalyScore::Elevated(_)));
        // z-score must fall in [2.0, 3.0) given the Elevated classification.
        assert!(score.z_score().is_some_and(|z| (2.0..3.0).contains(&z)));
    }

    #[test]
    fn score_returns_anomalous_for_values_beyond_three_sigma() {
        let b = build_baseline_with_known_stats(0.0, 1.0, 500);
        let score = b.score(4.0);
        assert!(matches!(score, AnomalyScore::Anomalous(_)));
        assert!(score.is_anomalous());
        // z-score must be ≥ 3.0 given the Anomalous classification.
        assert!(score.z_score().is_some_and(|z| z >= 3.0));
    }

    #[test]
    fn score_returns_insufficient_data_when_count_below_minimum() {
        let mut b = Baseline::new();
        for v in 0..9 {
            b.observe(f64::from(v));
        }
        assert_eq!(b.count(), 9);
        assert_eq!(b.score(0.0), AnomalyScore::InsufficientData);
    }

    #[test]
    fn score_becomes_available_at_ten_observations() {
        let mut b = Baseline::new();
        for v in 0..10 {
            b.observe(f64::from(v));
        }
        assert_eq!(b.count(), 10);
        // Just checks it no longer returns InsufficientData for the mean
        assert_ne!(b.score(4.5), AnomalyScore::InsufficientData);
    }

    #[test]
    fn merge_combines_two_baselines_correctly() {
        let values_a = [1.0_f64, 2.0, 3.0];
        let values_b = [4.0_f64, 5.0, 6.0];

        let mut a = Baseline::new();
        for v in values_a {
            a.observe(v);
        }
        let mut b = Baseline::new();
        for v in values_b {
            b.observe(v);
        }

        let mut merged = a.clone();
        merged.merge(&b);

        let mut combined = Baseline::new();
        for v in values_a.iter().chain(values_b.iter()) {
            combined.observe(*v);
        }

        assert_eq!(merged.count(), combined.count());
        assert!(
            merged
                .mean()
                .zip(combined.mean())
                .is_some_and(|(m, c)| (m - c).abs() < 1e-10)
        );
        assert!(
            merged
                .variance()
                .zip(combined.variance())
                .is_some_and(|(m, c)| (m - c).abs() < 1e-10)
        );
    }

    #[test]
    fn merge_with_empty_other_is_identity() {
        let mut a = Baseline::new();
        a.observe(5.0);
        let original_mean = a.mean();
        let original_count = a.count();
        a.merge(&Baseline::new());
        assert_eq!(a.mean(), original_mean);
        assert_eq!(a.count(), original_count);
    }

    #[test]
    fn merge_into_empty_self_copies_other() {
        let mut b = Baseline::new();
        b.observe(7.0);
        let mut empty = Baseline::new();
        empty.merge(&b);
        assert_eq!(empty.mean(), b.mean());
        assert_eq!(empty.count(), b.count());
    }

    // ── TimeWindowedBaseline tests ────────────────────────────────────────────

    #[test]
    fn time_windowed_baseline_evicts_by_count() {
        let mut twb = TimeWindowedBaseline::new(3);
        twb.observe(0, 1.0);
        twb.observe(1, 2.0);
        twb.observe(2, 3.0);
        assert_eq!(twb.baseline().count(), 3);
        twb.observe(3, 4.0); // evicts the first observation (value=1.0)
        assert_eq!(twb.baseline().count(), 3);
        assert!(
            twb.baseline()
                .mean()
                .is_some_and(|m| (m - 3.0).abs() < 1e-10)
        );
    }

    #[test]
    fn time_windowed_baseline_evicts_by_age() {
        let mut twb = TimeWindowedBaseline::new(100).with_max_age(10);
        twb.observe(0, 10.0);
        twb.observe(5, 20.0);
        twb.observe(15, 30.0); // ts=0 observation is now >10ms old → evicted
        assert_eq!(twb.baseline().count(), 2);
        assert!(
            twb.baseline()
                .mean()
                .is_some_and(|m| (m - 25.0).abs() < 1e-10)
        );
    }

    // ── TemporalBucketedBaseline tests ────────────────────────────────────────

    #[test]
    fn temporal_bucketed_baseline_routes_to_correct_bucket() {
        let mut tbb = TemporalBucketedBaseline::new();
        tbb.observe(2, 15, 99.0); // Wednesday, 15:00
        tbb.observe(2, 15, 101.0);
        tbb.observe(5, 10, 42.0); // Saturday, 10:00

        let wednesday_15 = tbb.bucket(2, 15).unwrap();
        assert_eq!(wednesday_15.count(), 2);
        assert!(
            wednesday_15
                .mean()
                .is_some_and(|m| (m - 100.0).abs() < 1e-10)
        );

        let saturday_10 = tbb.bucket(5, 10).unwrap();
        assert_eq!(saturday_10.count(), 1);

        // Untouched bucket must remain empty.
        let monday_0 = tbb.bucket(0, 0).unwrap();
        assert_eq!(monday_0.count(), 0);
    }

    #[test]
    fn temporal_bucketed_baseline_out_of_range_returns_none() {
        let tbb = TemporalBucketedBaseline::new();
        assert!(tbb.bucket(7, 0).is_none());
        assert!(tbb.bucket(0, 24).is_none());
    }

    #[test]
    fn temporal_bucketed_baseline_global_baseline_merges_all() {
        let mut tbb = TemporalBucketedBaseline::new();
        // Observe one value INTO every bucket.
        let mut total = 0.0_f64;
        let mut count = 0u64;
        for day in 0..7_u8 {
            for hour in 0..24_u8 {
                let v = f64::from(day).mul_add(24.0, f64::from(hour));
                tbb.observe(day, hour, v);
                total += v;
                count += 1;
            }
        }
        let global = tbb.global_baseline();
        assert_eq!(global.count(), count);
        let expected_mean = total / count as f64;
        assert!(
            global
                .mean()
                .is_some_and(|m| (m - expected_mean).abs() < 1e-9)
        );
    }

    // ── Property-based tests ──────────────────────────────────────────────────

    /// Xorshift64 PRNG for deterministic test data generation.
    fn xorshift64_uniform(state: &mut u64) -> f64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        // Produce (0, 1]  -  state is always ≥ 1 FROM a non-zero seed.
        *state as f64 / u64::MAX as f64
    }

    /// Generates `n` normally-distributed samples via Box-Muller transform.
    fn generate_normal_samples(seed: u64, mu: f64, sigma: f64, n: usize) -> Vec<f64> {
        let mut state = seed | 1; // ensure non-zero for xorshift64
        let mut samples = Vec::with_capacity(n);
        while samples.len() < n {
            let u1 = xorshift64_uniform(&mut state);
            let u2 = xorshift64_uniform(&mut state);
            let r = (-2.0 * u1.ln()).sqrt();
            let theta = 2.0 * std::f64::consts::PI * u2;
            samples.push(sigma.mul_add(r * theta.cos(), mu));
            if samples.len() < n {
                samples.push(sigma.mul_add(r * theta.sin(), mu));
            }
        }
        samples
    }

    /// Builds a baseline approximating N(mu, sigma) FROM 500 deterministic samples.
    fn build_baseline_with_known_stats(mu: f64, sigma: f64, n: usize) -> Baseline {
        let samples = generate_normal_samples(0xDEAD_BEEF_CAFE_BABE, mu, sigma, n);
        let mut b = Baseline::new();
        for s in samples {
            b.observe(s);
        }
        b
    }

    // --- Behavioral tests ---

    /// Feed N copies of the same value → mean = that value, population stddev = 0.
    #[test]
    fn baseline_mean_of_constant_sequence() {
        let mut b = Baseline::new();
        for _ in 0..50 {
            b.observe(42.0);
        }
        assert_eq!(b.count(), 50);
        assert!(
            b.mean().is_some_and(|m| (m - 42.0).abs() < 1e-10),
            "expected mean 42.0, got {:?}",
            b.mean()
        );
        // Population variance of a constant sequence is 0; sample variance requires ≥2 and
        // for identical values m2 stays 0, so variance() returns Some(0.0).
        assert!(
            b.variance().is_some_and(|v| v.abs() < 1e-10),
            "expected variance 0.0 for constant sequence, got {:?}",
            b.variance()
        );
    }

    /// After 100 observations near 50.0 the baseline is stable; a value of 500.0
    /// must score as Anomalous (it is far beyond 3 sigma).
    #[test]
    fn baseline_detects_outlier_after_stable_period() {
        let mut b = Baseline::new();
        // 100 observations drawn from N(50, 1) using the deterministic generator.
        let samples = generate_normal_samples(0x00C0_FFEE_DEAD, 50.0, 1.0, 100);
        for s in samples {
            b.observe(s);
        }
        // 500.0 is hundreds of standard deviations away from 50.0.
        let score = b.score(500.0);
        assert!(
            score.is_anomalous(),
            "expected Anomalous for 500.0 against baseline ≈ N(50,1), got {score:?}"
        );
    }

    /// Merging two baselines with different means produces a merged mean that lies
    /// strictly between the two individual means.
    #[test]
    fn baseline_merge_preserves_global_statistics() {
        let mut a = Baseline::new();
        for v in (1..=20).map(f64::from) {
            a.observe(v); // mean ≈ 10.5
        }
        let mut b = Baseline::new();
        for v in (81..=100).map(f64::from) {
            b.observe(v); // mean ≈ 90.5
        }
        let mean_a = a.mean().unwrap();
        let mean_b = b.mean().unwrap();

        let mut merged = a.clone();
        merged.merge(&b);
        let merged_mean = merged.mean().unwrap();

        assert_eq!(merged.count(), 40);
        assert!(
            merged_mean > mean_a && merged_mean < mean_b,
            "merged mean {merged_mean} must be between {mean_a} and {mean_b}"
        );
        // With equal-sized groups the merged mean must be the midpoint ≈ 50.5.
        assert!(
            (merged_mean - 50.5).abs() < 1e-9,
            "expected merged mean 50.5, got {merged_mean}"
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(1000))]

        /// Welford's algorithm converges to within 10% of σ for N ≥ 10 000 samples.
        #[test]
        fn convergence_to_distribution_parameters(
            mu in -50.0_f64..50.0_f64,
            sigma in 1.0_f64..10.0_f64,
            seed in 1_u64..u64::MAX,
        ) {
            let samples = generate_normal_samples(seed, mu, sigma, 10_000);
            let mut baseline = Baseline::new();
            for s in samples {
                baseline.observe(s);
            }
            let tol = 0.1 * sigma;
            proptest::prop_assert!(
                baseline.mean().is_some_and(|m| (m - mu).abs() < tol),
                "mean: computed={:?}, expected≈{mu} (tol={tol})",
                baseline.mean()
            );
            proptest::prop_assert!(
                baseline.stddev().is_some_and(|s| (s - sigma).abs() < tol),
                "stddev: computed={:?}, expected≈{sigma} (tol={tol})",
                baseline.stddev()
            );
        }

        /// Merging two baselines produces the same result as computing FROM the combined SET.
        #[test]
        fn merge_matches_combined_computation(
            set_a in proptest::collection::vec(-1000.0_f64..1000.0_f64, 1_usize..100),
            set_b in proptest::collection::vec(-1000.0_f64..1000.0_f64, 1_usize..100),
        ) {
            let mut baseline_a = Baseline::new();
            for v in &set_a {
                baseline_a.observe(*v);
            }
            let mut baseline_b = Baseline::new();
            for v in &set_b {
                baseline_b.observe(*v);
            }

            let mut merged = baseline_a.clone();
            merged.merge(&baseline_b);

            let mut combined = Baseline::new();
            for v in set_a.iter().chain(set_b.iter()) {
                combined.observe(*v);
            }

            proptest::prop_assert_eq!(merged.count(), combined.count());

            if let (Some(mm), Some(cm)) = (merged.mean(), combined.mean()) {
                let tol = 1e-6 * (mm.abs().max(cm.abs()) + 1.0);
                proptest::prop_assert!(
                    (mm - cm).abs() < tol,
                    "merged mean {mm} ≠ combined mean {cm}"
                );
            }
            if let (Some(mv), Some(cv)) = (merged.variance(), combined.variance()) {
                let tol = 1e-6 * (mv.abs().max(cv.abs()) + 1.0);
                proptest::prop_assert!(
                    (mv - cv).abs() < tol,
                    "merged variance {mv} ≠ combined variance {cv}"
                );
            }
        }

        /// z_score evaluated at the mean is zero for any baseline with ≥ 2 observations.
        #[test]
        fn z_score_of_mean_is_zero(
            values in proptest::collection::vec(-1000.0_f64..1000.0_f64, 2_usize..50),
        ) {
            let mut baseline = Baseline::new();
            for v in &values {
                baseline.observe(*v);
            }
            if let Some(mean) = baseline.mean() {
                if let Some(z) = baseline.z_score(mean) {
                    proptest::prop_assert!(
                        z.abs() < 1e-10,
                        "z_score(mean) = {z}, expected ≈ 0"
                    );
                }
            }
        }
    }
}
