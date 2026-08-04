//! Gateway bridge  -  routes messages between mesh and internet services.
//!
//! Manages multi-gateway failover with health monitoring. Gateway selection
//! follows a priority hierarchy: dedicated gateway (RAK2245), WiFi-capable
//! mesh node, MQTT-bridged node, then multi-hop relay chain.
//!
//! # Tuning
//!
//! Behavioral thresholds (health check cadence, degraded/offline cutoffs,
//! failover cooldown) are grouped in [`crate::config::BridgeConfig`]. The
//! historical hard-coded values are retained as public `const`s so callers
//! that do not wish to tune can continue to reference them, and as the
//! single source of truth for [`BridgeConfig::default`].

use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::config::BridgeConfig;
use crate::error::Error;
use crate::types::NodeNum;

// Historical default values now live in the [`BridgeConfig::default`] impl.
// See `crate::config::BridgeConfig` for authoritative defaults:
//   health_check_interval_secs   = 60
//   degraded_response_threshold_ms = 5000
//   degraded_loss_threshold      = 0.20
//   offline_check_threshold      = 3
//   failover_cooldown_secs       = 30

/// Health state of a gateway node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GatewayHealth {
    /// Gateway is responding within acceptable latency and loss thresholds.
    Healthy,
    /// Gateway is responding but with degraded performance.
    Degraded {
        /// Human-readable reason for degradation.
        reason: String,
    },
    /// Gateway has not responded for multiple consecutive checks.
    Offline {
        /// When the gateway was last seen healthy or degraded.
        #[serde(skip)]
        since: Option<Instant>,
    },
}

/// Tracks the state of a single gateway node.
#[derive(Debug)]
pub struct GatewayState {
    /// Node number of the gateway.
    pub node: NodeNum,
    /// Lower VALUES are preferred during selection.
    pub priority: u8,
    /// When this gateway was last heard FROM.
    pub last_seen: Instant,
    /// Current health assessment.
    pub health: GatewayHealth,
    /// Number of consecutive failed health checks.
    pub consecutive_failures: u32,
    /// Average response time FROM recent pings.
    pub avg_response_ms: Option<f64>,
    /// Packet loss ratio FROM recent checks (0.0–1.0).
    pub packet_loss: f32,
}

impl GatewayState {
    /// Creates a new gateway state with the given node number and priority.
    #[must_use]
    pub fn new(node: NodeNum, priority: u8) -> Self {
        Self {
            node,
            priority,
            last_seen: Instant::now(),
            health: GatewayHealth::Healthy,
            consecutive_failures: 0,
            avg_response_ms: None,
            packet_loss: 0.0,
        }
    }

    /// Returns `true` if this gateway is available for traffic.
    #[must_use]
    pub const fn is_available(&self) -> bool {
        !matches!(self.health, GatewayHealth::Offline { .. })
    }
}

/// Events emitted by the gateway bridge during health and failover transitions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum GatewayEvent {
    /// Active gateway changed due to failover.
    Failover {
        /// Previous active gateway, if any.
        from: Option<NodeNum>,
        /// New active gateway.
        to: NodeNum,
    },
    /// A gateway's health state changed.
    StatusChange {
        /// Gateway whose status changed.
        node: NodeNum,
        /// New health state.
        health: GatewayHealth,
    },
}

/// Routes messages between mesh and internet services via gateway nodes.
///
/// Maintains a list of known gateways, monitors their health, and provides
/// automatic failover when the active gateway becomes unreachable.
#[derive(Debug)]
pub struct GatewayBridge {
    gateways: Vec<GatewayState>,
    active_gateway: Option<NodeNum>,
    last_failover: Option<Instant>,
    events: Vec<GatewayEvent>,
    config: BridgeConfig,
    config_tx: watch::Sender<BridgeConfig>,
}

