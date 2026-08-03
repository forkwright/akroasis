//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use super::*;
use crate::outbound::PendingMessage;
use crate::proto::{Data, PortNum, mesh_packet};

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
    // WHY: pre-INSERT a node so telemetry has something to UPDATE.
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

    let mut packet = make_mesh_packet(0x1111, portnum::NEIGHBORINFO_APP, payload);
    // WHY: hop_limit == hop_start -> hop_count 0 -> direct, so passive learning adds the +1 edge.
    packet.hop_limit = packet.hop_start;
    let events = proc.process_mesh_packet(&packet);

    // WHY: 2 FROM neighborinfo + 1 FROM passive learning (direct link).
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
        "expected at least 3 edges FROM traceroute path"
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
    // WHY: hop_limit == hop_start → hop_count 0 → no relay traversed (direct).
    let mut packet = make_mesh_packet(0xCCCC, portnum::NODEINFO_APP, vec![]);
    packet.hop_limit = packet.hop_start;
    proc.process_mesh_packet(&packet);

    // WHY: direct packet should CREATE a link FROM sender to our node.
    let my_node = proc.node_db().my_node().unwrap();
    let neighbors = proc.topology().neighbors(NodeNum(0xCCCC));
    assert!(
        neighbors.iter().any(|(n, _)| *n == my_node),
        "direct packet should CREATE link to server node"
    );
}

#[test]
fn passive_learning_one_relay_does_not_create_direct_link() {
    let mut proc = make_processor();
    // WHY: hop_start=3, hop_limit=2 → hop_count 1 → one relay traversed (not direct).
    let packet = make_mesh_packet(0xDDDD, portnum::NODEINFO_APP, vec![]);
    proc.process_mesh_packet(&packet);

    let my_node = proc.node_db().my_node().unwrap();
    let neighbors = proc.topology().neighbors(NodeNum(0xDDDD));
    assert!(
        !neighbors.iter().any(|(n, _)| *n == my_node),
        "one-relay packet should NOT create a direct link to server node"
    );
}

// ── Routing processor tests ──────────────────────────────────────────

fn make_routing_packet(request_id: u32, error_code: i32) -> MeshPacket {
    let routing = Routing {
        variant: Some(routing::Variant::ErrorReason(error_code)),
    };
    let routing_bytes = routing.encode_to_vec();

    let data = Data {
        portnum: i32::from(PortNum::RoutingApp),
        payload: routing_bytes,
        want_response: false,
        dest: 0,
        source: 0,
        request_id,
        reply_id: 0,
        emoji: vec![],
    };

    MeshPacket {
        from: 0x1111,
        to: 0x2222,
        channel: 0,
        id: 0xFFFF,
        rx_time: 0,
        rx_snr: 0.0,
        hop_limit: 3,
        want_ack: false,
        priority: i32::from(mesh_packet::Priority::Default),
        rx_rssi: 0,
        via_mqtt: false,
        hop_start: 3,
        payload_variant: Some(mesh_packet::PayloadVariant::Decoded(data)),
    }
}

#[test]
fn ack_packet_detected() {
    let pkt = make_routing_packet(0x1234, i32::from(routing::Error::None));
    let result = RoutingProcessor::process_routing(&pkt);
    assert_eq!(
        result,
        RoutingResult::Ack {
            request_id: PacketId(0x1234)
        }
    );
}

#[test]
fn nak_no_route_detected() {
    let pkt = make_routing_packet(0x5678, i32::from(routing::Error::NoRoute));
    let result = RoutingProcessor::process_routing(&pkt);
    assert_eq!(
        result,
        RoutingResult::Nak {
            request_id: PacketId(0x5678),
            error: routing::Error::NoRoute,
        }
    );
}

#[test]
fn nak_max_retransmit_detected() {
    let pkt = make_routing_packet(0xABCD, i32::from(routing::Error::MaxRetransmit));
    let result = RoutingProcessor::process_routing(&pkt);
    assert_eq!(
        result,
        RoutingResult::Nak {
            request_id: PacketId(0xABCD),
            error: routing::Error::MaxRetransmit,
        }
    );
}

