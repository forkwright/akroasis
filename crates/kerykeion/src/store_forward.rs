//! Store-and-forward message queuing for offline mesh nodes.

use std::collections::{HashMap, VecDeque};

use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::config::StoreForwardConfig;
use crate::error::{Error, QueueFullSnafu};
use crate::types::NodeNum;

/// A message stored for later delivery to an offline node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    /// The raw packet bytes (already encrypted).
    pub packet_bytes: Vec<u8>,
    /// Packet ID for tracking.
    pub packet_id: u32,
    /// Destination node number.
    pub dest: u32,
    /// Port number of the original application.
    pub portnum: i32,
    /// Priority level (i32 matching `mesh_packet::Priority`).
    pub priority: i32,
    /// When this message was stored (milliseconds since an epoch — serializable).
    pub stored_at_ms: u64,
    /// Time-to-live in seconds.
    pub ttl_secs: u64,
    /// Number of delivery attempts so far.
    pub delivery_attempts: u8,
}

/// Bounded queue for a single destination node.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct BoundedQueue {
    messages: VecDeque<StoredMessage>,
}

/// Server-side message queuing for offline nodes.
pub struct StoreForward {
    queues: HashMap<NodeNum, BoundedQueue>,
    config: StoreForwardConfig,
    /// Monotonic base time for TTL checks in-process.
    #[expect(
        dead_code,
        reason = "used for TTL baseline in production; tests use stored_at_ms, tracked in #244"
    )]
    base_instant: Instant,
}

impl StoreForward {
    /// Create a new store-and-forward queue with the given configuration.
    #[must_use]
    pub fn new(config: StoreForwardConfig) -> Self {
        Self {
            queues: HashMap::new(),
            config,
            base_instant: Instant::now(),
        }
    }

    /// Store a message for an offline destination node.
    ///
    /// # Errors
    ///
    /// Returns [`Error::QueueFull`] if the destination's queue is at capacity
    /// and the incoming message is not higher priority than any queued message.
    pub fn store(&mut self, dest: NodeNum, msg: StoredMessage) -> Result<(), Error> {
        let queue = self.queues.entry(dest).or_default();

        if queue.messages.len() >= self.config.max_queue_per_dest {
            // Evict lowest-priority message if the new one is higher priority.
            let min_priority = queue.messages.iter().map(|m| m.priority).min().unwrap_or(0);
            if msg.priority > min_priority {
                if let Some(pos) = queue
                    .messages
                    .iter()
                    .position(|m| m.priority == min_priority)
                {
                    queue.messages.remove(pos);
                }
            } else {
                return QueueFullSnafu { dest: dest.0 }.fail();
            }
        }

        queue.messages.push_back(msg);
        Ok(())
    }

    /// Drain all queued messages for a destination (when the node comes online).
    pub fn drain_for(&mut self, dest: NodeNum) -> Vec<StoredMessage> {
        self.queues
            .remove(&dest)
            .map(|q| q.messages.into_iter().collect())
            .unwrap_or_default()
    }

    /// Remove messages that have exceeded their TTL.
    ///
    /// Uses the `stored_at_ms` field and `ttl_secs` to determine expiration.
    /// `now_ms` is the current wall-clock time in milliseconds since the epoch.
    pub fn prune_expired(&mut self, now_ms: u64) {
        for queue in self.queues.values_mut() {
            queue.messages.retain(|msg| {
                let expires_at_ms = msg
                    .stored_at_ms
                    .saturating_add(msg.ttl_secs.saturating_mul(1000));
                now_ms < expires_at_ms
            });
        }
        // Remove empty queues.
        self.queues.retain(|_, q| !q.messages.is_empty());
    }

    /// Current queue depth for a destination node.
    #[must_use]
    pub fn queue_depth(&self, dest: NodeNum) -> usize {
        self.queues.get(&dest).map_or(0, |q| q.messages.len())
    }

    /// Total messages stored across all destination queues.
    #[must_use]
    pub fn total_stored(&self) -> usize {
        self.queues.values().map(|q| q.messages.len()).sum()
    }

    /// Serialize the store-forward state to JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StoreForwardSerde`] if serialization fails.
    pub fn serialize(&self) -> Result<Vec<u8>, Error> {
        let serializable: HashMap<u32, Vec<StoredMessage>> = self
            .queues
            .iter()
            .map(|(k, v)| (k.0, v.messages.iter().cloned().collect()))
            .collect();
        serde_json::to_vec(&serializable).map_err(|source| Error::StoreForwardSerde {
            source,
            location: snafu::location!(),
        })
    }

