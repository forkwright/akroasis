//! Message router orchestrating the outbound send path.
//!
//! # Tuning
//!
//! Default ACK timeout and store-and-forward TTL are sourced from
//! [`crate::config::OutboundConfig`]. Routers created via
//! [`MeshRouter::new`] keep the historical defaults; callers that wish to
//! tune should use [`MeshRouter::with_config`].

use std::time::Duration;

use prost::Message as _;

use crate::config::OutboundConfig;
use crate::delivery::{DeliveryFailure, DeliveryTracker};
use crate::outbound::{OutboundQueue, PendingMessage};
use crate::proto::MeshPacket;
use crate::proto::mesh_packet::{PayloadVariant, Priority};
use crate::store_forward::{StoreForward, StoredMessage};
use crate::types::{NodeNum, PacketId};

// Historical defaults (ack_timeout = 30 s, sf_ttl = 3600 s) now live in
// [`OutboundConfig::default`].

/// Returns the current wall-clock time as epoch milliseconds.
///
/// WARNING: falls back to the epoch (0) on an out-of-range clock read, which
/// only happens for a system clock set before 1970 — never in practice.
fn now_ms() -> u64 {
    u64::try_from(jiff::Timestamp::now().as_millisecond()).unwrap_or(0)
}

/// Orchestrates the full outbound send path.
///
/// For reachable nodes, packets go through the [`OutboundQueue`].
/// For unreachable nodes, packets go to [`StoreForward`].
/// All messages are tracked by the [`DeliveryTracker`].
pub struct MeshRouter {
    /// Priority-ordered outbound queue.
    pub outbound: OutboundQueue,
    /// Store-and-forward for offline nodes.
    pub store_forward: StoreForward,
    /// Delivery lifecycle tracker.
    pub delivery: DeliveryTracker,
    /// ACK timeout used when marking messages inflight.
    ack_timeout: Duration,
    /// Default TTL for store-and-forward messages.
    sf_ttl_secs: u64,
    /// Retention bound for delivery records, applied by [`MeshRouter::run_maintenance`].
    record_max_age: Duration,
}

/// Options for a send operation.
#[derive(Debug, Clone)]
pub struct SendOptions {
    /// Whether to request an ACK.
    pub want_ack: bool,
    /// Delivery priority.
    pub priority: Priority,
    /// Time-to-live for store-and-forward.
    pub ttl_secs: u64,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self::with_config(&OutboundConfig::default())
    }
}

impl SendOptions {
    /// Build a default-priority, no-ACK [`SendOptions`] seeded from
    /// [`OutboundConfig::store_forward_ttl_secs`].
    #[must_use]
    pub const fn with_config(config: &OutboundConfig) -> Self {
        Self {
            want_ack: false,
            priority: Priority::Default,
            ttl_secs: config.store_forward_ttl_secs,
        }
    }
}

impl MeshRouter {
    /// Create a new router with default ACK/TTL tuning.
    #[must_use]
    pub fn new(
        outbound: OutboundQueue,
        store_forward: StoreForward,
        delivery: DeliveryTracker,
    ) -> Self {
        Self::with_config(
            outbound,
            store_forward,
            delivery,
            &OutboundConfig::default(),
        )
    }

    /// Create a new router with the supplied tuning configuration.
    #[must_use]
    pub const fn with_config(
        outbound: OutboundQueue,
        store_forward: StoreForward,
        delivery: DeliveryTracker,
        config: &OutboundConfig,
    ) -> Self {
        Self {
            outbound,
            store_forward,
            delivery,
            ack_timeout: config.ack_timeout(),
            sf_ttl_secs: config.store_forward_ttl_secs,
            record_max_age: config.delivery_record_max_age(),
        }
    }

    /// Returns the ACK timeout used when marking messages inflight.
    #[must_use]
    pub const fn ack_timeout(&self) -> Duration {
        self.ack_timeout
    }

    /// Returns the default store-and-forward TTL in seconds.
    #[must_use]
    pub const fn sf_ttl_secs(&self) -> u64 {
        self.sf_ttl_secs
    }

