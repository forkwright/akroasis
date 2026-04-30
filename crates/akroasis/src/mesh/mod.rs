//! Mesh networking CLI — status, nodes, send, topology.

use std::io::Write;

use clap::Subcommand;
use comfy_table::{Attribute, Cell, ContentArrangement, Table};
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

    #[snafu(display("I/O error: {source}"))]
    Io { source: std::io::Error },
}

/// Mesh subcommands.
#[derive(Subcommand)]
pub enum MeshCommand {
    /// Show mesh network status summary
    Status,

    /// List all known mesh nodes
    Nodes,

    /// Send a text message to a mesh node
    Send {
        /// Destination node number (hex, e.g. `0xdeadbeef`) or name
        dest: String,

        /// Message text
        message: String,

        /// Channel index to send on (default: primary channel 0)
        #[arg(long, default_value = "0")]
        channel: u8,

        /// Fire-and-forget — do not wait for ACK
        #[arg(long)]
        no_ack: bool,
    },

    /// Display mesh network topology
    Topology,
}

/// Dispatch a mesh subcommand.
///
/// # Errors
///
/// Returns `MeshError` if the command fails.
pub fn dispatch(command: &MeshCommand, out: &mut dyn Write) -> Result<(), MeshError> {
    match command {
        MeshCommand::Status => {
            print_status(out)?;
            Ok(())
        }
        MeshCommand::Nodes => {
            print_nodes(out)?;
            Ok(())
        }
        MeshCommand::Send {
            dest,
            message,
            channel,
            no_ack,
        } => print_send(dest, message, *channel, *no_ack, out),
        MeshCommand::Topology => {
            print_topology(out)?;
            Ok(())
        }
    }
}

/// Print mesh network status summary.
fn print_status(out: &mut dyn Write) -> Result<(), MeshError> {
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
    writeln!(
        out,
        "Start the daemon with `akroasis serve` for live mesh data."
    )
    .context(IoSnafu)?;
    Ok(())
}

/// Print detailed node table.
fn print_nodes(out: &mut dyn Write) -> Result<(), MeshError> {
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
    writeln!(out, "No live connection. Start the daemon for node data.").context(IoSnafu)?;
    Ok(())
}

/// Format and print a send command.
fn print_send(
    dest: &str,
    message: &str,
    channel: u8,
    no_ack: bool,
    out: &mut dyn Write,
) -> Result<(), MeshError> {
    let dest_num = parse_node_identifier(dest).ok_or_else(|| MeshError::NodeNotFound {
        identifier: dest.to_string(),
    })?;

    writeln!(out, "Sending to {dest_num} on channel {channel}:").context(IoSnafu)?;
    writeln!(out, "  \"{message}\"").context(IoSnafu)?;
    if no_ack {
        writeln!(out, "  (fire-and-forget — no ACK requested)").context(IoSnafu)?;
    } else {
        writeln!(out, "  (awaiting ACK...)").context(IoSnafu)?;
    }
    writeln!(out).context(IoSnafu)?;
    writeln!(
        out,
        "Send requires a running daemon. Use `akroasis serve` first."
    )
    .context(IoSnafu)?;

    Ok(())
}

/// Print mesh topology as adjacency list.
fn print_topology(out: &mut dyn Write) -> Result<(), MeshError> {
    writeln!(out, "Mesh Topology").context(IoSnafu)?;
    writeln!(out, "─────────────").context(IoSnafu)?;
    writeln!(
        out,
        "No live connection. Start the daemon for topology data."
    )
    .context(IoSnafu)?;
    writeln!(out).context(IoSnafu)?;
    writeln!(out, "When running, topology shows:").context(IoSnafu)?;
    writeln!(out, "  NodeA -> NodeB (SNR: -5.2 dB, hops: 1)").context(IoSnafu)?;
    writeln!(out, "  NodeB -> NodeC (SNR: -8.1 dB, hops: 1)").context(IoSnafu)?;
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
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
mod tests {
    use kerykeion::node_db::{DeviceMetrics, UserInfo};

    use super::*;

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
        dispatch(&MeshCommand::Status, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("kerykeion"));
        assert!(s.contains("Start the daemon"));
    }

    #[test]
    fn dispatch_nodes_captures_output() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Nodes, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Node"));
        assert!(s.contains("No live connection"));
    }

    #[test]
    fn dispatch_topology_captures_output() {
        let mut out = Vec::new();
        dispatch(&MeshCommand::Topology, &mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Mesh Topology"));
    }

    #[test]
    fn dispatch_send_valid_dest() {
        let mut out = Vec::new();
        assert!(
            dispatch(
                &MeshCommand::Send {
                    dest: "0x1234".into(),
                    message: "hello".into(),
                    channel: 0,
                    no_ack: false,
                },
                &mut out,
            )
            .is_ok()
        );
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("Sending to"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn dispatch_send_invalid_dest() {
        let mut out = Vec::new();
        let result = dispatch(
            &MeshCommand::Send {
                dest: "not_a_node".into(),
                message: "hello".into(),
                channel: 0,
                no_ack: false,
            },
            &mut out,
        );
        assert!(result.is_err());
    }
}
