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
use crate::types::{MAX_LIVE_LINKS, MAX_LIVE_NODES, NodeNum};

// Historical default (30.0) now lives in [`TopologyConfig::default`].

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

impl LinkQuality {
    /// Whether this link's last observation is older than `cutoff`.
    #[must_use]
    pub fn is_stale(&self, cutoff: Instant) -> bool {
        self.last_observed < cutoff
    }
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
    // TODO(#204): cardinality bound lands in the next commit.
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
            // TODO(#204): cardinality bound lands in the next commit.
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
                    .any(|e| !e.weight().is_stale(cutoff));
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
                    .is_some_and(|w| w.is_stale(cutoff))
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
    /// restore is bounded at [`MAX_LIVE_NODES`] / [`MAX_LIVE_LINKS`] — the
    /// same live-cardinality ceiling [`Self::add_node`] / [`Self::update_link`]
    /// enforce (#204), so a restored topology can never exceed what the live
    /// insertion path would ever admit.
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
        if snapshot.nodes.len() > MAX_LIVE_NODES {
            tracing::warn!(
                present = snapshot.nodes.len(),
                cap = MAX_LIVE_NODES,
                "topology snapshot exceeds node cap; restoring a prefix"
            );
        }
        if snapshot.links.len() > MAX_LIVE_LINKS {
            tracing::warn!(
                present = snapshot.links.len(),
                cap = MAX_LIVE_LINKS,
                "topology snapshot exceeds link cap; restoring a prefix"
            );
        }

        let mut topo = Self::new();
        for node in snapshot.nodes.iter().take(MAX_LIVE_NODES) {
            topo.add_node(*node);
        }
        for link in snapshot.links.iter().take(MAX_LIVE_LINKS) {
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
#[path = "topology_tests.rs"]
mod tests;
