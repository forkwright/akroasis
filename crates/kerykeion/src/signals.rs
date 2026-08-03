//! Mesh event types and conversion to [`koinon::GeoSignal`] for the broadcast channel.

use koinon::signal::MeshDetail;
use koinon::{Confidence, Coordinates, GeoSignal, SignalKind, Timestamp};

use crate::node_db::NodePosition;
use crate::types::NodeNum;

/// Internal mesh event produced by the packet processor and discovery manager.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum MeshEvent {
    /// A new node was discovered on the mesh.
    NodeDiscovered {
        /// Discovered node number.
        node: NodeNum,
        /// Short name if available.
        short_name: Option<String>,
        /// SNR at discovery.
        snr: f32,
        /// Hop count from observer.
        hop_count: u8,
    },
    /// A previously-seen node has gone offline.
    NodeOffline {
        /// Node that went offline.
        node: NodeNum,
    },
    /// A node reported a new position.
    PositionUpdate {
        /// Node that reported position.
        node: NodeNum,
        /// Latitude in decimal degrees.
        lat: f64,
        /// Longitude in decimal degrees.
        lon: f64,
        /// Altitude in metres MSL, if reported.
        alt: Option<f32>,
    },
    /// A topology link was added or updated.
    TopologyChange {
        /// Link source node.
        from: NodeNum,
        /// Link destination node.
        to: NodeNum,
        /// SNR on this link.
        snr: f32,
    },
    /// Link quality degraded significantly.
    LinkDegraded {
        /// Link source node.
        from: NodeNum,
        /// Link destination node.
        to: NodeNum,
        /// Previous SNR value.
        old_snr: f32,
        /// Current SNR value.
        new_snr: f32,
    },
    /// Mesh network split into disconnected components.
    PartitionDetected {
        /// Each inner vec is a set of nodes in one component.
        components: Vec<Vec<NodeNum>>,
    },
    /// Previously partitioned nodes are now reachable again.
    PartitionHealed {
        /// Nodes that rejoined the main mesh.
        reunited_nodes: Vec<NodeNum>,
    },
    /// A node's gateway status changed.
    GatewayStatusChange {
        /// The gateway node.
        node: NodeNum,
        /// Whether it is now acting as a gateway.
        is_gateway: bool,
    },
    /// Updated telemetry from a node.
    TelemetryUpdate {
        /// Reporting node.
        node: NodeNum,
        /// Battery percentage (0–100), if reported.
        battery_pct: Option<u32>,
        /// Battery voltage in volts, if reported.
        voltage: Option<f32>,
    },
}

impl MeshEvent {
    /// The node this event is about, when it names exactly one.
    ///
    /// This is the node whose position locates the emitted signal. It is not
    /// the packet sender: `NEIGHBORINFO` carries a reporter id in its payload,
    /// so a relayed report describes links the relay is not an endpoint of.
    ///
    /// Returns `None` for the partition events, which describe a set of nodes
    /// rather than one; their conversions carry no location either.
    #[must_use]
    pub const fn subject(&self) -> Option<NodeNum> {
        match self {
            Self::NodeDiscovered { node, .. }
            | Self::NodeOffline { node }
            | Self::PositionUpdate { node, .. }
            | Self::GatewayStatusChange { node, .. }
            | Self::TelemetryUpdate { node, .. } => Some(*node),
            Self::TopologyChange { from, .. } | Self::LinkDegraded { from, .. } => Some(*from),
            Self::PartitionDetected { .. } | Self::PartitionHealed { .. } => None,
        }
    }
}

