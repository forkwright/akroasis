//! Gateway node detection and health tracking.

use std::collections::HashMap;

use tokio::time::Instant;

use crate::config::TopologyConfig;
use crate::topology::MeshTopology;
use crate::types::NodeNum;

/// Device role values from Meshtastic `Config.DeviceConfig.Role` protobuf enum.
mod device_role {
    pub const ROUTER_CLIENT: u32 = 3;
}

/// Tracks which nodes are identified as gateways and their health.
pub struct GatewayDetector {
    /// Manually designated gateway nodes from config.
    manual_gateways: Vec<NodeNum>,
    /// Auto-detected gateway nodes with last-seen time.
    detected_gateways: HashMap<NodeNum, GatewayState>,
}

/// Health state for a tracked gateway node.
#[derive(Debug, Clone)]
pub struct GatewayState {
    /// When this gateway was last seen.
    pub last_seen: Instant,
    /// Best observed SNR from the server node to this gateway.
    pub snr: f32,
    /// Whether this node was auto-detected (vs. manually configured).
    pub auto_detected: bool,
}

impl GatewayDetector {
    /// Create a new detector from configuration.
    #[must_use]
    pub fn new(config: &TopologyConfig) -> Self {
        Self {
            manual_gateways: config.gateway_nodes.iter().map(|&n| NodeNum(n)).collect(),
            detected_gateways: HashMap::new(),
        }
    }

    /// Check if a node qualifies as a gateway based on its device config.
    ///
    /// A node is auto-detected as a gateway if it has `role == ROUTER_CLIENT`
    /// AND either wifi is enabled or MQTT is enabled.
    pub fn evaluate_node(
        &mut self,
        node: NodeNum,
        role: u32,
        wifi_enabled: bool,
        mqtt_enabled: bool,
        snr: f32,
    ) {
        let is_gateway = role == device_role::ROUTER_CLIENT && (wifi_enabled || mqtt_enabled);

        if is_gateway {
            let state = self
                .detected_gateways
                .entry(node)
                .or_insert_with(|| GatewayState {
                    last_seen: Instant::now(),
                    snr,
                    auto_detected: true,
                });
            state.last_seen = Instant::now();
            state.snr = snr;
        } else if let Some(state) = self.detected_gateways.get(&node) {
            // WHY: only remove auto-detected gateways; manual ones persist.
            if state.auto_detected {
                self.detected_gateways.remove(&node);
            }
        }
    }

    /// Mark a gateway as seen (update `last_seen` and SNR).
    pub fn mark_seen(&mut self, node: NodeNum, snr: f32) {
        if let Some(state) = self.detected_gateways.get_mut(&node) {
            state.last_seen = Instant::now();
            state.snr = snr;
        }
    }

    /// Return all nodes identified as gateways (manual + auto-detected).
    #[must_use]
    pub fn gateway_nodes(&self) -> Vec<NodeNum> {
        let mut nodes: Vec<NodeNum> = self.manual_gateways.clone();
        for &node in self.detected_gateways.keys() {
            if !nodes.contains(&node) {
                nodes.push(node);
            }
        }
        nodes
    }

    /// Check if a specific node is a gateway.
    #[must_use]
    pub fn is_gateway(&self, node: NodeNum) -> bool {
        self.manual_gateways.contains(&node) || self.detected_gateways.contains_key(&node)
    }

    /// Return the healthiest gateway — lowest cost path from the server node.
    ///
    /// Prefers gateways with higher SNR and more recent last-seen time.
    #[must_use]
    pub fn best_gateway(&self, topology: &MeshTopology, server_node: NodeNum) -> Option<NodeNum> {
        self.gateway_nodes()
            .into_iter()
            .filter_map(|gw| {
                let hops = topology.hop_count(server_node, gw)?;
                let snr = self.detected_gateways.get(&gw).map_or(0.0, |s| s.snr);
                // WHY: score combines hop distance (weighted) and SNR; lower is better.
                let score = f32::from(hops).mul_add(10.0, -snr);
                Some((gw, score))
            })
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(gw, _)| gw)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(gw_nodes: Vec<u32>) -> TopologyConfig {
        TopologyConfig {
            gateway_nodes: gw_nodes,
            ..TopologyConfig::default()
        }
    }

    #[test]
    fn manual_gateways_from_config() {
        let det = GatewayDetector::new(&make_config(vec![100, 200]));
        assert!(det.is_gateway(NodeNum(100)));
        assert!(det.is_gateway(NodeNum(200)));
        assert!(!det.is_gateway(NodeNum(300)));
    }

    #[test]
    fn auto_detect_router_client_with_wifi() {
        let mut det = GatewayDetector::new(&make_config(vec![]));
        det.evaluate_node(NodeNum(42), device_role::ROUTER_CLIENT, true, false, 10.0);
        assert!(det.is_gateway(NodeNum(42)));
    }

    #[test]
    fn auto_detect_router_client_with_mqtt() {
        let mut det = GatewayDetector::new(&make_config(vec![]));
        det.evaluate_node(NodeNum(42), device_role::ROUTER_CLIENT, false, true, 10.0);
        assert!(det.is_gateway(NodeNum(42)));
    }

    #[test]
    fn non_router_client_not_auto_detected() {
        let mut det = GatewayDetector::new(&make_config(vec![]));
        det.evaluate_node(NodeNum(42), 0, true, true, 10.0);
        assert!(!det.is_gateway(NodeNum(42)));
    }

    #[test]
    fn gateway_nodes_combines_manual_and_auto() {
        let mut det = GatewayDetector::new(&make_config(vec![100]));
        det.evaluate_node(NodeNum(200), device_role::ROUTER_CLIENT, true, false, 5.0);
        let gateways = det.gateway_nodes();
        assert_eq!(gateways.len(), 2);
        assert!(gateways.contains(&NodeNum(100)));
        assert!(gateways.contains(&NodeNum(200)));
    }

    #[test]
    fn best_gateway_prefers_closest() {
        let mut det = GatewayDetector::new(&make_config(vec![]));
        det.evaluate_node(NodeNum(10), device_role::ROUTER_CLIENT, true, false, 10.0);
        det.evaluate_node(NodeNum(20), device_role::ROUTER_CLIENT, true, false, 10.0);

        let mut topo = MeshTopology::new();
        topo.update_link(NodeNum(1), NodeNum(10), 10.0); // 1 hop
        topo.update_link(NodeNum(1), NodeNum(5), 10.0);
        topo.update_link(NodeNum(5), NodeNum(20), 10.0); // 2 hops

        let best = det.best_gateway(&topo, NodeNum(1));
        assert_eq!(best, Some(NodeNum(10)));
    }

    #[test]
    fn auto_detected_gateway_removed_on_role_change() {
        let mut det = GatewayDetector::new(&make_config(vec![]));
        det.evaluate_node(NodeNum(42), device_role::ROUTER_CLIENT, true, false, 10.0);
        assert!(det.is_gateway(NodeNum(42)));

        // WHY: node changes role to CLIENT (0) — no longer qualifies.
        det.evaluate_node(NodeNum(42), 0, true, false, 10.0);
        assert!(!det.is_gateway(NodeNum(42)));
    }
}
