//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use tracing::Instrument as _;

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

fn make_tx() -> broadcast::Sender<GeoSignal> {
    broadcast::channel(16).0
}

#[test]
fn name_is_kerykeion() {
    let c = MeshCollector::new(make_config(vec![]));
    assert_eq!(c.name(), "kerykeion");
}

#[tokio::test]
async fn probe_false_when_no_connections() {
    let c = MeshCollector::new(make_config(vec![]));
    assert!(!c.probe().await);
}

#[tokio::test]
async fn probe_false_when_serial_device_missing() {
    let c = MeshCollector::new(make_config(vec![ConnectionConfig::Serial {
        port: "/dev/nonexistent_device_xyz".into(),
        baud: 115_200,
    }]));
    assert!(!c.probe().await, "should not probe true for missing device");
}

#[tokio::test]
async fn run_exits_with_no_connections() {
    let mut c = MeshCollector::new(make_config(vec![]));
    let token = CancellationToken::new();
    let result = c.run(make_tx(), token).await;
    assert!(result.is_ok(), "should exit cleanly with no connections");
}

#[tokio::test]
async fn node_db_starts_empty() {
    let c = MeshCollector::new(make_config(vec![]));
    assert!(c.node_db().lock().await.is_empty());
}

#[tokio::test]
async fn process_packet_adds_node_to_db() {
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
    let node = db.get(crate::types::NodeNum(0xDEAD)).cloned();
    drop(db);
    assert!(node.is_some(), "node should be in database");
    #[expect(clippy::unwrap_used, reason = "test-only: checked above")]
    let node = node.unwrap();
    assert_eq!(node.snr, Some(5.0));
    assert_eq!(node.hop_count, Some(2));
}

#[tokio::test]
async fn process_packet_updates_existing_node() {
    let c = MeshCollector::new(make_config(vec![]));

    {
        let mut db = c.node_db().lock().await;
        db.insert(crate::node_db::MeshNode {
            num: crate::types::NodeNum(0xBEEF),
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: Some(1.0),
            hop_count: None,
        });
    }

    let pkt = FromRadio {
        id: 2,
        payload_variant: Some(from_radio::PayloadVariant::Packet(
            crate::proto::MeshPacket {
                from: 0xBEEF,
                to: 0xFFFF_FFFF,
                rx_snr: 8.5,
                hop_start: 3,
                hop_limit: 2,
                ..Default::default()
            },
        )),
    };

    c.process_packet(&pkt).await;

    let db = c.node_db().lock().await;
    let node = db.get(crate::types::NodeNum(0xBEEF)).cloned();
    drop(db);
    #[expect(clippy::unwrap_used, reason = "test-only: known present")]
    let node = node.unwrap();
    assert_eq!(node.snr, Some(8.5), "SNR should be updated");
    assert_eq!(node.hop_count, Some(1), "hop count should be updated");
}

#[tokio::test]
async fn process_empty_payload_is_noop() {
    let c = MeshCollector::new(make_config(vec![]));
    let pkt = FromRadio {
        id: 0,
        payload_variant: None,
    };
    c.process_packet(&pkt).await;
    assert!(
        c.node_db().lock().await.is_empty(),
        "empty payload should not modify db"
    );
}

#[test]
fn compute_hop_count_valid() {
    assert_eq!(MeshCollector::compute_hop_count(3, 1), Some(2));
    assert_eq!(MeshCollector::compute_hop_count(7, 0), Some(7));
}

#[test]
fn compute_hop_count_zero_start() {
    assert_eq!(MeshCollector::compute_hop_count(0, 0), None);
}

#[test]
fn compute_hop_count_limit_exceeds_start() {
    assert_eq!(MeshCollector::compute_hop_count(2, 5), None);
}

#[tokio::test]
async fn router_starts_empty() {
    let c = MeshCollector::new(make_config(vec![]));
    let pending = c.router().lock().await.outbound.pending_count();
    assert_eq!(pending, 0, "outbound queue should start empty");
}

