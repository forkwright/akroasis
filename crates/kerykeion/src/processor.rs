//! Central packet dispatch for incoming `FromRadio` messages after handshake.

use prost::Message as _;
use tokio::sync::broadcast;

use crate::node_db::{DeviceMetrics, MeshNode, NodeDb, NodePosition, UserInfo};
use crate::signals::{MeshEvent, mesh_event_to_signal};
use crate::topology::MeshTopology;
use crate::types::NodeNum;
use koinon::GeoSignal;

/// `NeighborInfo` protobuf (portnum 71) — not in vendored protos, decoded manually.
#[derive(prost::Message)]
struct NeighborInfo {
    #[prost(uint32, tag = "1")]
    node_id: u32,
    #[prost(uint32, tag = "2")]
    last_sent_by_id: u32,
    #[prost(uint32, tag = "3")]
    node_broadcast_interval_secs: u32,
    #[prost(message, repeated, tag = "4")]
    neighbors: Vec<Neighbor>,
}

/// A single neighbor entry in a `NeighborInfo` broadcast.
#[derive(prost::Message)]
struct Neighbor {
    #[prost(uint32, tag = "1")]
    node_id: u32,
    #[prost(float, tag = "2")]
    snr: f32,
}

/// Meshtastic port numbers for decoded `Data` payloads.
mod portnum {
    pub const POSITION_APP: i32 = 3;
    pub const NODEINFO_APP: i32 = 4;
    pub const ROUTING_APP: i32 = 5;
    pub const TELEMETRY_APP: i32 = 67;
    pub const TRACEROUTE_APP: i32 = 70;
    pub const NEIGHBORINFO_APP: i32 = 71;
}

/// Central processor that dispatches decoded packets to `NodeDb`, `MeshTopology`,
/// and emits [`GeoSignal`]s on the broadcast channel.
pub struct PacketProcessor {
    node_db: NodeDb,
    topology: MeshTopology,
    tx: broadcast::Sender<GeoSignal>,
}

impl PacketProcessor {
    /// Create a new processor with the given broadcast sender.
    pub const fn new(
        node_db: NodeDb,
        topology: MeshTopology,
        tx: broadcast::Sender<GeoSignal>,
    ) -> Self {
        Self {
            node_db,
            topology,
            tx,
        }
    }

    /// Access the node database.
    #[must_use]
    pub const fn node_db(&self) -> &NodeDb {
        &self.node_db
    }

    /// Mutable access to the node database.
    pub const fn node_db_mut(&mut self) -> &mut NodeDb {
        &mut self.node_db
    }

    /// Access the topology graph.
    #[must_use]
    pub const fn topology(&self) -> &MeshTopology {
        &self.topology
    }

    /// Mutable access to the topology graph.
    pub const fn topology_mut(&mut self) -> &mut MeshTopology {
        &mut self.topology
    }

    /// Process a decoded `MeshPacket` after the handshake phase.
    ///
    /// Dispatches based on portnum and updates internal state. Returns any
    /// produced events for external handling.
    pub fn process_mesh_packet(&mut self, packet: &crate::proto::MeshPacket) -> Vec<MeshEvent> {
        let mut events = Vec::new();
        let from = NodeNum(packet.from);

        // WHY: passive learning — every received packet provides link metadata.
        self.apply_passive_learning(packet);

        let Some(crate::proto::mesh_packet::PayloadVariant::Decoded(decoded)) =
            &packet.payload_variant
        else {
            return events;
        };

        match decoded.portnum {
            p if p == portnum::NODEINFO_APP => {
                self.handle_nodeinfo(from, &decoded.payload, &mut events);
            }
            p if p == portnum::POSITION_APP => {
                self.handle_position(from, &decoded.payload, &mut events);
            }
            p if p == portnum::TELEMETRY_APP => {
                self.handle_telemetry(from, &decoded.payload, &mut events);
            }
            p if p == portnum::NEIGHBORINFO_APP => {
                self.handle_neighborinfo(&decoded.payload, &mut events);
            }
            p if p == portnum::TRACEROUTE_APP => {
                self.handle_traceroute(from, packet, &decoded.payload, &mut events);
            }
            p if p == portnum::ROUTING_APP => {
                handle_routing(&decoded.payload);
            }
            _ => {
                tracing::trace!(
                    portnum = decoded.portnum,
                    from = from.0,
                    "unhandled portnum"
                );
            }
        }

        // WHY: emit GeoSignals for each produced event.
        for event in &events {
            let position = self.node_db.get(from).and_then(|n| n.position.as_ref());
            let signal = mesh_event_to_signal(event, position);
            // WHY: broadcast send errors mean no receivers are listening; not fatal.
            let _ = self.tx.send(signal);
        }

        events
    }

