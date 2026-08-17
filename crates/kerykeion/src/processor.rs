//! Central packet dispatch for incoming `FromRadio` messages after handshake.

use koinon::GeoSignal;
use prost::Message as _;
use tokio::sync::broadcast;

use crate::delivery::{DeliveryFailure, DeliveryTracker};
use crate::node_db::{DeviceMetrics, MeshNode, NodeDb, NodePosition, UserInfo};
use crate::outbound::OutboundQueue;
use crate::proto::mesh_packet::PayloadVariant;
use crate::proto::{MeshPacket, Routing, routing};
use crate::signals::{MeshEvent, mesh_event_to_signal};
use crate::topology::MeshTopology;
use crate::types::{NodeNum, PacketId};

/// `NeighborInfo` protobuf (portnum 71)  -  not in vendored protos, decoded manually.
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
    pub(super) const POSITION_APP: i32 = 3;
    pub(super) const NODEINFO_APP: i32 = 4;
    pub(super) const ROUTING_APP: i32 = 5;
    pub(super) const TELEMETRY_APP: i32 = 67;
    pub(super) const TRACEROUTE_APP: i32 = 70;
    pub(super) const NEIGHBORINFO_APP: i32 = 71;
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

        // WHY: passive learning  -  every received packet provides link metadata.
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
                // WHY: ACK/NAK delivery confirmation needs `DeliveryTracker` +
                // `OutboundQueue`, which `PacketProcessor` does not own; the
                // collector receive loop runs `RoutingProcessor` against the
                // shared router state instead (see `MeshCollector::run`).
            }
            _ => {
                tracing::trace!(
                    portnum = decoded.portnum,
                    from = from.0,
                    "unhandled portnum"
                );
            }
        }

        // WHY: an event names its own subject, which is not always the packet
        // sender. NEIGHBORINFO reports carry a reporter id from the payload, so
        // locating every signal at `from` puts a relayed report at the relay
        // rather than at the node the report is about.
        for event in &events {
            let position = event
                .subject()
                .and_then(|subject| self.node_db.get(subject))
                .and_then(|n| n.position.as_ref());
            let signal = mesh_event_to_signal(event, position);
            // WHY: broadcast send errors mean no receivers are listening; not fatal.
            if let Err(error) = self.tx.send(signal) {
                tracing::trace!(%error, "no active receiver for mesh signal");
            }
        }

        events
    }

    /// Infer link quality FROM packet metadata without explicit topology messages.
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
                reason = "hop VALUES are bounded by MAX_HOP_LIMIT (7)"
            )]
            Some((packet.hop_start.saturating_sub(packet.hop_limit)) as u8) // SAFETY: saturating_sub result is bounded by hop_start (u8 domain)
        } else {
            None
        };

        // WHY: UPDATE or CREATE the node record with latest packet metadata.
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

        // WHY: if hop_count is 0 (direct, hop_start == hop_limit), establish a direct link.
        if hop_count == Some(0) {
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

        // WHY: hw_model is an i32 on the wire. Collapsing an out-of-range value
        // to 0 makes a malformed report indistinguishable from UNSET, which is
        // also what a radio that never declared its model reports.
        let hw_model = u32::try_from(user_proto.hw_model).unwrap_or_else(|_| {
            tracing::warn!(
                from = from.0,
                hw_model = user_proto.hw_model,
                "NODEINFO hw_model out of range; recording as UNSET"
            );
            0
        });

        let user = UserInfo {
            id: user_proto.id.into(),
            long_name: user_proto.long_name,
            short_name: user_proto.short_name.clone(),
            hw_model,
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
            // WHY: invalid/zero timestamps in Position protobufs map to None rather
            // than propagating an error; position lat/lon remain usable.
            timestamp: jiff::Timestamp::from_second(i64::from(pos_proto.time)).ok(), // kanon:ignore RUST/silent-error-ok -- timestamp is optional metadata, invalid→None is correct
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
            reason = "altitude i32→f32 is acceptable for metre-scale VALUES"
        )]
        let alt_f32 = alt.map(|a| a as f32); // SAFETY: altitude is f32-representable; f64→f32 precision loss is acceptable for position telemetry
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
            let (hop_from, hop_to) = (window[0], window[1]); // kanon:ignore RUST/indexing-slicing -- windows(2) always yields 2-element slice; indices 0 and 1 are compile-time bounded
            #[expect(
                clippy::cast_precision_loss,
                reason = "SNR i32→f32 preserves sufficient precision for dB VALUES"
            )]
            let snr = route.snr_towards.get(i).map_or(0.0, |&s| s as f32); // SAFETY: SNR values are small-magnitude (±60 dB); f64→f32 is safe for this range

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
                let (hop_from, hop_to) = (window[0], window[1]); // kanon:ignore RUST/indexing-slicing -- windows(2) always yields 2-element slice; indices 0 and 1 are compile-time bounded
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "SNR i32→f32 preserves sufficient precision"
                )]
                let snr = route.snr_back.get(i).map_or(0.0, |&s| s as f32); // SAFETY: SNR values are small-magnitude (±60 dB); f64→f32 is safe for this range

                self.topology.update_link(hop_from, hop_to, snr);
            }
        }
    }
}

