//! In-memory node database for tracking mesh participants.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::error::{Error, NodeNotFoundSnafu};
use crate::types::NodeNum;

/// In-memory database of known mesh nodes.
#[derive(Debug, Default)]
pub struct NodeDb {
    nodes: HashMap<NodeNum, MeshNode>,
    my_node: Option<NodeNum>,
}

/// A single mesh node's cached state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshNode {
    /// 4-byte node address.
    pub num: NodeNum,
    /// User identity (long name, short name).
    pub user: Option<UserInfo>,
    /// Last known GPS position.
    pub position: Option<NodePosition>,
    /// Device telemetry (battery, voltage, uptime).
    pub metrics: Option<DeviceMetrics>,
    /// When this node was last heard from.
    #[serde(skip)]
    pub last_heard: Option<Instant>,
    /// Signal-to-noise ratio of last received packet (dB).
    pub snr: Option<f32>,
    /// Hop count from last received packet.
    pub hop_count: Option<u8>,
}

/// User identity information broadcast by a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// Full display name.
    pub long_name: String,
    /// 4-character short name.
    pub short_name: String,
    /// Hardware model identifier.
    pub hw_model: i32,
}

/// GPS position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePosition {
    /// Latitude in degrees.
    pub latitude: f64,
    /// Longitude in degrees.
    pub longitude: f64,
    /// Altitude in metres above MSL.
    pub altitude: Option<f64>,
}

/// Device telemetry metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetrics {
    /// Battery level (0–100).
    pub battery_level: u8,
    /// Supply voltage in volts.
    pub voltage: f32,
    /// Uptime in seconds.
    pub uptime_secs: u32,
}

impl NodeDb {
    /// Create an empty node database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update a node.
    pub fn insert(&mut self, node: MeshNode) {
        self.nodes.insert(node.num, node);
    }

    /// Look up a node by its address.
    #[must_use]
    pub fn get(&self, num: NodeNum) -> Option<&MeshNode> {
        self.nodes.get(&num)
    }

    /// Remove a node, returning it if it existed.
    pub fn remove(&mut self, num: NodeNum) -> Option<MeshNode> {
        self.nodes.remove(&num)
    }

    /// Iterate over all known nodes.
    pub fn iter(&self) -> impl Iterator<Item = &MeshNode> {
        self.nodes.values()
    }

    /// Number of tracked nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the database is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Set the local node address (received during config handshake).
    pub const fn set_my_node(&mut self, num: NodeNum) {
        self.my_node = Some(num);
    }

    /// Return the local node address.
    #[must_use]
    pub const fn my_node(&self) -> Option<NodeNum> {
        self.my_node
    }

    /// Look up a node, returning an error if not found.
    ///
    /// # Errors
    ///
    /// Returns `NodeNotFound` if the node is not in the database.
    pub fn require(&self, num: NodeNum) -> Result<&MeshNode, Error> {
        self.nodes
            .get(&num)
            .ok_or_else(|| NodeNotFoundSnafu { node_num: num.0 }.build())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn test_node(num: u32) -> MeshNode {
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
        db.insert(test_node(1));
        assert!(db.get(NodeNum(1)).is_some());
        assert!(db.get(NodeNum(2)).is_none());
    }

    #[test]
    fn remove_returns_node() {
        let mut db = NodeDb::new();
        db.insert(test_node(42));
        let removed = db.remove(NodeNum(42));
        assert!(removed.is_some());
        assert!(db.is_empty());
    }

    #[test]
    fn remove_missing_returns_none() {
        let mut db = NodeDb::new();
        assert!(db.remove(NodeNum(99)).is_none());
    }

    #[test]
    fn iter_returns_all_nodes() {
        let mut db = NodeDb::new();
        db.insert(test_node(1));
        db.insert(test_node(2));
        db.insert(test_node(3));
        assert_eq!(db.iter().count(), 3);
    }

    #[test]
    fn len_and_is_empty() {
        let mut db = NodeDb::new();
        assert!(db.is_empty());
        assert_eq!(db.len(), 0);
        db.insert(test_node(1));
        assert!(!db.is_empty());
        assert_eq!(db.len(), 1);
    }

    #[test]
    fn my_node_lifecycle() {
        let mut db = NodeDb::new();
        assert!(db.my_node().is_none());
        db.set_my_node(NodeNum(0xDEAD));
        assert_eq!(db.my_node(), Some(NodeNum(0xDEAD)));
    }

    #[test]
    fn require_returns_error_for_missing_node() {
        let db = NodeDb::new();
        let err = db.require(NodeNum(0xBEEF));
        assert!(err.is_err());
    }

    #[test]
    fn require_returns_node_when_present() {
        let mut db = NodeDb::new();
        db.insert(test_node(5));
        let node = db.require(NodeNum(5)).expect("should find node");
        assert_eq!(node.num, NodeNum(5));
    }

    #[test]
    fn insert_overwrites_existing() {
        let mut db = NodeDb::new();
        let mut node = test_node(1);
        node.snr = Some(5.0);
        db.insert(node);

        let mut updated = test_node(1);
        updated.snr = Some(10.0);
        db.insert(updated);

        assert_eq!(db.len(), 1);
        assert_eq!(db.get(NodeNum(1)).expect("exists").snr, Some(10.0));
    }
}