    /// Infer link quality from packet metadata without explicit topology messages.
    fn apply_passive_learning(&mut self, packet: &crate::proto::MeshPacket) {
        let from = NodeNum(packet.from);
        let snr = if packet.rx_snr == 0.0 {
            None
        } else {
            Some(packet.rx_snr)
        };

        let hop_count = if packet.hop_start > 0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "hop values are bounded by MAX_HOP_LIMIT (7)"
            )]
            Some((packet.hop_start.saturating_sub(packet.hop_limit)) as u8)
        } else {
            None
        };

        // WHY: update or create the node record with latest packet metadata.
        let mut node = self.node_db.get(from).cloned().unwrap_or(MeshNode {
            num: from,
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: None,
            hop_count: None,
        });
        node.last_heard = Some(jiff::Timestamp::now());
        if let Some(s) = snr {
            node.snr = Some(s);
        }
        if let Some(h) = hop_count {
            node.hop_count = Some(h);
        }
        self.node_db.insert(node);

        // WHY: if hop_count is 1 (direct), establish a direct link.
        if hop_count == Some(1) {
            if let Some(my_node) = self.node_db.my_node() {
                if let Some(s) = snr {
                    self.topology.update_link(from, my_node, s);
                }
            }
        }
    }

    fn handle_nodeinfo(&mut self, from: NodeNum, payload: &[u8], events: &mut Vec<MeshEvent>) {
        let user_proto = match crate::proto::User::decode(payload) {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!(from = from.0, error = %e, "failed to decode NODEINFO_APP payload");
                return;
            }
        };

        let user = UserInfo {
            id: user_proto.id,
            long_name: user_proto.long_name,
            short_name: user_proto.short_name.clone(),
            hw_model: u32::try_from(user_proto.hw_model).unwrap_or(0),
            is_licensed: user_proto.is_licensed,
        };

        // WHY: passive learning may have already created a bare node entry, so
        // check for user info to determine if this is a genuinely new node.
        let is_new = self.node_db.get(from).is_none_or(|n| n.user.is_none());

        let mut node = self.node_db.get(from).cloned().unwrap_or(MeshNode {
            num: from,
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: None,
            hop_count: None,
        });
        node.user = Some(user.clone());
        node.last_heard = Some(jiff::Timestamp::now());
        self.node_db.insert(node);
        self.topology.add_node(from);

        if is_new {
            events.push(MeshEvent::NodeDiscovered {
                node: from,
                short_name: Some(user.short_name),
                snr: self.node_db.get(from).and_then(|n| n.snr).unwrap_or(0.0),
                hop_count: self
                    .node_db
                    .get(from)
                    .and_then(|n| n.hop_count)
                    .unwrap_or(0),
            });
        }
    }

    fn handle_position(&mut self, from: NodeNum, payload: &[u8], events: &mut Vec<MeshEvent>) {
        let pos_proto = match crate::proto::Position::decode(payload) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(from = from.0, error = %e, "failed to decode POSITION_APP payload");
                return;
            }
        };

        // WHY: Meshtastic encodes lat/lon as integer degrees × 1e7.
        let lat = f64::from(pos_proto.latitude_i) * 1e-7;
        let lon = f64::from(pos_proto.longitude_i) * 1e-7;
        let alt = if pos_proto.altitude != 0 {
            Some(pos_proto.altitude)
        } else {
            None
        };

        let position = NodePosition {
            latitude: lat,
            longitude: lon,
            altitude: alt,
            timestamp: jiff::Timestamp::from_second(i64::from(pos_proto.time)).ok(),
        };

        let mut node = self.node_db.get(from).cloned().unwrap_or(MeshNode {
            num: from,
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: None,
            hop_count: None,
        });
        node.position = Some(position);
        node.last_heard = Some(jiff::Timestamp::now());
        self.node_db.insert(node);

        #[expect(
            clippy::cast_precision_loss,
            reason = "altitude i32→f32 is acceptable for metre-scale values"
        )]
        let alt_f32 = alt.map(|a| a as f32);
        events.push(MeshEvent::PositionUpdate {
            node: from,
            lat,
            lon,
            alt: alt_f32,
        });
    }

    fn handle_telemetry(&mut self, from: NodeNum, payload: &[u8], events: &mut Vec<MeshEvent>) {
        let telem = match crate::proto::Telemetry::decode(payload) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(from = from.0, error = %e, "failed to decode TELEMETRY_APP payload");
                return;
            }
        };

        if let Some(crate::proto::telemetry::Variant::DeviceMetrics(dm)) = telem.variant {
            let metrics = DeviceMetrics {
                battery_level: if dm.battery_level > 0 {
                    Some(dm.battery_level)
                } else {
                    None
                },
                voltage: if dm.voltage > 0.0 {
                    Some(dm.voltage)
                } else {
                    None
                },
                channel_utilization: if dm.channel_utilization > 0.0 {
                    Some(dm.channel_utilization)
                } else {
                    None
                },
                air_util_tx: if dm.air_util_tx > 0.0 {
                    Some(dm.air_util_tx)
                } else {
                    None
                },
            };

            if let Some(existing) = self.node_db.get(from) {
                let mut updated = existing.clone();
                updated.metrics = Some(metrics.clone());
                updated.last_heard = Some(jiff::Timestamp::now());
                self.node_db.insert(updated);
            }

            events.push(MeshEvent::TelemetryUpdate {
                node: from,
                battery_pct: metrics.battery_level,
                voltage: metrics.voltage,
            });
        }
    }

    fn handle_neighborinfo(&mut self, payload: &[u8], events: &mut Vec<MeshEvent>) {
        let ni = match NeighborInfo::decode(payload) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(error = %e, "failed to decode NEIGHBORINFO_APP payload");
                return;
            }
        };

        let reporter = NodeNum(ni.node_id);
        self.topology.add_node(reporter);

        for neighbor in &ni.neighbors {
            let neighbor_num = NodeNum(neighbor.node_id);
            self.topology
                .update_link(reporter, neighbor_num, neighbor.snr);
            events.push(MeshEvent::TopologyChange {
                from: reporter,
                to: neighbor_num,
                snr: neighbor.snr,
            });
        }
    }

    #[expect(
        clippy::indexing_slicing,
        reason = "windows(2) guarantees exactly 2 elements"
    )]
    fn handle_traceroute(
        &mut self,
        from: NodeNum,
        packet: &crate::proto::MeshPacket,
        payload: &[u8],
        events: &mut Vec<MeshEvent>,
    ) {
        let route = match crate::proto::RouteDiscovery::decode(payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(from = from.0, error = %e, "failed to decode TRACEROUTE_APP payload");
                return;
            }
        };

        // WHY: build the full path: originator → route hops → destination.
        let dest = NodeNum(packet.to);
        let mut path = vec![from];
        path.extend(route.route.iter().map(|&n| NodeNum(n)));
        path.push(dest);

        // WHY: add all nodes in the path first.
        for &node in &path {
            self.topology.add_node(node);
        }

        for (i, window) in path.windows(2).enumerate() {
            let (hop_from, hop_to) = (window[0], window[1]);
            #[expect(
                clippy::cast_precision_loss,
                reason = "SNR i32→f32 preserves sufficient precision for dB values"
            )]
            let snr = route.snr_towards.get(i).map_or(0.0, |&s| s as f32);

            self.topology.update_link(hop_from, hop_to, snr);
            events.push(MeshEvent::TopologyChange {
                from: hop_from,
                to: hop_to,
                snr,
            });
        }

        // WHY: process the reverse path (snr_back) similarly.
        if !route.back.is_empty() {
            let mut back_path = vec![dest];
            back_path.extend(route.back.iter().map(|&n| NodeNum(n)));
            back_path.push(from);

            for (i, window) in back_path.windows(2).enumerate() {
                let (hop_from, hop_to) = (window[0], window[1]);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "SNR i32→f32 preserves sufficient precision"
                )]
                let snr = route.snr_back.get(i).map_or(0.0, |&s| s as f32);

                self.topology.update_link(hop_from, hop_to, snr);
            }
        }
    }
}

