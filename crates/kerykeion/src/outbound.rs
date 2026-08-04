//! Priority-ordered outbound message queue with inflight tracking.
//!
//! # Tuning
//!
//! Maximum inflight messages, retry count, and ACK/TTL defaults are grouped
//! in [`crate::config::OutboundConfig`]. Constructors are available both
//! with and without an explicit config so that historical call sites
//! continue to work unchanged.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use tokio::time::Instant;

use crate::config::OutboundConfig;
use crate::proto::MeshPacket;
use crate::proto::mesh_packet::Priority;
use crate::types::PacketId;

// Historical defaults (max_inflight = 8, max_retries = 5) now live in
// [`OutboundConfig::default`].

/// A message waiting to be sent.
#[derive(Debug)]
pub struct PendingMessage {
    /// The fully-constructed mesh packet.
    pub packet: MeshPacket,
    /// When this message was enqueued.
    pub created: Instant,
    /// Time-to-live: message is discarded after this duration.
    pub ttl: Duration,
    /// Delivery priority (higher numeric value = higher priority).
    pub priority: Priority,
    /// Number of prior retry attempts (carried forward FROM inflight).
    pub retries: u8,
}

impl PendingMessage {
    /// Whether this message has exceeded its TTL as of `now`.
    #[must_use]
    pub fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.created) >= self.ttl
    }
}

/// A message that has been sent and is awaiting ACK.
#[derive(Debug)]
pub struct InflightMessage {
    /// The packet that was sent.
    pub packet: MeshPacket,
    /// When this message was originally enqueued (preserved across retries).
    pub created: Instant,
    /// Time-to-live FROM `created`: message is discarded after this duration,
    /// carried forward unchanged FROM the original `PendingMessage`.
    pub ttl: Duration,
    /// When the packet was transmitted.
    pub sent_at: Instant,
    /// How many times this packet has been retried.
    pub retries: u8,
    /// Maximum retry attempts before failure.
    pub max_retries: u8,
    /// Duration to wait for ACK before timeout.
    pub ack_timeout: Duration,
}

impl InflightMessage {
    /// Whether this message has exceeded its TTL as of `now`.
    #[must_use]
    pub fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.created) >= self.ttl
    }

    /// Whether this message has exceeded its ACK timeout as of `now`.
    #[must_use]
    pub fn has_timed_out(&self, now: Instant) -> bool {
        now.duration_since(self.sent_at) >= self.ack_timeout
    }
}

/// Manages outbound message flow with priority ordering and inflight tracking.
pub struct OutboundQueue {
    pending: VecDeque<PendingMessage>,
    inflight: HashMap<PacketId, InflightMessage>,
    max_inflight: usize,
    max_retries: u8,
}

