//! Signal aggregation with per-kind temporal baselines (REQ-05).
//!
//! # Observability
//!
//! - `semaino::aggregator::signal_received` — emitted for every inbound signal
//! - `semaino::aggregator::anomaly_emitted` — emitted when an [`AggregatedSignal`] is forwarded

use std::collections::HashMap;

use koinon::{
    AnomalyScore, GeoSignal, TemporalBucketedBaseline,
    signal::{EnvironmentalDetail, GpsDetail, MeshDetail, ProximityDetail, RfDetail, SignalKind},
};
use snafu::Snafu;
use tokio::sync::{broadcast, mpsc};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the signal aggregation pipeline.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum Error {
    /// The downstream receiver was dropped before the aggregator could send.
    #[snafu(display("aggregator send failed: channel closed"))]
    SendFailed,
}

// ---------------------------------------------------------------------------
// AggregatedSignal
// ---------------------------------------------------------------------------

/// A [`GeoSignal`] annotated with the anomaly score from baseline comparison.
#[derive(Debug, Clone)]
pub struct AggregatedSignal {
    /// The original signal event.
    pub signal: GeoSignal,
    /// Anomaly score at the time of ingestion.
    pub score: AnomalyScore,
    /// Baseline mean at the time of scoring, if sufficient data existed.
    pub baseline_mean: Option<f64>,
    /// Baseline standard deviation at the time of scoring, if sufficient data existed.
    pub baseline_stddev: Option<f64>,
}

// ---------------------------------------------------------------------------
// SignalAggregator
// ---------------------------------------------------------------------------

/// Discriminant key for per-signal-kind baseline tracking.
///
/// Uses integer discriminants rather than the full [`SignalKind`] enum value so
/// that structurally distinct variants of the same domain share a single baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum KindKey {
    Rf,
    Mesh,
    Network,
    Proximity,
    Gps,
    Environmental,
    Osint,
}

impl KindKey {
    pub(crate) const fn from_signal(kind: &SignalKind) -> Self {
        match kind {
            SignalKind::Rf(_) => Self::Rf,
            SignalKind::Mesh(_) => Self::Mesh,
            SignalKind::Network(_) => Self::Network,
            SignalKind::Proximity(_) => Self::Proximity,
            SignalKind::Gps(_) => Self::Gps,
            SignalKind::Environmental(_) => Self::Environmental,
            // WHY: SignalKind is #[non_exhaustive] in koinon; the wildcard arm
            // handles Osint and any future variants without triggering
            // unreachable_patterns. Grouping unknowns with Osint keeps the
            // baseline discriminant stable.
            _ => Self::Osint,
        }
    }
}

/// Signal aggregator with per-kind temporal baselines.
///
/// Receives [`GeoSignal`]s from a broadcast channel, maintains per-kind
/// [`TemporalBucketedBaseline`]s, scores each signal, and forwards
/// [`AggregatedSignal`]s downstream when the score is elevated or anomalous.
pub struct SignalAggregator {
    baselines: HashMap<KindKey, TemporalBucketedBaseline>,
}

impl Default for SignalAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalAggregator {
    /// Create a new aggregator with empty per-kind baselines.
    #[must_use]
    pub fn new() -> Self {
        Self {
            baselines: HashMap::new(),
        }
    }

