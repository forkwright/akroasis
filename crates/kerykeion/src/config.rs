//! Mesh network configuration with TOML deserialization.

use serde::{Deserialize, Serialize};

use crate::types::ChannelIndex;

/// Top-level mesh configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Device connection endpoints.
    pub connections: Vec<ConnectionConfig>,
    /// Per-channel pre-shared keys.
    pub channel_psk: Vec<ChannelPsk>,
    /// Store-and-forward relay settings.
    pub store_forward: StoreForwardConfig,
    /// Topology discovery settings.
    pub topology: TopologyConfig,
}

/// A single device connection endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConnectionConfig {
    /// Serial port connection.
    Serial {
        /// Device path (e.g. `/dev/ttyUSB0`).
        port: String,
        /// Baud rate in bits per second.
        baud: u32,
    },
    /// TCP socket connection.
    Tcp {
        /// Hostname or IP address.
        addr: String,
        /// TCP port number.
        port: u16,
    },
    /// Bluetooth Low Energy connection.
    Ble {
        /// BLE device name or address.
        device_name: String,
    },
}

/// Pre-shared key for a Meshtastic channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPsk {
    /// Channel slot index (0–7).
    pub index: ChannelIndex,
    /// Human-readable channel name.
    pub name: String,
    /// Pre-shared key bytes (0, 16, or 32 bytes).
    pub psk: Vec<u8>,
}

/// Store-and-forward relay configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreForwardConfig {
    /// Whether store-and-forward is enabled.
    pub enabled: bool,
    /// Maximum queued messages per destination node.
    pub max_queue_per_dest: usize,
    /// Time-to-live for queued messages in seconds.
    pub message_ttl_secs: u64,
}

/// Topology discovery configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyConfig {
    /// Interval between traceroute probes in seconds.
    pub traceroute_interval_secs: u64,
    /// Duration after which a node is considered stale, in seconds.
    pub stale_node_timeout_secs: u64,
    /// Whether to request neighbor info from nodes.
    pub neighbor_info_enabled: bool,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    const TOML_CONFIG: &str = r#"
[[connections]]
type = "serial"
port = "/dev/ttyUSB0"
baud = 115200

[[connections]]
type = "tcp"
addr = "192.0.2.1"
port = 4403

[[connections]]
type = "ble"
device_name = "Meshtastic_abcd"

[[channel_psk]]
index = 0
name = "Default"
psk = [1]

[store_forward]
enabled = true
max_queue_per_dest = 100
message_ttl_secs = 3600

[topology]
traceroute_interval_secs = 900
stale_node_timeout_secs = 7200
neighbor_info_enabled = true
"#;

    #[test]
    fn deserialize_full_config_from_toml() {
        let config: MeshConfig = toml::from_str(TOML_CONFIG).expect("valid TOML");
        assert_eq!(config.connections.len(), 3);
        assert_eq!(config.channel_psk.len(), 1);
        assert!(config.store_forward.enabled);
        assert_eq!(config.topology.traceroute_interval_secs, 900);
    }

    #[test]
    fn serial_connection_parsed_correctly() {
        let config: MeshConfig = toml::from_str(TOML_CONFIG).expect("valid TOML");
        match &config.connections[0] {
            ConnectionConfig::Serial { port, baud } => {
                assert_eq!(port, "/dev/ttyUSB0");
                assert_eq!(*baud, 115_200);
            }
            other => panic!("expected Serial, got {other:?}"),
        }
    }

    #[test]
    fn tcp_connection_parsed_correctly() {
        let config: MeshConfig = toml::from_str(TOML_CONFIG).expect("valid TOML");
        match &config.connections[1] {
            ConnectionConfig::Tcp { addr, port } => {
                assert_eq!(addr, "192.0.2.1");
                assert_eq!(*port, 4403);
            }
            other => panic!("expected Tcp, got {other:?}"),
        }
    }

    #[test]
    fn config_roundtrip_through_toml() {
        let config: MeshConfig = toml::from_str(TOML_CONFIG).expect("parse");
        let serialized = toml::to_string(&config).expect("serialize");
        let reparsed: MeshConfig = toml::from_str(&serialized).expect("reparse");
        assert_eq!(reparsed.connections.len(), config.connections.len());
        assert_eq!(
            reparsed.topology.stale_node_timeout_secs,
            config.topology.stale_node_timeout_secs
        );
    }
}
