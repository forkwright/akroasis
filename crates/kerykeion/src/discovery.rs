//! Active and passive mesh node discovery manager.

use std::time::Duration;

use koinon::GeoSignal;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::instrument;

use crate::config::TopologyConfig;
use crate::connection::MeshConnection;
use crate::processor::PacketProcessor;
use crate::signals::{MeshEvent, mesh_event_to_signal};
use crate::types::NodeNum;

/// Node lifecycle state based on time since last heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NodeState {
    /// Node is actively communicating.
    Active,
    /// Not heard within 1× stale timeout.
    Stale,
    /// Not heard within 2× stale timeout.
    Offline,
    /// Not heard within 3× stale timeout  -  removed FROM active topology.
    Removed,
}

/// Determine a node's lifecycle state based on elapsed time since last heard.
#[must_use]
pub fn classify_node_state(elapsed: Duration, stale_timeout: Duration) -> NodeState {
    if elapsed < stale_timeout {
        NodeState::Active
    } else if elapsed < stale_timeout * 2 {
        NodeState::Stale
    } else if elapsed < stale_timeout * 3 {
        NodeState::Offline
    } else {
        NodeState::Removed
    }
}

/// Build a `ToRadio` traceroute request packet targeting `dest_node`.
///
/// The packet uses `TRACEROUTE_APP` (portnum 70) with `want_response = true`.
#[must_use]
pub const fn build_traceroute_request(dest_node: NodeNum) -> crate::proto::ToRadio {
    use crate::proto::{Data, MeshPacket, ToRadio, mesh_packet, to_radio};

    let data = Data {
        portnum: 70, // TRACEROUTE_APP
        payload: Vec::new(),
        want_response: true,
        dest: 0,
        source: 0,
        request_id: 0,
        reply_id: 0,
        emoji: vec![],
    };

    ToRadio {
        payload_variant: Some(to_radio::PayloadVariant::Packet(MeshPacket {
            from: 0, // WHY: radio fills in from field
            to: dest_node.0,
            channel: 0,
            payload_variant: Some(mesh_packet::PayloadVariant::Decoded(data)),
            id: 0, // WHY: radio assigns packet ID
            rx_time: 0,
            rx_snr: 0.0,
            hop_limit: 7,
            want_ack: true,
            priority: 70, // RELIABLE
            rx_rssi: 0,
            via_mqtt: false,
            hop_start: 0,
        })),
    }
}

/// Run the discovery loop: periodic traceroutes and stale node detection.
///
/// Runs as a tokio task cancelled via `token`. Sends traceroute requests through
/// `conn` and processes stale detection based on `config` timeouts.
///
/// # Cancellation
///
/// Exits cleanly when `token` is cancelled. Cancel-safe: all state mutations
/// are completed before the next `.await`.
#[instrument(
    level = "debug",
    skip(conn, processor, config, tx, token),
    fields(
        traceroute_interval_secs = config.traceroute_interval_secs,
        stale_node_timeout_secs = config.stale_node_timeout_secs
    )
)]
pub async fn run_discovery<C>(
    conn: &tokio::sync::Mutex<C>,
    processor: &tokio::sync::Mutex<PacketProcessor>,
    config: &TopologyConfig,
    tx: &broadcast::Sender<GeoSignal>,
    token: CancellationToken,
) where
    C: MeshConnection,
{
    let traceroute_interval = Duration::from_secs(config.traceroute_interval_secs);
    let stale_timeout = Duration::from_secs(config.stale_node_timeout_secs);
    let mut traceroute_timer = tokio::time::interval(traceroute_interval);
    traceroute_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut stale_timer = tokio::time::interval(stale_timeout / 2);
    stale_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // WHY: skip the first immediate tick for both timers.
    traceroute_timer.tick().await;
    stale_timer.tick().await;

    loop {
        tokio::select! {
            biased;

            () = token.cancelled() => {
                tracing::debug!("discovery task cancelled");
                break;
            }

            _ = traceroute_timer.tick() => {
                send_traceroutes(conn, processor, token.clone()).await;
            }

            _ = stale_timer.tick() => {
                run_stale_detection(processor, stale_timeout, tx).await;
            }
        }
    }
}

