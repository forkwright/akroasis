//! Delivery tracking for outbound mesh messages.

use std::collections::HashMap;

use tokio::time::Instant;

use crate::proto::routing;
use crate::types::PacketId;

/// Lifecycle state of an outbound message.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DeliveryStatus {
    /// Message is queued but not yet sent.
    Queued,
    /// Message has been transmitted.
    Sent {
        /// When the message was sent.
        at: Instant,
    },
    /// Positive ACK received FROM the mesh.
    Acknowledged {
        /// When the ACK was received.
        at: Instant,
        /// Number of hops traversed, if known.
        hops: Option<u8>,
    },
    /// Delivery permanently failed.
    Failed {
        /// Reason for failure.
        reason: DeliveryFailure,
    },
    /// Message TTL expired before delivery could be confirmed.
    Expired,
}

/// Why a message delivery failed.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum DeliveryFailure {
    /// No route to the destination node.
    NoRoute,
    /// Maximum retry attempts exhausted.
    MaxRetries,
    /// Explicit NAK with a routing error code.
    Nak(routing::Error),
    /// NAK carrying a routing error code this build does not recognize.
    ///
    // WHY a distinct variant rather than folding into `Nak` (#208):
    // `routing::Error` cannot represent "unrecognized" without reusing an
    // existing, semantically-wrong code — see `RoutingResult::UnknownError`.
    UnknownNak {
        /// The raw wire code that did not match any known `routing::Error` variant.
        code: i32,
    },
    /// Message TTL expired.
    Ttl,
    /// Destination node is offline and S&F is not available.
    NodeOffline,
}

/// Per-destination delivery statistics.
#[derive(Debug, Default, Clone)]
pub struct DestStats {
    /// Total messages attempted.
    pub attempted: u64,
    /// Total messages successfully acknowledged.
    pub acknowledged: u64,
    /// Total messages that failed delivery.
    pub failed: u64,
    /// Cumulative delivery latency in milliseconds (for computing averages).
    pub latency_sum_ms: u64,
    /// Total retries across all messages.
    pub total_retries: u64,
}

impl DestStats {
    /// Average delivery latency in milliseconds, or `None` if no messages acknowledged.
    #[must_use]
    pub const fn average_latency_ms(&self) -> Option<u64> {
        self.latency_sum_ms.checked_div(self.acknowledged)
    }

    /// Success rate as a fraction (0.0–1.0), or `None` if no messages attempted.
    #[must_use]
    pub fn success_rate(&self) -> Option<f64> {
        if self.attempted > 0 {
            #[expect(
                clippy::as_conversions,
                reason = "u64→f64 for ratio calculation; precision loss acceptable for stats"
            )]
            Some((self.acknowledged as f64) / (self.attempted as f64)) // SAFETY: u32→f64 always fits; ratio is intentionally lossy for display
        } else {
            None
        }
    }
}

/// Internal record for a tracked message.
struct DeliveryRecord {
    status: DeliveryStatus,
    dest: u32,
    created: Instant,
    retries: u8,
}

/// Tracks the full delivery lifecycle of outbound messages.
pub struct DeliveryTracker {
    records: HashMap<PacketId, DeliveryRecord>,
    dest_stats: HashMap<u32, DestStats>,
}

