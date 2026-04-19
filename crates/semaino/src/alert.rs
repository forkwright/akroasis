//! Alert deduplication, severity classification, and routing (REQ-07).
//!
//! # Deduplication
//!
//! Every alert carries an [`AlertFingerprint`] derived from its signal domain,
//! optional grid cell, and severity. Fingerprints that arrive within the
//! configurable `suppression_window` are dropped so that burst events produce
//! a single alert rather than a flood.
//!
//! # Severity rules
//!
//! | Condition | Severity |
//! |-----------|----------|
//! | z ≥ 3 + convergence ≥ 3 domains | Critical |
//! | z ≥ 3 alone | High |
//! | z ≥ 2 + any convergence | Medium |
//! | z ≥ 2 alone | Low |

use std::collections::HashMap;

use koinon::{
    AnomalyScore, SignalId, Timestamp,
    signal::{AlertSeverity, SignalKind},
};
use ulid::Ulid;

use crate::{
    AggregatedSignal,
    convergence::{Convergence, GridCell},
};

// ---------------------------------------------------------------------------
// AlertFingerprint
// ---------------------------------------------------------------------------

/// Stable deduplication key for a class of alerts.
///
/// Two alerts share a fingerprint when they originate from the same signal
/// domain, optionally the same grid cell, and carry the same severity.
/// This prevents high-frequency sensors from flooding the alert sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AlertFingerprint {
    /// Stable integer discriminant for the signal domain.
    pub kind_discriminant: u8,
    /// Optional quantized grid cell.
    pub cell: Option<GridCell>,
    /// Severity discriminant.
    pub severity_discriminant: u8,
}

impl AlertFingerprint {
    /// Construct a fingerprint from its components.
    #[must_use]
    pub const fn new(
        kind_discriminant: u8,
        cell: Option<GridCell>,
        severity_discriminant: u8,
    ) -> Self {
        Self {
            kind_discriminant,
            cell,
            severity_discriminant,
        }
    }
}

// ---------------------------------------------------------------------------
// Alert
// ---------------------------------------------------------------------------

/// A deduplicated, severity-classified alert produced by the pipeline.
#[derive(Debug, Clone)]
pub struct Alert {
    /// Unique identifier for this alert instance.
    pub id: Ulid,
    /// Severity classification.
    pub severity: AlertSeverity,
    /// IDs of the source signals that triggered this alert.
    pub source_signals: Vec<SignalId>,
    /// Wall-clock time the alert was generated.
    pub timestamp: Timestamp,
    /// Grid cell of the convergence event, if any.
    pub cell: Option<GridCell>,
    /// Human-readable summary of the event.
    pub summary: String,
    /// Deduplication key.
    pub fingerprint: AlertFingerprint,
}

// ---------------------------------------------------------------------------
// AlertSink
// ---------------------------------------------------------------------------

/// A destination that receives classified [`Alert`]s.
///
/// Implementations route alerts to notification channels, databases, or
/// operator dashboards. The trait is object-safe so multiple sinks can be
/// stored as `Box<dyn AlertSink>`.
pub trait AlertSink: Send + Sync {
    /// Emit an alert to this sink.
    fn emit(&self, alert: &Alert);
}

/// A [`AlertSink`] that logs every alert via [`tracing::info!`].
pub struct TracingSink;

impl AlertSink for TracingSink {
    fn emit(&self, alert: &Alert) {
        tracing::info!(
            alert_id = %alert.id,
            severity = ?alert.severity,
            summary = %alert.summary,
            signals = alert.source_signals.len(),
            "semaino alert"
        );
    }
}

// ---------------------------------------------------------------------------
// AlertPipeline
// ---------------------------------------------------------------------------

/// Deduplicates and classifies [`AggregatedSignal`]s into [`Alert`]s.
///
/// Maintains a suppression map keyed by [`AlertFingerprint`]. A new alert is
/// only emitted when its fingerprint has not been seen within `suppression_window`.
pub struct AlertPipeline {
    /// `fingerprint → last emission timestamp (unix ms)`.
    suppression: HashMap<AlertFingerprint, i64>,
    /// How long (ms) to suppress repeated fingerprints.
    suppression_window_ms: i64,
    /// Registered alert sinks.
    sinks: Vec<Box<dyn AlertSink>>,
}

