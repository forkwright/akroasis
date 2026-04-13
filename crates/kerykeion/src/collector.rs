//! `MeshCollector`  -  kerykeion integration point for the Akroasis collection pipeline.
//!
//! Wires together all kerykeion components: transport connections, config handshake,
//! packet processor, discovery manager, heartbeat keepalive, gateway health monitoring,
//! router background task, and signal emission INTO a single collection loop driven
//! by a `CancellationToken`.

use std::sync::Arc;
use std::time::Duration;

use koinon::GeoSignal;
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::bridge::{self, GatewayBridge};
use crate::config::MeshConfig;
use crate::connection::MeshConnection;
use crate::delivery::DeliveryTracker;
use crate::discovery::run_discovery;
use crate::error::Error;
use crate::handshake;
use crate::heartbeat;
use crate::node_db::NodeDb;
use crate::outbound::OutboundQueue;
use crate::processor::PacketProcessor;
use crate::proto::{FromRadio, from_radio};
use crate::router::MeshRouter;
use crate::store_forward::StoreForward;
use crate::topology::MeshTopology;
use crate::transport::{self, ConnectionHandle};
use crate::types::NodeNum;

/// Interval for the router flush task  -  checks timeouts and drains outbound queue.
const ROUTER_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Trait for Akroasis data collectors.
///
/// Defines the minimal lifecycle interface shared across all collector crates.
/// This is a local definition pending the addition of `koinon::Collector`.
pub trait Collector: Send + Sync {
    /// Returns the canonical name of this collector.
    fn name(&self) -> &'static str;

    /// Returns `true` if the collector's hardware or network target is reachable.
    ///
    /// Used during startup to decide whether to activate the collector.
    fn probe(&self) -> impl std::future::Future<Output = bool> + Send;

    /// Starts the collection loop.
    ///
    /// Broadcasts [`GeoSignal`]s on `tx` as observations arrive. Returns when
    /// the collector has shut down cleanly or a fatal error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the collector encounters a fatal, unrecoverable failure.
    fn run(
        &mut self,
        tx: broadcast::Sender<GeoSignal>,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
}

/// Meshtastic mesh networking collector.
///
/// Manages connections to one or more Meshtastic radios, receives mesh packets,
/// maintains the node database and gateway bridge, forwards observations
/// INTO the Akroasis pipeline, and manages outbound message routing.
pub struct MeshCollector {
    config: MeshConfig,
    node_db: Arc<Mutex<NodeDb>>,
    bridge: Arc<Mutex<GatewayBridge>>,
    router: Arc<Mutex<MeshRouter>>,
}

impl MeshCollector {
    /// Creates a new `MeshCollector` with the given configuration.
    #[must_use]
    pub fn new(config: MeshConfig) -> Self {
        let sf_config = config.store_forward.clone();
        Self {
            node_db: Arc::new(Mutex::new(NodeDb::new())),
            bridge: Arc::new(Mutex::new(GatewayBridge::new())),
            router: Arc::new(Mutex::new(MeshRouter::new(
                OutboundQueue::new(),
                StoreForward::new(sf_config),
                DeliveryTracker::new(),
            ))),
            config,
        }
    }

    /// Returns a reference to the shared node database.
    #[must_use]
    pub const fn node_db(&self) -> &Arc<Mutex<NodeDb>> {
        &self.node_db
    }

    /// Returns a reference to the shared gateway bridge.
    #[must_use]
    pub const fn bridge(&self) -> &Arc<Mutex<GatewayBridge>> {
        &self.bridge
    }

    /// Returns a reference to the shared mesh router.
    #[must_use]
    pub const fn router(&self) -> &Arc<Mutex<MeshRouter>> {
        &self.router
    }

