//! Top-level async orchestrator wiring aggregation, convergence, and alerting.
//!
//! [`SemainoPipeline`] owns all three processing stages and drives them from a
//! single `broadcast::Receiver<GeoSignal>`. Run it with [`SemainoPipeline::run`].

use std::time::Duration;

use koinon::GeoSignal;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::Instrument as _;

use crate::{
    SignalAggregator, aggregator::AggregatedSignal, alert::AlertPipeline,
    convergence::ConvergenceGrid,
};

// ---------------------------------------------------------------------------
// SemainoConfig
// ---------------------------------------------------------------------------

/// Runtime configuration for the semaino pipeline.
///
/// Every field is a behavioral tuning knob: changing it alters when
/// convergence is detected and when alerts are emitted, but does not
/// change the signal protocol or the external event schema. Serde
/// support + `#[serde(default)]` lets operators and agents override
/// a subset of fields via TOML without knowing the rest of the schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SemainoConfig {
    /// Grid quantization factor. 10 000 ≈ 10 m resolution.
    pub grid_resolution: u32,
    /// Sliding convergence detection window in seconds.
    pub time_window_secs: u64,
    /// Alert suppression window in seconds.
    pub suppression_window_secs: u64,
    /// Minimum distinct signal domains in a cell to trigger convergence.
    pub min_convergence_domains: usize,
}

