//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use super::*;

fn n(v: u32) -> NodeNum {
    NodeNum(v)
}

#[test]
fn add_node_idempotent() {
    let mut topo = MeshTopology::new();
    let idx1 = topo.add_node(n(1));
    let idx2 = topo.add_node(n(1));
    assert_eq!(idx1, idx2);
    assert_eq!(topo.node_count(), 1);
}

#[test]
fn update_link_creates_edge() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 5.0);
    assert_eq!(topo.edge_count(), 1);
    let neighbors = topo.neighbors(n(1));
    assert_eq!(neighbors.len(), 1);
    assert_eq!(neighbors[0].0, n(2));
    assert!((neighbors[0].1.snr - 5.0).abs() < f32::EPSILON);
}

#[test]
fn update_link_updates_existing() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 5.0);
    topo.update_link(n(1), n(2), 8.0);
    assert_eq!(topo.edge_count(), 1, "should not duplicate edges");
    let neighbors = topo.neighbors(n(1));
    assert!((neighbors[0].1.snr - 8.0).abs() < f32::EPSILON);
    assert_eq!(neighbors[0].1.packet_count, 2);
}

#[test]
fn update_link_rejects_non_finite_snr() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), f32::NAN);
    topo.update_link(n(1), n(2), f32::INFINITY);
    assert_eq!(
        topo.edge_count(),
        0,
        "non-finite SNR must not create an edge"
    );
}

#[test]
fn shortest_path_simple_chain() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.update_link(n(2), n(3), 10.0);
    topo.update_link(n(1), n(3), 1.0); // direct but weak link
    let path = topo.shortest_path(n(1), n(3)).unwrap();
    // WHY: via n(2) has cost 20+20=40, direct has cost 29 — direct is cheaper
    assert_eq!(path, vec![n(1), n(3)]);
}

#[test]
fn shortest_path_prefers_strong_signal() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 25.0); // cost 5
    topo.update_link(n(2), n(3), 25.0); // cost 5, total 10
    topo.update_link(n(1), n(3), 5.0); // cost 25
    let path = topo.shortest_path(n(1), n(3)).unwrap();
    assert_eq!(path, vec![n(1), n(2), n(3)]);
}

#[test]
fn shortest_path_unreachable_returns_none() {
    let mut topo = MeshTopology::new();
    topo.add_node(n(1));
    topo.add_node(n(2));
    assert!(topo.shortest_path(n(1), n(2)).is_none());
}

#[test]
fn hop_count_returns_minimum_hops() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.update_link(n(2), n(3), 10.0);
    assert_eq!(topo.hop_count(n(1), n(3)), Some(2));
    assert_eq!(topo.hop_count(n(1), n(2)), Some(1));
}

#[test]
fn connected_components_single_cluster() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.update_link(n(2), n(3), 10.0);
    let comps = topo.connected_components();
    assert_eq!(comps.len(), 1);
    assert_eq!(comps[0].len(), 3);
}

#[test]
fn connected_components_two_clusters() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.update_link(n(3), n(4), 10.0);
    let comps = topo.connected_components();
    assert_eq!(comps.len(), 2, "two disconnected clusters");
}

#[test]
fn is_partitioned_detects_unreachable() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.add_node(n(3));
    assert!(!topo.is_partitioned(n(2), n(1)));
    assert!(topo.is_partitioned(n(3), n(1)));
}

#[test]
fn is_partitioned_false_for_node_to_server_directed_edge() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(2), n(1), 10.0);
    assert!(
        !topo.is_partitioned(n(2), n(1)),
        "a node->server edge must not read as partitioned"
    );
}

#[tokio::test(start_paused = true)]
async fn remove_stale_links_prunes_old_edges() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    tokio::time::advance(Duration::from_secs(120)).await;
    topo.update_link(n(1), n(3), 10.0);
    topo.remove_stale_links(Duration::from_secs(60));
    assert_eq!(topo.edge_count(), 1, "only the fresh edge should remain");
    assert!(topo.neighbors(n(1)).iter().any(|(num, _)| *num == n(3)));
}

#[tokio::test(start_paused = true)]
async fn remove_stale_nodes_prunes_isolated() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    tokio::time::advance(Duration::from_secs(120)).await;
    topo.update_link(n(3), n(4), 10.0);
    topo.remove_stale_nodes(Duration::from_secs(60));
    assert!(!topo.contains_node(n(1)), "stale node 1 should be removed");
    assert!(!topo.contains_node(n(2)), "stale node 2 should be removed");
    assert!(topo.contains_node(n(3)));
    assert!(topo.contains_node(n(4)));
}

// WHY: regression for the monotonic-clock underflow panic (#206) — a
// fresh process (t≈0, no `tokio::time::advance`) pruning against the
// default 7200s stale window must not panic, and nothing is old enough
// to be considered stale yet.
#[tokio::test(start_paused = true)]
async fn remove_stale_links_no_panic_when_timeout_exceeds_uptime() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.remove_stale_links(Duration::from_secs(7200));
    assert_eq!(topo.edge_count(), 1, "nothing is stale yet at t=0");
}

#[tokio::test(start_paused = true)]
async fn remove_stale_nodes_no_panic_when_timeout_exceeds_uptime() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 10.0);
    topo.remove_stale_nodes(Duration::from_secs(7200));
    assert!(topo.contains_node(n(1)), "nothing is stale yet at t=0");
    assert!(topo.contains_node(n(2)), "nothing is stale yet at t=0");
}