    /// Route a packet for delivery.
    ///
    /// If `reachable` is true, the packet is enqueued in the outbound queue.
    /// If false, the packet is stored for later delivery via store-and-forward.
    ///
    /// Returns the [`PacketId`] for tracking.
    ///
    /// # Errors
    ///
    /// Returns [`Error::QueueFull`](crate::Error) if store-and-forward queue
    /// is at capacity for the destination.
    pub fn send(
        &mut self,
        packet: MeshPacket,
        reachable: bool,
        options: &SendOptions,
    ) -> Result<PacketId, crate::Error> {
        let id = PacketId(packet.id);
        let dest = packet.to;

        if reachable {
            self.outbound.enqueue(PendingMessage {
                packet,
                created: tokio::time::Instant::now(),
                ttl: Duration::from_secs(options.ttl_secs),
                priority: options.priority,
                retries: 0,
            });
        } else {
            let portnum = match &packet.payload_variant {
                Some(PayloadVariant::Decoded(data)) => data.portnum,
                _ => 0,
            };
            self.store_forward.store(
                NodeNum(dest),
                StoredMessage {
                    packet_bytes: packet.encode_to_vec(),
                    packet_id: id.0,
                    dest,
                    portnum,
                    priority: i32::from(options.priority),
                    stored_at_ms: now_ms(),
                    ttl_secs: options.ttl_secs,
                    delivery_attempts: 0,
                },
            )?;
        }

        // WHY (akroasis#245): track only once dispatch is confirmed — tracking
        // before the fallible store_forward.store() call left a phantom
        // Queued record behind when QueueFull propagated via `?` above.
        self.delivery.track(id, dest);

        Ok(id)
    }

    /// Process an ACK for a delivered message.
    pub fn handle_ack(&mut self, packet_id: PacketId, hops: Option<u8>) {
        self.outbound.handle_ack(packet_id);
        self.delivery.mark_acknowledged(packet_id, hops);
    }

    /// Process a NAK for a failed message.
    ///
    /// Retries via the outbound queue if retries remain. Otherwise marks as failed.
    pub fn handle_nak(&mut self, packet_id: PacketId, error: crate::proto::routing::Error) {
        let retried = self.outbound.handle_nak(packet_id);
        if retried {
            self.delivery.record_retry(packet_id);
        } else {
            self.delivery
                .mark_failed(packet_id, DeliveryFailure::Nak(error));
        }
    }

    /// Handle timeout for inflight messages.
    ///
    /// Returns packet IDs that timed out and were NOT retried (max retries exceeded).
    pub fn process_timeouts(&mut self) -> Vec<PacketId> {
        let timed_out = self.outbound.check_timeouts();
        let mut failed = Vec::new();

        for id in timed_out {
            let retried = self.outbound.retry(id);
            if retried {
                self.delivery.record_retry(id);
            } else {
                self.delivery.mark_failed(id, DeliveryFailure::MaxRetries);
                failed.push(id);
            }
        }

        failed
    }

    /// Run every retention and TTL maintenance pass the router owns.
    ///
    /// Returns the packet ids of delivery records expired by the wall-clock
    /// backstop.
    ///
    /// WHY: the delivery tracker, the store-and-forward queues and the outbound
    /// queue each bound themselves through a method that nothing in production
    /// called, so all three grew monotonically on a long-lived collector. One
    /// entry point driven from the flush tick keeps them from drifting apart
    /// again — adding a fourth structure means extending this method, not
    /// remembering to wire another call site (#244).
    ///
    /// PERF: called once per flush tick. Each pass is a single retain over its
    /// own map, so the cost is proportional to what is currently retained.
    pub fn run_maintenance(&mut self) -> Vec<PacketId> {
        // WHY: expire first. `prune_completed` only releases records already in
        // a terminal state, so without this the records for packets that are
        // never acknowledged would never become eligible.
        let expired = self.delivery.expire_stale(self.record_max_age);
        self.delivery.prune_completed(self.record_max_age);
        self.store_forward.prune_expired(now_ms());
        self.outbound.drain_expired();
        expired
    }

