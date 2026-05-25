//! Mesh networking CLI — status, nodes, send, topology.

use std::io::Write;

use clap::Subcommand;
use comfy_table::{Attribute, Cell, ContentArrangement, Table};
use serde::Serialize;
use snafu::{ResultExt, Snafu};

use kerykeion::bridge::{GatewayBridge, GatewayHealth};
use kerykeion::node_db::{MeshNode, NodeDb};
use kerykeion::types::NodeNum;

/// Mesh CLI errors.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum MeshError {
    /// Node not found by name or number.
    #[snafu(display("node not found: {identifier}"))]
    NodeNotFound {
        /// The identifier used to look up the node.
        identifier: String,
    },

    #[snafu(display("failed to write JSON report: {source}"))]
    JsonReport { source: serde_json::Error },

    #[snafu(display("I/O error: {source}"))]
    Io { source: std::io::Error },
}

const MESH_JSON_SCHEMA: u8 = 1;
const CLI_STATUS: &str = "not_connected_cli_mode";
const LIVE_TRANSPORT_MESSAGE: &str = "Live mesh transport is not wired in this CLI build.";
const LIVE_NODES_MESSAGE: &str = "No live connection. Live node collection is not wired yet.";
const LIVE_TOPOLOGY_MESSAGE: &str =
    "No live connection. Live topology collection is not wired yet.";

#[derive(Serialize)]
struct StatusReport {
    schema_version: u8,
    command: &'static str,
    collector: &'static str,
    status: &'static str,
    active_connections: u32,
    gateway: Option<&'static str>,
    known_nodes: u32,
    message: &'static str,
}

#[derive(Serialize)]
struct NodesReport {
    schema_version: u8,
    command: &'static str,
    status: &'static str,
    node_count: usize,
    nodes: Vec<NodeReport>,
    message: &'static str,
}

#[derive(Serialize)]
struct NodeReport {
    node: String,
    long_name: Option<String>,
    short_name: Option<String>,
    hardware_model: Option<String>,
    battery_percent: Option<u8>,
    snr_db: Option<f32>,
    hop_count: Option<u8>,
    last_heard: Option<String>,
}

#[derive(Serialize)]
struct TopologyReport {
    schema_version: u8,
    command: &'static str,
    status: &'static str,
    node_count: usize,
    edge_count: usize,
    edges: Vec<TopologyEdgeReport>,
    message: &'static str,
}

#[derive(Serialize)]
struct TopologyEdgeReport {
    from: String,
    to: String,
    snr_db: Option<f32>,
    hops: Option<u8>,
}

/// Mesh subcommands.
#[derive(Subcommand)]
pub enum MeshCommand {
    /// Show mesh network status summary
    Status {
        /// Emit a machine-readable JSON report instead of human text.
        #[arg(long)]
        json: bool,
    },

    /// List all known mesh nodes
    Nodes {
        /// Emit a machine-readable JSON report instead of human text.
        #[arg(long)]
        json: bool,
    },

    /// Display mesh network topology
    Topology {
        /// Emit a machine-readable JSON report instead of human text.
        #[arg(long)]
        json: bool,
    },
}

/// Dispatch a mesh subcommand.
///
/// # Errors
///
/// Returns `MeshError` if the command fails.
pub fn dispatch(command: &MeshCommand, out: &mut dyn Write) -> Result<(), MeshError> {
    match command {
        MeshCommand::Status { json } => {
            print_status(*json, out)?;
            Ok(())
        }
        MeshCommand::Nodes { json } => {
            print_nodes(*json, out)?;
            Ok(())
        }
        MeshCommand::Topology { json } => {
            print_topology(*json, out)?;
            Ok(())
        }
    }
}

/// Print mesh network status summary.
fn print_status(json: bool, out: &mut dyn Write) -> Result<(), MeshError> {
    if json {
        write_json_report(
            out,
            &StatusReport {
                schema_version: MESH_JSON_SCHEMA,
                command: "mesh status",
                collector: "kerykeion",
                status: CLI_STATUS,
                active_connections: 0,
                gateway: None,
                known_nodes: 0,
                message: LIVE_TRANSPORT_MESSAGE,
            },
        )?;
        return Ok(());
    }

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Property").add_attribute(Attribute::Bold),
        Cell::new("Value").add_attribute(Attribute::Bold),
    ]);

    table.add_row(vec!["Collector", "kerykeion"]);
    table.add_row(vec!["Status", "not connected (CLI mode)"]);
    table.add_row(vec!["Active connections", "0"]);
    table.add_row(vec!["Gateway", "none"]);
    table.add_row(vec!["Known nodes", "0"]);

    writeln!(out, "{table}").context(IoSnafu)?;
    writeln!(out).context(IoSnafu)?;
    writeln!(out, "{LIVE_TRANSPORT_MESSAGE}").context(IoSnafu)?;
    Ok(())
}

