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

use crate::config::TopologyConfig;
use crate::types::NodeNum;

// Historical default (30.0) now lives in [`TopologyConfig::default`].

/// Maximum nodes accepted from a persisted topology snapshot.
///
// WHY: `load_from_bytes` allocates one graph node per entry from a file that
// may be truncated, corrupt or attacker-written. A Meshtastic node DB holds low
// hundreds of nodes, so this ceiling is far above any real mesh while still
// bounding the allocation. Exceeding it is recoverable: passive learning
// re-observes live links, so a truncated restore self-heals.
const MAX_SNAPSHOT_NODES: usize = 4096;

/// Maximum links accepted from a persisted topology snapshot.
const MAX_SNAPSHOT_LINKS: usize = 16384;

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
        // WHY: a non-finite SNR (NaN/Inf from OTA protobuf) corrupts astar
        // cost (line below: `ceiling - snr` → -Inf cost, a free edge) and
        // cannot round-trip through the JSON snapshot. Reject before storage
        // and before touching the graph — a later valid observation still
        // adds the nodes.
        if !snr.is_finite() {
            tracing::warn!(
                from = from.0,
                to = to.0,
                snr,
                "rejecting non-finite link SNR"
            );
            return;
        }

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
        // WHY: `Instant::now() - timeout` panics on underflow when `timeout`
        // exceeds process uptime (e.g. the default 7200s stale window on a
        // freshly booted host). `checked_sub` returning `None` means nothing
        // has been up long enough to be stale yet.
        let Some(cutoff) = Instant::now().checked_sub(timeout) else {
            return;
        };
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
        // WHY: see `remove_stale_nodes` — avoid underflow panic when
        // `timeout` exceeds process uptime.
        let Some(cutoff) = Instant::now().checked_sub(timeout) else {
            return;
        };
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
    ///
    /// Uses [`TopologyConfig::default`]'s `snr_ceiling` for cost derivation.
    /// Call [`Self::shortest_path_with_config`] to supply an operator- or
    /// agent-tuned ceiling via [`TopologyConfig`].
    #[must_use]
    pub fn shortest_path(&self, from: NodeNum, to: NodeNum) -> Option<Vec<NodeNum>> {
        self.shortest_path_with_ceiling(from, to, TopologyConfig::default().snr_ceiling)
    }

    /// Like [`Self::shortest_path`] but sources the SNR ceiling from
    /// [`TopologyConfig::snr_ceiling`].
    #[must_use]
    pub fn shortest_path_with_config(
        &self,
        from: NodeNum,
        to: NodeNum,
        config: &TopologyConfig,
    ) -> Option<Vec<NodeNum>> {
        self.shortest_path_with_ceiling(from, to, config.snr_ceiling)
    }

    /// Dijkstra shortest path with an explicit SNR ceiling.
    ///
    /// `cost = max(ceiling - observed_snr, 0)`. Higher ceilings flatten the
    /// cost function; lower ceilings amplify the preference for strong
    /// links at the expense of hop count.
    #[must_use]
    pub fn shortest_path_with_ceiling(
        &self,
        from: NodeNum,
        to: NodeNum,
        ceiling: f32,
    ) -> Option<Vec<NodeNum>> {
        let &from_idx = self.node_index.get(&from)?;
        let &to_idx = self.node_index.get(&to)?;

        // WHY: petgraph::algo::astar returns the full path; dijkstra only returns costs.
        let (_, path) = petgraph::algo::astar(
            &self.graph,
            from_idx,
            |n| n == to_idx,
            |e| {
                let cost = ceiling - e.weight().snr;
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
    ///
    /// NOTE: reuses [`Self::connected_components`] (undirected) rather than a
    /// directed dijkstra query — mesh edges point toward the server
    /// (`update_link(from=heard_node, to=my_node)`), so a directed
    /// server->node query false-positives on nodes only reachable via a
    /// node->server edge.
    #[must_use]
    pub fn is_partitioned(&self, node: NodeNum, server_node: NodeNum) -> bool {
        if !self.node_index.contains_key(&node) || !self.node_index.contains_key(&server_node) {
            return true;
        }
        !self
            .connected_components()
            .iter()
            .any(|group| group.contains(&node) && group.contains(&server_node))
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
            location: snafu::location!(),
        })
    }

    /// Restore topology from a serialized snapshot. All links are marked as observed now.
    ///
    /// Repeated `(from, to)` pairs are folded into a single edge, and the
    /// restore is bounded at [`MAX_SNAPSHOT_NODES`] / [`MAX_SNAPSHOT_LINKS`].
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::TopologySnapshot`] if JSON deserialization fails.
    pub fn load_from_bytes(data: &[u8]) -> Result<Self, crate::Error> {
        let snapshot: TopologySnapshot =
            serde_json::from_slice(data).map_err(|source| crate::Error::TopologySnapshot {
                source,
                location: snafu::location!(),
            })?;

        // WHY: never truncate silently  -  a restore that dropped half the mesh
        // without saying so reads as a small mesh rather than a bad snapshot.
        if snapshot.nodes.len() > MAX_SNAPSHOT_NODES {
            tracing::warn!(
                present = snapshot.nodes.len(),
                cap = MAX_SNAPSHOT_NODES,
                "topology snapshot exceeds node cap; restoring a prefix"
            );
        }
        if snapshot.links.len() > MAX_SNAPSHOT_LINKS {
            tracing::warn!(
                present = snapshot.links.len(),
                cap = MAX_SNAPSHOT_LINKS,
                "topology snapshot exceeds link cap; restoring a prefix"
            );
        }

        let mut topo = Self::new();
        for node in snapshot.nodes.iter().take(MAX_SNAPSHOT_NODES) {
            topo.add_node(*node);
        }
        for link in snapshot.links.iter().take(MAX_SNAPSHOT_LINKS) {
            let from_idx = topo.add_node(link.from);
            let to_idx = topo.add_node(link.to);

            // WHY: `update_link` keeps at most one edge per ordered pair, and
            // `to_bytes` re-emits whatever edges exist. Restoring with a bare
            // `add_edge` admits parallel edges the live path can never create,
            // and each save/load cycle multiplies them. Fold instead: the last
            // observation wins, and the counts add.
            let existing = topo
                .graph
                .edges_connecting(from_idx, to_idx)
                .next()
                .map(|e| e.id());

            if let Some(edge_id) = existing {
                if let Some(weight) = topo.graph.edge_weight_mut(edge_id) {
                    weight.snr = link.snr;
                    weight.packet_count = weight.packet_count.saturating_add(link.packet_count);
                }
            } else {
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
    clippy::expect_used,
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
    fn update_link_rejects_non_finite_snr() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), f32::NAN);
        topo.update_link(n(1), n(2), f32::INFINITY);
        assert_eq!(
            topo.edge_count(),
            0,
            "non-finite SNR must not create an edge"
        );
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

    #[test]
    fn is_partitioned_false_for_node_to_server_directed_edge() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(2), n(1), 10.0);
        assert!(
            !topo.is_partitioned(n(2), n(1)),
            "a node->server edge must not read as partitioned"
        );
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

    // WHY: regression for the monotonic-clock underflow panic (#206) — a
    // fresh process (t≈0, no `tokio::time::advance`) pruning against the
    // default 7200s stale window must not panic, and nothing is old enough
    // to be considered stale yet.
    #[tokio::test(start_paused = true)]
    async fn remove_stale_links_no_panic_when_timeout_exceeds_uptime() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.remove_stale_links(Duration::from_secs(7200));
        assert_eq!(topo.edge_count(), 1, "nothing is stale yet at t=0");
    }

    #[tokio::test(start_paused = true)]
    async fn remove_stale_nodes_no_panic_when_timeout_exceeds_uptime() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 10.0);
        topo.remove_stale_nodes(Duration::from_secs(7200));
        assert!(topo.contains_node(n(1)), "nothing is stale yet at t=0");
        assert!(topo.contains_node(n(2)), "nothing is stale yet at t=0");
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

    #[test]
    fn shortest_path_ceiling_changes_selected_route() {
        // WHY: parameterization-observability test — the same graph must
        // produce a different path depending on snr_ceiling.
        //
        // With default ceiling 30: direct link (snr 29 → cost 1) beats the
        // 2-hop route (snr 28 each → cost 4) — direct wins.
        // With ceiling 5 (clamped to 0 for any snr>=5): both paths cost 0,
        // but the 1-hop direct path is selected by astar's determinism.
        // A ceiling just above the stronger links asymmetrically penalises
        // the weaker direct link more than the two-hop route, so raising
        // the ceiling from an "equal" value to a value where only the
        // direct link is below ceiling flips the answer.
        //
        // Construction: direct link snr=10, 2-hop path snr=19 each.
        //   ceiling=20  →  direct cost=10, 2-hop cost=1+1=2 → 2-hop wins
        //   ceiling=11  →  direct cost=1,  2-hop cost=0+0=0 (clamped) → 2-hop still wins by cost
        //   ceiling=9   →  direct cost=0 (clamped), 2-hop cost=0 → direct wins (1 hop)
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 19.0);
        topo.update_link(n(2), n(3), 19.0);
        topo.update_link(n(1), n(3), 10.0);

        let path_high = topo
            .shortest_path_with_ceiling(n(1), n(3), 20.0)
            .expect("reachable");
        assert_eq!(
            path_high,
            vec![n(1), n(2), n(3)],
            "ceiling 20 penalises direct link (cost 10) more than 2-hop (cost 2)"
        );

        let path_low = topo
            .shortest_path_with_ceiling(n(1), n(3), 9.0)
            .expect("reachable");
        assert_eq!(
            path_low,
            vec![n(1), n(3)],
            "ceiling 9 clamps all costs to 0; astar picks the 1-hop path"
        );
    }

    #[test]
    fn shortest_path_with_config_uses_supplied_ceiling() {
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 19.0);
        topo.update_link(n(2), n(3), 19.0);
        topo.update_link(n(1), n(3), 10.0);

        let cfg_high = TopologyConfig {
            snr_ceiling: 20.0,
            ..TopologyConfig::default()
        };
        let cfg_low = TopologyConfig {
            snr_ceiling: 9.0,
            ..TopologyConfig::default()
        };

        assert_eq!(
            topo.shortest_path_with_config(n(1), n(3), &cfg_high)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            topo.shortest_path_with_config(n(1), n(3), &cfg_low)
                .unwrap()
                .len(),
            2
        );
    }

    // ── akroasis#229: snapshot restore must dedup and stay bounded ────────

    fn link(from: u32, to: u32, snr: f32, packet_count: u32) -> LinkSnapshot {
        LinkSnapshot {
            from: n(from),
            to: n(to),
            snr,
            packet_count,
        }
    }

    #[test]
    fn load_from_bytes_folds_repeated_link_pairs() {
        // WHY: `update_link` keeps at most one edge per ordered pair, so a
        // restore that admits parallel edges produces a graph the live path
        // could never reach  -  and `to_bytes` re-emits them, compounding.
        let snapshot = TopologySnapshot {
            nodes: vec![n(1), n(2)],
            links: vec![link(1, 2, 5.0, 3), link(1, 2, 7.5, 4)],
        };
        let bytes = serde_json::to_vec(&snapshot).unwrap();

        let topo = MeshTopology::load_from_bytes(&bytes).unwrap();

        assert_eq!(topo.edge_count(), 1, "repeated pair must fold to one edge");
        let neighbors = topo.neighbors(n(1));
        assert_eq!(neighbors.len(), 1);
        let (peer, quality) = &neighbors[0];
        assert_eq!(*peer, n(2));
        assert!(
            (quality.snr - 7.5).abs() < f32::EPSILON,
            "last observation should win, got {}",
            quality.snr
        );
        assert_eq!(quality.packet_count, 7, "counts should add");
    }

    #[test]
    fn load_from_bytes_round_trips_without_multiplying_edges() {
        // WHY: the compounding case  -  save/load/save must be a fixed point.
        let mut topo = MeshTopology::new();
        topo.update_link(n(1), n(2), 5.0);
        topo.update_link(n(2), n(3), 6.0);

        let once = MeshTopology::load_from_bytes(&topo.save_to_bytes().unwrap()).unwrap();
        let twice = MeshTopology::load_from_bytes(&once.save_to_bytes().unwrap()).unwrap();

        assert_eq!(once.edge_count(), 2);
        assert_eq!(twice.edge_count(), 2, "reload must not multiply edges");
    }

    #[test]
    fn load_from_bytes_caps_nodes_and_links() {
        let over = MAX_SNAPSHOT_NODES + 10;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test-only: indices are far below u32::MAX"
        )]
        let nodes: Vec<NodeNum> = (0..over as u32).map(n).collect();
        let snapshot = TopologySnapshot {
            nodes,
            links: Vec::new(),
        };
        let bytes = serde_json::to_vec(&snapshot).unwrap();

        let topo = MeshTopology::load_from_bytes(&bytes).unwrap();

        assert_eq!(
            topo.node_count(),
            MAX_SNAPSHOT_NODES,
            "restore must stop at the node cap"
        );
    }
}