impl GatewayBridge {
    /// Creates a new bridge with no gateways registered and default tuning.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(BridgeConfig::default())
    }

    /// Creates a new bridge with the supplied tuning configuration.
    #[must_use]
    pub fn with_config(config: BridgeConfig) -> Self {
        let (config_tx, _config_rx) = watch::channel(config.clone());
        Self {
            gateways: Vec::new(),
            active_gateway: None,
            last_failover: None,
            events: Vec::new(),
            config,
            config_tx,
        }
    }

    /// Returns a reference to the active tuning configuration.
    #[must_use]
    pub const fn config(&self) -> &BridgeConfig {
        &self.config
    }

    /// Replaces the active bridge tuning configuration and notifies monitors.
    pub fn update_config(&mut self, config: BridgeConfig) {
        self.config = config.clone();
        self.config_tx.send_replace(config);
    }

    fn subscribe_config(&self) -> watch::Receiver<BridgeConfig> {
        self.config_tx.subscribe()
    }

    /// Registers a gateway node with the given priority.
    ///
    /// Lower priority VALUES are preferred during selection.
    pub fn add_gateway(&mut self, node: NodeNum, priority: u8) {
        if !self.gateways.iter().any(|g| g.node == node) {
            self.gateways.push(GatewayState::new(node, priority));
        }
    }

    /// Removes a gateway node FROM the registry.
    pub fn remove_gateway(&mut self, node: NodeNum) {
        self.gateways.retain(|g| g.node != node);
        if self.active_gateway == Some(node) {
            self.active_gateway = None;
        }
    }

    /// Returns the currently active gateway node, if any.
    #[must_use]
    pub const fn active(&self) -> Option<NodeNum> {
        self.active_gateway
    }

    /// Returns a slice of all registered gateway states.
    #[must_use]
    pub fn gateways(&self) -> &[GatewayState] {
        &self.gateways
    }

    /// Drains and returns any pending gateway events.
    pub fn drain_events(&mut self) -> Vec<GatewayEvent> {
        std::mem::take(&mut self.events)
    }

    /// Selects the best available gateway by priority.
    ///
    /// Returns the node number of the healthiest gateway with the lowest
    /// priority value. Returns `None` if no gateways are available.
    #[must_use]
    pub fn select_gateway(&self) -> Option<NodeNum> {
        self.gateways
            .iter()
            .filter(|g| g.is_available())
            .min_by_key(|g| (health_rank(&g.health), g.priority))
            .map(|g| g.node)
    }

    /// Ensures the active gateway is the best available one.
    ///
    /// If no gateway is active or the current one is offline, selects the
    /// best available gateway and emits a failover event.
    pub fn ensure_active(&mut self) {
        let needs_selection = match self.active_gateway {
            None => true,
            Some(active) => !self
                .gateways
                .iter()
                .any(|g| g.node == active && g.is_available()),
        };

        if needs_selection {
            if let Some(cooldown) = self.last_failover {
                if cooldown.elapsed() < self.config.failover_cooldown() {
                    tracing::debug!(
                        "gateway reselection needed but suppressed by failover cooldown"
                    );
                    return;
                }
            }
            self.failover();
        }
    }

    /// Forces a failover to the next best available gateway.
    ///
    /// Emits a [`GatewayEvent::Failover`] if a new gateway is selected.
    pub fn failover(&mut self) {
        let previous = self.active_gateway;
        let next = self.select_gateway();

        if next != previous {
            self.active_gateway = next;
            self.last_failover = Some(Instant::now());

            if let Some(to) = next {
                tracing::warn!(
                    FROM = ?previous,
                    to = %to,
                    "gateway failover"
                );
                self.events
                    .push(GatewayEvent::Failover { from: previous, to });
            } else {
                tracing::error!("no gateway available after failover");
            }
        }
    }

    /// Records a successful health check response for a gateway.
    #[expect(
        clippy::cast_precision_loss,
        reason = "millis value fits in f64 without precision loss"
    )]
    pub fn record_health_success(&mut self, node: NodeNum, response_ms: f64) {
        let Some(gw) = self.gateways.iter_mut().find(|g| g.node == node) else {
            return;
        };

        gw.last_seen = Instant::now();
        gw.consecutive_failures = 0;
        gw.avg_response_ms = Some(response_ms);
        // WHY: decay packet loss toward zero on successful response.
        gw.packet_loss *= 0.5;

        let threshold_ms = self.config.degraded_response_threshold().as_millis() as f64; // SAFETY: degraded_response_threshold() returns Duration with ms value that fits any f64 exactly
        let new_health =
            if response_ms > threshold_ms || gw.packet_loss > self.config.degraded_loss_threshold {
                GatewayHealth::Degraded {
                    reason: format!(
                        "response {response_ms:.0}ms, loss {:.0}%",
                        gw.packet_loss * 100.0
                    ),
                }
            } else {
                GatewayHealth::Healthy
            };

        if std::mem::discriminant(&gw.health) != std::mem::discriminant(&new_health) {
            tracing::info!(node = %node, health = ?new_health, "gateway health transition");
            self.events.push(GatewayEvent::StatusChange {
                node,
                health: new_health.clone(),
            });
        }
        gw.health = new_health;
    }

    /// Records a failed health check for a gateway.
    ///
    /// After [`BridgeConfig::offline_check_threshold`] consecutive failures,
    /// the gateway transitions to [`GatewayHealth::Offline`] and an
    /// automatic failover is triggered if it was the active gateway.
    pub fn record_health_failure(&mut self, node: NodeNum) {
        let offline_threshold = self.config.offline_check_threshold;
        let Some(gw) = self.gateways.iter_mut().find(|g| g.node == node) else {
            return;
        };

        gw.consecutive_failures += 1;
        gw.packet_loss = gw.packet_loss.mul_add(0.7, 0.3);

        let was_available = gw.is_available();

        if gw.consecutive_failures >= offline_threshold {
            let new_health = GatewayHealth::Offline {
                since: Some(Instant::now()),
            };
            if std::mem::discriminant(&gw.health) != std::mem::discriminant(&new_health) {
                tracing::warn!(node = %node, failures = gw.consecutive_failures, "gateway offline");
                self.events.push(GatewayEvent::StatusChange {
                    node,
                    health: new_health.clone(),
                });
            }
            gw.health = new_health;
        } else if !matches!(gw.health, GatewayHealth::Degraded { .. }) {
            let new_health = GatewayHealth::Degraded {
                reason: format!("{} consecutive check failures", gw.consecutive_failures),
            };
            tracing::info!(node = %node, health = ?new_health, "gateway health transition");
            self.events.push(GatewayEvent::StatusChange {
                node,
                health: new_health.clone(),
            });
            gw.health = new_health;
        }

        // WHY: trigger failover if the active gateway just went offline.
        if was_available && !gw.is_available() && self.active_gateway == Some(node) {
            self.failover();
        }
    }

    /// Updates packet loss ratio for a gateway based on observed delivery rate.
    pub fn update_packet_loss(&mut self, node: NodeNum, loss_ratio: f32) {
        if let Some(gw) = self.gateways.iter_mut().find(|g| g.node == node) {
            gw.packet_loss = loss_ratio.clamp(0.0, 1.0);
        }
    }
}