#[test]
fn non_routing_packet_ignored() {
    let data = Data {
        portnum: i32::from(PortNum::TextMessageApp),
        payload: b"hello".to_vec(),
        ..Default::default()
    };
    let pkt = MeshPacket {
        from: 0x1111,
        to: 0x2222,
        channel: 0,
        id: 1,
        rx_time: 0,
        rx_snr: 0.0,
        hop_limit: 3,
        want_ack: false,
        priority: i32::from(mesh_packet::Priority::Default),
        rx_rssi: 0,
        via_mqtt: false,
        hop_start: 3,
        payload_variant: Some(mesh_packet::PayloadVariant::Decoded(data)),
    };
    assert_eq!(
        RoutingProcessor::process_routing(&pkt),
        RoutingResult::NotRouting
    );
}

#[test]
fn encrypted_packet_returns_not_routing() {
    let pkt = MeshPacket {
        from: 0x1111,
        to: 0x2222,
        channel: 0,
        id: 1,
        rx_time: 0,
        rx_snr: 0.0,
        hop_limit: 3,
        want_ack: false,
        priority: i32::from(mesh_packet::Priority::Default),
        rx_rssi: 0,
        via_mqtt: false,
        hop_start: 3,
        payload_variant: Some(mesh_packet::PayloadVariant::Encrypted(vec![0xFF; 16])),
    };
    assert_eq!(
        RoutingProcessor::process_routing(&pkt),
        RoutingResult::NotRouting
    );
}

#[test]
fn zero_request_id_ignored() {
    let pkt = make_routing_packet(0, i32::from(routing::Error::None));
    assert_eq!(
        RoutingProcessor::process_routing(&pkt),
        RoutingResult::NotRouting,
        "request_id=0 should be ignored"
    );
}

#[test]
fn apply_ack_updates_tracker() {
    let mut delivery = DeliveryTracker::new();
    let mut outbound = OutboundQueue::new();
    let id = PacketId(42);

    delivery.track(id, 0x1234);
    delivery.mark_sent(id);

    let result = RoutingResult::Ack { request_id: id };
    RoutingProcessor::apply_routing_result(&result, &mut delivery, &mut outbound);

    assert!(matches!(
        delivery.delivery_status(id),
        Some(crate::delivery::DeliveryStatus::Acknowledged { .. })
    ));
}

#[test]
fn apply_nak_marks_failed_when_no_inflight() {
    let mut delivery = DeliveryTracker::new();
    let mut outbound = OutboundQueue::new();
    let id = PacketId(77);

    delivery.track(id, 0x5678);
    delivery.mark_sent(id);

    let result = RoutingResult::Nak {
        request_id: id,
        error: routing::Error::NoRoute,
    };
    RoutingProcessor::apply_routing_result(&result, &mut delivery, &mut outbound);

    assert!(matches!(
        delivery.delivery_status(id),
        Some(crate::delivery::DeliveryStatus::Failed { .. })
    ));
}

#[test]
fn apply_nak_retries_while_the_packet_is_still_inflight() {
    // WHY(#229): the sibling of apply_nak_marks_failed_when_no_inflight. When
    // the packet IS inflight with retries remaining, the NAK must re-queue it
    // and leave the delivery record un-failed — the retry arm of
    // apply_routing_result, which no existing test reached.
    let mut delivery = DeliveryTracker::new();
    let mut outbound = OutboundQueue::new();
    let id = PacketId(78);

    delivery.track(id, 0x5678);
    outbound.track_inflight(
        PendingMessage {
            packet: crate::proto::MeshPacket {
                id: id.0,
                ..Default::default()
            },
            created: tokio::time::Instant::now(),
            ttl: std::time::Duration::from_secs(60),
            priority: crate::proto::mesh_packet::Priority::Default,
            retries: 0,
        },
        std::time::Duration::from_secs(30),
    );
    delivery.mark_sent(id);

    let result = RoutingResult::Nak {
        request_id: id,
        error: routing::Error::NoRoute,
    };
    RoutingProcessor::apply_routing_result(&result, &mut delivery, &mut outbound);

    assert_eq!(
        outbound.pending_count(),
        1,
        "a retryable NAK must re-queue the packet"
    );
    assert_eq!(outbound.inflight_count(), 0);
    assert!(
        !matches!(
            delivery.delivery_status(id),
            Some(crate::delivery::DeliveryStatus::Failed { .. })
        ),
        "a retryable NAK must not mark the delivery failed"
    );
}

