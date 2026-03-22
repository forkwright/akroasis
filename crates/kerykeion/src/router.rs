//! Message router orchestrating the outbound send path.

use std::time::Duration;

use crate::delivery::{DeliveryFailure, DeliveryTracker};
use crate::outbound::{OutboundQueue, PendingMessage};
use crate::proto::MeshPacket;
use crate::proto::mesh_packet::Priority;
use crate::store_forward::{StoreForward, StoredMessage};
use crate::types::{NodeNum, PacketId};

/// Default ACK timeout for inflight messages.
const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(30);

/// Default TTL for store-and-forward messages (1 hour).
const DEFAULT_SF_TTL_SECS: u64 = 3600;

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
        Self {
            want_ack: false,
            priority: Priority::Default,
            ttl_secs: DEFAULT_SF_TTL_SECS,
        }
    }
}

impl MeshRouter {
    /// Create a new router with the given sub-components.
    #[must_use]
    pub const fn new(
        outbound: OutboundQueue,
        store_forward: StoreForward,
        delivery: DeliveryTracker,
    ) -> Self {
        Self {
            outbound,
            store_forward,
            delivery,
        }
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

        self.delivery.track(id, dest);

        if reachable {
            self.outbound.enqueue(PendingMessage {
                packet,
                created: tokio::time::Instant::now(),
                ttl: Duration::from_secs(options.ttl_secs),
                priority: options.priority,
                retries: 0,
            });
        } else {
            self.store_forward.store(
                NodeNum(dest),
                StoredMessage {
                    packet_bytes: Vec::new(),
                    packet_id: id.0,
                    dest,
                    portnum: 0,
                    priority: options.priority as i32,
                    stored_at_ms: 0,
                    ttl_secs: options.ttl_secs,
                    delivery_attempts: 0,
                },
            )?;
        }

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

    /// Flush stored messages for a node that just came online.
    ///
    /// Moves messages from store-and-forward into the outbound queue.
    pub fn node_came_online(&mut self, dest: NodeNum) {
        let stored = self.store_forward.drain_for(dest);
        for msg in stored {
            let priority = Priority::try_from(msg.priority).unwrap_or(Priority::Default);
            // WHY: re-enqueue stored messages as fresh pending messages.
            let packet = MeshPacket {
                from: 0,
                to: msg.dest,
                channel: 0,
                id: msg.packet_id,
                rx_time: 0,
                rx_snr: 0.0,
                hop_limit: 3,
                want_ack: true,
                priority: msg.priority,
                rx_rssi: 0,
                via_mqtt: false,
                hop_start: 3,
                payload_variant: None,
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

    /// Pop the next message ready to send from the outbound queue.
    pub fn next_to_send(&mut self) -> Option<PendingMessage> {
        let msg = self.outbound.next_to_send()?;
        let id = PacketId(msg.packet.id);
        self.delivery.mark_sent(id);
        Some(msg)
    }

    /// Track a just-sent message as inflight.
    pub fn track_sent(&mut self, msg: PendingMessage) {
        self.outbound.track_inflight(msg, DEFAULT_ACK_TIMEOUT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StoreForwardConfig;
    use crate::delivery::DeliveryStatus;

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
            priority: Priority::Default as i32,
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

        // Node comes online — messages should move to outbound.
        router.node_came_online(dest);
        assert_eq!(router.store_forward.total_stored(), 0);
        assert_eq!(router.outbound.pending_count(), 2);
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
}