impl Default for GatewayBridge {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs the gateway health monitor as a background task.
///
/// Periodically checks all registered gateways and updates their health state.
/// Uses the bridge's internal health recording methods to trigger transitions
/// and failovers.
///
/// # Cancellation Safety
///
/// This function is cancel-safe. It checks the cancellation token at each
/// iteration boundary and exits cleanly when cancelled.
///
/// # Errors
///
/// Returns [`Error::SendFailed`] if the bridge encounters an unrecoverable error.
#[instrument(level = "debug", skip(bridge, token))]
pub async fn run_health_monitor(
    bridge: &tokio::sync::Mutex<GatewayBridge>,
    token: CancellationToken,
) -> Result<(), Error> {
    let mut config_rx = bridge.lock().await.subscribe_config();
    let mut tick_interval = config_rx.borrow().health_check_interval();
    let mut interval = tokio::time::interval(tick_interval);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;
            () = token.cancelled() => {
                tracing::debug!("gateway health monitor cancelled");
                return Ok(());
            }
            changed = config_rx.changed() => {
                if changed.is_err() {
                    tracing::debug!("gateway health monitor config channel closed");
                    return Ok(());
                }

                let next_interval = config_rx.borrow_and_update().health_check_interval();
                if next_interval != tick_interval {
                    tick_interval = next_interval;
                    interval = tokio::time::interval(tick_interval);
                    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    tracing::debug!(
                        interval_secs = tick_interval.as_secs(),
                        "gateway health monitor interval updated"
                    );
                }
            }
            _ = interval.tick() => {
                let nodes: Vec<NodeNum> = {
                    let b = bridge.lock().await;
                    b.gateways.iter().map(|g| g.node).collect()
                };

                for node in nodes {
                    // WHY: actual ping would require a connection reference. For now,
                    // health is updated externally when ack/nack packets arrive.
                    // This loop ensures the bridge re-evaluates active gateway.
                    bridge.lock().await.ensure_active();
                    tracing::trace!(node = %node, "health check tick");
                }
            }
        }
    }
}