/// Convert a [`MeshEvent`] to a [`GeoSignal`] using the closest matching [`MeshDetail`] variant.
///
/// Events that lack a direct `MeshDetail` mapping use `MeshDetail::NodeSeen` with
/// discriminating metadata. Observations note which new variants would improve this.
#[must_use]
pub fn mesh_event_to_signal(event: &MeshEvent, position: Option<&NodePosition>) -> GeoSignal {
    let location = position
        .and_then(|p| Coordinates::new(p.latitude, p.longitude, p.altitude.map(f64::from)).ok());
    let ts = Timestamp::now();

    match event {
        MeshEvent::NodeDiscovered {
            node,
            snr,
            hop_count,
            ..
        } => convert_node_discovered(*node, *snr, *hop_count, ts, location),
        MeshEvent::NodeOffline { node } => convert_node_offline(*node, ts, location),
        MeshEvent::PositionUpdate {
            node,
            lat,
            lon,
            alt,
        } => convert_position_update(*node, *lat, *lon, *alt, ts, location),
        MeshEvent::TopologyChange { from, to, snr } => {
            convert_topology_change(*from, *to, *snr, ts, location)
        }
        MeshEvent::LinkDegraded {
            from,
            to,
            old_snr,
            new_snr,
        } => convert_link_degraded(*from, *to, *old_snr, *new_snr, ts, location),
        MeshEvent::PartitionDetected { components } => convert_partition_detected(components, ts),
        MeshEvent::PartitionHealed { reunited_nodes } => {
            convert_partition_healed(reunited_nodes, ts)
        }
        MeshEvent::GatewayStatusChange { node, is_gateway } => {
            convert_gateway_status(*node, *is_gateway, ts, location)
        }
        MeshEvent::TelemetryUpdate {
            node,
            battery_pct,
            voltage,
        } => convert_telemetry(*node, *battery_pct, *voltage, ts, location),
    }
}

fn convert_node_discovered(
    node: NodeNum,
    snr: f32,
    hop_count: u8,
    ts: Timestamp,
    location: Option<Coordinates>,
) -> GeoSignal {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: node.0,
        snr,
        hop_count,
    });
    GeoSignal::new(kind, ts, location).with_metadata("event", serde_json::json!("discovered"))
}

fn convert_node_offline(node: NodeNum, ts: Timestamp, location: Option<Coordinates>) -> GeoSignal {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: node.0,
        snr: 0.0,
        hop_count: 0,
    });
    GeoSignal::new(kind, ts, location)
        .with_confidence(Confidence::new(0.5))
        .with_metadata("event", serde_json::json!("offline"))
}

fn convert_position_update(
    node: NodeNum,
    lat: f64,
    lon: f64,
    alt: Option<f32>,
    ts: Timestamp,
    location: Option<Coordinates>,
) -> GeoSignal {
    // WHY: position signals carry their own coordinates in the payload.
    let Ok(coords) = Coordinates::new(lat, lon, alt.map(f64::from)) else {
        // WHY: invalid coordinates still get reported with NodeSeen fallback.
        let fallback = SignalKind::Mesh(MeshDetail::NodeSeen {
            node_id: node.0,
            snr: 0.0,
            hop_count: 0,
        });
        return GeoSignal::new(fallback, ts, location)
            .with_metadata("event", serde_json::json!("invalid_position"));
    };
    let kind = SignalKind::Mesh(MeshDetail::Position {
        node_id: node.0,
        coordinates: coords,
    });
    GeoSignal::new(kind, ts, Some(coords))
}

fn convert_topology_change(
    from: NodeNum,
    to: NodeNum,
    snr: f32,
    ts: Timestamp,
    location: Option<Coordinates>,
) -> GeoSignal {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: from.0,
        snr,
        hop_count: 0,
    });
    GeoSignal::new(kind, ts, location)
        .with_metadata("event", serde_json::json!("topology_change"))
        .with_metadata("to_node", serde_json::json!(to.0))
}

fn convert_link_degraded(
    from: NodeNum,
    to: NodeNum,
    old_snr: f32,
    new_snr: f32,
    ts: Timestamp,
    location: Option<Coordinates>,
) -> GeoSignal {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: from.0,
        snr: new_snr,
        hop_count: 0,
    });
    GeoSignal::new(kind, ts, location)
        .with_metadata("event", serde_json::json!("link_degraded"))
        .with_metadata("to_node", serde_json::json!(to.0))
        .with_metadata("old_snr", serde_json::json!(old_snr))
}