/// Print detailed node table.
fn print_nodes(json: bool, out: &mut dyn Write) -> Result<(), MeshError> {
    if json {
        write_json_report(
            out,
            &NodesReport {
                schema_version: MESH_JSON_SCHEMA,
                command: "mesh nodes",
                status: CLI_STATUS,
                node_count: 0,
                nodes: Vec::new(),
                message: LIVE_NODES_MESSAGE,
            },
        )?;
        return Ok(());
    }

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Node").add_attribute(Attribute::Bold),
        Cell::new("Long Name").add_attribute(Attribute::Bold),
        Cell::new("Short").add_attribute(Attribute::Bold),
        Cell::new("HW Model").add_attribute(Attribute::Bold),
        Cell::new("Battery").add_attribute(Attribute::Bold),
        Cell::new("SNR").add_attribute(Attribute::Bold),
        Cell::new("Hops").add_attribute(Attribute::Bold),
        Cell::new("Last Heard").add_attribute(Attribute::Bold),
    ]);

    writeln!(out, "{table}").context(IoSnafu)?;
    writeln!(out).context(IoSnafu)?;
    writeln!(out, "{LIVE_NODES_MESSAGE}").context(IoSnafu)?;
    Ok(())
}

/// Print mesh topology as adjacency list.
fn print_topology(json: bool, out: &mut dyn Write) -> Result<(), MeshError> {
    if json {
        write_json_report(
            out,
            &TopologyReport {
                schema_version: MESH_JSON_SCHEMA,
                command: "mesh topology",
                status: CLI_STATUS,
                node_count: 0,
                edge_count: 0,
                edges: Vec::new(),
                message: LIVE_TOPOLOGY_MESSAGE,
            },
        )?;
        return Ok(());
    }

    writeln!(out, "Mesh Topology").context(IoSnafu)?;
    writeln!(out, "─────────────").context(IoSnafu)?;
    writeln!(out, "{LIVE_TOPOLOGY_MESSAGE}").context(IoSnafu)?;
    writeln!(out).context(IoSnafu)?;
    writeln!(out, "When running, topology shows:").context(IoSnafu)?;
    writeln!(out, "  NodeA -> NodeB (SNR: -5.2 dB, hops: 1)").context(IoSnafu)?;
    writeln!(out, "  NodeB -> NodeC (SNR: -8.1 dB, hops: 1)").context(IoSnafu)?;
    Ok(())
}

fn write_json_report<T: Serialize>(out: &mut dyn Write, report: &T) -> Result<(), MeshError> {
    serde_json::to_writer_pretty(&mut *out, report).context(JsonReportSnafu)?;
    writeln!(out).context(IoSnafu)?;
    Ok(())
}

/// Format a node table row from a [`MeshNode`].
#[must_use]
pub fn format_node_row(node: &MeshNode) -> Vec<String> {
    let num = node.num.to_string();
    let long_name = node
        .user
        .as_ref()
        .map_or_else(|| "—".to_string(), |u| u.long_name.clone());
    let short_name = node
        .user
        .as_ref()
        .map_or_else(|| "—".to_string(), |u| u.short_name.clone());
    let hw_model = node
        .user
        .as_ref()
        .map_or_else(|| "—".to_string(), |u| u.hw_model.to_string());
    let battery = node
        .metrics
        .as_ref()
        .and_then(|m| m.battery_level)
        .map_or_else(|| "—".to_string(), |b| format!("{b}%"));
    let snr = node
        .snr
        .map_or_else(|| "—".to_string(), |s| format!("{s:.1} dB"));
    let hops = node
        .hop_count
        .map_or_else(|| "—".to_string(), |h| h.to_string());
    let last_heard = node
        .last_heard
        .map_or_else(|| "—".to_string(), |t| t.to_string());

    vec![
        num, long_name, short_name, hw_model, battery, snr, hops, last_heard,
    ]
}

/// Format a gateway status row.
#[must_use]
pub const fn format_gateway_health(health: &GatewayHealth) -> &'static str {
    match health {
        GatewayHealth::Healthy => "healthy",
        GatewayHealth::Degraded { .. } => "degraded",
        GatewayHealth::Offline { .. } => "offline",
        _ => "unknown",
    }
}