#[tokio::test]
async fn router_flush_cancels_cleanly() {
    use crate::proto::FromRadio;

    // WHY: mock connection that never receives and accepts all sends.
    struct NoopConn;
    impl crate::connection::MeshConnection for NoopConn {
        async fn send(&mut self, _: crate::proto::ToRadio) -> Result<(), Error> {
            Ok(())
        }
        async fn recv(&mut self) -> Result<FromRadio, Error> {
            std::future::pending().await
        }
        fn is_connected(&self) -> bool {
            true
        }
        async fn reconnect(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    let router = Arc::new(Mutex::new(MeshRouter::new(
        OutboundQueue::new(),
        StoreForward::new(StoreForwardConfig::default()),
        DeliveryTracker::new(),
    )));
    let conn = Arc::new(Mutex::new(NoopConn));
    let token = CancellationToken::new();
    let task_token = token.clone();

    let handle = tokio::spawn(
        async move { run_router_flush(router, conn, Duration::from_secs(1), task_token).await }
            .instrument(tracing::info_span!("spawned_task")),
    );

    // Cancel immediately  -  biased SELECT exits before first tick.
    token.cancel();
    #[expect(clippy::unwrap_used, reason = "test-only")]
    let result = handle.await.unwrap();
    assert!(result.is_ok(), "router flush should cancel cleanly");
}

// ── akroasis#235: inbound ACK/NAK reaches DeliveryTracker ────────────

fn make_outbound_packet(id: u32, to: u32) -> crate::proto::MeshPacket {
    crate::proto::MeshPacket {
        from: 0,
        to,
        channel: 0,
        id,
        rx_time: 0,
        rx_snr: 0.0,
        hop_limit: 3,
        want_ack: true,
        priority: i32::from(crate::proto::mesh_packet::Priority::Default),
        rx_rssi: 0,
        via_mqtt: false,
        hop_start: 3,
        payload_variant: None,
    }
}

/// Builds an inbound `ROUTING_APP` packet ACKing/NAKing `request_id`.
fn make_routing_packet(request_id: u32, error_code: i32) -> crate::proto::MeshPacket {
    use prost::Message as _;

    let routing = crate::proto::Routing {
        variant: Some(crate::proto::routing::Variant::ErrorReason(error_code)),
    };
    let data = crate::proto::Data {
        portnum: i32::from(crate::proto::PortNum::RoutingApp),
        payload: routing.encode_to_vec(),
        want_response: false,
        dest: 0,
        source: 0,
        request_id,
        reply_id: 0,
        emoji: vec![],
    };
    crate::proto::MeshPacket {
        from: 0x2222,
        to: 0x1111,
        channel: 0,
        id: 0xFFFF,
        rx_time: 0,
        rx_snr: 0.0,
        hop_limit: 3,
        want_ack: false,
        priority: i32::from(crate::proto::mesh_packet::Priority::Default),
        rx_rssi: 0,
        via_mqtt: false,
        hop_start: 3,
        payload_variant: Some(crate::proto::mesh_packet::PayloadVariant::Decoded(data)),
    }
}

#[tokio::test]
async fn dispatch_routing_ack_marks_delivered() {
    let c = MeshCollector::new(make_config(vec![]));

    let packet_id = {
        let mut router = c.router().lock().await;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(
                make_outbound_packet(0x9001, 0xBBBB),
                true,
                &crate::router::SendOptions::default(),
            )
            .unwrap();
        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }
        id
    };

    let ack_pkt = make_routing_packet(0x9001, i32::from(crate::proto::routing::Error::None));
    c.dispatch_routing(&ack_pkt).await;

    let router = c.router().lock().await;
    assert!(
        matches!(
            router.delivery.delivery_status(packet_id),
            Some(crate::delivery::DeliveryStatus::Acknowledged { .. })
        ),
        "inbound ACK should mark the tracked message delivered"
    );
    drop(router);
}

#[tokio::test]
async fn dispatch_routing_nak_retries_when_budget_remains() {
    let c = MeshCollector::new(make_config(vec![]));

    let packet_id = {
        let mut router = c.router().lock().await;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let id = router
            .send(
                make_outbound_packet(0x9002, 0xCCCC),
                true,
                &crate::router::SendOptions::default(),
            )
            .unwrap();
        if let Some(msg) = router.next_to_send() {
            router.track_sent(msg);
        }
        id
    };

    let nak_pkt = make_routing_packet(0x9002, i32::from(crate::proto::routing::Error::NoRoute));
    c.dispatch_routing(&nak_pkt).await;

    let router = c.router().lock().await;
    assert_eq!(
        router.outbound.pending_count(),
        1,
        "NAK with retry budget remaining should re-enqueue the message"
    );
    assert!(
        matches!(
            router.delivery.delivery_status(packet_id),
            Some(crate::delivery::DeliveryStatus::Sent { .. })
        ),
        "should remain Sent (not Failed) while retries remain"
    );
    assert_eq!(
        router.delivery.stats_for(0xCCCC).map(|s| s.total_retries),
        Some(1),
        "DeliveryTracker::record_retry should have fired"
    );
    drop(router);
}

#[tokio::test]
async fn dispatch_routing_nak_marks_failed_when_not_inflight() {
    let c = MeshCollector::new(make_config(vec![]));
    let packet_id = crate::types::PacketId(0x9003);

    {
        let mut router = c.router().lock().await;
        // WHY: tracked + sent but never handed to the outbound queue,
        // mirroring processor_tests.rs's `apply_nak_marks_failed_when_no_inflight` -
        // no inflight record means the retry budget is already exhausted.
        router.delivery.track(packet_id, 0xDDDD);
        router.delivery.mark_sent(packet_id);
    }

    let nak_pkt = make_routing_packet(
        0x9003,
        i32::from(crate::proto::routing::Error::MaxRetransmit),
    );
    c.dispatch_routing(&nak_pkt).await;

    let router = c.router().lock().await;
    assert!(
        matches!(
            router.delivery.delivery_status(packet_id),
            Some(crate::delivery::DeliveryStatus::Failed { .. })
        ),
        "NAK with no retry budget should mark the message failed"
    );
    drop(router);
}

#[tokio::test]
async fn dispatch_routing_ignores_non_routing_packet() {
    let c = MeshCollector::new(make_config(vec![]));
    let pkt = make_outbound_packet(0x9004, 0xEEEE);

    c.dispatch_routing(&pkt).await;

    let router = c.router().lock().await;
    assert_eq!(
        router.delivery.tracked_count(),
        0,
        "non-routing packet must not create a delivery record"
    );
    drop(router);
}
