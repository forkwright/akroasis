//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use super::*;
use crate::SendOptions;
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

// WHY (#198): `PacketProcessor` owns topology + GeoSignal emission and must
// see every node the collector knows about, including ones learned only via
// a runtime `NodeInfo` frame with no subsequent mesh packet.

#[tokio::test]
async fn dispatch_to_processor_routes_runtime_nodeinfo_to_processor_node_db() {
    let c = MeshCollector::new(make_config(vec![]));
    let processor = c.make_processor(make_tx()).await;

    let from_radio = FromRadio {
        id: 9,
        payload_variant: Some(from_radio::PayloadVariant::NodeInfo(
            crate::proto::NodeInfo {
                num: 0x00C0_FFEE,
                ..Default::default()
            },
        )),
    };

    c.dispatch_to_processor(&from_radio, &processor).await;

    let guard = processor.lock().await;
    let found = guard
        .node_db()
        .get(crate::types::NodeNum(0x00C0_FFEE))
        .is_some();
    drop(guard);
    assert!(
        found,
        "node learned only via a runtime NodeInfo frame must reach the processor's NodeDb"
    );
}

#[tokio::test]
async fn make_processor_is_seeded_with_nodes_already_known_to_the_collector() {
    let c = MeshCollector::new(make_config(vec![]));

    // Simulate a handshake-discovered node landing in the collector's
    // NodeDb before the processor is constructed.
    c.node_db().lock().await.insert(crate::node_db::MeshNode {
        num: crate::types::NodeNum(0xFEED),
        user: None,
        position: None,
        metrics: None,
        last_heard: None,
        snr: None,
        hop_count: None,
    });

    let processor = c.make_processor(make_tx()).await;

    let guard = processor.lock().await;
    let found = guard.node_db().get(crate::types::NodeNum(0xFEED)).is_some();
    drop(guard);
    assert!(
        found,
        "processor must be seeded with nodes the collector already knew about"
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

#[tokio::test(start_paused = true)]
async fn router_flush_tick_drains_the_queue_and_sends() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::proto::FromRadio;

    // WHY(#229): counts what reached the radio, so the test observes the
    // drain/send half of the tick arm rather than only its cancellation
    // path (which `router_flush_cancels_cleanly` already covers).
    struct CountingConn(Arc<AtomicUsize>);
    impl crate::connection::MeshConnection for CountingConn {
        async fn send(&mut self, _: crate::proto::ToRadio) -> Result<(), Error> {
            self.0.fetch_add(1, Ordering::SeqCst);
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

    // Two reachable packets, so the `while let Some(msg)` drain loop has to
    // run more than once inside a single tick.
    {
        let mut r = router.lock().await;
        for id in [1_u32, 2] {
            // A reachable send only enqueues, so it cannot fail; the result is
            // asserted rather than unwrapped.
            let sent = r.send(
                crate::proto::MeshPacket {
                    id,
                    to: 0xBBBB,
                    ..Default::default()
                },
                true,
                &crate::router::SendOptions::default(),
            );
            assert!(sent.is_ok(), "a reachable send must enqueue");
        }
        assert_eq!(r.outbound.pending_count(), 2);
        drop(r);
    }

    let sent = Arc::new(AtomicUsize::new(0));
    let conn = Arc::new(Mutex::new(CountingConn(Arc::clone(&sent))));
    let token = CancellationToken::new();

    let flush_router = Arc::clone(&router);
    let task_token = token.clone();
    let handle = tokio::spawn(
        async move {
            run_router_flush(flush_router, conn, Duration::from_millis(100), task_token).await
        }
        .instrument(tracing::info_span!("spawned_task")),
    );

    // The first tick of a tokio interval fires immediately; advance past a
    // second one so the drain has certainly run.
    tokio::time::advance(Duration::from_millis(150)).await;
    tokio::task::yield_now().await;

    token.cancel();
    #[expect(clippy::unwrap_used, reason = "test-only")]
    let result = handle.await.unwrap();
    assert!(result.is_ok(), "router flush should cancel cleanly");

    assert_eq!(
        sent.load(Ordering::SeqCst),
        2,
        "both queued packets must reach the radio"
    );

    let r = router.lock().await;
    let (pending, inflight) = (r.outbound.pending_count(), r.outbound.inflight_count());
    drop(r);

    assert_eq!(pending, 0, "the queue must be drained");
    assert_eq!(
        inflight, 2,
        "drained packets must be tracked as inflight, awaiting ACK"
    );
}

// ── akroasis#190: recv_yielding must not starve send-side tasks ──────

/// Mock connection whose `recv()` never resolves, reproducing a quiet mesh.
struct BlockingRecvConn;

impl crate::connection::MeshConnection for BlockingRecvConn {
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

#[tokio::test(start_paused = true)]
async fn recv_yielding_releases_lock_for_waiting_sender() {
    let conn = Arc::new(Mutex::new(BlockingRecvConn));

    // Main-loop stand-in: parked on the quiet-mesh recv().
    let recv_conn = Arc::clone(&conn);
    let recv_handle = tokio::spawn(async move {
        let _ = recv_yielding(&*recv_conn).await;
    });
    tokio::task::yield_now().await;

    // Send-side stand-in (heartbeat / router-flush): queues on the same
    // lock before the recv loop's poll window elapses.
    let send_conn = Arc::clone(&conn);
    let send_handle = tokio::spawn(async move {
        send_conn
            .lock()
            .await
            .send(crate::proto::ToRadio {
                payload_variant: None,
            })
            .await
    });
    tokio::task::yield_now().await;

    // Elapse one poll window: recv_yielding's inner timeout fires and drops
    // the guard, handing the fair mutex to the already-queued sender.
    tokio::time::advance(RECV_LOCK_YIELD).await;
    tokio::task::yield_now().await;

    #[expect(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "test-only: recv_yielding must release the lock within one poll window"
    )]
    let send_result = tokio::time::timeout(RECV_LOCK_YIELD, send_handle)
        .await
        .expect("send should complete once recv_yielding releases the lock")
        .unwrap();

    recv_handle.abort();
    assert!(
        send_result.is_ok(),
        "queued send should succeed once the guard is dropped"
    );
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

// ── akroasis#229: total connection failure must be reported, not swallowed ──

#[tokio::test]
async fn connect_and_handshake_errors_when_every_connection_fails() {
    // WHY: returning Ok with an empty connection set spawns the whole
    // background task set against no radio. The collector then runs
    // indefinitely observing nothing while reporting success, which is
    // indistinguishable from a mesh that is merely quiet.
    let c = MeshCollector::new(make_config(vec![ConnectionConfig::Serial {
        port: "/dev/nonexistent_device_xyz".into(),
        baud: 115_200,
    }]));

    let result = c.connect_and_handshake().await;

    assert!(
        matches!(result, Err(Error::HandshakeFailed { .. })),
        "all-connections-failed should surface as Error::HandshakeFailed"
    );
}

#[tokio::test]
async fn connect_and_handshake_stays_ok_with_no_connections_configured() {
    // WHY: an empty connection list is a valid configuration, not a failure  -
    // this is what `run_exits_with_no_connections` depends on.
    let c = MeshCollector::new(make_config(vec![]));
    let result = c.connect_and_handshake().await;
    assert!(result.is_ok(), "no configured connections is not a failure");
}

// ── akroasis#229: the inflight ACK timeout starts after the transmit ──────

/// Connection whose `send()` parks until released, so the test can observe
/// router state during the transmit.
struct GatedSendConn {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl crate::connection::MeshConnection for GatedSendConn {
    async fn send(&mut self, _: crate::proto::ToRadio) -> Result<(), Error> {
        self.entered.notify_one();
        self.release.notified().await;
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

#[tokio::test]
async fn router_flush_starts_ack_timeout_after_transmit() {
    // WHY: track_sent starts the inflight ACK timeout. Starting it before the
    // transmit charges the radio's send latency and any wait on the connection
    // lock against the peer's time to ACK, so a slow link retries messages
    // that were never actually late.
    let router = Arc::new(Mutex::new(MeshRouter::new(
        OutboundQueue::new(),
        StoreForward::new(StoreForwardConfig::default()),
        DeliveryTracker::new(),
    )));
    #[expect(clippy::unwrap_used, reason = "test-only")]
    router
        .lock()
        .await
        .send(
            make_outbound_packet(42, 0x2222),
            true,
            &SendOptions::default(),
        )
        .unwrap();

    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let conn = Arc::new(Mutex::new(GatedSendConn {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    }));

    let token = CancellationToken::new();
    let flush_router = Arc::clone(&router);
    let task_token = token.clone();
    let handle = tokio::spawn(async move {
        run_router_flush(flush_router, conn, Duration::from_millis(10), task_token).await
    });

    // Park inside send(): the packet has left the queue but is not on air yet.
    entered.notified().await;
    assert_eq!(
        router.lock().await.outbound.inflight_count(),
        0,
        "the ACK timeout must not be running while the transmit is still in flight"
    );

    release.notify_one();

    // The transmit has now returned, so the message becomes inflight.
    let mut inflight = 0;
    for _ in 0..100 {
        inflight = router.lock().await.outbound.inflight_count();
        if inflight == 1 {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        inflight, 1,
        "message should be inflight once the send returns"
    );

    token.cancel();
    handle.abort();
}

// ── akroasis#205: background task failures escalate, not just log ─────────

#[test]
fn supervise_task_result_continues_on_clean_completion() {
    assert_eq!(
        supervise_task_result(Ok(Ok(()))),
        TaskOutcome::Continue,
        "a task that returns Ok must not trigger shutdown"
    );
}

#[test]
fn supervise_task_result_shuts_down_on_task_error() {
    let err = Error::ConnectionLost {
        detail: "synthetic test error".into(),
        location: snafu::location!(),
    };
    assert_eq!(
        supervise_task_result(Ok(Err(err))),
        TaskOutcome::Shutdown,
        "a task that returns Err must escalate to shutdown rather than only log"
    );
}

#[tokio::test]
async fn supervise_task_result_shuts_down_on_join_error() {
    // WHY: a real JoinError, not a hand-built stand-in -- JoinError has no
    // public constructor. Aborting a task and awaiting its handle yields a
    // genuine one without triggering an actual panic (clippy::panic denies
    // `panic!()` outside a deliberately-scoped exception, and the
    // classifier under test treats every `Err(JoinError)` identically
    // regardless of whether it came from a panic or a cancellation).
    let handle = tokio::spawn(std::future::pending::<()>());
    handle.abort();
    #[expect(clippy::expect_used, reason = "test-only")]
    let join_err = handle
        .await
        .expect_err("an aborted task must yield Err from JoinHandle::await");
    assert_eq!(
        supervise_task_result(Err(join_err)),
        TaskOutcome::Shutdown,
        "a panicked or aborted task must escalate to shutdown rather than only log"
    );
}

#[tokio::test]
async fn losing_router_flush_shuts_down_the_collector_loop() {
    // WHY(#205): this is the scenario the issue names as most consequential
    // -- router-flush dying used to leave the collector receiving forever
    // while silently never sending again. Drive the actual `select!` arm
    // (via a JoinSet standing in for `tasks`) and assert the loop's exit
    // condition, not just the pure classifier above.
    let mut tasks: JoinSet<Result<(), Error>> = JoinSet::new();
    tasks.spawn(async {
        Err(Error::ConnectionLost {
            detail: "router flush died".into(),
            location: snafu::location!(),
        })
    });

    let cancel = CancellationToken::new();
    let mut shut_down = false;
    tokio::select! {
        Some(task_result) = tasks.join_next() => {
            if supervise_task_result(task_result) == TaskOutcome::Shutdown {
                cancel.cancel();
                shut_down = true;
            }
        }
    }

    assert!(
        shut_down,
        "the loop must recognize a lost background task as a shutdown condition"
    );
    assert!(
        cancel.is_cancelled(),
        "losing a background task must cancel the collector, not just log it"
    );
}
