//! In-memory database of known mesh nodes.

use std::collections::HashMap;
use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::types::{MAX_LIVE_NODES, NodeIdStr, NodeNum};

/// In-memory store of all mesh nodes seen during a session.
#[derive(Debug, Default, Clone)]
pub struct NodeDb {
    nodes: HashMap<NodeNum, MeshNode>,
    my_node: Option<NodeNum>,
}

/// A node observed on the mesh network.
#[derive(Debug, Clone)]
pub struct MeshNode {
    /// Node number (last 4 bytes of MAC address).
    pub num: NodeNum,
    /// User profile if a `NODEINFO_APP` packet has been received.
    pub user: Option<UserInfo>,
    /// Last known GPS position, if any.
    pub position: Option<NodePosition>,
    /// Last reported device metrics (battery, channel utilization).
    pub metrics: Option<DeviceMetrics>,
    /// Time of the most recent packet from this node.
    pub last_heard: Option<Timestamp>,
    /// Signal-to-noise ratio of the most recent packet, in dB.
    pub snr: Option<f32>,
    /// Number of hops away from the gateway node.
    pub hop_count: Option<u8>,
}

impl MeshNode {
    /// Time elapsed since this node was last heard from, as of `now`.
    ///
    /// Returns `None` if the node has never sent a packet.
    #[must_use]
    pub fn elapsed_since_heard(&self, now: Timestamp) -> Option<Duration> {
        let last_heard = self.last_heard?;
        let elapsed_ms = now
            .as_millisecond()
            .saturating_sub(last_heard.as_millisecond());
        #[expect(
            clippy::cast_sign_loss,
            reason = "elapsed_ms is always non-negative since now >= last_heard"
        )]
        let elapsed = Duration::from_millis(elapsed_ms as u64); // SAFETY: last_heard is always <= now for any node reachable via NodeDb iteration
        Some(elapsed)
    }
}

/// User profile information for a mesh node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// Short unique node ID string (e.g. `!deadbeef`).
    pub id: NodeIdStr,
    /// Long display name.
    pub long_name: String,
    /// Short display name (up to 4 characters).
    pub short_name: String,
    /// Hardware model identifier.
    pub hw_model: u32,
    /// Whether the user holds an amateur radio licence.
    pub is_licensed: bool,
}

/// GPS position snapshot for a mesh node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePosition {
    /// Latitude in decimal degrees.
    pub latitude: f64,
    /// Longitude in decimal degrees.
    pub longitude: f64,
    /// Altitude in metres above sea level, if reported.
    pub altitude: Option<i32>,
    /// Time the position fix was taken.
    pub timestamp: Option<Timestamp>,
}

/// Device hardware metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetrics {
    /// Battery level as a percentage (0–100), if reported.
    pub battery_level: Option<u32>,
    /// Battery voltage in volts, if reported.
    pub voltage: Option<f32>,
    /// Fraction of airtime used by this node's channel, if reported.
    pub channel_utilization: Option<f32>,
    /// Fraction of airtime used for TX, if reported.
    pub air_util_tx: Option<f32>,
}

