//! Integration smoke tests for the `semaino` public API.
//!
//! Unit tests live alongside each module in `src/*.rs`. These tests exist so
//! that the library exposes a public test binary (required by
//! TESTING/no-tests).

use semaino::{ConvergenceGrid, SemainoConfig, SignalAggregator};
use stoicheion::signal::{RfDetail, SignalKind};
use stoicheion::{GeoSignal, Power, Timestamp};

#[test]
fn default_config_has_sensible_values() {
    let cfg = SemainoConfig::default();
    assert!(cfg.grid_resolution >= 1);
    assert!(cfg.time_window_secs > 0);
    assert!(cfg.min_convergence_domains >= 2);
}

#[test]
fn signal_aggregator_extracts_the_jamming_power_feature() {
    let signal = GeoSignal::new(
        SignalKind::Rf(RfDetail::Jamming {
            affected_band: "2.4 GHz".into(),
            estimated_power: Power::dbm(-40.0),
        }),
        Timestamp::now(),
        None,
    );
    assert_eq!(SignalAggregator::extract_feature(&signal), Some(-40.0));
}

#[test]
fn convergence_grid_detects_nothing_before_any_signal_is_ingested() {
    let grid = ConvergenceGrid::new(10_000);
    let hits = grid.detect(2, std::time::Duration::from_secs(60), Timestamp::now());
    assert!(
        hits.is_empty(),
        "a fresh grid with no ingested signals must never report a convergence"
    );
}