#[test]
fn snapshot_roundtrip() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 7.5);
    topo.update_link(n(2), n(3), 12.0);
    topo.add_node(n(4));

    let bytes = topo.save_to_bytes().unwrap();
    let restored = MeshTopology::load_from_bytes(&bytes).unwrap();

    assert_eq!(restored.node_count(), 4);
    assert_eq!(restored.edge_count(), 2);
    let neighbors = restored.neighbors(n(1));
    assert_eq!(neighbors.len(), 1);
    assert!((neighbors[0].1.snr - 7.5).abs() < f32::EPSILON);
}

#[test]
fn neighbors_of_unknown_node_returns_empty() {
    let topo = MeshTopology::new();
    assert!(topo.neighbors(n(99)).is_empty());
}

#[test]
fn shortest_path_ceiling_changes_selected_route() {
    // WHY: parameterization-observability test — the same graph must
    // produce a different path depending on snr_ceiling.
    //
    // With default ceiling 30: direct link (snr 29 → cost 1) beats the
    // 2-hop route (snr 28 each → cost 4) — direct wins.
    // With ceiling 5 (clamped to 0 for any snr>=5): both paths cost 0,
    // but the 1-hop direct path is selected by astar's determinism.
    // A ceiling just above the stronger links asymmetrically penalises
    // the weaker direct link more than the two-hop route, so raising
    // the ceiling from an "equal" value to a value where only the
    // direct link is below ceiling flips the answer.
    //
    // Construction: direct link snr=10, 2-hop path snr=19 each.
    //   ceiling=20  →  direct cost=10, 2-hop cost=1+1=2 → 2-hop wins
    //   ceiling=11  →  direct cost=1,  2-hop cost=0+0=0 (clamped) → 2-hop still wins by cost
    //   ceiling=9   →  direct cost=0 (clamped), 2-hop cost=0 → direct wins (1 hop)
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 19.0);
    topo.update_link(n(2), n(3), 19.0);
    topo.update_link(n(1), n(3), 10.0);

    let path_high = topo
        .shortest_path_with_ceiling(n(1), n(3), 20.0)
        .expect("reachable");
    assert_eq!(
        path_high,
        vec![n(1), n(2), n(3)],
        "ceiling 20 penalises direct link (cost 10) more than 2-hop (cost 2)"
    );

    let path_low = topo
        .shortest_path_with_ceiling(n(1), n(3), 9.0)
        .expect("reachable");
    assert_eq!(
        path_low,
        vec![n(1), n(3)],
        "ceiling 9 clamps all costs to 0; astar picks the 1-hop path"
    );
}

#[test]
fn shortest_path_with_config_uses_supplied_ceiling() {
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 19.0);
    topo.update_link(n(2), n(3), 19.0);
    topo.update_link(n(1), n(3), 10.0);

    let cfg_high = TopologyConfig {
        snr_ceiling: 20.0,
        ..TopologyConfig::default()
    };
    let cfg_low = TopologyConfig {
        snr_ceiling: 9.0,
        ..TopologyConfig::default()
    };

    assert_eq!(
        topo.shortest_path_with_config(n(1), n(3), &cfg_high)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        topo.shortest_path_with_config(n(1), n(3), &cfg_low)
            .unwrap()
            .len(),
        2
    );
}

// ── akroasis#229: snapshot restore must dedup and stay bounded ────────

fn link(from: u32, to: u32, snr: f32, packet_count: u32) -> LinkSnapshot {
    LinkSnapshot {
        from: n(from),
        to: n(to),
        snr,
        packet_count,
    }
}

#[test]
fn load_from_bytes_folds_repeated_link_pairs() {
    // WHY: `update_link` keeps at most one edge per ordered pair, so a
    // restore that admits parallel edges produces a graph the live path
    // could never reach  -  and `to_bytes` re-emits them, compounding.
    let snapshot = TopologySnapshot {
        nodes: vec![n(1), n(2)],
        links: vec![link(1, 2, 5.0, 3), link(1, 2, 7.5, 4)],
    };
    let bytes = serde_json::to_vec(&snapshot).unwrap();

    let topo = MeshTopology::load_from_bytes(&bytes).unwrap();

    assert_eq!(topo.edge_count(), 1, "repeated pair must fold to one edge");
    let neighbors = topo.neighbors(n(1));
    assert_eq!(neighbors.len(), 1);
    let (peer, quality) = &neighbors[0];
    assert_eq!(*peer, n(2));
    assert!(
        (quality.snr - 7.5).abs() < f32::EPSILON,
        "last observation should win, got {}",
        quality.snr
    );
    assert_eq!(quality.packet_count, 7, "counts should add");
}

#[test]
fn load_from_bytes_round_trips_without_multiplying_edges() {
    // WHY: the compounding case  -  save/load/save must be a fixed point.
    let mut topo = MeshTopology::new();
    topo.update_link(n(1), n(2), 5.0);
    topo.update_link(n(2), n(3), 6.0);

    let once = MeshTopology::load_from_bytes(&topo.save_to_bytes().unwrap()).unwrap();
    let twice = MeshTopology::load_from_bytes(&once.save_to_bytes().unwrap()).unwrap();

    assert_eq!(once.edge_count(), 2);
    assert_eq!(twice.edge_count(), 2, "reload must not multiply edges");
}

#[test]
fn load_from_bytes_caps_nodes_and_links() {
    let over = MAX_SNAPSHOT_NODES + 10;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "test-only: indices are far below u32::MAX"
    )]
    let nodes: Vec<NodeNum> = (0..over as u32).map(n).collect();
    let snapshot = TopologySnapshot {
        nodes,
        links: Vec::new(),
    };
    let bytes = serde_json::to_vec(&snapshot).unwrap();

    let topo = MeshTopology::load_from_bytes(&bytes).unwrap();

    assert_eq!(
        topo.node_count(),
        MAX_SNAPSHOT_NODES,
        "restore must stop at the node cap"
    );
}