impl DeliveryTracker {
    /// Create an empty delivery tracker.
    #[must_use]
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            dest_stats: HashMap::new(),
        }
    }

    /// Register a new outbound message as queued.
    pub fn track(&mut self, id: PacketId, dest: u32) {
        self.records.insert(
            id,
            DeliveryRecord {
                status: DeliveryStatus::Queued,
                dest,
                created: Instant::now(),
                retries: 0,
            },
        );
        self.dest_stats.entry(dest).or_default().attempted += 1;
    }

    /// Mark a message as sent.
    pub fn mark_sent(&mut self, id: PacketId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.status = DeliveryStatus::Sent { at: Instant::now() };
        }
    }

    /// Mark a message as acknowledged (ACK received).
    ///
    /// Idempotent against duplicate ACKs: a record already `Acknowledged` is left
    /// unchanged. A late ACK for a `Failed`/`Expired` record reconciles the
    /// destination's `failed` counter before recording the acknowledgment, since
    /// the message did reach its destination after all.
    pub fn mark_acknowledged(&mut self, id: PacketId, hops: Option<u8>) {
        if let Some(record) = self.records.get_mut(&id) {
            let reconcile_failed = match record.status {
                DeliveryStatus::Acknowledged { .. } => return,
                DeliveryStatus::Failed { .. } | DeliveryStatus::Expired => true,
                DeliveryStatus::Queued | DeliveryStatus::Sent { .. } => false,
            };

            let now = Instant::now();
            let latency_ms = now.duration_since(record.created).as_millis();
            record.status = DeliveryStatus::Acknowledged { at: now, hops };
            let stats = self.dest_stats.entry(record.dest).or_default();
            if reconcile_failed {
                stats.failed = stats.failed.saturating_sub(1);
            }
            stats.acknowledged += 1;
            // NOTE: u128→u64 safe because latencies never exceed u64 range.
            #[expect(
                clippy::as_conversions,
                reason = "u128→u64 after min(u64::MAX) guarantees no truncation"
            )]
            let latency_u64 = latency_ms.min(u128::from(u64::MAX)) as u64; // SAFETY: .min(u64::MAX) clamps the value into u64 range
            stats.latency_sum_ms = stats.latency_sum_ms.saturating_add(latency_u64);
        }
    }

    /// Mark a message as failed.
    pub fn mark_failed(&mut self, id: PacketId, reason: DeliveryFailure) {
        if let Some(record) = self.records.get_mut(&id) {
            record.status = DeliveryStatus::Failed { reason };
            self.dest_stats.entry(record.dest).or_default().failed += 1;
        }
    }

    /// Mark a message as expired.
    pub fn mark_expired(&mut self, id: PacketId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.status = DeliveryStatus::Expired;
            self.dest_stats.entry(record.dest).or_default().failed += 1;
        }
    }

    /// Increment the retry count for a message.
    pub fn record_retry(&mut self, id: PacketId) {
        if let Some(record) = self.records.get_mut(&id) {
            record.retries += 1;
            self.dest_stats
                .entry(record.dest)
                .or_default()
                .total_retries += 1;
        }
    }

    /// Query the current delivery status of a message.
    #[must_use]
    pub fn delivery_status(&self, id: PacketId) -> Option<&DeliveryStatus> {
        self.records.get(&id).map(|r| &r.status)
    }

    /// Get delivery statistics for a destination node.
    #[must_use]
    pub fn stats_for(&self, dest: u32) -> Option<&DestStats> {
        self.dest_stats.get(&dest)
    }

    /// Number of messages currently tracked.
    #[must_use]
    pub fn tracked_count(&self) -> usize {
        self.records.len()
    }

    /// Remove completed (acknowledged, failed, expired) records older than `max_age`.
    ///
    /// WARNING: this only releases records that already reached a terminal
    /// state. Call [`Self::expire_stale`] first, or a record for a packet that
    /// is never acknowledged stays active and is retained forever (#244).
    pub fn prune_completed(&mut self, max_age: std::time::Duration) {
        let now = Instant::now();
        self.records.retain(|_, record| {
            match &record.status {
                DeliveryStatus::Acknowledged { .. }
                | DeliveryStatus::Failed { .. }
                | DeliveryStatus::Expired => now.duration_since(record.created) < max_age,
                // Keep active records.
                DeliveryStatus::Queued | DeliveryStatus::Sent { .. } => true,
            }
        });
    }

    /// Move records still active after `max_age` into [`DeliveryStatus::Expired`].
    ///
    /// Returns the packet ids that were expired.
    ///
    /// WHY: a `Queued` or `Sent` record only leaves the active set on an ACK, a
    /// NAK, or retry exhaustion. A packet whose ACK never arrives and whose
    /// outbound entry is already gone reaches none of those, so without a
    /// wall-clock backstop it pins a record for the lifetime of the process
    /// and `prune_completed` can never release it (#244).
    pub fn expire_stale(&mut self, max_age: std::time::Duration) -> Vec<PacketId> {
        let now = Instant::now();
        let mut expired = Vec::new();
        for (id, record) in &mut self.records {
            let active = matches!(
                record.status,
                DeliveryStatus::Queued | DeliveryStatus::Sent { .. }
            );
            if active && now.duration_since(record.created) >= max_age {
                record.status = DeliveryStatus::Expired;
                let stats = self.dest_stats.entry(record.dest).or_default();
                stats.failed += 1;
                expired.push(*id);
            }
        }
        expired
    }
}