// ── Routing ACK/NAK processor ────────────────────────────────────────────────

/// Result of processing a routing packet.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RoutingResult {
    /// ACK received  -  message was delivered.
    Ack {
        /// The packet ID that was acknowledged.
        request_id: PacketId,
    },
    /// NAK received  -  routing error.
    Nak {
        /// The packet ID that failed.
        request_id: PacketId,
        /// The routing error code.
        error: routing::Error,
    },
    /// The `ROUTING_APP` packet decoded and carried an `error_reason`, but
    /// its wire value is not among the `routing::Error` variants this build
    /// knows.
    ///
    // WHY a distinct variant rather than folding into `Nak` or `Ack` (#208):
    // `routing::Error` cannot represent "unrecognized" without reusing an
    // existing code, which would misreport the failure reason — and reusing
    // `Error::None` (the pre-fix `unwrap_or` fallback) is exactly the
    // fail-open defect this variant exists to prevent. MUST NEVER be treated
    // as delivery confirmation: an out-of-enum code is exactly what a
    // forged NAK or an unrecognized future firmware code looks like.
    UnknownError {
        /// The packet ID the unrecognized error was reported against.
        request_id: PacketId,
        /// The raw wire code that did not match any known `routing::Error` variant.
        code: i32,
    },
    /// The packet was not a routing packet or had no actionable variant.
    NotRouting,
}

/// Processes inbound mesh packets for delivery confirmation (ACK/NAK).
///
/// Examines `ROUTING_APP` packets to UPDATE the delivery tracker and
/// outbound queue based on acknowledgement or error responses.
pub struct RoutingProcessor;

impl RoutingProcessor {
    /// Process an inbound mesh packet for routing ACK/NAK.
    ///
    /// Only `ROUTING_APP` packets with an `error_reason` variant are processed.
    /// ACK = `Error::None`, NAK = any other error value.
    #[must_use]
    pub fn process_routing(packet: &MeshPacket) -> RoutingResult {
        let Some(PayloadVariant::Decoded(data)) = &packet.payload_variant else {
            return RoutingResult::NotRouting;
        };

        if data.portnum != portnum::ROUTING_APP {
            return RoutingResult::NotRouting;
        }

        // The request_id in the Data message refers to the original packet being ACK'd/NAK'd.
        let request_id = data.request_id;
        if request_id == 0 {
            return RoutingResult::NotRouting;
        }

        let Ok(routing_msg) = Routing::decode(data.payload.as_slice()) else {
            tracing::warn!(
                packet_id = packet.id,
                "failed to decode Routing payload FROM ROUTING_APP packet"
            );
            return RoutingResult::NotRouting;
        };

        // TODO(#208): fail-open fix lands in the next commit.
        match routing_msg.variant {
            Some(routing::Variant::ErrorReason(code)) => {
                let error = routing::Error::try_from(code).unwrap_or(routing::Error::None);
                if error == routing::Error::None {
                    RoutingResult::Ack {
                        request_id: PacketId(request_id),
                    }
                } else {
                    RoutingResult::Nak {
                        request_id: PacketId(request_id),
                        error,
                    }
                }
            }
            _ => RoutingResult::NotRouting,
        }
    }

    /// Process a routing result and UPDATE the delivery tracker and outbound queue.
    pub fn apply_routing_result(
        result: &RoutingResult,
        delivery: &mut DeliveryTracker,
        outbound: &mut OutboundQueue,
    ) {
        match result {
            RoutingResult::Ack { request_id } => {
                tracing::debug!(packet_id = %request_id, "delivery confirmed via ACK");
                outbound.handle_ack(*request_id);
                delivery.mark_acknowledged(*request_id, None);
            }
            RoutingResult::Nak { request_id, error } => {
                tracing::debug!(
                    packet_id = %request_id,
                    error = ?error,
                    "delivery NAK received"
                );
                let retried = outbound.handle_nak(*request_id);
                if retried {
                    delivery.record_retry(*request_id);
                } else {
                    delivery.mark_failed(*request_id, DeliveryFailure::Nak(*error));
                }
            }
            RoutingResult::UnknownError { request_id, code } => {
                // WHY the same retry/fail pipeline as `Nak`, not a silent
                // drop (#208): an unrecognized code is still evidence the
                // packet was NOT delivered — treating it as inert would
                // leave `outbound`'s inflight slot and `delivery`'s record
                // stuck until TTL/timeout instead of retrying or failing
                // promptly, and would never surface the unrecognized code.
                tracing::debug!(
                    packet_id = %request_id,
                    code,
                    "delivery NAK received (unrecognized routing error code)"
                );
                let retried = outbound.handle_nak(*request_id);
                if retried {
                    delivery.record_retry(*request_id);
                } else {
                    delivery.mark_failed(*request_id, DeliveryFailure::UnknownNak { code: *code });
                }
            }
            RoutingResult::NotRouting => {}
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "processor_tests.rs"]
mod tests;
