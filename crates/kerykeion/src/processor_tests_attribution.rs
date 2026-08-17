//! Tests for [`super`]'s unauthenticated-attribution guard on `PacketProcessor`'s
//! OWN `NodeDb` + `MeshTopology` -- SEPARATE state from `MeshCollector`'s
//! display-only `NodeDb`, which `collector_tests_attribution.rs` covers.
//! `#246`'s original fix guarded only the collector's copy; this file
//! guards against the same defect resurfacing here. Split out rather than
//! added to `processor_tests.rs`, which is already at the
//! RUST/file-too-long 800-line threshold.

use super::*;

fn make_processor() -> PacketProcessor {
    let (tx, _rx) = broadcast::channel(64);
    let mut node_db = NodeDb::new();
    node_db.set_my_node(NodeNum(0xAAAA));
    PacketProcessor::new(node_db, MeshTopology::new(), tx)
}

/// A packet shaped to exercise the passive-learning direct-link write:
/// `hop_start == hop_limit` (hop_count 0) with a non-zero `rx_snr` is
/// exactly what makes pre-fix `apply_passive_learning` call
/// `topology.update_link` -- the reviewer's crafted attack packet for the
/// blocking finding on #381.
fn sentinel_packet(from: u32) -> crate::proto::MeshPacket {
    crate::proto::MeshPacket {
        from,
        to: 0xFFFF_FFFF,
        rx_snr: 5.0,
        hop_start: 1,
        hop_limit: 1,
        ..Default::default()
    }
}

#[test]
fn process_mesh_packet_ignores_zero_from_sentinel() {
    // WHY(#381): pre-fix, `PacketProcessor::apply_passive_learning` read
    // `packet.from` directly with no sentinel check, unlike the guarded
    // `MeshCollector::handle_mesh_packet` path -- a spoofed `from: 0`
    // created a node-DB entry in the processor's OWN NodeDb and, because
    // this packet's hop_count is 0, fabricated a direct topology edge FROM
    // the sentinel identity to `my_node`.
    let mut proc = make_processor();
    let packet = sentinel_packet(0);

    let events = proc.process_mesh_packet(&packet);

    assert!(events.is_empty(), "sentinel `from` must emit no events");
    assert!(
        proc.node_db().get(NodeNum(0)).is_none(),
        "from == 0 must never create a node-DB entry"
    );
    assert!(
        proc.node_db().is_empty(),
        "from == 0 must not touch the node DB at all"
    );
    assert!(
        !proc.topology().contains_node(NodeNum(0)),
        "from == 0 must never appear in the topology graph"
    );
    assert_eq!(
        proc.topology().edge_count(),
        0,
        "from == 0 must never create a topology edge"
    );
}

#[test]
fn process_mesh_packet_ignores_broadcast_from_sentinel() {
    // WHY(#381): same as the zero case, for the broadcast sentinel.
    let mut proc = make_processor();
    let packet = sentinel_packet(0xFFFF_FFFF);

    let events = proc.process_mesh_packet(&packet);

    assert!(events.is_empty(), "sentinel `from` must emit no events");
    assert!(
        proc.node_db().get(NodeNum(0xFFFF_FFFF)).is_none(),
        "from == broadcast must never create a node-DB entry"
    );
    assert!(proc.node_db().is_empty());
    assert!(
        !proc.topology().contains_node(NodeNum(0xFFFF_FFFF)),
        "from == broadcast must never appear in the topology graph"
    );
    assert_eq!(proc.topology().edge_count(), 0);
}

#[test]
fn process_mesh_packet_still_admits_a_real_node_num() {
    // WHY: the falsifiable half of the two sentinel-rejection tests above --
    // without this, a guard that rejected EVERY `from` value (not just the
    // two sentinels) would also pass them.
    let mut proc = make_processor();
    let packet = sentinel_packet(0xDEAD);

    proc.process_mesh_packet(&packet);

    let my_node = proc.node_db().my_node().unwrap();
    assert!(proc.node_db().get(NodeNum(0xDEAD)).is_some());
    let neighbors = proc.topology().neighbors(NodeNum(0xDEAD));
    assert!(
        neighbors.iter().any(|(n, _)| *n == my_node),
        "a real node number must still create the direct topology link"
    );
}

#[test]
fn process_mesh_packet_sentinel_from_blocks_nodeinfo_dispatch_too() {
    // WHY(#381): the fix gates once, at `process_mesh_packet`'s single
    // derivation of `from`, rather than inside `apply_passive_learning`
    // alone -- this proves the portnum-dispatched handlers (`handle_nodeinfo`
    // here) are ALSO unreachable for a sentinel `from`, not merely the
    // passive-learning write the reviewer's citation named. A fix mirroring
    // only that literal citation (processor.rs:161-206) would have left
    // this second path exploitable -- the same "protects the wrong copy"
    // shape one level deeper in the same file.
    let mut proc = make_processor();
    let user = crate::proto::User {
        id: "!deadbeef".into(),
        long_name: "Spoofed Node".into(),
        short_name: "SPF".into(),
        macaddr: vec![],
        hw_model: 9,
        is_licensed: false,
        role: 0,
    };
    let mut payload = Vec::new();
    user.encode(&mut payload).unwrap();

    let mut packet = sentinel_packet(0);
    packet.payload_variant = Some(crate::proto::mesh_packet::PayloadVariant::Decoded(
        crate::proto::Data {
            portnum: portnum::NODEINFO_APP,
            payload,
            want_response: false,
            dest: 0,
            source: 0,
            request_id: 0,
            reply_id: 0,
            emoji: vec![],
        },
    ));

    let events = proc.process_mesh_packet(&packet);

    assert!(
        events.is_empty(),
        "sentinel `from` must emit no NodeDiscovered event"
    );
    assert!(
        proc.node_db().is_empty(),
        "sentinel `from` must not reach handle_nodeinfo's insert"
    );
}