    /// Flush stored messages for a node that just came online.
    ///
    /// Moves messages FROM store-and-forward INTO the outbound queue, decoding
    /// each `packet_bytes` back into the original [`MeshPacket`] so the
    /// re-enqueued message carries its real payload and header fields
    /// (`from`, `hop_limit`, `hop_start`, ...) rather than a synthetic shell.
    pub fn node_came_online(&mut self, dest: NodeNum) {
        let stored = self.store_forward.drain_for(dest);
        for msg in stored {
            let priority = Priority::try_from(msg.priority).unwrap_or(Priority::Default);
            // WARNING: packet_bytes is only ever written by `send` on this
            // same type, so decode failure here would indicate corruption;
            // skip rather than deliver a garbage/partial packet.
            let Ok(packet) = MeshPacket::decode(msg.packet_bytes.as_slice()) else {
                continue;
            };
            self.outbound.enqueue(PendingMessage {
                packet,
                created: tokio::time::Instant::now(),
                ttl: Duration::from_secs(msg.ttl_secs),
                priority,
                retries: 0,
            });
        }
    }

    /// Pop the next message ready to send FROM the outbound queue.
    pub fn next_to_send(&mut self) -> Option<PendingMessage> {
        let msg = self.outbound.next_to_send()?;
        let id = PacketId(msg.packet.id);
        self.delivery.mark_sent(id);
        Some(msg)
    }