#[test]
fn malformed_payloads_are_dropped_without_panicking() {
    // WHY(#229): every decode site on the OTA path is fed attacker-influenced
    // bytes. A malformed payload must be logged and skipped, never unwrapped —
    // so each handler returns no events and leaves the node database clean.
    let mut proc = make_processor();
    // A leading tag byte of 0xFF is not a valid protobuf field header, so
    // prost rejects it for every message type below.
    let garbage = vec![0xFF_u8, 0xFF, 0xFF, 0xFF];

    for portnum in [
        portnum::NODEINFO_APP,
        portnum::POSITION_APP,
        portnum::TELEMETRY_APP,
        portnum::NEIGHBORINFO_APP,
        portnum::TRACEROUTE_APP,
    ] {
        let packet = make_mesh_packet(0xBAD0, portnum, garbage.clone());
        let events = proc.process_mesh_packet(&packet);

        assert!(
            events.is_empty(),
            "portnum {portnum} produced events from a malformed payload"
        );
    }

    // Passive learning still records the sender; only the decoded payload is
    // discarded.
    #[expect(clippy::unwrap_used, reason = "test-only: passive learning inserts it")]
    let node = proc.node_db().get(NodeNum(0xBAD0)).unwrap();
    assert!(node.user.is_none(), "no user info may survive a bad decode");
    assert!(node.position.is_none());
}

#[test]
fn repeat_nodeinfo_for_a_known_node_emits_no_rediscovery() {
    // WHY(#229): NodeDiscovered drives downstream alerting, so a periodic
    // NODEINFO refresh from an already-known node must not re-announce it.
    // The is_new test keys on whether user info was already present.
    let mut proc = make_processor();
    let user = crate::proto::User {
        id: "!feedface".into(),
        long_name: "Repeat Node".into(),
        short_name: "RPT".into(),
        macaddr: vec![],
        hw_model: 9,
        is_licensed: false,
        role: 0,
    };
    let mut payload = Vec::new();
    user.encode(&mut payload).unwrap();

    let first = proc.process_mesh_packet(&make_mesh_packet(
        0xFEED,
        portnum::NODEINFO_APP,
        payload.clone(),
    ));
    assert!(
        first
            .iter()
            .any(|e| matches!(e, MeshEvent::NodeDiscovered { .. })),
        "the first NODEINFO must announce the node"
    );

    let second =
        proc.process_mesh_packet(&make_mesh_packet(0xFEED, portnum::NODEINFO_APP, payload));
    assert!(
        !second
            .iter()
            .any(|e| matches!(e, MeshEvent::NodeDiscovered { .. })),
        "a repeat NODEINFO must not re-announce a known node"
    );
}

#[test]
fn traceroute_inserts_the_reverse_path_links() {
    // WHY(#229): the `back`/`snr_back` half of handle_traceroute runs only
    // when `back` is non-empty, and inserts links without emitting events —
    // so nothing but the topology shows it ran.
    let mut proc = make_processor();
    let route = crate::proto::RouteDiscovery {
        route: vec![0x20],
        snr_towards: vec![40, 32],
        back: vec![0x20],
        snr_back: vec![24, 16],
    };
    let mut payload = Vec::new();
    route.encode(&mut payload).unwrap();

    let mut packet = make_mesh_packet(0x10, portnum::TRACEROUTE_APP, payload);
    packet.to = 0x30;

    proc.process_mesh_packet(&packet);

    // Links are directed. The forward path lays 0x10 → 0x20 → 0x30; the
    // reverse path lays 0x30 → 0x20 → 0x10. So the 0x20 → 0x10 edge exists
    // only if the reverse loop ran, and its SNR comes from snr_back.
    let topology = proc.topology();
    assert!(topology.contains_node(NodeNum(0x10)));
    assert!(topology.contains_node(NodeNum(0x20)));
    assert!(topology.contains_node(NodeNum(0x30)));

    let neighbours = topology.neighbors(NodeNum(0x20));
    assert_eq!(
        neighbours.len(),
        2,
        "the middle hop must have an outgoing link on each path"
    );

    #[expect(clippy::unwrap_used, reason = "test-only: asserted present above")]
    let to_origin = neighbours
        .iter()
        .find(|(n, _)| *n == NodeNum(0x10))
        .unwrap()
        .1;
    // snr_back[1] = 16 is the 0x20 → 0x10 leg of the reverse path.
    assert!(
        (to_origin.snr - 16.0).abs() < f32::EPSILON,
        "the reverse leg must carry its snr_back reading, got {}",
        to_origin.snr
    );
}