fn convert_partition_detected(components: &[Vec<NodeNum>], ts: Timestamp) -> GeoSignal {
    let node_lists: Vec<Vec<u32>> = components
        .iter()
        .map(|c| c.iter().map(|n| n.0).collect())
        .collect();
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: 0,
        snr: 0.0,
        hop_count: 0,
    });
    GeoSignal::new(kind, ts, None)
        .with_confidence(Confidence::new(0.8))
        .with_metadata("event", serde_json::json!("partition_detected"))
        .with_metadata("components", serde_json::json!(node_lists))
}

fn convert_partition_healed(reunited_nodes: &[NodeNum], ts: Timestamp) -> GeoSignal {
    let nodes: Vec<u32> = reunited_nodes.iter().map(|n| n.0).collect();
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: 0,
        snr: 0.0,
        hop_count: 0,
    });
    GeoSignal::new(kind, ts, None)
        .with_metadata("event", serde_json::json!("partition_healed"))
        .with_metadata("reunited_nodes", serde_json::json!(nodes))
}

fn convert_gateway_status(
    node: NodeNum,
    is_gateway: bool,
    ts: Timestamp,
    location: Option<Coordinates>,
) -> GeoSignal {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: node.0,
        snr: 0.0,
        hop_count: 0,
    });
    GeoSignal::new(kind, ts, location)
        .with_metadata("event", serde_json::json!("gateway_status"))
        .with_metadata("is_gateway", serde_json::json!(is_gateway))
}

fn convert_telemetry(
    node: NodeNum,
    battery_pct: Option<u32>,
    voltage: Option<f32>,
    ts: Timestamp,
    location: Option<Coordinates>,
) -> GeoSignal {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: node.0,
        snr: 0.0,
        hop_count: 0,
    });
    let mut signal =
        GeoSignal::new(kind, ts, location).with_metadata("event", serde_json::json!("telemetry"));
    if let Some(pct) = battery_pct {
        signal = signal.with_metadata("battery_pct", serde_json::json!(pct));
    }
    if let Some(v) = voltage {
        signal = signal.with_metadata("voltage", serde_json::json!(v));
    }
    signal
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_discovered_produces_node_seen_signal() {
        let event = MeshEvent::NodeDiscovered {
            node: NodeNum(42),
            short_name: Some("TEST".into()),
            snr: 7.5,
            hop_count: 2,
        };
        let signal = mesh_event_to_signal(&event, None);
        assert!(matches!(
            signal.kind,
            SignalKind::Mesh(MeshDetail::NodeSeen { node_id: 42, .. })
        ));
    }

    #[test]
    fn position_update_produces_position_signal() {
        let event = MeshEvent::PositionUpdate {
            node: NodeNum(10),
            lat: 51.5074,
            lon: -0.1278,
            alt: Some(11.0),
        };
        let signal = mesh_event_to_signal(&event, None);
        assert!(matches!(
            signal.kind,
            SignalKind::Mesh(MeshDetail::Position { node_id: 10, .. })
        ));
        assert!(signal.location.is_some());
    }

    #[test]
    fn telemetry_update_includes_battery_metadata() {
        let event = MeshEvent::TelemetryUpdate {
            node: NodeNum(5),
            battery_pct: Some(85),
            voltage: Some(3.7),
        };
        let signal = mesh_event_to_signal(&event, None);
        assert!(signal.metadata.contains_key("battery_pct"));
        assert!(signal.metadata.contains_key("voltage"));
    }

    #[test]
    fn partition_detected_includes_component_metadata() {
        let event = MeshEvent::PartitionDetected {
            components: vec![vec![NodeNum(1), NodeNum(2)], vec![NodeNum(3)]],
        };
        let signal = mesh_event_to_signal(&event, None);
        assert!(signal.metadata.contains_key("components"));
    }

    #[test]
    fn offline_event_has_reduced_confidence() {
        let event = MeshEvent::NodeOffline { node: NodeNum(99) };
        let signal = mesh_event_to_signal(&event, None);
        assert!((signal.confidence.as_f32() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn topology_change_includes_to_node_metadata() {
        let event = MeshEvent::TopologyChange {
            from: NodeNum(1),
            to: NodeNum(2),
            snr: 12.0,
        };
        let signal = mesh_event_to_signal(&event, None);
        assert!(signal.metadata.contains_key("to_node"));
    }
}