impl AlertPipeline {
    /// Create a pipeline with a 60-second default suppression window.
    #[must_use]
    pub fn new(suppression_window_secs: u64) -> Self {
        Self {
            suppression: HashMap::new(),
            #[expect(
                clippy::cast_possible_wrap,
                reason = "suppression_window_secs comes from config; realistic values are seconds, not close to u64::MAX / 1_000, so wrap is not a concern"
            )]
            suppression_window_ms: (suppression_window_secs * 1_000) as i64, // SAFETY: suppression_window_secs is config-derived seconds; *1000 fits i64 for any realistic window
            sinks: Vec::new(),
        }
    }

    /// Register an additional [`AlertSink`].
    pub fn add_sink(&mut self, sink: impl AlertSink + 'static) {
        self.sinks.push(Box::new(sink));
    }

    /// Process one [`AggregatedSignal`] and optional [`Convergence`].
    ///
    /// Returns `Some(Alert)` when the signal is notable and its fingerprint is
    /// outside the suppression window, `None` otherwise.
    ///
    /// # Side effects
    ///
    /// When an alert is produced it is forwarded to every registered sink.
    pub fn process(
        &mut self,
        aggregated: &AggregatedSignal,
        convergence: Option<&Convergence>,
    ) -> Option<Alert> {
        let severity = classify(&aggregated.score, convergence)?;

        let kind_disc = kind_discriminant(&aggregated.signal.kind);
        let cell = aggregated
            .signal
            .location
            .map(|c| crate::convergence::quantize(&c, 10_000));
        let sev_disc = severity_discriminant(&severity);
        let fingerprint = AlertFingerprint::new(kind_disc, cell, sev_disc);

        let now_ms = Timestamp::now().as_unix_millis();

        // Suppression check.
        if let Some(&last) = self.suppression.get(&fingerprint) {
            if now_ms - last < self.suppression_window_ms {
                return None;
            }
        }

        // Record emission timestamp before routing.
        self.suppression.insert(fingerprint, now_ms);

        let summary = build_summary(&aggregated.signal.kind, &severity, convergence);

        let alert = Alert {
            id: Ulid::new(),
            severity,
            source_signals: vec![aggregated.signal.signal_id],
            timestamp: Timestamp::now(),
            cell,
            summary,
            fingerprint,
        };

        for sink in &self.sinks {
            sink.emit(&alert);
        }

        Some(alert)
    }
}

// ---------------------------------------------------------------------------
// classify
// ---------------------------------------------------------------------------

