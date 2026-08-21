//! `MeshCollector`  -  kerykeion integration point for the Akroasis collection pipeline.
//!
//! Wires together all kerykeion components: transport connections, config handshake,
//! packet processor, discovery manager, heartbeat keepalive, gateway health monitoring,
//! router background task, and signal emission INTO a single collection loop driven
//! by a `CancellationToken`.

use std::sync::Arc;
use std::time::Duration;

use stoicheion::GeoSignal;
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument as _, instrument};

use crate::bridge::{self, GatewayBridge};
use crate::config::MeshConfig;
use crate::connection::MeshConnection;
use crate::delivery::DeliveryTracker;
use crate::discovery::run_discovery;
use crate::error::{Error, HandshakeFailedSnafu};
use crate::handshake;
use crate::heartbeat;
use crate::node_db::NodeDb;
use crate::outbound::OutboundQueue;
use crate::processor::{PacketProcessor, RoutingProcessor, RoutingResult};
use crate::proto::{FromRadio, from_radio};
use crate::router::MeshRouter;
use crate::store_forward::StoreForward;
use crate::topology::MeshTopology;
use crate::transport::{self, ConnectionHandle};
use crate::types::ClaimedNodeNum;

// Historical default (1 s) now lives in [`CollectorConfig::default`].