impl OutboundQueue {
    /// Creates an empty outbound queue with the default tuning.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(&OutboundConfig::default())
    }

    /// Creates an outbound queue with the supplied tuning configuration.
    #[must_use]
    pub fn with_config(config: &OutboundConfig) -> Self {
        Self {
            pending: VecDeque::new(),
            inflight: HashMap::new(),
            max_inflight: config.max_inflight,
            max_retries: config.max_retries,
        }
    }

    /// Creates an outbound queue with a custom inflight LIMIT.
    #[must_use]
    pub fn with_max_inflight(max_inflight: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            inflight: HashMap::new(),
            max_inflight,
            max_retries: OutboundConfig::default().max_retries,
        }
    }

    /// Returns the current max-inflight limit.
    #[must_use]
    pub const fn max_inflight(&self) -> usize {
        self.max_inflight
    }

    /// Returns the current max-retries limit.
    #[must_use]
    pub const fn max_retries(&self) -> u8 {
        self.max_retries
    }

    /// Insert a message by priority (higher priority first).
    pub fn enqueue(&mut self, msg: PendingMessage) {
        let insert_pos = self
            .pending
            .iter()
            .position(|existing| i32::from(existing.priority) < i32::from(msg.priority))
            .unwrap_or(self.pending.len());
        self.pending.insert(insert_pos, msg);
    }

    /// Pop the highest-priority message that hasn't expired.
    ///
    /// Silently drops expired messages encountered during the search.
    pub fn next_to_send(&mut self) -> Option<PendingMessage> {
        if self.inflight.len() >= self.max_inflight {
            return None;
        }

        let now = Instant::now();
        while let Some(front) = self.pending.front() {
            if front.is_expired(now) {
                // Expired  -  discard.
                self.pending.pop_front();
                continue;
            }
            return self.pending.pop_front();
        }
        None
    }

    /// Move a sent packet to inflight tracking.
    pub fn mark_sent(&mut self, id: PacketId, timeout: Duration) {
        let max_retries = self.max_retries;
        if let Some(pos) = self.pending.iter().position(|m| m.packet.id == id.0) {
            if let Some(msg) = self.pending.remove(pos) {
                self.inflight.insert(
                    id,
                    InflightMessage {
                        packet: msg.packet,
                        created: msg.created,
                        ttl: msg.ttl,
                        sent_at: Instant::now(),
                        retries: 0,
                        max_retries,
                        ack_timeout: timeout,
                    },
                );
            }
        }
        // NOTE: also allow marking sent for packets already popped via next_to_send
    }

    /// Record a sent packet directly INTO inflight tracking (after `next_to_send` pop).
    pub fn track_inflight(&mut self, msg: PendingMessage, timeout: Duration) {
        self.inflight.insert(
            PacketId(msg.packet.id),
            InflightMessage {
                packet: msg.packet,
                created: msg.created,
                ttl: msg.ttl,
                sent_at: Instant::now(),
                retries: msg.retries,
                max_retries: self.max_retries,
                ack_timeout: timeout,
            },
        );
    }

    /// Handle an ACK: remove FROM inflight, message successfully delivered.
    ///
    /// Returns the inflight message if it was being tracked.
    pub fn handle_ack(&mut self, id: PacketId) -> Option<InflightMessage> {
        self.inflight.remove(&id)
    }

    /// Handle a NAK: schedule retry or declare failed.
    ///
    /// Returns `true` if the message will be retried, `false` if max retries exceeded.
    pub fn handle_nak(&mut self, id: PacketId) -> bool {
        self.retry(id)
    }

    /// Return packet IDs of inflight messages that have timed out.
    #[must_use]
    pub fn check_timeouts(&self) -> Vec<PacketId> {
        let now = Instant::now();
        self.inflight
            .iter()
            .filter(|(_, msg)| msg.has_timed_out(now))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Increment retry count and re-enqueue. Returns `false` if max retries exceeded.
    pub fn retry(&mut self, id: PacketId) -> bool {
        let Some(mut msg) = self.inflight.remove(&id) else {
            return false;
        };

        if msg.retries >= msg.max_retries {
            return false;
        }

        msg.retries += 1;
        let priority = Priority::try_from(msg.packet.priority).unwrap_or(Priority::Default);

        // INVARIANT: `created`/`ttl` are the ORIGINAL enqueue time and configured
        // TTL, carried forward unchanged so the message expires at its originally
        // configured deadline regardless of how many retries it goes through.
        self.enqueue(PendingMessage {
            packet: msg.packet,
            created: msg.created,
            ttl: msg.ttl,
            priority,
            retries: msg.retries,
        });
        true
    }

    /// Remove messages past TTL FROM both pending and inflight.
    pub fn drain_expired(&mut self) {
        let now = Instant::now();
        self.pending.retain(|msg| !msg.is_expired(now));
        self.inflight.retain(|_, msg| !msg.is_expired(now));
    }

    /// Number of messages waiting to be sent.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Number of messages currently inflight.
    #[must_use]
    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }
}

