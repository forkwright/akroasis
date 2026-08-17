//! Tests for [`super`]'s unauthenticated-attribution guard (#246); split out
//! from `collector_tests.rs` rather than added to it, which is already at the
//! RUST/file-too-long 800-line threshold.

use super::*;
use crate::config::{ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};

fn make_config(connections: Vec<ConnectionConfig>) -> MeshConfig {
    MeshConfig {
        connections,
        store_forward: StoreForwardConfig::default(),
        topology: TopologyConfig::default(),
        ..MeshConfig::default()
    }
}

#[test]
fn claimed_node_num_rejects_zero_and_broadcast() {
    assert!(
        ClaimedNodeNum::from_wire(0).is_none(),
        "from == 0 must be rejected"
    );
    assert!(
        ClaimedNodeNum::from_wire(0xFFFF_FFFF).is_none(),
        "from == broadcast must be rejected"
    );
}

#[test]
fn claimed_node_num_accepts_a_real_value() {
    // WHY: the falsifiable half of the sentinel-rejection test above --
    // without this, a guard that rejected EVERY `from` value (not just the
    // two sentinels) would also pass it.
    assert_eq!(
        ClaimedNodeNum::from_wire(0xDEAD_BEEF).map(ClaimedNodeNum::accept_unauthenticated),
        Some(crate::types::NodeNum(0xDEAD_BEEF))
    );
}

#[tokio::test]
async fn process_packet_ignores_zero_from_sentinel() {
    // WHY(#246): pre-fix, `mesh_packet.from == 0` created a node-DB entry
    // keyed on a value that is never a real node -- ANY node on the mesh
    // could spoof `from: 0` and still land in the DB.
    let c = MeshCollector::new(make_config(vec![]));
    let pkt = FromRadio {
        id: 1,
        payload_variant: Some(from_radio::PayloadVariant::Packet(
            crate::proto::MeshPacket {
                from: 0,
                to: 0xFFFF_FFFF,
                rx_snr: 5.0,
                hop_start: 3,
                hop_limit: 1,
                ..Default::default()
            },
        )),
    };

    c.process_packet(&pkt).await;

    let db = c.node_db().lock().await;
    let has_entry = db.get(crate::types::NodeNum(0)).is_some();
    let db_is_empty = db.is_empty();
    drop(db);
    assert!(!has_entry, "from == 0 must never create a node-DB entry");
    assert!(db_is_empty, "from == 0 must not touch the node DB at all");
}

#[tokio::test]
async fn process_packet_ignores_broadcast_from_sentinel() {
    // WHY(#246): pre-fix, a spoofed `from == 0xFFFF_FFFF` (broadcast) would
    // insert/update a node-DB entry keyed on the broadcast address, feeding
    // a phantom entry into topology/discovery.
    let c = MeshCollector::new(make_config(vec![]));
    let pkt = FromRadio {
        id: 1,
        payload_variant: Some(from_radio::PayloadVariant::Packet(
            crate::proto::MeshPacket {
                from: 0xFFFF_FFFF,
                to: 0x1234,
                rx_snr: 5.0,
                hop_start: 3,
                hop_limit: 1,
                ..Default::default()
            },
        )),
    };

    c.process_packet(&pkt).await;

    let db = c.node_db().lock().await;
    let has_entry = db.get(crate::types::NodeNum(0xFFFF_FFFF)).is_some();
    let db_is_empty = db.is_empty();
    drop(db);
    assert!(
        !has_entry,
        "from == broadcast must never create a node-DB entry"
    );
    assert!(db_is_empty);
}

#[tokio::test]
async fn process_packet_still_admits_a_real_node_num() {
    // WHY: the falsifiable half of the two sentinel-rejection tests above --
    // without this, a guard that rejected EVERY packet (not just the
    // sentinels) would also pass them.
    let c = MeshCollector::new(make_config(vec![]));
    let pkt = FromRadio {
        id: 1,
        payload_variant: Some(from_radio::PayloadVariant::Packet(
            crate::proto::MeshPacket {
                from: 0xDEAD,
                to: 0xFFFF_FFFF,
                rx_snr: 5.0,
                hop_start: 3,
                hop_limit: 1,
                ..Default::default()
            },
        )),
    };

    c.process_packet(&pkt).await;

    let db = c.node_db().lock().await;
    let has_entry = db.get(crate::types::NodeNum(0xDEAD)).is_some();
    drop(db);
    assert!(has_entry);
}