    /// Extract the numeric feature used for baselining from a signal.
    ///
    /// Returns `None` for signal kinds that have no meaningful scalar dimension
    /// (e.g., OSINT feed items, Network DNS queries, GPS spoofing events).
    #[must_use]
    pub fn extract_feature(signal: &GeoSignal) -> Option<f64> {
        // WHY: All detail enums are #[non_exhaustive]; the wildcard `_ => None`
        // arm handles future variants that have no defined numeric feature.
        match &signal.kind {
            SignalKind::Rf(rf) => match rf {
                RfDetail::Transmission { power, .. } => Some(power.as_dbm()),
                RfDetail::Jamming {
                    estimated_power, ..
                } => Some(estimated_power.as_dbm()),
                _ => None,
            },
            SignalKind::Mesh(MeshDetail::NodeSeen { hop_count, .. }) => Some(f64::from(*hop_count)),
            SignalKind::Proximity(
                ProximityDetail::Wifi { rssi, .. } | ProximityDetail::Ble { rssi, .. },
            ) => Some(f64::from(*rssi)),
            SignalKind::Gps(GpsDetail::Fix { .. }) => signal.location.and_then(|c| c.altitude),
            SignalKind::Environmental(env) => match env {
                EnvironmentalDetail::Temperature { celsius } => Some(f64::from(*celsius)),
                EnvironmentalDetail::Humidity { percent } => Some(f64::from(*percent)),
                EnvironmentalDetail::Barometric { hpa } => Some(f64::from(*hpa)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Main processing loop.
    ///
    /// Receives signals from `rx`, scores them against per-kind baselines, and
    /// forwards [`AggregatedSignal`] values to `tx` when the score is elevated
    /// or anomalous. Exits when `rx` is closed or `tx` is dropped.
    ///
    /// # Cancellation Safety
    ///
    /// `recv()` on a broadcast receiver is cancel-safe; no signal is lost on
    /// cancellation at the `select!` boundary.
    #[tracing::instrument(
        level = "debug",
        skip(self, rx, tx),
        fields(baselines = self.baselines.len())
    )]
    pub async fn run(
        &mut self,
        mut rx: broadcast::Receiver<GeoSignal>,
        tx: mpsc::Sender<AggregatedSignal>,
    ) {
        loop {
            let signal = match rx.recv().await {
                Ok(s) => s,
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("aggregator: broadcast channel closed");
                    return;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(dropped = n, "aggregator: lagged behind broadcast");
                    continue;
                }
            };

            tracing::trace!(signal_id = %signal.signal_id, kind = ?KindKey::from_signal(&signal.kind), "signal received");

            let Some(feature) = Self::extract_feature(&signal) else {
                continue;
            };

            // Derive temporal bucket from the signal timestamp.
            let (day_of_week, hour) = day_hour_from_timestamp(&signal.timestamp);

            let key = KindKey::from_signal(&signal.kind);
            let baseline = self.baselines.entry(key).or_default();

            let score = baseline.score(day_of_week, hour, feature);

            // Only update the baseline after scoring to avoid contaminating it with
            // the outlier value we just detected.
            baseline.observe(day_of_week, hour, feature);

            let is_notable = matches!(
                score,
                AnomalyScore::Elevated(_) | AnomalyScore::Anomalous(_)
            );

            if is_notable {
                let (baseline_mean, baseline_stddev) = baseline
                    .bucket(day_of_week, hour)
                    .map_or((None, None), |bucket| (bucket.mean(), bucket.stddev()));

                let aggregated = AggregatedSignal {
                    signal,
                    score,
                    baseline_mean,
                    baseline_stddev,
                };

                tracing::debug!(
                    signal_id = %aggregated.signal.signal_id,
                    "aggregator: anomaly emitted"
                );

                if tx.send(aggregated).await.is_err() {
                    tracing::debug!("aggregator: downstream receiver dropped, exiting");
                    return;
                }
            }
        }
    }
}

/// Extract (`day_of_week`, `hour`) from a [`koinon::Timestamp`].
///
/// Returns (0, 0) — Monday 00:00 — on any conversion failure so the caller
/// always gets a valid bucket index.
pub(crate) fn day_hour_from_timestamp(ts: &koinon::Timestamp) -> (u8, u8) {
    use jiff::civil::Weekday;

    let millis = ts.as_unix_millis();
    let Ok(jiff_ts) = jiff::Timestamp::from_millisecond(millis) else {
        return (0, 0);
    };
    // WHY: in_tz returns Result<Zoned, Error>; "UTC" is always valid so we
    // fall back to (0, 0) on the unlikely parse error rather than panicking.
    let Ok(zoned) = jiff_ts.in_tz("UTC") else {
        return (0, 0);
    };
    let dt = zoned.datetime();
    // WHY: jiff Weekday is 1-indexed (Mon=1..Sun=7); subtract 1 to match koinon's 0-indexed layout.
    let day = match dt.weekday() {
        Weekday::Monday => 0u8,
        Weekday::Tuesday => 1,
        Weekday::Wednesday => 2,
        Weekday::Thursday => 3,
        Weekday::Friday => 4,
        Weekday::Saturday => 5,
        Weekday::Sunday => 6,
    };
    // WHY: jiff DateTime::hour() returns i8 (range 0–23); casting to u8 is safe.
    let hour = u8::try_from(dt.hour()).unwrap_or(0);
    (day, hour)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use koinon::{
        Coordinates, Frequency, Power, Timestamp,
        signal::{
            EnvironmentalDetail, GpsDetail, MeshDetail, NetworkDetail, OsintDetail,
            ProximityDetail, RfDetail,
        },
    };

    use super::*;

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

    fn env_signal(celsius: f32) -> GeoSignal {
        GeoSignal::new(
            SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius }),
            Timestamp::now(),
            None,
        )
    }

    fn osint_signal() -> GeoSignal {
        GeoSignal::new(
            SignalKind::Osint(OsintDetail::FeedItem {
                source: "threatfeed.test".into(),
                title: "IOC UPDATE".into(),
            }),
            Timestamp::now(),
            None,
        )
    }

    #[test]
    fn extract_feature_returns_power_for_rf_transmission() {
        let sig = rf_signal(-30.0);
        let feature = SignalAggregator::extract_feature(&sig);
        assert!(
            feature.is_some_and(|v| (v - (-30.0_f64)).abs() < 1e-9),
            "expected power dbm, got {feature:?}"
        );
    }

    #[test]
    fn extract_feature_returns_power_for_rf_jamming() {
        let sig = GeoSignal::new(
            SignalKind::Rf(RfDetail::Jamming {
                affected_band: "2.4 GHz".into(),
                estimated_power: Power::dbm(20.0),
            }),
            Timestamp::now(),
            None,
        );
        let feature = SignalAggregator::extract_feature(&sig);
        assert!(feature.is_some_and(|v| (v - 20.0_f64).abs() < 1e-9));
    }

    #[test]
    fn extract_feature_returns_hop_count_for_mesh_node_seen() {
        let sig = GeoSignal::new(
            SignalKind::Mesh(MeshDetail::NodeSeen {
                node_id: 1,
                snr: 5.0,
                hop_count: 3,
            }),
            Timestamp::now(),
            None,
        );
        let feature = SignalAggregator::extract_feature(&sig);
        assert_eq!(feature, Some(3.0));
    }

    #[test]
    fn extract_feature_returns_rssi_for_wifi_proximity() {
        let sig = GeoSignal::new(
            SignalKind::Proximity(ProximityDetail::Wifi {
                ssid: None,
                bssid: [0u8; 6],
                rssi: -65,
                channel: 6,
            }),
            Timestamp::now(),
            None,
        );
        let feature = SignalAggregator::extract_feature(&sig);
        assert_eq!(feature, Some(f64::from(-65_i8)));
    }

    #[test]
    fn extract_feature_returns_rssi_for_ble_proximity() {
        let sig = GeoSignal::new(
            SignalKind::Proximity(ProximityDetail::Ble {
                mac: [0u8; 6],
                rssi: -80,
                name: None,
            }),
            Timestamp::now(),
            None,
        );
        assert_eq!(
            SignalAggregator::extract_feature(&sig),
            Some(f64::from(-80_i8))
        );
    }

    #[test]
    fn extract_feature_returns_altitude_for_gps_fix() {
        let coords = Coordinates::new(51.5, -0.1, Some(42.0)).unwrap();
        let sig = GeoSignal::new(
            SignalKind::Gps(GpsDetail::Fix {
                satellites: 8,
                hdop: 1.2,
                speed_mps: None,
            }),
            Timestamp::now(),
            Some(coords),
        );
        assert_eq!(SignalAggregator::extract_feature(&sig), Some(42.0));
    }

    #[test]
    fn extract_feature_returns_none_for_osint() {
        let sig = osint_signal();
        assert_eq!(SignalAggregator::extract_feature(&sig), None);
    }

    #[test]
    fn extract_feature_returns_none_for_network_flow() {
        use std::net::{IpAddr, Ipv4Addr};

        let sig = GeoSignal::new(
            SignalKind::Network(NetworkDetail::Flow {
                src_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                dst_ip: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)),
                src_port: 12_345,
                dst_port: 443,
                protocol: 6,
            }),
            Timestamp::now(),
            None,
        );
        assert_eq!(SignalAggregator::extract_feature(&sig), None);
    }

    #[test]
    fn extract_feature_returns_temperature_for_environmental() {
        let sig = env_signal(22.5);
        let feature = SignalAggregator::extract_feature(&sig);
        // WHY: f32 → f64 promotes precision, so compare with sufficient epsilon.
        assert!(feature.is_some_and(|v| (v - f64::from(22.5_f32)).abs() < 1e-5));
    }

    #[tokio::test]
    async fn aggregator_scores_anomaly_after_baseline() {
        let (broadcast_tx, broadcast_rx) = broadcast::channel::<GeoSignal>(64);
        let (agg_tx, mut agg_rx) = mpsc::channel::<AggregatedSignal>(64);

        let mut aggregator = SignalAggregator::new();
        let handle = tokio::spawn(async move {
            aggregator.run(broadcast_rx, agg_tx).await;
        });

        // Feed 20 normal signals around -50.0 dBm to build a stable baseline.
        for i in 0..20_i32 {
            // Vary slightly so stddev > 0.
            let power = -50.0 + f64::from(i % 3);
            broadcast_tx.send(rf_signal(power)).unwrap();
        }

        // Small delay to ensure the aggregator processes the normal signals before
        // we send the outlier.
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await; // kanon:ignore TESTING/sleep-in-test -- synchronises with spawned aggregator task; replacing with a barrier would re-implement the channel's ready signal

        // Drain any elevated signals from the warm-up phase.
        while agg_rx.try_recv().is_ok() {}

        // Send a clear outlier far from the baseline mean.
        broadcast_tx.send(rf_signal(50.0)).unwrap();

        // Wait for the aggregated signal to arrive.
        let result =
            tokio::time::timeout(tokio::time::Duration::from_millis(500), agg_rx.recv()).await;
        assert!(result.is_ok(), "timed out waiting for anomaly signal");
        let aggregated = result.unwrap();
        assert!(aggregated.is_some(), "channel closed unexpectedly");
        let aggregated = aggregated.unwrap();
        assert!(
            matches!(
                aggregated.score,
                AnomalyScore::Elevated(_) | AnomalyScore::Anomalous(_)
            ),
            "expected elevated or anomalous score, got {:?}",
            aggregated.score
        );

        drop(broadcast_tx);
        handle.await.unwrap();
    }

    #[test]
    fn extract_feature_returns_none_for_tracker() {
        use koinon::signal::TrackerKind;

        let sig = GeoSignal::new(
            SignalKind::Proximity(ProximityDetail::Tracker {
                kind: TrackerKind::AirTag,
                mac: [0u8; 6],
            }),
            Timestamp::now(),
            None,
        );
        assert_eq!(SignalAggregator::extract_feature(&sig), None);
    }
}