/// Trait for Akroasis data collectors.
///
/// Defines the minimal lifecycle interface shared across all collector crates.
/// This is a local definition pending a shared one. Which crate it belongs in
/// is genuinely open: a collector produces signals, which is `stoicheion`'s
/// layer, but it is a pipeline role rather than an element, so it may belong
/// in neither. Left local until something forces the choice.
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
    ///
    /// Threads the sub-configs ([`crate::config::OutboundConfig`],
    /// [`crate::config::BridgeConfig`], [`crate::config::StoreForwardConfig`])
    /// into the router, bridge, and store-and-forward components so tuning
    /// applied via TOML / agent overrides takes effect.
    #[must_use]
    pub fn new(config: MeshConfig) -> Self {
        let sf_config = config.store_forward.clone();
        let outbound_cfg = config.outbound.clone();
        let bridge_cfg = config.bridge.clone();
        Self {
            node_db: Arc::new(Mutex::new(NodeDb::new())),
            bridge: Arc::new(Mutex::new(GatewayBridge::with_config(bridge_cfg))),
            router: Arc::new(Mutex::new(MeshRouter::with_config(
                OutboundQueue::with_config(&outbound_cfg),
                StoreForward::new(sf_config),
                DeliveryTracker::new(),
                &outbound_cfg,
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
    ///
    /// Delegates to [`crate::types::hop_count_from_wire`] so the bound lives in
    /// one place; see there for why the fields cannot be trusted to hold it.
    fn compute_hop_count(hop_start: u32, hop_limit: u32) -> Option<u8> {
        crate::types::hop_count_from_wire(hop_start, hop_limit)
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
    ///
    /// `mesh_packet.from` is the sender this layer actually received the
    /// packet attributed to — the strongest identity signal available here —
    /// but it is an unauthenticated claim, not a verified fact (see
    /// [`ClaimedNodeNum`]). A packet claiming the non-node sentinels (`0` or
    /// broadcast) is dropped before it can create or update a node-DB entry
    /// (#246).
    async fn handle_mesh_packet(&self, mesh_packet: &crate::proto::MeshPacket) {
        let Some(node_num) =
            ClaimedNodeNum::from_wire(mesh_packet.from).map(ClaimedNodeNum::accept_unauthenticated)
        else {
            tracing::trace!(
                from = mesh_packet.from,
                "ignoring mesh packet with sentinel `from` (unset or broadcast)"
            );
            return;
        };
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

    /// Applies inbound `ROUTING_APP` ACK/NAK results to the shared router state.
    ///
    /// A NAK either schedules a retry (message re-enqueued, still inflight
    /// budget remaining) or, once retries are exhausted, marks the delivery
    /// record failed  -  both are [`DeliveryTracker`]'s existing semantics via
    /// [`RoutingProcessor::apply_routing_result`].
    async fn dispatch_routing(&self, mesh_packet: &crate::proto::MeshPacket) {
        let routing_result = RoutingProcessor::process_routing(mesh_packet);
        if routing_result == RoutingResult::NotRouting {
            return;
        }

        let mut guard = self.router.lock().await;
        let router = &mut *guard;
        RoutingProcessor::apply_routing_result(
            &routing_result,
            &mut router.delivery,
            &mut router.outbound,
        );
        drop(guard);
    }

    /// Dispatches a decoded `FromRadio` message to the `PacketProcessor`.
    ///
    /// Mesh packets are routed to [`PacketProcessor::process_mesh_packet`] for
    /// topology + `GeoSignal` emission. Top-level `NodeInfo` frames never pass
    /// through that decode path (`process_mesh_packet` only sees `NODEINFO_APP`
    /// payloads carried inside a `MeshPacket`), so they are mirrored directly
    /// into the processor's `NodeDb` here  -  otherwise a node learned only via
    /// a runtime `NodeInfo` frame stays invisible to the processor (#198).
    async fn dispatch_to_processor(
        &self,
        from_radio: &FromRadio,
        processor: &Arc<Mutex<PacketProcessor>>,
    ) {
        match &from_radio.payload_variant {
            Some(from_radio::PayloadVariant::Packet(pkt)) => {
                processor.lock().await.process_mesh_packet(pkt);
                // Inbound ACK/NAK: update the delivery tracker + outbound queue.
                self.dispatch_routing(pkt).await;
            }
            Some(from_radio::PayloadVariant::NodeInfo(node_info)) => {
                let mesh_node = crate::handshake::node_info_to_mesh_node(node_info);
                processor.lock().await.node_db_mut().insert(mesh_node);
            }
            _ => {} // WHY: every other variant is already logged by `process_packet`, which runs unconditionally before this call — nothing else concerns the processor.
        }
    }

    /// Builds the `PacketProcessor`, seeded with every node already known to
    /// the collector (typically handshake-discovered nodes) so the processor
    /// does not start blind to nodes learned before its construction (#198).
    async fn make_processor(
        &self,
        tx: broadcast::Sender<GeoSignal>,
    ) -> Arc<Mutex<PacketProcessor>> {
        let seeded = self.node_db.lock().await.clone();
        Arc::new(Mutex::new(PacketProcessor::new(
            seeded,
            MeshTopology::new(),
            tx,
        )))
    }

    /// Connects to all configured transports and performs handshakes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::HandshakeFailed`] if connections are configured but
    /// none of them reached a completed handshake.
    async fn connect_and_handshake(&self) -> Result<Vec<Arc<Mutex<ConnectionHandle>>>, Error> {
        let mut connections: Vec<ConnectionHandle> = Vec::new();
        for conn_cfg in &self.config.connections {
            match transport::connect_with_config(conn_cfg, &self.config.transport).await {
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
            // WHY: the handshake awaits a full config dump from the radio, which
            // is unbounded network time. Holding the shared node_db across it
            // stalls every other task that needs the DB for exactly that long.
            // The handshake only needs somewhere to accumulate, and
            // `HandshakeResult` already carries everything it wrote, so give it
            // a scratch DB and merge under a short lock afterwards.
            let mut scratch = NodeDb::new();
            match handshake::handshake_with_config(&mut conn, &mut scratch, &self.config.handshake)
                .await
            {
                Ok(result) => {
                    tracing::info!(
                        my_node = %result.my_node_num,
                        nodes = result.known_nodes.len(),
                        channels = result.channels.len(),
                        "handshake complete"
                    );
                    let mut db = self.node_db.lock().await;
                    db.set_my_node(result.my_node_num);
                    for node in result.known_nodes {
                        db.insert(node);
                    }
                    drop(db);
                    active.push(Arc::new(Mutex::new(conn)));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "handshake failed, skipping connection");
                }
            }
        }

        // WHY: with connections configured and none active, every transport or
        // handshake failed. Returning Ok here spawns the whole background task
        // set against no radio: the collector then runs indefinitely observing
        // nothing while reporting success, which is indistinguishable from a
        // quiet mesh. Fail instead so the caller can retry or surface it.
        if active.is_empty() && !self.config.connections.is_empty() {
            return HandshakeFailedSnafu {
                detail: format!(
                    "all {} configured connections failed to connect or handshake",
                    self.config.connections.len()
                ),
            }
            .fail();
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
            let heartbeat_cfg = self.config.heartbeat.clone();
            tasks.spawn(
                async move {
                    heartbeat::run_heartbeat_with_config(&*conn, &heartbeat_cfg, token).await
                }
                .instrument(tracing::info_span!("kerykeion.heartbeat")),
            );
        }

        // Gateway health monitor.
        let bridge = Arc::clone(&self.bridge);
        let token = cancel.child_token();
        tasks.spawn(
            async move { bridge::run_health_monitor(&bridge, token).await }
                .instrument(tracing::info_span!("kerykeion.gateway_health")),
        );

        // Discovery manager: periodic traceroutes + stale node detection.
        if let Some(primary) = connections.first() {
            let conn = Arc::clone(primary);
            let proc = Arc::clone(processor);
            let topo_cfg = self.config.topology.clone();
            let tx_clone = tx.clone();
            let token = cancel.child_token();
            tasks.spawn(
                async move {
                    run_discovery(&*conn, &proc, &topo_cfg, &tx_clone, token).await;
                    Ok(())
                }
                .instrument(tracing::info_span!("kerykeion.discovery")),
            );
        }

        // Router flush: drain outbound queue and process timeouts.
        let router = Arc::clone(&self.router);
        if let Some(primary) = connections.first() {
            let conn = Arc::clone(primary);
            let token = cancel.child_token();
            let flush_interval = self.config.collector.router_flush_interval();
            tasks.spawn(
                async move { run_router_flush(router, conn, flush_interval, token).await }
                    .instrument(tracing::info_span!("kerykeion.router_flush")),
            );
        }
    }
}

#[rustfmt::skip]
impl Collector for MeshCollector { // kanon:ignore ARCHITECTURE/trait-impl-colocation -- Collector is a kerykeion-defined abstraction; MeshCollector is the only implementation
    fn name(&self) -> &'static str {
        "kerykeion"
    }

    #[instrument(
        level = "debug",
        skip(self),
        fields(connections = self.config.connections.len())
    )]
    async fn probe(&self) -> bool {
        if self.config.connections.is_empty() {
            return false;
        }

        for conn_cfg in &self.config.connections {
            match conn_cfg {
                crate::config::ConnectionConfig::Tcp { addr, port } => {
                    match tokio::time::timeout(
                        self.config.transport.tcp_connect_timeout(),
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

    #[instrument(
        level = "debug",
        skip(self, tx, cancel),
        fields(connections = self.config.connections.len())
    )]
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

        // 3. Create packet processor (owns topology graph, emits GeoSignals),
        // seeded with every node the handshake already discovered so it does
        // not start blind to nodes learned before this point (#198).
        let processor = self.make_processor(tx.clone()).await;

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
                        location: snafu::location!(),
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
                result = recv_yielding(&*primary_conn) => {
                    match result {
                        Ok(from_radio) => {
                            // Update node_db for CLI display.
                            self.process_packet(&from_radio).await;
                            // Dispatch to PacketProcessor for topology + GeoSignal emission.
                            self.dispatch_to_processor(&from_radio, &processor).await;
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
                    if supervise_task_result(task_result) == TaskOutcome::Shutdown {
                        tracing::error!("collector shutting down: a background task subsystem was lost");
                        cancel.cancel();
                        break;
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

/// What the main receive loop should do after observing a background task's
/// join result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskOutcome {
    /// The task exited cleanly (or is merely being logged); keep running.
    Continue,
    /// The task panicked or returned an error; the collector must not keep
    /// running silently degraded with that subsystem gone.
    Shutdown,
}

/// Classify a background task's [`JoinSet::join_next`] result and log it.
///
/// WHY(#205): heartbeat, gateway health, discovery, and router-flush used to
/// be logged-and-ignored on panic or error, leaving the collector running --
/// and reporting healthy -- with a subsystem permanently gone. Losing
/// router-flush in particular silently stops all outbound sending while the
/// collector keeps receiving, so the operator has no signal that the tool
/// has stopped doing half its job. Every background task is now supervised
/// uniformly: any panic or error escalates to a clean shutdown rather than
/// an indefinite silent partial failure.
fn supervise_task_result(result: Result<Result<(), Error>, tokio::task::JoinError>) -> TaskOutcome {
    match result {
        Ok(Ok(())) => {
            tracing::debug!("background task completed");
            TaskOutcome::Continue
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "background task returned an error");
            TaskOutcome::Shutdown
        }
        Err(e) => {
            tracing::error!(error = %e, "background task panicked");
            TaskOutcome::Shutdown
        }
    }
}

/// Bound on how long [`recv_yielding`] holds the connection lock during a
/// single `recv()` poll before releasing it.
const RECV_LOCK_YIELD: Duration = Duration::from_millis(250);

/// Awaits the next inbound frame on `conn` without holding its lock for the
/// full, potentially-indefinite duration of `MeshConnection::recv()`.
///
/// akroasis#190: on a quiet mesh `recv()` blocks until the next frame
/// arrives  -  unbounded. The previous `conn.lock().await.recv().await`
/// pattern held the `MutexGuard` across that whole wait, starving every
/// send-only task sharing the same `Arc<Mutex<ConnectionHandle>>`  -  heartbeat,
/// discovery, and router-flush all only ever need the lock briefly to
/// `send()`. This polls `recv()` in `RECV_LOCK_YIELD`-bounded windows,
/// dropping the guard between polls so a waiting sender is guaranteed a
/// turn (`tokio::sync::Mutex` is FIFO-fair: a task already queued on
/// `lock()` is served before a fresh, uncontended re-lock from this loop).
///
/// # Cancellation safety
///
/// Both transports read through a `Framed`, whose internal buffer persists
/// across a cancelled poll, so a timed-out window loses no already-buffered
/// bytes  -  the next iteration resumes cleanly.
async fn recv_yielding<C>(conn: &Mutex<C>) -> Result<FromRadio, Error>
where
    C: MeshConnection,
{
    loop {
        let mut guard = conn.lock().await;
        match tokio::time::timeout(RECV_LOCK_YIELD, guard.recv()).await {
            Ok(result) => return result,
            Err(_elapsed) => drop(guard),
        }
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
#[instrument(
    level = "debug",
    skip(router, conn, token),
    fields(tick_interval_ms = tick_interval.as_millis())
)]
async fn run_router_flush<C>(
    router: Arc<Mutex<MeshRouter>>,
    conn: Arc<Mutex<C>>,
    tick_interval: Duration,
    token: CancellationToken,
) -> Result<(), Error>
where
    C: crate::connection::MeshConnection,
{
    use crate::proto::{ToRadio, to_radio};

    let mut interval = tokio::time::interval(tick_interval);
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
                    let expired = r.run_maintenance();
                    if !expired.is_empty() {
                        tracing::debug!(
                            count = expired.len(),
                            "router flush: expired stale delivery records"
                        );
                    }
                    let mut msgs = Vec::new();
                    while let Some(msg) = r.next_to_send() {
                        msgs.push(msg);
                    }
                    drop(r);
                    msgs
                };

                for msg in pending {
                    let to_radio = ToRadio {
                        payload_variant: Some(to_radio::PayloadVariant::Packet(msg.packet.clone())),
                    };
                    if let Err(e) = conn.lock().await.send(to_radio).await {
                        tracing::warn!(error = %e, "router flush: send error");
                    }
                    // WHY: track_sent starts the inflight ACK timeout. Starting
                    // it before the transmit charges the radio's send latency
                    // and any wait on the connection lock against the peer's
                    // time to ACK, so a slow link retries messages that were
                    // never actually late. Tracked even after a send error, so
                    // the existing timeout/retry path owns the failure rather
                    // than the message being dropped here.
                    router.lock().await.track_sent(msg);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "collector_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "collector_tests_attribution.rs"]
mod tests_attribution;