    /// Computes hop count FROM packet hop fields.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "hop VALUES are bounded by MAX_HOP_LIMIT (7) in Meshtastic firmware"
    )]
    const fn compute_hop_count(hop_start: u32, hop_limit: u32) -> Option<u8> {
        if hop_start > 0 && hop_limit <= hop_start {
            Some((hop_start - hop_limit) as u8)
        } else {
            None
        }
    }

    /// Processes a single `FromRadio` message, updating the node database.
    async fn process_packet(&self, from_radio: &FromRadio) {
        let Some(ref variant) = from_radio.payload_variant else {
            return;
        };

        match variant {
            from_radio::PayloadVariant::Packet(mesh_packet) => {
                self.handle_mesh_packet(mesh_packet).await;
            }
            from_radio::PayloadVariant::NodeInfo(node_info) => {
                let mesh_node = crate::handshake::node_info_to_mesh_node(node_info);
                self.node_db.lock().await.insert(mesh_node);
                tracing::debug!(node = node_info.num, "node info updated");
            }
            _ => {
                tracing::trace!(id = from_radio.id, "unhandled FromRadio variant");
            }
        }
    }

    /// Handles a received mesh packet by updating the node database.
    async fn handle_mesh_packet(&self, mesh_packet: &crate::proto::MeshPacket) {
        let node_num = NodeNum(mesh_packet.from);
        let snr = if mesh_packet.rx_snr == 0.0 {
            None
        } else {
            Some(mesh_packet.rx_snr)
        };
        let hop_count = Self::compute_hop_count(mesh_packet.hop_start, mesh_packet.hop_limit);

        let mut db = self.node_db.lock().await;
        if let Some(node) = db.get(node_num) {
            let mut updated = node.clone();
            updated.snr = snr.or(updated.snr);
            updated.hop_count = hop_count.or(updated.hop_count);
            updated.last_heard = Some(jiff::Timestamp::now());
            db.insert(updated);
        } else {
            db.insert(crate::node_db::MeshNode {
                num: node_num,
                user: None,
                position: None,
                metrics: None,
                last_heard: Some(jiff::Timestamp::now()),
                snr,
                hop_count,
            });
        }
        drop(db);

        tracing::debug!(
            from = mesh_packet.from,
            to = mesh_packet.to,
            id = mesh_packet.id,
            "mesh packet received"
        );
    }

    /// Connects to all configured transports and performs handshakes.
    ///
    /// Returns `Ok` with the list of active connections, which may be empty
    /// if all connections fail.
    async fn connect_and_handshake(&self) -> Result<Vec<Arc<Mutex<ConnectionHandle>>>, Error> {
        let mut connections: Vec<ConnectionHandle> = Vec::new();
        for conn_cfg in &self.config.connections {
            match transport::connect(conn_cfg).await {
                Ok(handle) => {
                    tracing::info!(config = ?conn_cfg, "transport connected");
                    connections.push(handle);
                }
                Err(e) => {
                    tracing::warn!(config = ?conn_cfg, error = %e, "transport connect failed");
                }
            }
        }

        let mut active: Vec<Arc<Mutex<ConnectionHandle>>> = Vec::new();
        for mut conn in connections {
            let mut db = self.node_db.lock().await;
            match handshake::handshake(&mut conn, &mut db).await {
                Ok(result) => {
                    tracing::info!(
                        my_node = %result.my_node_num,
                        nodes = result.known_nodes.len(),
                        channels = result.channels.len(),
                        "handshake complete"
                    );
                    drop(db);
                    active.push(Arc::new(Mutex::new(conn)));
                }
                Err(e) => {
                    drop(db);
                    tracing::warn!(error = %e, "handshake failed, skipping connection");
                }
            }
        }

        Ok(active)
    }

    /// Spawns all background tasks: heartbeat, gateway health monitor, discovery, router flush.
    fn spawn_background_tasks(
        &self,
        tasks: &mut JoinSet<Result<(), Error>>,
        connections: &[Arc<Mutex<ConnectionHandle>>],
        processor: &Arc<Mutex<PacketProcessor>>,
        tx: &broadcast::Sender<GeoSignal>,
        cancel: &CancellationToken,
    ) {
        // Heartbeat per connection.
        for conn in connections {
            let conn = Arc::clone(conn);
            let token = cancel.child_token();
            tasks.spawn(async move { heartbeat::run_heartbeat(&*conn, token).await });
        }

        // Gateway health monitor.
        let bridge = Arc::clone(&self.bridge);
        let token = cancel.child_token();
        tasks.spawn(async move { bridge::run_health_monitor(&bridge, token).await });

        // Discovery manager: periodic traceroutes + stale node detection.
        if let Some(primary) = connections.first() {
            let conn = Arc::clone(primary);
            let proc = Arc::clone(processor);
            let topo_cfg = self.config.topology.clone();
            let tx_clone = tx.clone();
            let token = cancel.child_token();
            tasks.spawn(async move {
                run_discovery(&*conn, &proc, &topo_cfg, &tx_clone, token).await;
                Ok(())
            });
        }

        // Router flush: drain outbound queue and process timeouts.
        let router = Arc::clone(&self.router);
        if let Some(primary) = connections.first() {
            let conn = Arc::clone(primary);
            let token = cancel.child_token();
            tasks.spawn(async move { run_router_flush(router, conn, token).await });
        }
    }
}

