//! Integration smoke tests for the `semaino` public API.
//!
//! Unit tests live alongside each module in `src/*.rs`. These tests exist so
//! that the library exposes a public test binary (required by
//! TESTING/no-tests).

use semaino::{ConvergenceGrid, SemainoConfig, SignalAggregator};

#[test]
fn default_config_has_sensible_values() {
    let cfg = SemainoConfig::default();
    assert!(cfg.grid_resolution >= 1);
    assert!(cfg.time_window_secs > 0);
    assert!(cfg.min_convergence_domains >= 2);
}

#[test]
fn signal_aggregator_is_constructible() {
    let _agg = SignalAggregator::new();
}

#[test]
fn convergence_grid_is_constructible() {
    let _grid = ConvergenceGrid::new(10_000);
}