#[test]
fn traceroute_without_a_reverse_path_leaves_the_forward_snr() {
    // WHY(#229): the falsifiable half — with `back` empty the reverse loop is
    // skipped, so the forward SNR survives. Without this the assertion above
    // could pass on any code that merely wrote some SNR.
    let mut proc = make_processor();
    let route = crate::proto::RouteDiscovery {
        route: vec![0x20],
        snr_towards: vec![40, 32],
        back: vec![],
        snr_back: vec![],
    };
    let mut payload = Vec::new();
    route.encode(&mut payload).unwrap();

    let mut packet = make_mesh_packet(0x10, portnum::TRACEROUTE_APP, payload);
    packet.to = 0x30;

    proc.process_mesh_packet(&packet);

    let neighbours = proc.topology().neighbors(NodeNum(0x20));
    assert!(
        !neighbours.iter().any(|(n, _)| *n == NodeNum(0x10)),
        "with no reverse path the 0x20 → 0x10 edge must not exist"
    );
    assert_eq!(
        neighbours.len(),
        1,
        "only the forward 0x20 → 0x30 leg may be present"
    );

    // The forward path itself is unaffected: 0x10 → 0x20 still carries
    // snr_towards[0].
    #[expect(clippy::unwrap_used, reason = "test-only: forward path links it")]
    let forward = proc
        .topology()
        .neighbors(NodeNum(0x10))
        .iter()
        .find(|(n, _)| *n == NodeNum(0x20))
        .unwrap()
        .1
        .snr;
    assert!(
        (forward - 40.0).abs() < f32::EPSILON,
        "the forward leg must carry snr_towards, got {forward}"
    );
}

// ── akroasis#229: a signal is located at its subject, not the packet sender ──

fn node_at(num: u32, latitude: f64, longitude: f64) -> MeshNode {
    MeshNode {
        num: NodeNum(num),
        user: None,
        position: Some(crate::node_db::NodePosition {
            latitude,
            longitude,
            altitude: None,
            timestamp: None,
        }),
        metrics: None,
        last_heard: None,
        snr: None,
        hop_count: None,
    }
}

#[tokio::test]
async fn neighborinfo_signal_is_located_at_the_reporter_not_the_packet_sender() {
    // WHY: NEIGHBORINFO carries its reporter id in the payload, so a relayed
    // report describes links the relay is not an endpoint of. Locating every
    // signal at the packet sender puts those links at the relay's coordinates.
    let (tx, mut rx) = broadcast::channel(64);
    let mut node_db = NodeDb::new();
    node_db.set_my_node(NodeNum(0xAAAA));
    node_db.insert(node_at(0x1111, 10.0, 10.0)); // the relay that transmitted
    node_db.insert(node_at(0x2222, 50.0, 60.0)); // the node the report is about
    let mut proc = PacketProcessor::new(node_db, MeshTopology::new(), tx);

    let ni = NeighborInfo {
        node_id: 0x2222,
        last_sent_by_id: 0,
        node_broadcast_interval_secs: 0,
        neighbors: vec![Neighbor {
            node_id: 0x3333,
            snr: 4.0,
        }],
    };
    let mut payload = Vec::new();
    ni.encode(&mut payload).unwrap();

    let packet = make_mesh_packet(0x1111, portnum::NEIGHBORINFO_APP, payload);
    let events = proc.process_mesh_packet(&packet);
    assert_eq!(events.len(), 1, "one neighbor should yield one event");

    let signal = rx.recv().await.unwrap();
    #[expect(clippy::expect_used, reason = "test-only")]
    let coords = signal
        .location
        .expect("topology signal should carry the reporter's position");
    assert!(
        (coords.latitude - 50.0).abs() < f64::EPSILON
            && (coords.longitude - 60.0).abs() < f64::EPSILON,
        "signal should be located at reporter 0x2222 (50, 60), got ({}, {})",
        coords.latitude,
        coords.longitude
    );
}

#[tokio::test]
async fn partition_events_carry_no_subject_and_no_location() {
    // WHY: the partition events describe a set of nodes rather than one, so
    // there is no single position to attribute them to.
    let detected = MeshEvent::PartitionDetected {
        components: vec![vec![NodeNum(1)], vec![NodeNum(2)]],
    };
    let healed = MeshEvent::PartitionHealed {
        reunited_nodes: vec![NodeNum(1)],
    };
    assert!(detected.subject().is_none());
    assert!(healed.subject().is_none());
}