/// Send traceroute requests to all known nodes.
async fn send_traceroutes<C>(
    conn: &tokio::sync::Mutex<C>,
    processor: &tokio::sync::Mutex<PacketProcessor>,
    token: CancellationToken,
) where
    C: MeshConnection,
{
    let nodes: Vec<NodeNum> = {
        let proc = processor.lock().await;
        let my_node = proc.node_db().my_node();
        proc.node_db()
            .iter()
            .map(|(&num, _)| num)
            .filter(|num| Some(*num) != my_node)
            .collect()
    };

    for node in nodes {
        if token.is_cancelled() {
            break;
        }
        let request = build_traceroute_request(node);
        let mut connection = conn.lock().await;
        if let Err(e) = connection.send(request).await {
            tracing::warn!(node = node.0, error = %e, "failed to send traceroute request");
        }
    }
}

/// Check for stale/offline nodes and emit appropriate events.
async fn run_stale_detection(
    processor: &tokio::sync::Mutex<PacketProcessor>,
    stale_timeout: Duration,
    tx: &broadcast::Sender<GeoSignal>,
) {
    let mut proc = processor.lock().await;
    let now = jiff::Timestamp::now();

    let offline_nodes: Vec<NodeNum> = proc
        .node_db()
        .iter()
        .filter_map(|(&num, node)| {
            let last_heard = node.last_heard?;
            let elapsed_ms = now
                .as_millisecond()
                .saturating_sub(last_heard.as_millisecond());
            #[expect(
                clippy::cast_sign_loss,
                reason = "elapsed_ms is always non-negative since now >= last_heard"
            )]
            let elapsed = Duration::from_millis(elapsed_ms as u64); // SAFETY: elapsed_ms comes from Instant::elapsed().as_millis() capped earlier; fits u64
            let state = classify_node_state(elapsed, stale_timeout);

            if state == NodeState::Offline {
                Some(num)
            } else {
                None
            }
        })
        .collect();

    for node in &offline_nodes {
        let event = MeshEvent::NodeOffline { node: *node };
        let position = proc.node_db().get(*node).and_then(|n| n.position.as_ref());
        let signal = mesh_event_to_signal(&event, position);
        let _ = tx.send(signal);
    }

    // WHY: remove nodes past 3× timeout FROM the active topology.
    proc.topology_mut().remove_stale_links(stale_timeout * 3);

    // WHY: check for partitions after removing stale links.
    let components = proc.topology().connected_components();
    drop(proc);
    if components.len() > 1 {
        let event = MeshEvent::PartitionDetected { components };
        let signal = mesh_event_to_signal(&event, None);
        let _ = tx.send(signal);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn classify_active_within_timeout() {
        let timeout = Duration::from_secs(7200);
        assert_eq!(
            classify_node_state(Duration::from_secs(3600), timeout),
            NodeState::Active
        );
    }

    #[test]
    fn classify_stale_at_1x_timeout() {
        let timeout = Duration::from_secs(7200);
        assert_eq!(
            classify_node_state(Duration::from_secs(7200), timeout),
            NodeState::Stale
        );
    }

    #[test]
    fn classify_offline_at_2x_timeout() {
        let timeout = Duration::from_secs(7200);
        assert_eq!(
            classify_node_state(Duration::from_secs(14400), timeout),
            NodeState::Offline
        );
    }

    #[test]
    fn classify_removed_at_3x_timeout() {
        let timeout = Duration::from_secs(7200);
        assert_eq!(
            classify_node_state(Duration::from_secs(21601), timeout),
            NodeState::Removed
        );
    }

    #[test]
    fn traceroute_request_targets_correct_node() {
        let req = build_traceroute_request(NodeNum(0x1234));
        match req.payload_variant {
            Some(crate::proto::to_radio::PayloadVariant::Packet(pkt)) => {
                assert_eq!(pkt.to, 0x1234);
                assert!(pkt.want_ack);
                match pkt.payload_variant {
                    Some(crate::proto::mesh_packet::PayloadVariant::Decoded(data)) => {
                        assert_eq!(data.portnum, 70);
                        assert!(data.want_response);
                    }
                    _ => panic!("expected decoded payload"),
                }
            }
            _ => panic!("expected packet payload"),
        }
    }
}