impl Default for SemainoConfig {
    /// Returns production-safe defaults: 10 m grid, 30 s window, 60 s suppression, 2 domains.
    fn default() -> Self {
        Self {
            grid_resolution: 10_000,
            time_window_secs: 30,
            suppression_window_secs: 60,
            min_convergence_domains: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// SemainoPipeline
// ---------------------------------------------------------------------------

/// Async orchestrator integrating aggregation, convergence, and alerting.
///
/// Call [`run`](SemainoPipeline::run) to process signals from a
/// `broadcast::Receiver<GeoSignal>` until the channel closes.
pub struct SemainoPipeline {
    grid: ConvergenceGrid,
    alerts: AlertPipeline,
    time_window: Duration,
    min_convergence_domains: usize,
}

impl SemainoPipeline {
    /// Construct a pipeline from `config`.
    #[must_use]
    pub fn new(config: &SemainoConfig) -> Self {
        Self {
            grid: ConvergenceGrid::new(config.grid_resolution),
            alerts: AlertPipeline::new(config.suppression_window_secs, config.grid_resolution),
            time_window: Duration::from_secs(config.time_window_secs),
            min_convergence_domains: config.min_convergence_domains,
        }
    }

    /// Register an alert sink on the internal [`AlertPipeline`].
    pub fn add_sink(&mut self, sink: impl crate::alert::AlertSink + 'static) {
        self.alerts.add_sink(sink);
    }

    /// Drive the pipeline until the broadcast channel is closed.
    ///
    /// Architecture:
    ///
    /// 1. A cloned broadcast receiver feeds the aggregator's `run()` loop in a
    ///    spawned task, emitting [`AggregatedSignal`]s via an mpsc channel.
    /// 2. The main task fans out each incoming [`GeoSignal`] to the convergence
    ///    grid and consumes [`AggregatedSignal`]s from the mpsc channel to run
    ///    the alert pipeline.
    #[tracing::instrument(
        level = "debug",
        skip(self, rx),
        fields(
            time_window_secs = self.time_window.as_secs(),
            min_convergence_domains = self.min_convergence_domains
        )
    )]
    pub async fn run(&mut self, rx: broadcast::Receiver<GeoSignal>) {
        // WHY: We need two consumers of the broadcast:
        //   (a) the aggregator, for baseline scoring, and
        //   (b) the grid ingestion path.
        // broadcast::Receiver is not Clone, but Sender::subscribe() produces a
        // new receiver. The caller provides one receiver; we subscribe a second
        // one from the same sender would require a Sender reference. Instead we
        // drive both stages inline to avoid needing an Arc<Sender>.
        //
        // Inline approach: receive once per signal, feed aggregator inline (by
        // duplicating the scoring logic via the public extract_feature API) and
        // feed the grid from the same signal. The aggregator's public API is
        // sufficient: extract_feature and the internal baseline are accessed
        // through run(). We drive the aggregator's channel ourselves here.
        let (agg_tx, mut agg_rx) = mpsc::channel::<AggregatedSignal>(256);
        let (signal_tx, signal_rx) = mpsc::channel::<GeoSignal>(256);

        // Spawn aggregator task: receives GeoSignals via mpsc, emits AggregatedSignals.
        let mut aggregator = SignalAggregator::new();
        // WHY: We pipe GeoSignals into the aggregator via a local broadcast-backed
        // channel so we can reuse its run() loop without forking the implementation.
        let (inner_tx, inner_rx) = broadcast::channel::<GeoSignal>(256);
        let agg_task = tokio::spawn(
            async move {
                aggregator.run(inner_rx, agg_tx).await;
            }
            .instrument(tracing::info_span!("semaino.aggregator")),
        );

        // Spawn broadcast drain task: reads the caller's broadcast receiver and
        // fans the signals to (a) the aggregator and (b) the grid feed channel.
        let fan_task = tokio::spawn(
            async move {
                let mut rx = rx;
                loop {
                    match rx.recv().await {
                        Ok(signal) => {
                            // Fan to aggregator (send error means the aggregator exited).
                            if let Err(error) = inner_tx.send(signal.clone()) {
                                tracing::trace!(%error, "aggregator channel closed, dropping fanned signal");
                            }
                            // Fan to grid channel (ignore send error — pipeline exited).
                            if signal_tx.send(signal).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            tracing::debug!("semaino: broadcast channel closed");
                            break;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(
                                dropped = n,
                                "semaino: pipeline lagged, signals dropped"
                            );
                        }
                    }
                }
                // Dropping inner_tx closes the aggregator's broadcast receiver,
                // causing the aggregator task to exit.
            }
            .instrument(tracing::info_span!("semaino.fan")),
        );

        // Main loop: interleave grid ingestion and alert processing.
        //
        // WHY(#224): this used to be `biased`, always polling the aggregated
        // (anomaly) branch first. Under a sustained burst of notable
        // signals that unconditionally starves the grid-ingest branch,
        // blinding convergence detection precisely during the flood the
        // pipeline exists to catch. Fair (unbiased) polling removes that
        // starvation. The other half of #224 -- a triggering signal's own
        // data missing from the grid at the moment it is evaluated -- is
        // handled unconditionally in `handle_aggregated` below, so
        // correctness here no longer depends on which branch scheduling
        // favors.
        let mut signal_rx = signal_rx;
        loop {
            tokio::select! {
                // Drain aggregated signals (anomaly → alert path).
                Some(aggregated) = agg_rx.recv() => {
                    self.handle_aggregated(&aggregated);
                }

                // Ingest raw signals into the convergence grid.
                signal = signal_rx.recv() => {
                    match signal {
                        Some(s) => {
                            self.grid.ingest(&s);

                            // Periodic grid eviction.
                            #[expect(
                                clippy::cast_possible_truncation,
                                reason = "Duration::as_millis() returns u128; cast to i64 is safe because i64::MAX ms is larger than any realistic eviction window"
                            )]
                            let evict_before_ms = koinon::Timestamp::now().as_unix_millis()
                                - self.time_window.as_millis() as i64; // SAFETY: Duration::as_millis() returns u128 but time_window is config-bounded; fits i64
                            if let Ok(ts) = koinon::Timestamp::from_unix_millis(evict_before_ms) {
                                self.grid.evict(ts);
                            }
                        }
                        None => {
                            // signal_rx closed — broadcast drain task exited.
                            break;
                        }
                    }
                }
            }
        }