/// Process a routing payload (ACK/NAK tracking).
fn handle_routing(payload: &[u8]) {
    let routing = match crate::proto::Routing::decode(payload) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "failed to decode ROUTING_APP payload");
            return;
        }
    };

    match &routing.variant {
        Some(crate::proto::routing::Variant::ErrorReason(code)) => {
            tracing::debug!(error_code = code, "routing error received");
        }
        Some(
            crate::proto::routing::Variant::RouteRequest(_)
            | crate::proto::routing::Variant::RouteReply(_),
        ) => {
            // WHY: route_request/route_reply are handled via TRACEROUTE_APP portnum.
            tracing::trace!("routing route_request/route_reply (handled via traceroute)");
        }
        None => {}
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn make_processor() -> PacketProcessor {
        let (tx, _rx) = broadcast::channel(64);
        let mut node_db = NodeDb::new();
        node_db.set_my_node(NodeNum(0xAAAA));
        PacketProcessor::new(node_db, MeshTopology::new(), tx)
    }

    fn make_mesh_packet(from: u32, portnum: i32, payload: Vec<u8>) -> crate::proto::MeshPacket {
        crate::proto::MeshPacket {
            from,
            to: 0xFFFF_FFFF,
            channel: 0,
            id: 1,
            rx_time: 0,
            rx_snr: 5.0,
            hop_limit: 2,
            want_ack: false,
            priority: 0,
            rx_rssi: -90,
            via_mqtt: false,
            hop_start: 3,
            payload_variant: Some(crate::proto::mesh_packet::PayloadVariant::Decoded(
                crate::proto::Data {
                    portnum,
                    payload,
                    want_response: false,
                    dest: 0,
                    source: 0,
                    request_id: 0,
                    reply_id: 0,
                    emoji: vec![],
                },
            )),
        }
    }

    #[test]
    fn process_nodeinfo_creates_node_and_event() {
        let mut proc = make_processor();
        let user = crate::proto::User {
            id: "!deadbeef".into(),
            long_name: "Test Node".into(),
            short_name: "TST".into(),
            macaddr: vec![],
            hw_model: 9, // RAK4631
            is_licensed: false,
            role: 0,
        };
        let mut payload = Vec::new();
        user.encode(&mut payload).unwrap();

        let packet = make_mesh_packet(0xDEAD, portnum::NODEINFO_APP, payload);
        let events = proc.process_mesh_packet(&packet);

        assert!(proc.node_db().get(NodeNum(0xDEAD)).is_some());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MeshEvent::NodeDiscovered { .. }))
        );
    }

    #[test]
    fn process_position_updates_node_and_emits_event() {
        let mut proc = make_processor();
        let pos = crate::proto::Position {
            latitude_i: 515_074_000, // 51.5074
            longitude_i: -1_278_000, // -0.1278
            altitude: 11,
            time: 1_700_000_000,
            ..Default::default()
        };
        let mut payload = Vec::new();
        pos.encode(&mut payload).unwrap();

        let packet = make_mesh_packet(0xBEEF, portnum::POSITION_APP, payload);
        let events = proc.process_mesh_packet(&packet);

        let node = proc.node_db().get(NodeNum(0xBEEF)).unwrap();
        assert!(node.position.is_some());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MeshEvent::PositionUpdate { .. }))
        );
    }

    #[test]
    fn process_telemetry_updates_metrics() {
        let mut proc = make_processor();
        // WHY: pre-insert a node so telemetry has something to update.
        proc.node_db_mut().insert(MeshNode {
            num: NodeNum(0x1111),
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: None,
            hop_count: None,
        });

        let telem = crate::proto::Telemetry {
            time: 1_700_000_000,
            variant: Some(crate::proto::telemetry::Variant::DeviceMetrics(
                crate::proto::DeviceMetrics {
                    battery_level: 85,
                    voltage: 3.7,
                    channel_utilization: 0.15,
                    air_util_tx: 0.05,
                    uptime_seconds: 3600,
                },
            )),
        };
        let mut payload = Vec::new();
        telem.encode(&mut payload).unwrap();

        let packet = make_mesh_packet(0x1111, portnum::TELEMETRY_APP, payload);
        let events = proc.process_mesh_packet(&packet);

        let node = proc.node_db().get(NodeNum(0x1111)).unwrap();
        assert!(node.metrics.is_some());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MeshEvent::TelemetryUpdate { .. }))
        );
    }

    #[test]
    fn process_neighborinfo_updates_topology() {
        let mut proc = make_processor();

        let ni = NeighborInfo {
            node_id: 0x1111,
            last_sent_by_id: 0x1111,
            node_broadcast_interval_secs: 3600,
            neighbors: vec![
                Neighbor {
                    node_id: 0x2222,
                    snr: 8.5,
                },
                Neighbor {
                    node_id: 0x3333,
                    snr: 3.0,
                },
            ],
        };
        let mut payload = Vec::new();
        ni.encode(&mut payload).unwrap();

        let packet = make_mesh_packet(0x1111, portnum::NEIGHBORINFO_APP, payload);
        let events = proc.process_mesh_packet(&packet);

        // WHY: 2 from neighborinfo + 1 from passive learning (direct link).
        assert_eq!(proc.topology().edge_count(), 3);
        assert!(proc.topology().contains_node(NodeNum(0x2222)));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, MeshEvent::TopologyChange { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn process_traceroute_builds_path() {
        let mut proc = make_processor();

        let route = crate::proto::RouteDiscovery {
            route: vec![0x2222, 0x3333],
            snr_towards: vec![10, 8],
            back: vec![],
            snr_back: vec![],
        };
        let mut payload = Vec::new();
        route.encode(&mut payload).unwrap();

        let mut packet = make_mesh_packet(0x1111, portnum::TRACEROUTE_APP, payload);
        packet.to = 0x4444;
        let events = proc.process_mesh_packet(&packet);

        // WHY: path is 0x1111 → 0x2222 → 0x3333 → 0x4444 = 3 edges.
        assert!(
            proc.topology().edge_count() >= 3,
            "expected at least 3 edges from traceroute path"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MeshEvent::TopologyChange { .. }))
        );
    }

    #[test]
    fn passive_learning_infers_hop_count() {
        let mut proc = make_processor();
        let packet = make_mesh_packet(0xBBBB, portnum::NODEINFO_APP, vec![]);
        // hop_start=3, hop_limit=2 → 1 hop traversed
        proc.process_mesh_packet(&packet);

        let node = proc.node_db().get(NodeNum(0xBBBB)).unwrap();
        assert_eq!(node.hop_count, Some(1));
    }

    #[test]
    fn passive_learning_creates_direct_link() {
        let mut proc = make_processor();
        // hop_start=3, hop_limit=2 → 1 hop (direct)
        let packet = make_mesh_packet(0xCCCC, portnum::NODEINFO_APP, vec![]);
        proc.process_mesh_packet(&packet);

        // WHY: direct packet should create a link from sender to our node.
        let my_node = proc.node_db().my_node().unwrap();
        let neighbors = proc.topology().neighbors(NodeNum(0xCCCC));
        assert!(
            neighbors.iter().any(|(n, _)| *n == my_node),
            "direct packet should create link to server node"
        );
    }
}
