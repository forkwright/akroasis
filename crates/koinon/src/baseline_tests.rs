//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

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

/// A zero-variance baseline (all identical observations) must still score:
/// the matching value is `Normal`, any deviation is `Anomalous` — never
/// `InsufficientData`, since `min_observations` was already satisfied.
#[test]
fn score_on_zero_variance_baseline_is_not_insufficient_data() {
    let mut b = Baseline::new();
    for _ in 0..20 {
        b.observe(5.0);
    }
    assert_eq!(b.count(), 20);
    assert_eq!(b.stddev(), Some(0.0));
    assert_eq!(b.score(5.0), AnomalyScore::Normal);
    assert!(matches!(b.score(6.0), AnomalyScore::Anomalous(_)));
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