        // WHY(#232): this comment used to sit above a bare `drop(agg_rx)`,
        // which promised a drain and performed a discard. The window is real:
        // the loop above breaks as soon as signal_rx is closed and empty, and
        // the fan task feeds inner_tx before signal_tx, so the aggregator can
        // still be scoring queued signals at that point and emit afterwards.
        //
        // NOTE: measured, not assumed — no scenario tried (warm-up depth,
        // escalating outliers, immediate close, multi-threaded runtime) ever
        // lost an alert to the pre-drain code; the aggregator finished its
        // backlog and emitted before the loop above observed `signal_rx`
        // close in every case tried. This is therefore a correctness
        // hardening that makes the comment true, not a fix for an observed
        // loss.
        //
        // INVARIANT: this terminates. signal_rx closing means the fan task
        // exited and dropped inner_tx; that closes the aggregator's receiver,
        // so it returns and drops agg_tx — the only sender — and recv()
        // yields None once the buffered alerts are drained.
        while let Some(aggregated) = agg_rx.recv().await {
            self.handle_aggregated(&aggregated);
        }

        // Wait for background tasks to complete.
        if let Err(e) = fan_task.await {
            tracing::warn!(error = ?e, "semaino: fan task error");
        }
        if let Err(e) = agg_task.await {
            tracing::warn!(error = ?e, "semaino: aggregator task error");
        }
    }

    /// Process one [`AggregatedSignal`] through convergence detection and alerting.
    ///
    /// WHY(#224): ingests `aggregated.signal` into the grid before running
    /// detection. The signal already reached the grid path independently via
    /// `signal_rx`, but that delivery races this one across two
    /// unsynchronized channels with no ordering guarantee -- detection could
    /// otherwise run against a grid that does not yet contain the very
    /// signal that produced the anomaly being evaluated. Re-ingesting here
    /// is idempotent (`DomainSlots` keeps one hit per domain, see
    /// `convergence.rs`), so this is safe regardless of whether the
    /// `signal_rx` delivery already landed.
    fn handle_aggregated(&mut self, aggregated: &AggregatedSignal) {
        self.grid.ingest(&aggregated.signal);

        let now = koinon::Timestamp::now();
        // WHY(#223): this read the whole grid and then took `.first()`, under a
        // comment claiming it was "the cell matching the signal". A HashMap has
        // no first, so with more than one converged cell the alert was attributed
        // to an arbitrary one and the choice varied between runs. Asking for the
        // triggering signal's own cell is both what the comment meant and a hash
        // lookup instead of a scan whose cost an adversary controls.
        let matching_convergence = aggregated.signal.location.as_ref().and_then(|coords| {
            self.grid
                .detect_at(coords, self.min_convergence_domains, self.time_window, now)
        });

        if let Some(alert) = self
            .alerts
            .process(aggregated, matching_convergence.as_ref())
        {
            tracing::info!(
                alert_id = %alert.id,
                severity = ?alert.severity,
                "semaino: alert produced"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use std::sync::{Arc, Mutex};

    use koinon::{
        AnomalyScore, Coordinates, Frequency, GeoSignal, Power, Timestamp,
        signal::{RfDetail, SignalKind},
    };
    use tokio::sync::broadcast;

    use super::*;
    use crate::alert::{Alert, AlertSink};

    /// A sink that collects alerts for assertion.
    #[derive(Clone, Default)]
    struct CollectingSink(Arc<Mutex<Vec<Alert>>>);

    impl AlertSink for CollectingSink {
        fn emit(&self, alert: &Alert) {
            self.0.lock().unwrap().push(alert.clone());
        }
    }

    fn rf_signal(power_dbm: f64) -> GeoSignal {
        GeoSignal::new(
            SignalKind::Rf(RfDetail::Transmission {
                frequency: Frequency::mhz(146),
                power: Power::dbm(power_dbm),
                modulation: "FM".into(),
                bandwidth: Frequency::khz(25),
            }),
            Timestamp::now(),
            None,
        )
    }

    #[tokio::test]
    async fn end_to_end_broadcast_to_alert() {
        let (tx, rx) = broadcast::channel::<GeoSignal>(256);
        let mut pipeline = SemainoPipeline::new(&SemainoConfig {
            suppression_window_secs: 0, // no suppression for the test
            min_convergence_domains: 2,
            ..SemainoConfig::default()
        });

        let sink = CollectingSink::default();
        let sink_data = Arc::clone(&sink.0);
        pipeline.add_sink(sink);

        // Spawn the pipeline.
        let handle = tokio::spawn(async move {
            pipeline.run(rx).await;
        });

        // Build a stable baseline with 20 normal signals.
        for i in 0..20_i32 {
            let power = -50.0 + f64::from(i % 3);
            tx.send(rf_signal(power)).unwrap();
        }

        // Small pause for the pipeline to process the baseline signals.
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await; // kanon:ignore TESTING/sleep-in-test -- integration test drives a real broadcast channel; deterministic time would bypass the async runtime

        // Send a clear outlier.
        tx.send(rf_signal(50.0)).unwrap();

        // Allow time for the pipeline to process the outlier.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await; // kanon:ignore TESTING/sleep-in-test -- integration test drives a real broadcast channel; deterministic time would bypass the async runtime

        // Close the broadcast to shut down the pipeline.
        drop(tx);
        handle.await.unwrap();

        let (is_non_empty, first_severity) = {
            let alerts = sink_data.lock().unwrap();
            let first_sev = alerts.first().map(|a| a.severity.clone());
            (!alerts.is_empty(), first_sev)
        };
        assert!(
            is_non_empty,
            "pipeline should have produced at least one alert"
        );
        let severity = first_severity.expect("checked non-empty above");
        assert!(
            matches!(
                severity,
                koinon::signal::AlertSeverity::High | koinon::signal::AlertSeverity::Critical
            ),
            "outlier signal should produce High or Critical alert, got {severity:?}",
        );
    }

    // ── config defaults ────────────────────────────────────────────────────────

    #[test]
    fn default_config_has_expected_values() {
        let cfg = SemainoConfig::default();
        assert_eq!(cfg.grid_resolution, 10_000);
        assert_eq!(cfg.time_window_secs, 30);
        assert_eq!(cfg.suppression_window_secs, 60);
        assert_eq!(cfg.min_convergence_domains, 2);
    }

    #[test]
    fn config_toml_roundtrip_preserves_values() {
        // WHY: agent or operator tuning expressed in TOML must survive a
        // serialize→deserialize round-trip so the persisted config remains
        // canonical.
        let cfg = SemainoConfig {
            grid_resolution: 50_000,
            time_window_secs: 12,
            suppression_window_secs: 7,
            min_convergence_domains: 4,
        };
        let toml_str = toml::to_string(&cfg).unwrap();
        let parsed: SemainoConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.grid_resolution, 50_000);
        assert_eq!(parsed.time_window_secs, 12);
        assert_eq!(parsed.suppression_window_secs, 7);
        assert_eq!(parsed.min_convergence_domains, 4);
    }

    #[test]
    fn config_partial_toml_falls_through_to_defaults() {
        // WHY: agents must be able to override a single knob without
        // restating every other knob in the file.
        let partial = "grid_resolution = 99";
        let parsed: SemainoConfig = toml::from_str(partial).unwrap();
        assert_eq!(parsed.grid_resolution, 99);
        let defaults = SemainoConfig::default();
        assert_eq!(parsed.time_window_secs, defaults.time_window_secs);
        assert_eq!(
            parsed.suppression_window_secs,
            defaults.suppression_window_secs
        );
        assert_eq!(
            parsed.min_convergence_domains,
            defaults.min_convergence_domains
        );
    }

    // ── pipeline ignores OSINT signals gracefully ──────────────────────────────

    #[tokio::test]
    async fn pipeline_ignores_osint_no_alert() {
        use koinon::signal::{OsintDetail, SignalKind};

        let (tx, rx) = broadcast::channel::<GeoSignal>(64);
        let mut pipeline = SemainoPipeline::new(&SemainoConfig::default());
        let sink = CollectingSink::default();
        let sink_data = Arc::clone(&sink.0);
        pipeline.add_sink(sink);

        let handle = tokio::spawn(async move {
            pipeline.run(rx).await;
        });

        for _ in 0..5 {
            tx.send(GeoSignal::new(
                SignalKind::Osint(OsintDetail::FeedItem {
                    source: "feed".into(),
                    title: "item".into(),
                }),
                Timestamp::now(),
                None,
            ))
            .unwrap();
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test -- integration test drives a real broadcast channel; deterministic time would bypass the async runtime
        drop(tx);
        handle.await.unwrap();

        let is_empty = sink_data.lock().unwrap().is_empty();
        assert!(is_empty, "OSINT-only signals must not produce alerts");
    }

    // ── classify_normal_score_returns_none ────────────────────────────────────

    #[test]
    fn classify_normal_score_returns_none() {
        use crate::alert::classify;

        let result = classify(&AnomalyScore::Normal, None);
        assert!(result.is_none());
    }

    // ── #224: handle_aggregated must not miss its own triggering signal ────

    #[test]
    fn handle_aggregated_ingests_its_own_trigger_before_detecting() {
        // WHY(#224): a biased select could previously run convergence
        // detection before the grid-ingest path delivered the very signal
        // that produced the anomaly -- the same defect this test targets,
        // reproduced deterministically (no task scheduling involved) by
        // calling handle_aggregated directly against a grid that has NOT
        // yet seen the RF trigger signal, only a co-located Mesh signal that
        // arrived through the ordinary path.
        use koinon::signal::MeshDetail;

        let loc = Coordinates::new(51.5, -0.1, None).expect("valid coordinates");

        let mut pipeline = SemainoPipeline::new(&SemainoConfig {
            suppression_window_secs: 0,
            min_convergence_domains: 2,
            ..SemainoConfig::default()
        });
        let sink = CollectingSink::default();
        let sink_data = Arc::clone(&sink.0);
        pipeline.add_sink(sink);

        // Simulates a signal that already reached the grid through the
        // ordinary signal_rx path.
        pipeline.grid.ingest(&GeoSignal::new(
            SignalKind::Mesh(MeshDetail::NodeSeen {
                node_id: 1,
                snr: 5.0,
                hop_count: 1,
            }),
            Timestamp::now(),
            Some(loc),
        ));

        // The RF trigger itself has NOT been ingested through signal_rx --
        // handle_aggregated is the only place that has seen it.
        let aggregated = AggregatedSignal {
            signal: GeoSignal::new(
                SignalKind::Rf(RfDetail::Transmission {
                    frequency: Frequency::mhz(146),
                    power: Power::dbm(50.0),
                    modulation: "FM".into(),
                    bandwidth: Frequency::khz(25),
                }),
                Timestamp::now(),
                Some(loc),
            ),
            score: AnomalyScore::Elevated(2.2),
            baseline_mean: Some(-50.0),
            baseline_stddev: Some(1.0),
        };

        pipeline.handle_aggregated(&aggregated);

        let (alert_count, first_severity) = {
            let alerts = sink_data.lock().expect("sink lock");
            (alerts.len(), alerts.first().map(|a| a.severity.clone()))
        };
        assert_eq!(
            alert_count, 1,
            "the co-located Mesh signal plus the RF trigger together must \
             cross the 2-domain convergence threshold"
        );
        assert_eq!(
            first_severity,
            Some(koinon::signal::AlertSeverity::Medium),
            "an Elevated score with 2-domain convergence classifies as \
             Medium; a Low severity here means the RF trigger's own signal \
             was missing from the grid at detection time (#224)"
        );
    }

    /// WHY(#223): convergence detection read the whole grid and then took
    /// `.first()`, so a cluster anywhere on the map could supply the
    /// convergence used to classify a signal that landed somewhere else
    /// entirely. Here the trigger's own cell holds only the trigger, and a
    /// two-domain cluster sits far away; an Elevated score with no convergence
    /// at the trigger must classify Low. Under the old scan-then-first this
    /// returned Medium, escalating an alert on the strength of an unrelated
    /// location.
    #[test]
    fn a_convergence_in_another_cell_does_not_escalate_this_signal() {
        use koinon::signal::MeshDetail;

        let elsewhere = Coordinates::new(51.5, -0.1, None).expect("valid coordinates");
        let trigger_loc = Coordinates::new(48.85, 2.35, None).expect("valid coordinates");

        let mut pipeline = SemainoPipeline::new(&SemainoConfig {
            suppression_window_secs: 0,
            min_convergence_domains: 2,
            ..SemainoConfig::default()
        });
        let sink = CollectingSink::default();
        let sink_data = Arc::clone(&sink.0);
        pipeline.add_sink(sink);

        // A genuine two-domain convergence, far from the signal below.
        pipeline.grid.ingest(&GeoSignal::new(
            SignalKind::Mesh(MeshDetail::NodeSeen {
                node_id: 1,
                snr: 5.0,
                hop_count: 1,
            }),
            Timestamp::now(),
            Some(elsewhere),
        ));
        pipeline.grid.ingest(&GeoSignal::new(
            SignalKind::Rf(RfDetail::Transmission {
                frequency: Frequency::mhz(146),
                power: Power::dbm(50.0),
                modulation: "FM".into(),
                bandwidth: Frequency::khz(25),
            }),
            Timestamp::now(),
            Some(elsewhere),
        ));

        let aggregated = AggregatedSignal {
            signal: GeoSignal::new(
                SignalKind::Rf(RfDetail::Transmission {
                    frequency: Frequency::mhz(433),
                    power: Power::dbm(20.0),
                    modulation: "FM".into(),
                    bandwidth: Frequency::khz(25),
                }),
                Timestamp::now(),
                Some(trigger_loc),
            ),
            score: AnomalyScore::Elevated(2.2),
            baseline_mean: Some(-50.0),
            baseline_stddev: Some(1.0),
        };

        pipeline.handle_aggregated(&aggregated);

        let first_severity = {
            let alerts = sink_data.lock().expect("sink lock");
            alerts.first().map(|a| a.severity.clone())
        };
        assert_eq!(
            first_severity,
            Some(koinon::signal::AlertSeverity::Low),
            "the trigger's own cell holds one domain, so this must classify \
             Low; Medium means an unrelated cell's convergence was used"
        );
    }

    #[tokio::test]
    async fn immediate_shutdown_still_delivers_every_anomaly() {
        // WHY(#232): guards the shutdown drain. It does not reproduce a loss
        // under the old `drop(agg_rx)` — see the NOTE at that site; no
        // scenario tried did. What it does catch is a regression in the drain
        // loop itself: the loop awaits a receiver whose only sender lives in a
        // spawned task, so a future change that keeps an agg_tx clone alive in
        // this scope would hang shutdown forever, and this test would time out
        // rather than pass quietly.
        const OUTLIERS: usize = 6;

        let (tx, rx) = broadcast::channel::<GeoSignal>(1024);
        let mut pipeline = SemainoPipeline::new(&SemainoConfig {
            suppression_window_secs: 0, // no suppression: every anomaly must surface
            min_convergence_domains: 1,
            ..SemainoConfig::default()
        });

        let sink = CollectingSink::default();
        let sink_data = Arc::clone(&sink.0);
        pipeline.add_sink(sink);

        let handle = tokio::spawn(async move {
            pipeline.run(rx).await;
        });

        // Warm the baseline, then close the feed with the outliers still at
        // the very end of the stream and no settling delay.
        for i in 0..20_i32 {
            tx.send(rf_signal(-50.0 + f64::from(i % 3))).unwrap();
        }
        // WHY: escalating magnitudes stay beyond 3 sigma as observing each one
        // widens the baseline, so all six score anomalous rather than the
        // baseline absorbing them after the first few.
        let mut magnitude = 50.0_f64;
        for _ in 0..OUTLIERS {
            tx.send(rf_signal(magnitude)).unwrap();
            magnitude *= 4.0;
        }

        drop(tx);
        handle.await.unwrap();

        let count = sink_data.lock().unwrap().len();
        assert_eq!(
            count, OUTLIERS,
            "every anomaly must survive an immediate shutdown"
        );
    }
}
