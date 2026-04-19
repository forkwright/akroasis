//! Directed weighted mesh topology graph for node connectivity tracking.

use std::collections::HashMap;
use std::time::Duration;

use petgraph::Direction;
use petgraph::algo::dijkstra;
use petgraph::graph::NodeIndex;
use petgraph::stable_graph::StableGraph;
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::types::NodeNum;

/// Ceiling SNR value for Dijkstra cost derivation: `cost = SNR_CEILING - snr`.
const SNR_CEILING: f32 = 30.0;

/// Directed edge weight representing radio link quality between two nodes.
#[derive(Debug, Clone)]
pub struct LinkQuality {
    /// Signal-to-noise ratio in dB.
    pub snr: f32,
    /// When this link was last observed.
    pub last_observed: Instant,
    /// Number of packets observed on this link.
    pub packet_count: u32,
}

/// Directed weighted graph tracking mesh node connectivity.
pub struct MeshTopology {
    graph: StableGraph<NodeNum, LinkQuality, petgraph::Directed>,
    node_index: HashMap<NodeNum, NodeIndex>,
}

impl MeshTopology {
    /// Create an empty topology graph.
    #[must_use]
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            node_index: HashMap::new(),
        }
    }

    /// Insert a node or return its existing index.
    pub fn add_node(&mut self, node: NodeNum) -> NodeIndex {
        if let Some(&idx) = self.node_index.get(&node) {
            return idx;
        }
        let idx = self.graph.add_node(node);
        self.node_index.insert(node, idx);
        idx
    }

    /// Add or update a directed edge from `from` to `to` with the given SNR.
    pub fn update_link(&mut self, from: NodeNum, to: NodeNum, snr: f32) {
        let from_idx = self.add_node(from);
        let to_idx = self.add_node(to);

        // WHY: search existing edges to update rather than create duplicates.
        let existing = self
            .graph
            .edges_connecting(from_idx, to_idx)
            .next()
            .map(|e| e.id());

        if let Some(edge_id) = existing {
            if let Some(weight) = self.graph.edge_weight_mut(edge_id) {
                weight.snr = snr;
                weight.last_observed = Instant::now();
                weight.packet_count = weight.packet_count.saturating_add(1);
            }
        } else {
            self.graph.add_edge(
                from_idx,
                to_idx,
                LinkQuality {
                    snr,
                    last_observed: Instant::now(),
                    packet_count: 1,
                },
            );
        }
    }

    /// Remove nodes not heard within `timeout`.
    pub fn remove_stale_nodes(&mut self, timeout: Duration) {
        let cutoff = Instant::now() - timeout;
        let stale: Vec<NodeNum> = self
            .node_index
            .iter()
            .filter(|&(_, idx)| {
                // WHY: a node is stale if ALL its edges (incoming + outgoing) are older
                // than cutoff, or it has no edges at all.
                let has_recent = self
                    .graph
                    .edges_directed(*idx, Direction::Incoming)
                    .chain(self.graph.edges_directed(*idx, Direction::Outgoing))
                    .any(|e| e.weight().last_observed > cutoff);
                !has_recent
            })
            .map(|(num, _)| *num)
            .collect();

        for num in stale {
            if let Some(idx) = self.node_index.remove(&num) {
                self.graph.remove_node(idx);
            }
        }
    }

    /// Remove edges not observed within `timeout`.
    pub fn remove_stale_links(&mut self, timeout: Duration) {
        let cutoff = Instant::now() - timeout;
        let stale_edges: Vec<petgraph::graph::EdgeIndex> = self
            .graph
            .edge_indices()
            .filter(|&idx| {
                self.graph
                    .edge_weight(idx)
                    .is_some_and(|w| w.last_observed < cutoff)
            })
            .collect();

        for idx in stale_edges {
            self.graph.remove_edge(idx);
        }
    }

    /// Return direct neighbors of `node` with their link quality.
    #[must_use]
    pub fn neighbors(&self, node: NodeNum) -> Vec<(NodeNum, &LinkQuality)> {
        let Some(&idx) = self.node_index.get(&node) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .filter_map(|edge| {
                let target = self.graph.node_weight(edge.target());
                target.map(|&num| (num, edge.weight()))
            })
            .collect()
    }

    /// Dijkstra shortest path using SNR-derived weights (lower SNR = higher cost).
    #[must_use]
    pub fn shortest_path(&self, from: NodeNum, to: NodeNum) -> Option<Vec<NodeNum>> {
        let &from_idx = self.node_index.get(&from)?;
        let &to_idx = self.node_index.get(&to)?;

        // WHY: petgraph::algo::astar returns the full path; dijkstra only returns costs.
        let (_, path) = petgraph::algo::astar(
            &self.graph,
            from_idx,
            |n| n == to_idx,
            |e| {
                let cost = SNR_CEILING - e.weight().snr;
                if cost < 0.0 { 0.0_f32 } else { cost }
            },
            |_| 0.0_f32,
        )?;

        Some(
            path.iter()
                .filter_map(|&idx| self.graph.node_weight(idx).copied())
                .collect(),
        )
    }

    /// Minimum hop count between two nodes, ignoring link quality.
    #[must_use]
    pub fn hop_count(&self, from: NodeNum, to: NodeNum) -> Option<u8> {
        let &from_idx = self.node_index.get(&from)?;
        let &to_idx = self.node_index.get(&to)?;

        let costs = dijkstra(&self.graph, from_idx, Some(to_idx), |_| 1u32);
        let &hops = costs.get(&to_idx)?;
        u8::try_from(hops).ok()
    }

    /// Detect weakly connected components (mesh partitions).
    #[must_use]
    pub fn connected_components(&self) -> Vec<Vec<NodeNum>> {
        if self.node_index.is_empty() {
            return Vec::new();
        }

        // WHY: treat directed graph as undirected via BFS for partition detection.
        let mut component: HashMap<NodeIndex, usize> = HashMap::new();
        let mut components: Vec<Vec<NodeNum>> = Vec::new();

        for &idx in self.node_index.values() {
            if component.contains_key(&idx) {
                continue;
            }

            let comp_id = components.len();
            let mut group = Vec::new();
            let mut stack = vec![idx];

            while let Some(current) = stack.pop() {
                if component.contains_key(&current) {
                    continue;
                }
                component.insert(current, comp_id);
                if let Some(&num) = self.graph.node_weight(current) {
                    group.push(num);
                }

                // WHY: traverse both directions for weakly connected components.
                for edge in self.graph.edges_directed(current, Direction::Outgoing) {
                    if !component.contains_key(&edge.target()) {
                        stack.push(edge.target());
                    }
                }
                for edge in self.graph.edges_directed(current, Direction::Incoming) {
                    if !component.contains_key(&edge.source()) {
                        stack.push(edge.source());
                    }
                }
            }

            if !group.is_empty() {
                components.push(group);
            }
        }

        // WHY: nodes with no edges still need to appear as singleton components.
        for &idx in self.node_index.values() {
            if let std::collections::hash_map::Entry::Vacant(entry) = component.entry(idx) {
                if let Some(&num) = self.graph.node_weight(idx) {
                    components.push(vec![num]);
                    entry.insert(components.len() - 1);
                }
            }
        }

        components
    }

    /// Returns `true` if `node` is unreachable from `server_node`.
    #[must_use]
    pub fn is_partitioned(&self, node: NodeNum, server_node: NodeNum) -> bool {
        let Some(&server_idx) = self.node_index.get(&server_node) else {
            return true;
        };
        let Some(&node_idx) = self.node_index.get(&node) else {
            return true;
        };

        let costs = dijkstra(&self.graph, server_idx, Some(node_idx), |_| 1u32);
        !costs.contains_key(&node_idx)
    }

    /// Return the number of tracked nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.node_index.len()
    }

    /// Return the number of tracked links.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Check if a node is in the topology.
    #[must_use]
    pub fn contains_node(&self, node: NodeNum) -> bool {
        self.node_index.contains_key(&node)
    }

    /// Serialize the current graph state for persistence.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::TopologySnapshot`] if JSON serialization fails.
    pub fn save_to_bytes(&self) -> Result<Vec<u8>, crate::Error> {
        let nodes: Vec<NodeNum> = self.node_index.keys().copied().collect();

        let links: Vec<LinkSnapshot> = self
            .graph
            .edge_indices()
            .filter_map(|idx| {
                let (src, dst) = self.graph.edge_endpoints(idx)?;
                let from = *self.graph.node_weight(src)?;
                let to = *self.graph.node_weight(dst)?;
                let w = self.graph.edge_weight(idx)?;
                Some(LinkSnapshot {
                    from,
                    to,
                    snr: w.snr,
                    packet_count: w.packet_count,
                })
            })
            .collect();

        let snapshot = TopologySnapshot { nodes, links };
        serde_json::to_vec(&snapshot).map_err(|source| crate::Error::TopologySnapshot {
            source,
            location: snafu::Location::new(file!(), line!(), column!()),
        })
    }

    /// Restore topology from a serialized snapshot. All links are marked as observed now.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::TopologySnapshot`] if JSON deserialization fails.
    pub fn load_from_bytes(data: &[u8]) -> Result<Self, crate::Error> {
        let snapshot: TopologySnapshot =
            serde_json::from_slice(data).map_err(|source| crate::Error::TopologySnapshot {
                source,
                location: snafu::Location::new(file!(), line!(), column!()),
            })?;

        let mut topo = Self::new();
        for node in &snapshot.nodes {
            topo.add_node(*node);
        }
        for link in &snapshot.links {
            let from_idx = topo.add_node(link.from);
            let to_idx = topo.add_node(link.to);
            topo.graph.add_edge(
                from_idx,
                to_idx,
                LinkQuality {
                    snr: link.snr,
                    last_observed: Instant::now(),
                    packet_count: link.packet_count,
                },
            );
        }
        Ok(topo)
    }
}