/// Ranks health states for sorting: Healthy < Degraded < Offline.
const fn health_rank(health: &GatewayHealth) -> u8 {
    match health {
        GatewayHealth::Healthy => 0,
        GatewayHealth::Degraded { .. } => 1,
        GatewayHealth::Offline { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tracing::Instrument as _;

    use super::*;

    fn first_health(bridge: &GatewayBridge) -> &GatewayHealth {
        #[expect(clippy::indexing_slicing, reason = "test-only: first gateway exists")]
        &bridge.gateways[0].health
    }

    #[test]
    fn select_gateway_by_priority() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 3);
        bridge.add_gateway(NodeNum(2), 1);
        bridge.add_gateway(NodeNum(3), 2);

        assert_eq!(
            bridge.select_gateway(),
            Some(NodeNum(2)),
            "should SELECT lowest priority"
        );
    }

    #[test]
    fn select_gateway_skips_offline() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);
        bridge.add_gateway(NodeNum(2), 2);

        // WHY: mark node 1 offline via consecutive failures.
        for _ in 0..BridgeConfig::default().offline_check_threshold {
            bridge.record_health_failure(NodeNum(1));
        }

        assert_eq!(
            bridge.select_gateway(),
            Some(NodeNum(2)),
            "should skip offline gateway"
        );
    }

    #[test]
    fn select_gateway_prefers_healthy_over_degraded() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);
        bridge.add_gateway(NodeNum(2), 2);

        // WHY: degrade node 1 by recording one failure.
        bridge.record_health_failure(NodeNum(1));

        assert_eq!(
            bridge.select_gateway(),
            Some(NodeNum(2)),
            "should prefer healthy gateway over degraded"
        );
    }

    #[test]
    fn failover_emits_event() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);
        bridge.add_gateway(NodeNum(2), 2);
        bridge.active_gateway = Some(NodeNum(1));

        // WHY: force node 1 offline to trigger failover.
        for _ in 0..BridgeConfig::default().offline_check_threshold {
            bridge.record_health_failure(NodeNum(1));
        }

        let events = bridge.drain_events();
        let has_failover = events
            .iter()
            .any(|e| matches!(e, GatewayEvent::Failover { to, .. } if *to == NodeNum(2)));
        assert!(has_failover, "should emit failover event to node 2");
        assert_eq!(bridge.active(), Some(NodeNum(2)));
    }

    #[test]
    fn health_transitions_healthy_to_degraded_to_offline() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);

        assert!(
            matches!(first_health(&bridge), GatewayHealth::Healthy),
            "initial state should be healthy"
        );

        bridge.record_health_failure(NodeNum(1));
        assert!(
            matches!(first_health(&bridge), GatewayHealth::Degraded { .. }),
            "single failure should degrade"
        );

        for _ in 1..BridgeConfig::default().offline_check_threshold {
            bridge.record_health_failure(NodeNum(1));
        }
        assert!(
            matches!(first_health(&bridge), GatewayHealth::Offline { .. }),
            "should go offline after threshold"
        );
    }

    #[test]
    fn health_success_restores_to_healthy() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);

        bridge.record_health_failure(NodeNum(1));
        assert!(matches!(
            first_health(&bridge),
            GatewayHealth::Degraded { .. }
        ));

        bridge.record_health_success(NodeNum(1), 100.0);
        assert!(
            matches!(first_health(&bridge), GatewayHealth::Healthy),
            "success with good latency should restore healthy"
        );
    }

    #[test]
    fn no_gateways_returns_none() {
        let bridge = GatewayBridge::new();
        assert_eq!(bridge.select_gateway(), None);
        assert_eq!(bridge.active(), None);
    }

    #[test]
    fn add_duplicate_gateway_is_idempotent() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);
        bridge.add_gateway(NodeNum(1), 2);
        assert_eq!(bridge.gateways.len(), 1, "should not add duplicate");
    }

    #[test]
    fn remove_gateway_clears_active() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);
        bridge.active_gateway = Some(NodeNum(1));
        bridge.remove_gateway(NodeNum(1));
        assert_eq!(bridge.active(), None);
        assert!(bridge.gateways.is_empty());
    }

    #[test]
    fn ensure_active_selects_when_none() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);
        assert_eq!(bridge.active(), None);
        bridge.ensure_active();
        assert_eq!(bridge.active(), Some(NodeNum(1)));
    }

    #[tokio::test(start_paused = true)]
    async fn health_monitor_cancels_cleanly() {
        let bridge = tokio::sync::Mutex::new(GatewayBridge::new());
        let token = CancellationToken::new();
        let task_token = token.clone();

        let handle = tokio::spawn(
            async move { run_health_monitor(&bridge, task_token).await }
                .instrument(tracing::info_span!("spawned_task")),
        );

        token.cancel();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn health_monitor_uses_updated_tick_interval() {
        let initial_cfg = BridgeConfig {
            health_check_interval_secs: 60,
            ..BridgeConfig::default()
        };
        let bridge = Arc::new(tokio::sync::Mutex::new(GatewayBridge::with_config(
            initial_cfg,
        )));
        let token = CancellationToken::new();
        let task_bridge = Arc::clone(&bridge);
        let task_token = token.clone();

        let handle = tokio::spawn(
            async move { run_health_monitor(task_bridge.as_ref(), task_token).await }
                .instrument(tracing::info_span!("spawned_task")),
        );

        tokio::task::yield_now().await;

        {
            let mut bridge = bridge.lock().await;
            bridge.add_gateway(NodeNum(1), 1);
            assert_eq!(bridge.active(), None);
            bridge.update_config(BridgeConfig {
                health_check_interval_secs: 1,
                ..BridgeConfig::default()
            });
        }

        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;

        assert_eq!(bridge.lock().await.active(), Some(NodeNum(1)));

        token.cancel();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }

    #[test]
    fn high_latency_marks_degraded() {
        let mut bridge = GatewayBridge::new();
        bridge.add_gateway(NodeNum(1), 1);

        bridge.record_health_success(NodeNum(1), 6000.0);
        assert!(
            matches!(first_health(&bridge), GatewayHealth::Degraded { .. }),
            "high latency should degrade"
        );
    }

    #[test]
    fn configured_offline_threshold_observably_changes_transition() {
        // WHY: parameterization-observability test — lowering
        // offline_check_threshold from 3 (default) to 1 must make the
        // gateway transition to Offline after a single failure.
        let cfg = BridgeConfig {
            offline_check_threshold: 1,
            ..BridgeConfig::default()
        };
        let mut bridge = GatewayBridge::with_config(cfg);
        bridge.add_gateway(NodeNum(1), 1);

        bridge.record_health_failure(NodeNum(1));
        assert!(
            matches!(first_health(&bridge), GatewayHealth::Offline { .. }),
            "with offline_check_threshold=1, a single failure must mark offline"
        );
    }

    #[test]
    fn configured_degraded_threshold_observably_changes_transition() {
        // WHY: a stricter degraded_response_threshold must mark responses
        // Degraded that would have been Healthy under the default.
        let cfg = BridgeConfig {
            degraded_response_threshold_ms: 100,
            ..BridgeConfig::default()
        };
        let mut bridge = GatewayBridge::with_config(cfg);
        bridge.add_gateway(NodeNum(1), 1);

        // 200 ms response: below default 5000 ms threshold, above configured 100 ms.
        bridge.record_health_success(NodeNum(1), 200.0);
        assert!(
            matches!(first_health(&bridge), GatewayHealth::Degraded { .. }),
            "with degraded_response_threshold_ms=100, a 200 ms response must degrade"
        );
    }

    #[test]
    fn config_accessor_returns_configured_values() {
        let cfg = BridgeConfig {
            failover_cooldown_secs: 7,
            ..BridgeConfig::default()
        };
        let bridge = GatewayBridge::with_config(cfg);
        assert_eq!(bridge.config().failover_cooldown_secs, 7);
    }

    /// Drives the bridge to the one state in which `ensure_active` actually
    /// consults the cooldown: no active gateway, a healthy candidate
    /// available, and `last_failover` freshly stamped.
    ///
    /// WHY: `record_health_failure` calls `failover()` directly when the
    /// ACTIVE gateway drops, bypassing the cooldown entirely — so a
    /// two-gateway setup never reaches the guard. Taking the sole gateway
    /// offline instead drives `active` to `None` and stamps `last_failover`;
    /// the replacement is only registered afterwards.
    fn bridge_needing_reselection(cfg: BridgeConfig) -> GatewayBridge {
        let mut bridge = GatewayBridge::with_config(cfg);
        bridge.add_gateway(NodeNum(1), 1);

        bridge.ensure_active();
        assert_eq!(
            bridge.active(),
            Some(NodeNum(1)),
            "the only healthy gateway must be chosen first"
        );

        for _ in 0..bridge.config().offline_check_threshold {
            bridge.record_health_failure(NodeNum(1));
        }
        assert_eq!(
            bridge.active(),
            None,
            "losing the sole gateway must leave the bridge with none active"
        );

        bridge.add_gateway(NodeNum(2), 2);
        let _ = bridge.drain_events();
        bridge
    }

    #[test]
    fn ensure_active_suppresses_a_second_failover_inside_the_cooldown() {
        // WHY(#229): the cooldown exists to stop a flapping gateway from
        // driving continuous failover churn. last_failover was stamped when
        // the sole gateway dropped, so adopting the replacement must wait even
        // though a healthy candidate is now available.
        let mut bridge = bridge_needing_reselection(BridgeConfig::default());

        bridge.ensure_active();

        assert_eq!(
            bridge.active(),
            None,
            "inside the cooldown no new gateway may be adopted"
        );
        assert!(
            bridge.drain_events().is_empty(),
            "a suppressed failover must emit no event"
        );
    }

    #[test]
    fn ensure_active_fails_over_when_the_cooldown_is_zero() {
        // WHY(#229): the falsifiable half — same sequence, cooldown of zero.
        // `elapsed() < 0` can never hold, so the guard falls through and the
        // failover proceeds. Without this the test above would also pass if
        // ensure_active simply never adopted a gateway at all.
        let mut bridge = bridge_needing_reselection(BridgeConfig {
            failover_cooldown_secs: 0,
            ..BridgeConfig::default()
        });

        bridge.ensure_active();

        assert_eq!(
            bridge.active(),
            Some(NodeNum(2)),
            "with no cooldown the bridge must adopt the healthy gateway"
        );
    }
}