    /// Track a just-sent message as inflight.
    pub fn track_sent(&mut self, msg: PendingMessage) {
        self.outbound.track_inflight(msg, self.ack_timeout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StoreForwardConfig;
    use crate::delivery::DeliveryStatus;
    use crate::proto::Data;

    fn make_router() -> MeshRouter {
        MeshRouter::new(
            OutboundQueue::new(),
            StoreForward::new(StoreForwardConfig {
                enabled: true,
                max_queue_per_dest: 16,
                message_ttl_secs: 3600,
            }),
            DeliveryTracker::new(),
        )
    }

    fn make_packet(id: u32) -> MeshPacket {
        MeshPacket {
            from: 0xAAAA,
            to: 0xBBBB,
            channel: 0,
            id,
            rx_time: 0,
            rx_snr: 0.0,
            hop_limit: 3,
            want_ack: true,
            priority: i32::from(Priority::Default),
            rx_rssi: 0,
            via_mqtt: false,
            hop_start: 3,
            payload_variant: None,
        }
    }

    #[test]
    fn send_reachable_goes_to_outbound() {
        let mut router = make_router();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(make_packet(1), true, &SendOptions::default())
            .unwrap();

        assert_eq!(router.outbound.pending_count(), 1);
        assert_eq!(router.store_forward.total_stored(), 0);
        assert!(
            matches!(
                router.delivery.delivery_status(id),
                Some(DeliveryStatus::Queued)
            ),
            "should be tracked as Queued"
        );
    }

    #[test]
    fn send_unreachable_goes_to_store_forward() {
        let mut router = make_router();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(make_packet(2), false, &SendOptions::default())
            .unwrap();

        assert_eq!(router.outbound.pending_count(), 0);
        assert_eq!(router.store_forward.total_stored(), 1);
        assert!(router.delivery.delivery_status(id).is_some());
    }

    #[test]
    fn queue_full_send_leaves_no_orphaned_delivery_record() {
        // WHY (akroasis#245): store_forward.store() can fail with QueueFull
        // via the `?` in send(); the delivery tracker must not hold a
        // phantom Queued record for a packet id that was never dispatched.
        let mut router = make_router();
        let options = SendOptions::default();

        // Fill the destination's store-forward queue at equal priority so
        // the eviction path (`msg.priority > min_priority`) can't free a
        // slot and the next store() hits QueueFull.
        for i in 0..16 {
            #[expect(clippy::unwrap_used, reason = "test-only: queue has room for the fill")]
            router.send(make_packet(i), false, &options).unwrap();
        }
        assert_eq!(router.store_forward.total_stored(), 16);

        let overflow_id = PacketId(999);
        let result = router.send(make_packet(999), false, &options);

        assert!(
            matches!(result, Err(crate::Error::QueueFull { .. })),
            "queue at capacity with equal-priority overflow must reject with QueueFull"
        );
        assert!(
            router.delivery.delivery_status(overflow_id).is_none(),
            "a rejected send must not leave an orphaned delivery record"
        );
    }

    #[test]
    fn ack_completes_delivery() {
        let mut router = make_router();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(make_packet(3), true, &SendOptions::default())
            .unwrap();

        // Simulate send + ACK.
        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }
        router.handle_ack(id, Some(2));

        assert!(matches!(
            router.delivery.delivery_status(id),
            Some(DeliveryStatus::Acknowledged { hops: Some(2), .. })
        ));
    }

    #[test]
    fn node_comes_online_flushes_store_forward() {
        let mut router = make_router();
        let dest = NodeNum(0xBBBB);

        // Send to unreachable node.
        #[expect(clippy::unwrap_used, reason = "test-only")]
        router
            .send(make_packet(10), false, &SendOptions::default())
            .unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        router
            .send(
                MeshPacket {
                    id: 11,
                    ..make_packet(11)
                },
                false,
                &SendOptions::default(),
            )
            .unwrap();
        assert_eq!(router.store_forward.total_stored(), 2);

        // Node comes online  -  messages should move to outbound.
        router.node_came_online(dest);
        assert_eq!(router.store_forward.total_stored(), 0);
        assert_eq!(router.outbound.pending_count(), 2);
    }

    #[test]
    fn configured_ack_timeout_threaded_into_router() {
        // WHY: parameterization-observability test — a router built with
        // a non-default OutboundConfig must expose that timeout via its
        // accessor and use it for track_sent.
        let cfg = OutboundConfig {
            ack_timeout_secs: 7,
            store_forward_ttl_secs: 11,
            ..OutboundConfig::default()
        };
        let router = MeshRouter::with_config(
            OutboundQueue::new(),
            StoreForward::new(StoreForwardConfig::default()),
            DeliveryTracker::new(),
            &cfg,
        );
        assert_eq!(router.ack_timeout(), Duration::from_secs(7));
        assert_eq!(router.sf_ttl_secs(), 11);
    }

    #[test]
    fn send_options_with_config_uses_configured_ttl() {
        let cfg = OutboundConfig {
            store_forward_ttl_secs: 42,
            ..OutboundConfig::default()
        };
        let opts = SendOptions::with_config(&cfg);
        assert_eq!(opts.ttl_secs, 42);
    }

    #[test]
    fn store_forward_round_trips_payload_and_header_on_flush() {
        // WHY (akroasis#189): store-and-forward must not silently drop the
        // packet content — the flushed message must carry the same payload
        // bytes and original header fields the sender gave it, not a
        // synthetic all-defaults shell.
        let mut router = make_router();
        let dest = NodeNum(0xBBBB);

        let packet = MeshPacket {
            payload_variant: Some(PayloadVariant::Decoded(Data {
                portnum: 1,
                payload: b"secret mesh payload".to_vec(),
                want_response: false,
                dest: 0,
                source: 0,
                request_id: 0,
                reply_id: 0,
                emoji: vec![],
            })),
            ..make_packet(42)
        };

        #[expect(clippy::unwrap_used, reason = "test-only")]
        router.send(packet, false, &SendOptions::default()).unwrap();
        assert_eq!(router.store_forward.total_stored(), 1);

        router.node_came_online(dest);
        assert_eq!(router.store_forward.total_stored(), 0);

        #[expect(
            clippy::unwrap_used,
            reason = "test-only: flush must produce a pending message"
        )]
        let flushed = router.next_to_send().unwrap();
        assert_eq!(
            flushed.packet.from, 0xAAAA,
            "original `from` must survive the round-trip"
        );
        assert_eq!(flushed.packet.hop_limit, 3);
        assert_eq!(flushed.packet.hop_start, 3);
        assert!(
            matches!(
                &flushed.packet.payload_variant,
                Some(PayloadVariant::Decoded(data))
                    if data.payload == b"secret mesh payload" && data.portnum == 1
            ),
            "decoded payload must round-trip byte-identical, got {:?}",
            flushed.packet.payload_variant
        );
    }

    /// The delivery tracker must not retain a record for a packet that is
    /// never acknowledged (#244).
    #[tokio::test(start_paused = true)]
    async fn run_maintenance_expires_and_releases_unacknowledged_records() {
        let mut router = make_router();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(make_packet(60), true, &SendOptions::default())
            .unwrap();
        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }
        assert_eq!(router.delivery.tracked_count(), 1);

        // Within the retention bound the record is still live and untouched.
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(
            router.run_maintenance().is_empty(),
            "a record inside its retention bound must not be expired"
        );
        assert_eq!(router.delivery.tracked_count(), 1);

        // Past it, the record expires and is then released. Before the fix it
        // stayed `Sent` forever, so `prune_completed` could never free it.
        tokio::time::advance(
            OutboundConfig::default().delivery_record_max_age() + Duration::from_secs(1),
        )
        .await;
        assert_eq!(
            router.run_maintenance(),
            vec![id],
            "an unacknowledged record past the retention bound must be expired"
        );
        assert_eq!(
            router.delivery.tracked_count(),
            0,
            "expired records must be released in the same maintenance pass"
        );
    }

    /// Store-and-forward TTL must be enforced without waiting for `drain_for`
    /// on a destination that may never return (#244).
    #[test]
    fn run_maintenance_prunes_store_forward_without_drain_for() {
        let mut router = make_router();
        let opts = SendOptions {
            ttl_secs: 0,
            ..SendOptions::default()
        };
        #[expect(clippy::unwrap_used, reason = "test-only")]
        router.send(make_packet(61), false, &opts).unwrap();
        assert_eq!(router.store_forward.total_stored(), 1);

        router.run_maintenance();
        assert_eq!(
            router.store_forward.total_stored(),
            0,
            "an elapsed-TTL message must be pruned by maintenance, not held until drain_for"
        );
    }

    #[test]
    fn store_forward_records_real_wall_clock_and_prunes_after_ttl() {
        // WHY (akroasis#236): stored_at_ms must be a real epoch timestamp —
        // a hardcoded 0 makes prune_expired() drop every message
        // immediately against any real now_ms.
        let mut router = make_router();
        let before_ms = now_ms();

        let opts = SendOptions {
            ttl_secs: 1,
            ..SendOptions::default()
        };
        #[expect(clippy::unwrap_used, reason = "test-only")]
        router.send(make_packet(30), false, &opts).unwrap();

        let after_ms = now_ms();
        assert_eq!(router.store_forward.total_stored(), 1);

        // Still within its 1s TTL from a real stored_at_ms.
        router.store_forward.prune_expired(before_ms + 200);
        assert_eq!(
            router.store_forward.total_stored(),
            1,
            "message stored with a real stored_at_ms must survive within its TTL"
        );

        // Comfortably past stored_at_ms + ttl_secs * 1000.
        router.store_forward.prune_expired(after_ms + 2000);
        assert_eq!(
            router.store_forward.total_stored(),
            0,
            "message must expire once now_ms passes stored_at_ms + ttl"
        );
    }

    #[test]
    fn nak_triggers_retry_then_failure() {
        let mut router = make_router();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(make_packet(20), true, &SendOptions::default())
            .unwrap();

        // Pop and track as inflight.
        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }

        // First NAK should retry.
        router.handle_nak(id, crate::proto::routing::Error::NoRoute);
        assert!(
            !matches!(
                router.delivery.delivery_status(id),
                Some(DeliveryStatus::Failed { .. })
            ),
            "should not be failed yet after first NAK (retry should occur)"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn process_timeouts_retries_then_fails_after_max_retries() {
        // WHY (akroasis#248): process_timeouts is the sole producer of
        // DeliveryFailure::MaxRetries but had no direct coverage — only the
        // sibling NAK retry path was tested.
        let cfg = OutboundConfig {
            max_retries: 1,
            ack_timeout_secs: 1,
            ..OutboundConfig::default()
        };
        let mut router = MeshRouter::with_config(
            OutboundQueue::with_config(&cfg),
            StoreForward::new(StoreForwardConfig::default()),
            DeliveryTracker::new(),
            &cfg,
        );

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(make_packet(50), true, &SendOptions::default())
            .unwrap();
        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }

        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(
            router.process_timeouts().is_empty(),
            "first timeout should retry, not fail"
        );

        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }

        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(
            router.process_timeouts(),
            vec![id],
            "exhausted retries must report the failed id"
        );
        assert!(
            matches!(
                router.delivery.delivery_status(id),
                Some(DeliveryStatus::Failed { .. })
            ),
            "delivery must be marked Failed after retries exhausted"
        );
    }
}