impl Collector for MeshCollector {
    fn name(&self) -> &'static str {
        "kerykeion"
    }

    async fn probe(&self) -> bool {
        if self.config.connections.is_empty() {
            return false;
        }

        for conn_cfg in &self.config.connections {
            match conn_cfg {
                crate::config::ConnectionConfig::Tcp { addr, port } => {
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        tokio::net::TcpStream::connect(format!("{addr}:{port}")),
                    )
                    .await
                    {
                        Ok(Ok(_)) => return true,
                        Ok(Err(e)) => tracing::debug!(addr, port, error = %e, "TCP probe failed"),
                        Err(_) => tracing::debug!(addr, port, "TCP probe timed out"),
                    }
                }
                crate::config::ConnectionConfig::Serial { port, .. } => {
                    if std::path::Path::new(port).exists() {
                        return true;
                    }
                }
                crate::config::ConnectionConfig::Ble { .. } => {}
            }
        }

        false
    }

    async fn run(
        &mut self,
        tx: broadcast::Sender<GeoSignal>,
        cancel: CancellationToken,
    ) -> Result<(), Error> {
        tracing::info!(
            collector = self.name(),
            connections = self.config.connections.len(),
            "starting mesh collector"
        );

        // 1 & 2. Connect transports and perform config handshake.
        let active_connections = self.connect_and_handshake().await?;
        if active_connections.is_empty() {
            tracing::warn!("no connections available, collector exiting");
            return Ok(());
        }

        // 3. Create packet processor (owns topology graph, emits GeoSignals).
        let processor = Arc::new(Mutex::new(PacketProcessor::new(
            NodeDb::new(),
            MeshTopology::new(),
            tx.clone(),
        )));

        // 4–7. Start heartbeat, gateway health, discovery, and router tasks.
        let mut tasks: JoinSet<Result<(), Error>> = JoinSet::new();
        self.spawn_background_tasks(&mut tasks, &active_connections, &processor, &tx, &cancel);

        tracing::info!("entering main receive loop");
        let primary_conn =
            Arc::clone(
                active_connections
                    .first()
                    .ok_or_else(|| Error::ConnectionLost {
                        detail: "no active connections".into(),
                        location: snafu::Location::new(file!(), line!(), column!()),
                    })?,
            );

        // 8. Main receive loop.
        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    tracing::info!("collector shutdown requested");
                    break;
                }
                result = async {
                    primary_conn.lock().await.recv().await
                } => {
                    match result {
                        Ok(from_radio) => {
                            // Update node_db for CLI display.
                            self.process_packet(&from_radio).await;
                            // Dispatch to PacketProcessor for topology + GeoSignal emission.
                            if let Some(from_radio::PayloadVariant::Packet(pkt)) =
                                &from_radio.payload_variant
                            {
                                processor.lock().await.process_mesh_packet(pkt);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "receive error");
                            if !primary_conn.lock().await.is_connected() {
                                tracing::error!("primary connection lost");
                                break;
                            }
                        }
                    }
                }
                Some(task_result) = tasks.join_next() => {
                    match task_result {
                        Ok(Ok(())) => tracing::debug!("background task completed"),
                        Ok(Err(e)) => tracing::warn!(error = %e, "background task error"),
                        Err(e) => tracing::warn!(error = %e, "background task panicked"),
                    }
                }
            }
        }

        // 9. Graceful shutdown: cancel children, drain router, disconnect.
        tracing::info!("shutting down collector");
        cancel.cancel();

        // Drain router timeouts before exit.
        self.router.lock().await.process_timeouts();

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::debug!(error = %e, "task error during shutdown"),
                Err(e) => tracing::debug!(error = %e, "task panic during shutdown"),
            }
        }

        tracing::info!("collector shutdown complete");
        Ok(())
    }
}

/// Periodically drains the outbound queue and processes message timeouts.
///
/// Sends pending messages via `conn`, handles inflight timeouts,
/// and retries or marks messages failed as appropriate.
///
/// # Cancellation Safety
///
/// Exits cleanly when `token` is cancelled at the next iteration boundary.
async fn run_router_flush<C>(
    router: Arc<Mutex<MeshRouter>>,
    conn: Arc<Mutex<C>>,
    token: CancellationToken,
) -> Result<(), Error>
where
    C: crate::connection::MeshConnection,
{
    use crate::proto::{ToRadio, to_radio};

    let mut interval = tokio::time::interval(ROUTER_FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            () = token.cancelled() => {
                tracing::debug!("router flush task cancelled");
                return Ok(());
            }
            _ = interval.tick() => {
                // Collect pending packets under lock, then send outside lock.
                let pending = {
                    let mut r = router.lock().await;
                    r.process_timeouts();
                    let mut packets = Vec::new();
                    while let Some(msg) = r.next_to_send() {
                        let packet = msg.packet.clone();
                        r.track_sent(msg);
                        packets.push(packet);
                    }
                    drop(r);
                    packets
                };

                for packet in pending {
                    let to_radio = ToRadio {
                        payload_variant: Some(to_radio::PayloadVariant::Packet(packet)),
                    };
                    if let Err(e) = conn.lock().await.send(to_radio).await {
                        tracing::warn!(error = %e, "router flush: send error");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};
    use tracing::Instrument as _;

    fn make_config(connections: Vec<ConnectionConfig>) -> MeshConfig {
        MeshConfig {
            connections,
            channel_psk: vec![],
            store_forward: StoreForwardConfig::default(),
            topology: TopologyConfig::default(),
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

        let handle = tokio::spawn(async move { run_router_flush(router, conn, task_token).await }.instrument(tracing::info_span!("spawned_task")));

        // Cancel immediately  -  biased SELECT exits before first tick.
        token.cancel();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handle.await.unwrap();
        assert!(result.is_ok(), "router flush should cancel cleanly");
    }
}