/// Map an [`AnomalyScore`] + optional [`Convergence`] to a [`AlertSeverity`].
///
/// Returns `None` when the score is `Normal` or `InsufficientData` — neither
/// warrants an alert.
#[must_use]
pub(crate) fn classify(
    score: &AnomalyScore,
    convergence: Option<&Convergence>,
) -> Option<AlertSeverity> {
    match score {
        AnomalyScore::Anomalous(_) => {
            if convergence.is_some_and(|c| c.domain_count >= 3) {
                Some(AlertSeverity::Critical)
            } else {
                Some(AlertSeverity::High)
            }
        }
        AnomalyScore::Elevated(_) => {
            if convergence.is_some() {
                Some(AlertSeverity::Medium)
            } else {
                Some(AlertSeverity::Low)
            }
        }
        // WHY: AnomalyScore is #[non_exhaustive]; the wildcard handles future
        // variants which are treated as non-notable until explicitly added.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stable integer discriminant for a [`SignalKind`] domain.
const fn kind_discriminant(kind: &SignalKind) -> u8 {
    match kind {
        SignalKind::Rf(_) => 0,
        SignalKind::Mesh(_) => 1,
        SignalKind::Network(_) => 2,
        SignalKind::Proximity(_) => 3,
        SignalKind::Gps(_) => 4,
        SignalKind::Environmental(_) => 5,
        SignalKind::Osint(_) => 6,
        // WHY: SignalKind is #[non_exhaustive]; unknown variants map to 255.
        _ => 255,
    }
}

/// Stable integer discriminant for an [`AlertSeverity`].
const fn severity_discriminant(sev: &AlertSeverity) -> u8 {
    match sev {
        AlertSeverity::Low => 0,
        AlertSeverity::Medium => 1,
        AlertSeverity::High => 2,
        AlertSeverity::Critical => 3,
        // WHY: AlertSeverity is #[non_exhaustive]; unknown variants map to 255.
        _ => 255,
    }
}

/// Build a human-readable summary string for an alert.
fn build_summary(
    kind: &SignalKind,
    severity: &AlertSeverity,
    convergence: Option<&Convergence>,
) -> String {
    let domain = match kind {
        SignalKind::Rf(_) => "RF",
        SignalKind::Mesh(_) => "Mesh",
        SignalKind::Network(_) => "Network",
        SignalKind::Proximity(_) => "Proximity",
        SignalKind::Gps(_) => "GPS",
        SignalKind::Environmental(_) => "Environmental",
        SignalKind::Osint(_) => "OSINT",
        // WHY: SignalKind is #[non_exhaustive]; unknown variants → "Unknown".
        _ => "Unknown",
    };

    convergence.map_or_else(
        || format!("{severity:?} {domain} anomaly detected"),
        |conv| {
            format!(
                "{severity:?} {domain} alert with {}-domain convergence",
                conv.domain_count
            )
        },
    )
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
        AnomalyScore, Coordinates, Frequency, Power, Timestamp,
        signal::{EnvironmentalDetail, RfDetail},
    };

    use crate::convergence::DomainHit;

    use super::*;

    // ── helper builders ───────────────────────────────────────────────────────

    fn rf_aggregated(score: AnomalyScore) -> AggregatedSignal {
        use koinon::GeoSignal;

        AggregatedSignal {
            signal: GeoSignal::new(
                koinon::signal::SignalKind::Rf(RfDetail::Transmission {
                    frequency: Frequency::mhz(146),
                    power: Power::dbm(-30.0),
                    modulation: "FM".into(),
                    bandwidth: Frequency::khz(25),
                }),
                Timestamp::now(),
                None,
            ),
            score,
            baseline_mean: Some(-50.0),
            baseline_stddev: Some(2.0),
        }
    }

    fn fake_convergence(domain_count: usize) -> Convergence {
        let kind = koinon::signal::SignalKind::Rf(RfDetail::Transmission {
            frequency: Frequency::mhz(146),
            power: Power::dbm(-30.0),
            modulation: "FM".into(),
            bandwidth: Frequency::khz(25),
        });
        let hits: Vec<DomainHit> = (0..domain_count)
            .map(|_| DomainHit {
                kind: kind.clone(),
                timestamp: Timestamp::now(),
            })
            .collect();
        Convergence {
            center: Coordinates::new(51.5, -0.1, None).unwrap(),
            hits,
            domain_count,
        }
    }

    // ── classify ─────────────────────────────────────────────────────────────

    #[test]
    fn classify_anomalous_with_convergence_is_critical() {
        let score = AnomalyScore::Anomalous(4.0);
        let conv = fake_convergence(3);
        assert_eq!(classify(&score, Some(&conv)), Some(AlertSeverity::Critical));
    }

    #[test]
    fn classify_anomalous_alone_is_high() {
        let score = AnomalyScore::Anomalous(3.5);
        assert_eq!(classify(&score, None), Some(AlertSeverity::High));
    }

    #[test]
    fn classify_anomalous_with_two_domain_convergence_is_high() {
        // < 3 domains does not qualify for Critical.
        let score = AnomalyScore::Anomalous(3.5);
        let conv = fake_convergence(2);
        assert_eq!(classify(&score, Some(&conv)), Some(AlertSeverity::High));
    }

    #[test]
    fn classify_elevated_with_convergence_is_medium() {
        let score = AnomalyScore::Elevated(2.5);
        let conv = fake_convergence(2);
        assert_eq!(classify(&score, Some(&conv)), Some(AlertSeverity::Medium));
    }

    #[test]
    fn classify_elevated_alone_is_low() {
        let score = AnomalyScore::Elevated(2.1);
        assert_eq!(classify(&score, None), Some(AlertSeverity::Low));
    }

    #[test]
    fn classify_normal_returns_none() {
        assert_eq!(classify(&AnomalyScore::Normal, None), None);
    }

    #[test]
    fn classify_insufficient_data_returns_none() {
        assert_eq!(classify(&AnomalyScore::InsufficientData, None), None);
    }

    // ── deduplication ─────────────────────────────────────────────────────────

    #[test]
    fn dedup_suppresses_within_window() {
        let mut pipeline = AlertPipeline::new(60);
        let agg = rf_aggregated(AnomalyScore::Anomalous(4.0));

        // First call should produce an alert.
        let first = pipeline.process(&agg, None);
        assert!(first.is_some(), "first alert should be emitted");

        // Immediate second call with same fingerprint should be suppressed.
        let second = pipeline.process(&agg, None);
        assert!(
            second.is_none(),
            "duplicate within window should be suppressed"
        );
    }

    #[test]
    fn dedup_passes_after_window() {
        // Use a suppression window of 0 s so every call passes.
        let mut pipeline = AlertPipeline::new(0);
        let agg = rf_aggregated(AnomalyScore::Anomalous(4.0));

        let first = pipeline.process(&agg, None);
        assert!(first.is_some());

        // Second call passes because window is 0 ms.
        let second = pipeline.process(&agg, None);
        assert!(second.is_some(), "alert should pass after window expires");
    }

    #[test]
    fn dedup_different_fingerprints_both_pass() {
        let mut pipeline = AlertPipeline::new(60);

        let agg_anomalous = rf_aggregated(AnomalyScore::Anomalous(4.0));
        let agg_elevated = rf_aggregated(AnomalyScore::Elevated(2.5));

        // Different severities → different fingerprints → both pass.
        let a = pipeline.process(&agg_anomalous, None);
        let b = pipeline.process(&agg_elevated, None);
        assert!(a.is_some());
        assert!(b.is_some());
    }

    // ── alert fields ──────────────────────────────────────────────────────────

    #[test]
    fn alert_fields_are_populated() {
        let mut pipeline = AlertPipeline::new(60);
        let agg = rf_aggregated(AnomalyScore::Anomalous(4.0));
        let conv = fake_convergence(3);
        let alert = pipeline.process(&agg, Some(&conv)).unwrap();
        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert!(!alert.source_signals.is_empty());
        assert!(!alert.summary.is_empty());
    }

    // ── TracingSink ──────────────────────────────────────────────────────────

    #[test]
    fn tracing_sink_emit_does_not_panic() {
        let sink = TracingSink;
        let alert = Alert {
            id: Ulid::new(),
            severity: AlertSeverity::High,
            source_signals: vec![],
            timestamp: Timestamp::now(),
            cell: None,
            summary: "test alert".into(),
            fingerprint: AlertFingerprint::new(0, None, 2),
        };
        // Calling emit must not panic; tracing output is discarded in tests.
        sink.emit(&alert);
    }

    // ── environmental signal path ──────────────────────────────────────────────

    #[test]
    fn classify_environmental_elevated_alone_is_low() {
        use koinon::GeoSignal;

        let agg = AggregatedSignal {
            signal: GeoSignal::new(
                koinon::signal::SignalKind::Environmental(EnvironmentalDetail::Temperature {
                    celsius: 99.0,
                }),
                Timestamp::now(),
                None,
            ),
            score: AnomalyScore::Elevated(2.2),
            baseline_mean: Some(22.0),
            baseline_stddev: Some(1.0),
        };
        let mut pipeline = AlertPipeline::new(60);
        let result = pipeline.process(&agg, None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().severity, AlertSeverity::Low);
    }
}