/// Parse a node identifier: hex `0xDEADBEEF`, decimal, or `!deadbeef` format.
#[must_use]
fn parse_node_identifier(id: &str) -> Option<NodeNum> {
    id.strip_prefix("0x")
        .or_else(|| id.strip_prefix("0X"))
        .or_else(|| id.strip_prefix('!'))
        .map_or_else(
            || id.parse::<u32>().ok().map(NodeNum),
            |hex| u32::from_str_radix(hex, 16).ok().map(NodeNum),
        )
}

/// Build a status table from live node database and bridge state.
///
/// Used by the daemon's status endpoint to produce formatted output.
#[must_use]
pub fn build_status_table(db: &NodeDb, bridge: &GatewayBridge) -> String {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Property").add_attribute(Attribute::Bold),
        Cell::new("Value").add_attribute(Attribute::Bold),
    ]);

    table.add_row(vec!["Collector", "kerykeion"]);
    table.add_row(vec!["Status", "connected"]);

    let active_gw = bridge
        .active()
        .map_or_else(|| "none".to_string(), |n| format!("{n}"));
    table.add_row(vec!["Active gateway".to_string(), active_gw]);

    let gw_health = bridge.active().and_then(|active| {
        bridge
            .gateways()
            .iter()
            .find(|g| g.node == active)
            .map(|g| format_gateway_health(&g.health))
    });
    table.add_row(vec![
        "Gateway health".to_string(),
        gw_health.unwrap_or("—").to_string(),
    ]);

    table.add_row(vec!["Known nodes".to_string(), db.len().to_string()]);

    let online = db.iter().filter(|(_, n)| n.last_heard.is_some()).count();
    table.add_row(vec!["Online nodes".to_string(), online.to_string()]);

    table.to_string()
}

