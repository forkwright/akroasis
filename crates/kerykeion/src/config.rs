//! Configuration types for kerykeion mesh networking.

use crate::types::ChannelIndex;
use serde::{Deserialize, Serialize};

/// Top-level kerykeion configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Transport connections to Meshtastic radios.
    pub connections: Vec<ConnectionConfig>,
    /// Channel pre-shared keys for decryption.
    pub channel_psk: Vec<ChannelPsk>,
    /// Store-and-forward server settings.
    pub store_forward: StoreForwardConfig,
    /// Topology maintenance settings.
    pub topology: TopologyConfig,
}

/// Transport backend for a single Meshtastic radio connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ConnectionConfig {
    /// USB serial connection.
    Serial {
        /// Serial port device path (e.g. `/dev/ttyUSB0`).
        port: String,
        /// Baud rate. Meshtastic uses `115200`.
        baud: u32,
    },
    /// TCP/IP connection (Meshtastic `WiFi` firmware).
    Tcp {
        /// Hostname or IP address.
        addr: String,
        /// TCP port number. Meshtastic default is `4403`.
        port: u16,
    },
    /// Bluetooth Low Energy GATT connection.
    Ble {
        /// Advertising name prefix of the target device.
        device_name: String,
    },
}

/// Pre-shared key material for a single mesh channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPsk {
    /// Channel index this PSK applies to.
    pub index: ChannelIndex,
    /// Human-readable channel name.
    pub name: String,
    /// Raw PSK bytes. Must be 0, 16, or 32 bytes.
    ///
    /// - 0 bytes: no encryption (use only for public channels).
    /// - 16 bytes: AES-128-CTR.
    /// - 32 bytes: AES-256-CTR.
    pub psk: Vec<u8>,
}

/// Store-and-forward server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreForwardConfig {
    /// Whether the store-and-forward feature is enabled on this node.
    pub enabled: bool,
    /// Maximum number of queued messages per destination node.
    pub max_queue_per_dest: usize,
    /// Number of seconds before a queued message expires.
    pub message_ttl_secs: u64,
}

impl Default for StoreForwardConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_queue_per_dest: 16,
            message_ttl_secs: 3600,
        }
    }
}

/// Topology maintenance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyConfig {
    /// How often to request a traceroute, in seconds.
    pub traceroute_interval_secs: u64,
    /// Seconds after which a node with no packets is considered stale.
    pub stale_node_timeout_secs: u64,
    /// Whether to request neighbor info packets from the radio.
    pub neighbor_info_enabled: bool,
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            traceroute_interval_secs: 3600,
            stale_node_timeout_secs: 7200,
            neighbor_info_enabled: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_TOML: &str = r#"
[[connections]]
Serial = { port = "/dev/ttyUSB0", baud = 115200 }

[[connections]]
Tcp = { addr = "192.168.1.100", port = 4403 }

[[channel_psk]]
index = 0
name = "LongFast"
psk = []

[store_forward]
enabled = false
max_queue_per_dest = 16
message_ttl_secs = 3600

[topology]
traceroute_interval_secs = 3600
stale_node_timeout_secs = 7200
neighbor_info_enabled = true
"#;

    #[test]
    fn deserialize_mesh_config_from_toml() {
        #[expect(clippy::unwrap_used, reason = "test-only: TOML is known valid")]
        let cfg: MeshConfig = toml::from_str(SAMPLE_TOML).unwrap();
        assert_eq!(cfg.connections.len(), 2);
        assert_eq!(cfg.channel_psk.len(), 1);
        assert!(!cfg.store_forward.enabled);
        assert!(cfg.topology.neighbor_info_enabled);
    }

    #[test]
    fn default_store_forward_config() {
        let cfg = StoreForwardConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_queue_per_dest, 16);
    }

    #[test]
    fn default_topology_config() {
        let cfg = TopologyConfig::default();
        assert!(cfg.neighbor_info_enabled);
        assert_eq!(cfg.stale_node_timeout_secs, 7200);
    }
}