impl Default for MeshTopology {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializable link snapshot for persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSnapshot {
    /// Source node.
    pub from: NodeNum,
    /// Destination node.
    pub to: NodeNum,
    /// SNR at last observation.
    pub snr: f32,
    /// Total observed packets.
    pub packet_count: u32,
}

/// Serializable topology state for restart recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySnapshot {
    /// All tracked node numbers.
    pub nodes: Vec<NodeNum>,
    /// All tracked links.
    pub links: Vec<LinkSnapshot>,
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    fn n(v: u32) -> NodeNum {
        NodeNum(v)
    }

    #[test]
    fn add_node_idempotent() {
        let mut topo = MeshTopology::new();
        let idx1 = topo.add_node(n(1));
        let idx2 = topo.add_node(n(1));
        assert_eq!(idx1, idx2);
        assert_eq!(topo.node_count(), 1);
    }

    #[test]
    fn update_link_creates_edge() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 5.0);
        assert_eq!(topo.edge_count(), 1);
        let neighbors = topo.neighbors(n(1));
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].0, n(2));
        assert!((neighbors[0].1.snr - 5.0).abs() < f32::EPSILON);
    }

    #[test]
    fn update_link_updates_existing() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 5.0);
        topo.update_link(n(1), n(2), 8.0);
        assert_eq!(topo.edge_count(), 1, "should not duplicate edges");
        let neighbors = topo.neighbors(n(1));
        assert!((neighbors[0].1.snr - 8.0).abs() < f32::EPSILON);
        assert_eq!(neighbors[0].1.packet_count, 2);
    }

    #[test]
    fn shortest_path_simple_chain() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.update_link(n(2), n(3), 10.0);
        topo.update_link(n(1), n(3), 1.0); // direct but weak link
        let path = topo.shortest_path(n(1), n(3)).unwrap();
        // WHY: via n(2) has cost 20+20=40, direct has cost 29 — direct is cheaper
        assert_eq!(path, vec![n(1), n(3)]);
    }

    #[test]
    fn shortest_path_prefers_strong_signal() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 25.0); // cost 5
        topo.update_link(n(2), n(3), 25.0); // cost 5, total 10
        topo.update_link(n(1), n(3), 5.0); // cost 25
        let path = topo.shortest_path(n(1), n(3)).unwrap();
        assert_eq!(path, vec![n(1), n(2), n(3)]);
    }

    #[test]
    fn shortest_path_unreachable_returns_none() {
        let mut topo = MeshTopology::new();
        topo.add_node(n(1));
        topo.add_node(n(2));
        assert!(topo.shortest_path(n(1), n(2)).is_none());
    }

    #[test]
    fn hop_count_returns_minimum_hops() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.update_link(n(2), n(3), 10.0);
        assert_eq!(topo.hop_count(n(1), n(3)), Some(2));
        assert_eq!(topo.hop_count(n(1), n(2)), Some(1));
    }

    #[test]
    fn connected_components_single_cluster() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.update_link(n(2), n(3), 10.0);
        let comps = topo.connected_components();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].len(), 3);
    }

    #[test]
    fn connected_components_two_clusters() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.update_link(n(3), n(4), 10.0);
        let comps = topo.connected_components();
        assert_eq!(comps.len(), 2, "two disconnected clusters");
    }

    #[test]
    fn is_partitioned_detects_unreachable() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.add_node(n(3));
        assert!(!topo.is_partitioned(n(2), n(1)));
        assert!(topo.is_partitioned(n(3), n(1)));
    }

    #[tokio::test(start_paused = true)]
    async fn remove_stale_links_prunes_old_edges() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        tokio::time::advance(Duration::from_secs(120)).await;
        topo.update_link(n(1), n(3), 10.0);
        topo.remove_stale_links(Duration::from_secs(60));
        assert_eq!(topo.edge_count(), 1, "only the fresh edge should remain");
        assert!(topo.neighbors(n(1)).iter().any(|(num, _)| *num == n(3)));
    }

    #[tokio::test(start_paused = true)]
    async fn remove_stale_nodes_prunes_isolated() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        tokio::time::advance(Duration::from_secs(120)).await;
        topo.update_link(n(3), n(4), 10.0);
        topo.remove_stale_nodes(Duration::from_secs(60));
        assert!(!topo.contains_node(n(1)), "stale node 1 should be removed");
        assert!(!topo.contains_node(n(2)), "stale node 2 should be removed");
        assert!(topo.contains_node(n(3)));
        assert!(topo.contains_node(n(4)));
    }

    #[test]
    fn snapshot_roundtrip() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 7.5);
        topo.update_link(n(2), n(3), 12.0);
        topo.add_node(n(4));

        let bytes = topo.save_to_bytes().unwrap();
        let restored = MeshTopology::load_from_bytes(&bytes).unwrap();

        assert_eq!(restored.node_count(), 4);
        assert_eq!(restored.edge_count(), 2);
        let neighbors = restored.neighbors(n(1));
        assert_eq!(neighbors.len(), 1);
        assert!((neighbors[0].1.snr - 7.5).abs() < f32::EPSILON);
    }

    #[test]
    fn neighbors_of_unknown_node_returns_empty() {
        let topo = MeshTopology::new();
        assert!(topo.neighbors(n(99)).is_empty());
    }
}