impl Default for OutboundQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT_ACK_TIMEOUT: Duration = Duration::from_secs(30);

    fn make_packet(id: u32, priority: Priority) -> MeshPacket {
        MeshPacket {
            from: 0xAAAA,
            to: 0xBBBB,
            channel: 0,
            id,
            rx_time: 0,
            rx_snr: 0.0,
            hop_limit: 3,
            want_ack: true,
            priority: i32::from(priority),
            rx_rssi: 0,
            via_mqtt: false,
            hop_start: 3,
            payload_variant: None,
        }
    }

    fn make_pending(id: u32, priority: Priority) -> PendingMessage {
        PendingMessage {
            packet: make_packet(id, priority),
            created: Instant::now(),
            ttl: Duration::from_secs(3600),
            priority,
            retries: 0,
        }
    }

    #[test]
    fn enqueue_orders_by_priority() {
        let mut q = OutboundQueue::new();
        q.enqueue(make_pending(1, Priority::Background));
        q.enqueue(make_pending(2, Priority::Reliable));
        q.enqueue(make_pending(3, Priority::Default));

        #[expect(clippy::unwrap_used, reason = "test-only: queue has 3 items")]
        let first = q.next_to_send().unwrap();
        assert_eq!(first.packet.id, 2, "Reliable (70) should come first");

        #[expect(clippy::unwrap_used, reason = "test-only: queue has 2 items")]
        let second = q.next_to_send().unwrap();
        assert_eq!(second.packet.id, 3, "Default (64) should come second");

        #[expect(clippy::unwrap_used, reason = "test-only: queue has 1 item")]
        let third = q.next_to_send().unwrap();
        assert_eq!(third.packet.id, 1, "Background (10) should come third");
    }

    #[tokio::test(start_paused = true)]
    async fn expired_messages_skipped() {
        let mut q = OutboundQueue::new();
        q.enqueue(PendingMessage {
            packet: make_packet(1, Priority::Default),
            created: Instant::now(),
            ttl: Duration::from_secs(1),
            priority: Priority::Default,
            retries: 0,
        });
        q.enqueue(PendingMessage {
            packet: make_packet(2, Priority::Default),
            created: Instant::now(),
            ttl: Duration::from_secs(3600),
            priority: Priority::Default,
            retries: 0,
        });

        // Advance past the first message's TTL.
        tokio::time::advance(Duration::from_secs(2)).await;

        #[expect(clippy::unwrap_used, reason = "test-only: non-expired message exists")]
        let msg = q.next_to_send().unwrap();
        assert_eq!(msg.packet.id, 2, "should skip expired message 1");
    }

    #[tokio::test(start_paused = true)]
    async fn inflight_timeout_detection() {
        let mut q = OutboundQueue::new();
        q.track_inflight(make_pending(42, Priority::Default), Duration::from_secs(10));

        assert!(q.check_timeouts().is_empty(), "not yet timed out");

        tokio::time::advance(Duration::from_secs(11)).await;

        let timed_out = q.check_timeouts();
        assert_eq!(timed_out.len(), 1);
        assert_eq!(
            timed_out.first().copied(),
            Some(PacketId(42)),
            "timed out packet should be 42"
        );
    }

    #[test]
    fn handle_ack_removes_inflight() {
        let mut q = OutboundQueue::new();
        q.track_inflight(make_pending(99, Priority::Default), DEFAULT_ACK_TIMEOUT);
        assert_eq!(q.inflight_count(), 1);

        let removed = q.handle_ack(PacketId(99));
        assert!(removed.is_some(), "should return the removed message");
        assert_eq!(q.inflight_count(), 0);
    }

    #[test]
    fn retry_requeues_until_max() {
        let mut q = OutboundQueue::new();
        q.track_inflight(make_pending(10, Priority::Default), DEFAULT_ACK_TIMEOUT);

        for i in 0..OutboundConfig::default().max_retries {
            assert!(q.retry(PacketId(10)), "retry {i} should succeed");
            // Re-track the re-enqueued message as inflight for the next retry.
            if let Some(msg) = q.next_to_send() {
                q.track_inflight(msg, DEFAULT_ACK_TIMEOUT);
            }
        }

        // One more retry should fail (max retries exceeded).
        assert!(!q.retry(PacketId(10)), "should fail after max retries");
    }

    #[tokio::test(start_paused = true)]
    async fn retry_preserves_original_ttl_deadline() {
        // WHY: regression test for retry() resetting `ttl`/`created` to a
        // hard-coded 3600s instead of honoring the message's own configured
        // TTL; a short-lived message must still expire at its ORIGINAL
        // deadline after being retried.
        let mut q = OutboundQueue::new();
        q.enqueue(PendingMessage {
            packet: make_packet(1, Priority::Default),
            created: Instant::now(),
            ttl: Duration::from_secs(60),
            priority: Priority::Default,
            retries: 0,
        });

        #[expect(clippy::unwrap_used, reason = "test-only: queue has 1 item")]
        let msg = q.next_to_send().unwrap();
        q.track_inflight(msg, DEFAULT_ACK_TIMEOUT);

        tokio::time::advance(Duration::from_secs(31)).await;
        assert!(
            q.retry(PacketId(1)),
            "retry should succeed before max_retries"
        );

        // Elapsed since original creation is now 61s, past the message's
        // originally configured 60s TTL — it must expire there, not be kept
        // alive by a fresh hour-long TTL substituted at retry time.
        tokio::time::advance(Duration::from_secs(30)).await;
        assert!(
            q.next_to_send().is_none(),
            "message should have expired at its original 60s deadline"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn drain_expired_removes_ttl_expired_inflight() {
        // WHY: regression test for drain_expired's inflight branch using a
        // hard-coded 3600s cutoff instead of each message's own TTL.
        let mut q = OutboundQueue::new();
        q.track_inflight(
            PendingMessage {
                packet: make_packet(5, Priority::Default),
                created: Instant::now(),
                ttl: Duration::from_secs(60),
                priority: Priority::Default,
                retries: 0,
            },
            DEFAULT_ACK_TIMEOUT,
        );
        assert_eq!(q.inflight_count(), 1);

        tokio::time::advance(Duration::from_secs(61)).await;
        q.drain_expired();
        assert_eq!(
            q.inflight_count(),
            0,
            "inflight message past its own TTL should be drained"
        );
    }

    #[test]
    fn max_inflight_limits_sends() {
        let mut q = OutboundQueue::with_max_inflight(2);
        q.enqueue(make_pending(1, Priority::Default));
        q.enqueue(make_pending(2, Priority::Default));
        q.enqueue(make_pending(3, Priority::Default));

        // Pop and track two as inflight.
        for _ in 0..2 {
            if let Some(msg) = q.next_to_send() {
                q.track_inflight(msg, DEFAULT_ACK_TIMEOUT);
            }
        }

        // Third should be blocked by max_inflight.
        assert!(
            q.next_to_send().is_none(),
            "should block when at max inflight"
        );
        assert_eq!(q.pending_count(), 1, "one message still pending");
    }

    #[test]
    fn drain_expired_removes_old_pending() {
        let mut q = OutboundQueue::new();
        // Create a message with zero TTL (already expired).
        q.pending.push_back(PendingMessage {
            packet: make_packet(1, Priority::Default),
            created: Instant::now(),
            ttl: Duration::ZERO,
            priority: Priority::Default,
            retries: 0,
        });
        q.enqueue(make_pending(2, Priority::Default));

        q.drain_expired();
        assert_eq!(q.pending_count(), 1, "expired message should be removed");
    }

    #[test]
    fn configured_max_retries_observably_caps_retries() {
        // WHY: parameterization-observability test — with max_retries=1,
        // retry() must return false after a single retry. Default (5) would
        // allow four more.
        let cfg = OutboundConfig {
            max_retries: 1,
            ..OutboundConfig::default()
        };
        let mut q = OutboundQueue::with_config(&cfg);
        q.track_inflight(make_pending(77, Priority::Default), DEFAULT_ACK_TIMEOUT);

        assert!(q.retry(PacketId(77)), "first retry should succeed");
        // Re-track for the next attempt.
        if let Some(msg) = q.next_to_send() {
            q.track_inflight(msg, DEFAULT_ACK_TIMEOUT);
        }
        assert!(
            !q.retry(PacketId(77)),
            "second retry should fail once max_retries=1 exceeded"
        );
    }

    #[test]
    fn configured_max_inflight_observably_limits_sends() {
        let cfg = OutboundConfig {
            max_inflight: 1,
            ..OutboundConfig::default()
        };
        let mut q = OutboundQueue::with_config(&cfg);
        q.enqueue(make_pending(1, Priority::Default));
        q.enqueue(make_pending(2, Priority::Default));

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let msg = q.next_to_send().unwrap();
        q.track_inflight(msg, DEFAULT_ACK_TIMEOUT);
        assert!(
            q.next_to_send().is_none(),
            "max_inflight=1 must block second send"
        );
    }

    #[test]
    fn alert_priority_sent_before_default() {
        let mut q = OutboundQueue::new();
        q.enqueue(make_pending(1, Priority::Default));
        q.enqueue(make_pending(2, Priority::Ack));

        #[expect(clippy::unwrap_used, reason = "test-only: queue has items")]
        let first = q.next_to_send().unwrap();
        assert_eq!(
            first.packet.id, 2,
            "ACK priority (120) should be sent before DEFAULT (64)"
        );
    }
}