    /// Restore store-forward state from serialized JSON bytes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StoreForwardSerde`] if deserialization fails.
    pub fn deserialize(&mut self, data: &[u8]) -> Result<(), Error> {
        let raw: HashMap<u32, Vec<StoredMessage>> =
            serde_json::from_slice(data).map_err(|source| Error::StoreForwardSerde {
                source,
                location: snafu::location!(),
            })?;
        self.queues = raw
            .into_iter()
            .map(|(node, messages)| {
                (
                    NodeNum(node),
                    BoundedQueue {
                        messages: messages.into(),
                    },
                )
            })
            .collect();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(max_per_dest: usize) -> StoreForwardConfig {
        StoreForwardConfig {
            enabled: true,
            max_queue_per_dest: max_per_dest,
            message_ttl_secs: 3600,
        }
    }

    fn make_stored(id: u32, priority: i32, stored_at_ms: u64, ttl_secs: u64) -> StoredMessage {
        StoredMessage {
            packet_bytes: vec![0xAA, 0xBB],
            packet_id: id,
            dest: 0x1234,
            portnum: 1,
            priority,
            stored_at_ms,
            ttl_secs,
            delivery_attempts: 0,
        }
    }

    #[test]
    fn store_and_drain() {
        let mut sf = StoreForward::new(make_config(16));
        let dest = NodeNum(0x1234);

        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(1, 64, 1000, 3600)).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(2, 64, 2000, 3600)).unwrap();

        assert_eq!(sf.queue_depth(dest), 2);
        assert_eq!(sf.total_stored(), 2);

        let messages = sf.drain_for(dest);
        assert_eq!(messages.len(), 2);
        assert_eq!(sf.queue_depth(dest), 0);
    }

    #[test]
    fn queue_depth_limit_evicts_low_priority() {
        let mut sf = StoreForward::new(make_config(2));
        let dest = NodeNum(0x5678);

        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(1, 10, 1000, 3600)).unwrap(); // Background
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(2, 64, 2000, 3600)).unwrap(); // Default
        // Queue is now full (2). Higher-priority insert should evict background.
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(3, 70, 3000, 3600)).unwrap(); // Reliable

        assert_eq!(sf.queue_depth(dest), 2);
        let msgs = sf.drain_for(dest);
        // Background (priority=10) should have been evicted.
        assert!(
            msgs.iter().all(|m| m.priority >= 64),
            "background message should be evicted"
        );
    }

    #[test]
    fn queue_full_rejects_low_priority() {
        let mut sf = StoreForward::new(make_config(1));
        let dest = NodeNum(0xAAAA);

        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(1, 70, 1000, 3600)).unwrap(); // Reliable

        // Try to insert a lower-priority message — should fail.
        let result = sf.store(dest, make_stored(2, 10, 2000, 3600));
        assert!(result.is_err(), "should reject lower-priority message");
    }

    #[test]
    fn prune_expired_removes_old_messages() {
        let mut sf = StoreForward::new(make_config(16));
        let dest = NodeNum(0xBBBB);

        // Message stored at t=1000ms with TTL=1s → expires at t=2000ms.
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(1, 64, 1000, 1)).unwrap();
        // Message stored at t=1000ms with TTL=3600s → expires much later.
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest, make_stored(2, 64, 1000, 3600)).unwrap();

        sf.prune_expired(3000); // now = 3000ms, first message expired

        assert_eq!(sf.queue_depth(dest), 1, "expired message should be pruned");
    }

    #[test]
    fn serialize_deserialize_roundtrip() {
        let mut sf = StoreForward::new(make_config(16));
        let dest_a = NodeNum(0x1111);
        let dest_b = NodeNum(0x2222);

        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest_a, make_stored(1, 64, 1000, 3600)).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest_a, make_stored(2, 70, 2000, 3600)).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf.store(dest_b, make_stored(3, 10, 3000, 3600)).unwrap();

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let serialized = sf.serialize().unwrap();

        let mut sf2 = StoreForward::new(make_config(16));
        #[expect(clippy::unwrap_used, reason = "test-only")]
        sf2.deserialize(&serialized).unwrap();

        assert_eq!(sf2.queue_depth(dest_a), 2);
        assert_eq!(sf2.queue_depth(dest_b), 1);
        assert_eq!(sf2.total_stored(), 3);

        let msgs_a = sf2.drain_for(dest_a);
        assert_eq!(
            msgs_a.first().map(|m| m.packet_id),
            Some(1),
            "first stored message should have id 1"
        );
        assert_eq!(
            msgs_a.get(1).map(|m| m.packet_id),
            Some(2),
            "second stored message should have id 2"
        );
    }

    #[test]
    fn drain_for_nonexistent_node_returns_empty() {
        let mut sf = StoreForward::new(make_config(16));
        let msgs = sf.drain_for(NodeNum(0xDEAD));
        assert!(msgs.is_empty());
    }

    #[test]
    fn total_stored_across_multiple_destinations() {
        let mut sf = StoreForward::new(make_config(16));
        for i in 0..5u32 {
            #[expect(clippy::unwrap_used, reason = "test-only")]
            sf.store(NodeNum(i), make_stored(i, 64, 1000, 3600))
                .unwrap();
        }
        assert_eq!(sf.total_stored(), 5);
    }
}