/// Build a detailed nodes table from the node database.
#[must_use]
pub fn build_nodes_table(db: &NodeDb) -> String {
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        Cell::new("Node").add_attribute(Attribute::Bold),
        Cell::new("Long Name").add_attribute(Attribute::Bold),
        Cell::new("Short").add_attribute(Attribute::Bold),
        Cell::new("HW Model").add_attribute(Attribute::Bold),
        Cell::new("Battery").add_attribute(Attribute::Bold),
        Cell::new("SNR").add_attribute(Attribute::Bold),
        Cell::new("Hops").add_attribute(Attribute::Bold),
        Cell::new("Last Heard").add_attribute(Attribute::Bold),
    ]);

    let mut nodes: Vec<&MeshNode> = db.iter().map(|(_, n)| n).collect();
    nodes.sort_by_key(|n| n.num.0);

    for node in nodes {
        table.add_row(format_node_row(node));
    }

    table.to_string()
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test assertions use unwrap, indexing, and panic for clarity"
)]
mod tests {
    use kerykeion::node_db::{DeviceMetrics, UserInfo};

    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: MeshCommand,
    }

    fn parse(args: &[&str]) -> MeshCommand {
        TestCli::parse_from(std::iter::once("test").chain(args.iter().copied())).command
    }

    #[test]
    fn parse_status_json_flag() {
        let cmd = parse(&["status", "--json"]);
        match cmd {
            MeshCommand::Status { json } => assert!(json),
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn parse_nodes_json_flag() {
        let cmd = parse(&["nodes", "--json"]);
        match cmd {
            MeshCommand::Nodes { json } => assert!(json),
            _ => panic!("expected nodes command"),
        }
    }

    #[test]
    fn parse_topology_json_flag() {
        let cmd = parse(&["topology", "--json"]);
        match cmd {
            MeshCommand::Topology { json } => assert!(json),
            _ => panic!("expected topology command"),
        }
    }

    #[test]
    fn parse_hex_node_id() {
        assert_eq!(
            parse_node_identifier("0xdeadbeef"),
            Some(NodeNum(0xDEAD_BEEF))
        );
    }

    #[test]
    fn parse_bang_node_id() {
        assert_eq!(
            parse_node_identifier("!aabbccdd"),
            Some(NodeNum(0xAABB_CCDD))
        );
    }

    #[test]
    fn parse_decimal_node_id() {
        assert_eq!(parse_node_identifier("12345"), Some(NodeNum(12345)));
    }

    #[test]
    fn parse_invalid_node_id() {
        assert_eq!(parse_node_identifier("not_a_number"), None);
    }

    #[test]
    fn format_node_row_with_full_data() {
        let node = MeshNode {
            num: NodeNum(0x1234),
            user: Some(UserInfo {
                id: "!00001234".into(),
                long_name: "Base Station".into(),
                short_name: "BS01".into(),
                hw_model: 43,
                is_licensed: false,
            }),
            position: None,
            metrics: Some(DeviceMetrics {
                battery_level: Some(85),
                voltage: Some(4.1),
                channel_utilization: None,
                air_util_tx: None,
            }),
            last_heard: None,
            snr: Some(5.5),
            hop_count: Some(1),
        };

        let row = format_node_row(&node);
        assert_eq!(row.len(), 8);
        #[expect(clippy::indexing_slicing, reason = "test-only: length checked above")]
        {
            assert!(row[1].contains("Base Station"));
            assert!(row[4].contains("85%"));
            assert!(row[5].contains("5.5"));
        }
    }

    #[test]
    fn format_node_row_with_missing_data() {
        let node = MeshNode {
            num: NodeNum(0xFFFF),
            user: None,
            position: None,
            metrics: None,
            last_heard: None,
            snr: None,
            hop_count: None,
        };

        let row = format_node_row(&node);
        #[expect(clippy::indexing_slicing, reason = "test-only: known row layout")]
        {
            assert_eq!(row[1], "—", "missing user should show dash");
            assert_eq!(row[4], "—", "missing battery should show dash");
        }
    }

    #[test]
    fn build_status_table_produces_output() {
        let db = NodeDb::new();
        let bridge = GatewayBridge::new();
        let output = build_status_table(&db, &bridge);
        assert!(output.contains("kerykeion"));
        assert!(output.contains("Known nodes"));
    }

    #[test]
    fn build_nodes_table_empty() {
        let db = NodeDb::new();
        let output = build_nodes_table(&db);
        assert!(output.contains("Node"), "header should be present");
    }

    #[test]
    fn build_nodes_table_with_data() {
        let mut db = NodeDb::new();
        db.insert(MeshNode {
            num: NodeNum(1),
            user: Some(UserInfo {
                id: "!00000001".into(),
                long_name: "Alpha".into(),
                short_name: "A".into(),
                hw_model: 7,
                is_licensed: false,
            }),
            position: None,
            metrics: None,
            last_heard: None,
            snr: Some(3.0),
            hop_count: Some(0),
        });
        let output = build_nodes_table(&db);
        assert!(output.contains("Alpha"));
    }

    #[test]
    fn gateway_health_formatting() {
        assert_eq!(format_gateway_health(&GatewayHealth::Healthy), "healthy");
        assert_eq!(
            format_gateway_health(&GatewayHealth::Degraded {
                reason: "test".into()
            }),
            "degraded"
        );
        assert_eq!(
            format_gateway_health(&GatewayHealth::Offline { since: None }),
            "offline"
        );
    }

    #[test]
    fn dispatch_status_captures_output() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Status { json: false }, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("kerykeion"));
        assert!(s.contains("Live mesh transport is not wired"));
    }

    #[test]
    fn dispatch_status_json_outputs_machine_readable_report() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Status { json: true }, &mut out).unwrap();

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "mesh status");
        assert_eq!(report["collector"], "kerykeion");
        assert_eq!(report["status"], "not_connected_cli_mode");
        assert_eq!(report["known_nodes"], 0);
    }

    #[test]
    fn dispatch_nodes_captures_output() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Nodes { json: false }, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Node"));
        assert!(s.contains("No live connection"));
    }

    #[test]
    fn dispatch_nodes_json_outputs_machine_readable_report() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Nodes { json: true }, &mut out).unwrap();

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "mesh nodes");
        assert_eq!(report["status"], "not_connected_cli_mode");
        assert_eq!(report["node_count"], 0);
        assert!(report["nodes"].as_array().unwrap().is_empty());
    }

    #[test]
    fn dispatch_topology_captures_output() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Topology { json: false }, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Mesh Topology"));
    }

    #[test]
    fn dispatch_topology_json_outputs_machine_readable_report() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Topology { json: true }, &mut out).unwrap();

        let report: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(report["schema_version"], 1);
        assert_eq!(report["command"], "mesh topology");
        assert_eq!(report["status"], "not_connected_cli_mode");
        assert_eq!(report["edge_count"], 0);
        assert!(report["edges"].as_array().unwrap().is_empty());
    }
}