impl Default for DeliveryTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_queued_to_sent_to_acknowledged() {
        let mut tracker = DeliveryTracker::new();
        let id = PacketId(100);

        tracker.track(id, 0x1234);
        assert!(
            matches!(tracker.delivery_status(id), Some(DeliveryStatus::Queued)),
            "should start as Queued"
        );

        tracker.mark_sent(id);
        assert!(
            matches!(
                tracker.delivery_status(id),
                Some(DeliveryStatus::Sent { .. })
            ),
            "should transition to Sent"
        );

        tracker.mark_acknowledged(id, Some(2));
        assert!(
            matches!(
                tracker.delivery_status(id),
                Some(DeliveryStatus::Acknowledged { hops: Some(2), .. })
            ),
            "should transition to Acknowledged with hops"
        );
    }

    #[test]
    fn track_queued_to_failed() {
        let mut tracker = DeliveryTracker::new();
        let id = PacketId(200);

        tracker.track(id, 0x5678);
        tracker.mark_sent(id);
        tracker.mark_failed(id, DeliveryFailure::MaxRetries);

        assert!(
            matches!(
                tracker.delivery_status(id),
                Some(DeliveryStatus::Failed {
                    reason: DeliveryFailure::MaxRetries
                })
            ),
            "should be Failed::MaxRetries"
        );
    }

    #[test]
    fn track_expired() {
        let mut tracker = DeliveryTracker::new();
        let id = PacketId(300);

        tracker.track(id, 0xABCD);
        tracker.mark_expired(id);

        assert!(
            matches!(tracker.delivery_status(id), Some(DeliveryStatus::Expired)),
            "should be Expired"
        );
    }

    #[test]
    fn dest_stats_accumulated() {
        let mut tracker = DeliveryTracker::new();
        let dest = 0x1111u32;

        tracker.track(PacketId(1), dest);
        tracker.mark_sent(PacketId(1));
        tracker.mark_acknowledged(PacketId(1), None);

        tracker.track(PacketId(2), dest);
        tracker.mark_sent(PacketId(2));
        tracker.mark_failed(PacketId(2), DeliveryFailure::NoRoute);

        tracker.track(PacketId(3), dest);
        tracker.record_retry(PacketId(3));

        #[expect(
            clippy::unwrap_used,
            reason = "test-only: stats guaranteed to exist after tracking"
        )]
        let stats = tracker.stats_for(dest).unwrap();
        assert_eq!(stats.attempted, 3);
        assert_eq!(stats.acknowledged, 1);
        assert_eq!(stats.failed, 1);
        assert_eq!(stats.total_retries, 1);
    }

    #[test]
    fn success_rate_calculation() {
        let stats = DestStats {
            attempted: 10,
            acknowledged: 7,
            failed: 3,
            latency_sum_ms: 0,
            total_retries: 0,
        };
        let rate = stats.success_rate().unwrap_or_default();
        assert!(
            (rate - 0.7).abs() < f64::EPSILON,
            "expected 0.7, got {rate}"
        );
    }

    #[test]
    fn success_rate_none_when_empty() {
        let stats = DestStats::default();
        assert!(stats.success_rate().is_none());
        assert!(stats.average_latency_ms().is_none());
    }

    #[test]
    fn unknown_packet_returns_none() {
        let tracker = DeliveryTracker::new();
        assert!(tracker.delivery_status(PacketId(999)).is_none());
    }

    #[test]
    fn tracked_count() {
        let mut tracker = DeliveryTracker::new();
        assert_eq!(tracker.tracked_count(), 0);
        tracker.track(PacketId(1), 0x1234);
        tracker.track(PacketId(2), 0x1234);
        assert_eq!(tracker.tracked_count(), 2);
    }

    #[test]
    fn mark_acknowledged_idempotent_on_duplicate_ack() {
        let mut tracker = DeliveryTracker::new();
        let id = PacketId(500);
        let dest = 0xCAFEu32;

        tracker.track(id, dest);
        tracker.mark_sent(id);
        tracker.mark_acknowledged(id, Some(3));

        #[expect(
            clippy::unwrap_used,
            reason = "test-only: stats guaranteed to exist after tracking"
        )]
        let first = tracker.stats_for(dest).unwrap().clone();
        assert_eq!(first.acknowledged, 1, "first ACK should count once");

        // A duplicate ACK for the same packet (multi-path / retransmission) must
        // not double-count.
        tracker.mark_acknowledged(id, Some(3));

        #[expect(
            clippy::unwrap_used,
            reason = "test-only: stats guaranteed to exist after tracking"
        )]
        let second = tracker.stats_for(dest).unwrap().clone();
        assert_eq!(
            second.acknowledged, first.acknowledged,
            "replayed ACK must not inflate acknowledged count"
        );
        assert_eq!(
            second.latency_sum_ms, first.latency_sum_ms,
            "replayed ACK must not inflate latency_sum_ms"
        );
    }

    #[test]
    fn mark_acknowledged_reconciles_failed_counter_on_late_ack() {
        let mut tracker = DeliveryTracker::new();
        let id = PacketId(501);
        let dest = 0xBABEu32;

        tracker.track(id, dest);
        tracker.mark_sent(id);
        tracker.mark_failed(id, DeliveryFailure::MaxRetries);

        #[expect(
            clippy::unwrap_used,
            reason = "test-only: stats guaranteed to exist after tracking"
        )]
        let failed_stats = tracker.stats_for(dest).unwrap().clone();
        assert_eq!(failed_stats.failed, 1);
        assert_eq!(failed_stats.acknowledged, 0);

        // A late ACK arrives after the record was already given up on.
        tracker.mark_acknowledged(id, None);

        #[expect(
            clippy::unwrap_used,
            reason = "test-only: stats guaranteed to exist after tracking"
        )]
        let reconciled = tracker.stats_for(dest).unwrap().clone();
        assert_eq!(
            reconciled.failed, 0,
            "late ACK should reconcile failed count"
        );
        assert_eq!(
            reconciled.acknowledged, 1,
            "late ACK should still count as delivered"
        );
        assert!(matches!(
            tracker.delivery_status(id),
            Some(DeliveryStatus::Acknowledged { .. })
        ));
    }

    #[test]
    fn nak_failure_variant() {
        let mut tracker = DeliveryTracker::new();
        let id = PacketId(400);
        tracker.track(id, 0xBEEF);
        tracker.mark_failed(id, DeliveryFailure::Nak(routing::Error::NoRoute));

        assert!(matches!(
            tracker.delivery_status(id),
            Some(DeliveryStatus::Failed {
                reason: DeliveryFailure::Nak(routing::Error::NoRoute)
            })
        ));
    }

    #[test]
    fn prune_completed_releases_only_terminal_records() {
        // WHY(#229): the retention rule is state-keyed, not purely age-keyed.
        // A zero max_age makes every record "too old", so whatever survives
        // survives because of its STATE — which is exactly the rule under test.
        let mut tracker = DeliveryTracker::new();

        let queued = PacketId(1);
        let sent = PacketId(2);
        let acked = PacketId(3);
        let failed = PacketId(4);
        let expired = PacketId(5);
        for id in [queued, sent, acked, failed, expired] {
            tracker.track(id, 0x1234);
        }
        tracker.mark_sent(sent);
        tracker.mark_acknowledged(acked, Some(1));
        tracker.mark_failed(failed, DeliveryFailure::MaxRetries);
        tracker.mark_expired(expired);

        tracker.prune_completed(std::time::Duration::ZERO);

        assert!(
            tracker.delivery_status(queued).is_some(),
            "a Queued record is still active and must be retained (#244)"
        );
        assert!(
            tracker.delivery_status(sent).is_some(),
            "a Sent record is still active and must be retained (#244)"
        );
        assert!(tracker.delivery_status(acked).is_none());
        assert!(tracker.delivery_status(failed).is_none());
        assert!(tracker.delivery_status(expired).is_none());
        assert_eq!(tracker.tracked_count(), 2);
    }

    #[test]
    fn prune_completed_retains_terminal_records_inside_max_age() {
        // WHY(#229): the falsifiable half — with a max_age no record can have
        // exceeded, the same terminal records that vanished above all survive,
        // so the test above proves the age comparison and not merely the match.
        let mut tracker = DeliveryTracker::new();
        let acked = PacketId(10);
        tracker.track(acked, 0x1234);
        tracker.mark_acknowledged(acked, Some(1));

        tracker.prune_completed(std::time::Duration::from_secs(3600));

        assert!(
            tracker.delivery_status(acked).is_some(),
            "a freshly acknowledged record is younger than max_age"
        );
    }
}