impl NodeDb {
    /// Creates an empty `NodeDb`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: std::collections::HashMap::new(),
            my_node: None,
        }
    }

    /// Inserts or replaces a node record.
    ///
    /// If `node.num` is not already tracked and the table is at
    /// [`MAX_LIVE_NODES`], the least-recently-heard tracked node is evicted
    /// first (#204) — `from` on an inbound frame is unauthenticated, so an
    /// OTA peer can announce unbounded distinct identities without this.
    pub fn insert(&mut self, node: MeshNode) {
        if !self.nodes.contains_key(&node.num) && self.nodes.len() >= MAX_LIVE_NODES {
            self.evict_stalest();
        }
        self.nodes.insert(node.num, node);
    }

    /// Remove the least-recently-heard tracked node to make room for an
    /// insertion, protecting [`Self::my_node`] — the local radio's own
    /// identity — from eviction.
    ///
    // WHY least-recently-heard rather than insertion order or a
    // hash/id-derived victim (#204): both of those give an attacker a
    // predictable target — flood enough distinct fake identities and the
    // Nth-inserted (or lowest-hashing) *real* node is evicted on schedule.
    // Staleness-by-`last_heard` means only entries an attacker themselves
    // stopped refreshing become evictable; a sustained flood of one-shot
    // identities degrades to evicting the flood's own earlier entries, and
    // any legitimate node that keeps transmitting keeps refreshing its
    // `last_heard` and stays out of eviction range.
    //
    // WARNING: if every tracked entry is protected (`my_node` is the only
    // entry) this is a no-op and `insert` grows one past the cap — not
    // reachable in practice since MAX_LIVE_NODES is far above a table of one.
    fn evict_stalest(&mut self) {
        let victim = self
            .nodes
            .iter()
            .filter(|&(&num, _)| Some(num) != self.my_node)
            .min_by_key(|&(_, node)| {
                node.last_heard
                    .map(Timestamp::as_millisecond)
                    .unwrap_or(i64::MIN)
            })
            .map(|(&num, _)| num);
        if let Some(victim) = victim {
            self.nodes.remove(&victim);
        }
    }

    /// Returns a reference to the node with the given number, if present.
    #[must_use]
    pub fn get(&self, num: NodeNum) -> Option<&MeshNode> {
        self.nodes.get(&num)
    }

    /// Removes and returns the node with the given number, if present.
    pub fn remove(&mut self, num: NodeNum) -> Option<MeshNode> {
        self.nodes.remove(&num)
    }

    /// Returns an iterator over all nodes in the database.
    pub fn iter(&self) -> impl Iterator<Item = (&NodeNum, &MeshNode)> {
        self.nodes.iter()
    }

    /// Returns the number of nodes in the database.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the database contains no nodes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Sets the node number of the local radio.
    pub const fn set_my_node(&mut self, num: NodeNum) {
        self.my_node = Some(num);
    }

    /// Returns the node number of the local radio, if known.
    #[must_use]
    pub const fn my_node(&self) -> Option<NodeNum> {
        self.my_node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(num: u32) -> MeshNode {
        MeshNode {
            num: NodeNum(num),
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: None,
            hop_count: None,
        }
    }

    #[test]
    fn insert_and_get() {
        let mut db = NodeDb::new();
        db.insert(make_node(0x1234));
        assert!(db.get(NodeNum(0x1234)).is_some());
    }

    #[test]
    fn remove_existing_node() {
        let mut db = NodeDb::new();
        db.insert(make_node(0xABCD));
        let removed = db.remove(NodeNum(0xABCD));
        assert!(removed.is_some());
        assert!(db.get(NodeNum(0xABCD)).is_none());
    }

    #[test]
    fn remove_missing_node_returns_none() {
        let mut db = NodeDb::new();
        assert!(db.remove(NodeNum(0xDEAD)).is_none());
    }

    #[test]
    fn iter_returns_all_nodes() {
        let mut db = NodeDb::new();
        db.insert(make_node(1));
        db.insert(make_node(2));
        db.insert(make_node(3));
        assert_eq!(db.iter().count(), 3);
    }

    #[test]
    fn len_and_is_empty() {
        let mut db = NodeDb::new();
        assert!(db.is_empty());
        db.insert(make_node(42));
        assert_eq!(db.len(), 1);
        assert!(!db.is_empty());
    }

    #[test]
    fn my_node_roundtrip() {
        let mut db = NodeDb::new();
        assert!(db.my_node().is_none());
        db.set_my_node(NodeNum(0xCAFE_BABE));
        assert_eq!(db.my_node(), Some(NodeNum(0xCAFE_BABE)));
    }

    #[test]
    fn insert_replaces_existing() {
        let mut db = NodeDb::new();
        db.insert(make_node(1));
        let mut updated = make_node(1);
        updated.snr = Some(4.5);
        db.insert(updated);
        assert_eq!(db.len(), 1);
        assert_eq!(db.get(NodeNum(1)).and_then(|n| n.snr), Some(4.5));
    }

    fn make_node_heard(num: u32, secs: i64) -> MeshNode {
        let mut node = make_node(num);
        #[expect(clippy::unwrap_used, reason = "test-only: secs is a small fixed value")]
        {
            node.last_heard = Some(Timestamp::from_second(secs).unwrap());
        }
        node
    }

    #[test]
    fn insert_bounds_live_cardinality_at_the_cap() {
        // WHY(#204): `from` on an inbound frame is unauthenticated, so an
        // OTA peer announcing MAX_LIVE_NODES+N distinct identities must
        // never grow the table past the cap.
        let mut db = NodeDb::new();
        for i in 0..(MAX_LIVE_NODES as u32 + 500) {
            db.insert(make_node(i));
        }
        assert!(
            db.len() <= MAX_LIVE_NODES,
            "len()={} exceeds MAX_LIVE_NODES={MAX_LIVE_NODES}",
            db.len()
        );
    }

    #[test]
    fn insert_evicts_the_stalest_node_and_protects_my_node() {
        let mut db = NodeDb::new();
        let my_num = NodeNum(0xAAAA);
        db.set_my_node(my_num);
        // my_node is the freshest entry — if eviction ever picked it despite
        // that, this test still would not catch a staleness-ordering bug;
        // the explicit `my_node` protection is what's under test here.
        db.insert(make_node_heard(my_num.0, 1_000_000_000));

        // Fill to the cap with MAX_LIVE_NODES-1 more distinct, strictly
        // increasing-freshness nodes (node `i` has timestamp `i`), so node 0
        // is the single stalest entry and no eviction has fired yet.
        for i in 0..(MAX_LIVE_NODES as u32 - 1) {
            db.insert(make_node_heard(i + 1, i64::from(i)));
        }
        assert_eq!(db.len(), MAX_LIVE_NODES, "setup must reach the cap");
        assert!(
            db.get(NodeNum(1)).is_some(),
            "setup must not have evicted node 1 yet"
        );

        db.insert(make_node_heard(0xFFFF, 2_000_000_000));

        assert!(
            db.get(NodeNum(1)).is_none(),
            "the stalest node (timestamp 0) must be the one evicted"
        );
        assert_eq!(
            db.my_node(),
            Some(my_num),
            "my_node identity must survive eviction pressure"
        );
        assert!(
            db.get(my_num).is_some(),
            "my_node's own record must survive"
        );
        assert!(
            db.get(NodeNum(0xFFFF)).is_some(),
            "the new node must be present"
        );
        assert_eq!(db.len(), MAX_LIVE_NODES);
    }
}
