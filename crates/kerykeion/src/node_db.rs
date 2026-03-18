//! In-memory database of known mesh nodes.

use crate::types::NodeNum;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// In-memory store of all mesh nodes seen during a session.
#[derive(Debug, Default)]
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

/// User profile information for a mesh node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// Short unique node ID string (e.g. `!deadbeef`).
    pub id: String,
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
    pub fn insert(&mut self, node: MeshNode) {
        self.nodes.insert(node.num, node);
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
}
