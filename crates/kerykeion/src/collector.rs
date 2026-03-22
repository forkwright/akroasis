//! `MeshCollector` — kerykeion integration point for the Akroasis collection pipeline.
//!
//! Wires together all kerykeion components: transport connections, config handshake,
//! heartbeat keepalive, gateway health monitoring, node database updates, and
//! packet processing into a single collection loop driven by a `CancellationToken`.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::bridge::{self, GatewayBridge};
use crate::config::MeshConfig;
use crate::connection::MeshConnection;
use crate::error::Error;
use crate::handshake;
use crate::heartbeat;
use crate::node_db::NodeDb;
use crate::proto::{FromRadio, from_radio};
use crate::transport::{self, ConnectionHandle};

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
    /// Returns when the collector has shut down cleanly or a fatal error occurs.
    ///
    /// # Errors
    ///
    /// Returns an error if the collector encounters a fatal, unrecoverable failure.
    fn run(
        &mut self,
        cancel: CancellationToken,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send;
}

/// Meshtastic mesh networking collector.
///
/// Manages connections to one or more Meshtastic radios, receives mesh packets,
/// maintains the node database and gateway bridge, and forwards observations
/// into the Akroasis pipeline.
pub struct MeshCollector {
    config: MeshConfig,
    node_db: Arc<Mutex<NodeDb>>,
    bridge: Arc<Mutex<GatewayBridge>>,
}

impl MeshCollector {
    /// Creates a new `MeshCollector` with the given configuration.
    #[must_use]
    pub fn new(config: MeshConfig) -> Self {
        Self {
            config,
            node_db: Arc::new(Mutex::new(NodeDb::new())),
            bridge: Arc::new(Mutex::new(GatewayBridge::new())),
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

    /// Computes hop count from packet hop fields.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "hop values are bounded by MAX_HOP_LIMIT (7) in Meshtastic firmware"
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
        let node_num = crate::types::NodeNum(mesh_packet.from);
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
    /// # Errors
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

    /// Spawns background tasks (heartbeat, gateway health monitor).
    fn spawn_background_tasks(
        &self,
        tasks: &mut tokio::task::JoinSet<Result<(), Error>>,
        connections: &[Arc<Mutex<ConnectionHandle>>],
        cancel: &CancellationToken,
    ) {
        for conn in connections {
            let conn = Arc::clone(conn);
            let token = cancel.child_token();
            tasks.spawn(async move { heartbeat::run_heartbeat(&*conn, token).await });
        }

        let bridge = Arc::clone(&self.bridge);
        let token = cancel.child_token();
        tasks.spawn(async move { bridge::run_health_monitor(&bridge, token).await });
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

    async fn run(&mut self, cancel: CancellationToken) -> Result<(), Error> {
        tracing::info!(
            collector = self.name(),
            connections = self.config.connections.len(),
            "starting mesh collector"
        );

        let active_connections = self.connect_and_handshake().await?;
        if active_connections.is_empty() {
            tracing::warn!("no connections available, collector exiting");
            return Ok(());
        }

        let mut tasks = tokio::task::JoinSet::new();
        self.spawn_background_tasks(&mut tasks, &active_connections, &cancel);

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
                        Ok(from_radio) => self.process_packet(&from_radio).await,
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

        tracing::info!("shutting down collector");
        cancel.cancel();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};

    fn make_config(connections: Vec<ConnectionConfig>) -> MeshConfig {
        MeshConfig {
            connections,
            channel_psk: vec![],
            store_forward: StoreForwardConfig::default(),
            topology: TopologyConfig::default(),
        }
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
        let result = c.run(token).await;
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
}
