//! Configuration types for kerykeion mesh networking.
//!
//! # Parameter Taxonomy
//!
//! This module groups behavioral tuning knobs (timeouts, retries, thresholds,
//! intervals) into per-subsystem [`serde`]-serializable structs that flow
//! through the kerykeion API as `&Config` arguments.
//!
//! Protocol invariants (e.g. [`crate::types::MAX_HOP_LIMIT`],
//! [`crate::types::MAX_PACKET_SIZE`]) and compile-time sizes remain as `const`
//! items since the Meshtastic firmware dictates them. Only tuning parameters
//! — values that can be changed without violating the on-wire protocol —
//! are exposed here.
//!
//! Every sub-config implements [`Default`] so callers that do not care about
//! tuning can use [`MeshConfig::default()`] (or a sub-config's `::default()`)
//! and get the historical hard-coded values.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::types::ChannelIndex;

/// Top-level kerykeion configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshConfig {
    /// Transport connections to Meshtastic radios.
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
    /// Channel pre-shared keys for decryption.
    #[serde(default)]
    pub channel_psk: Vec<ChannelPsk>,
    /// Store-and-forward server settings.
    #[serde(default)]
    pub store_forward: StoreForwardConfig,
    /// Topology maintenance settings.
    #[serde(default)]
    pub topology: TopologyConfig,
    /// Gateway bridge health-check and failover tuning.
    #[serde(default)]
    pub bridge: BridgeConfig,
    /// Outbound queue retry / inflight tuning.
    #[serde(default)]
    pub outbound: OutboundConfig,
    /// Transport connect + reconnect tuning.
    #[serde(default)]
    pub transport: TransportConfig,
    /// Config-dump handshake tuning.
    #[serde(default)]
    pub handshake: HandshakeConfig,
    /// Keepalive heartbeat tuning.
    #[serde(default)]
    pub heartbeat: HeartbeatConfig,
    /// Collector background-task tuning.
    #[serde(default)]
    pub collector: CollectorConfig,
    /// Outbound [`crate::message::MessageBuilder`] default values.
    #[serde(default)]
    pub message: MessageConfig,
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
    ///
    /// WHY(#229) this is checked at deserialization: the rule above was a
    /// comment, and any length was accepted. A mistyped key in a config file
    /// then travelled all the way to AES, which rejected it, and the channel was
    /// skipped as though it had simply been unencrypted.
    ///
    /// The single-byte channel-index form that [`crate::crypto::resolve_psk`]
    /// also accepts is deliberately not permitted here: that shorthand comes off
    /// the wire, and an operator writing a config means a key.
    #[serde(deserialize_with = "deserialize_psk")]
    pub psk: Vec<u8>,
}

/// Accept only the PSK lengths [`ChannelPsk::psk`] documents.
fn deserialize_psk<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;

    let bytes = Vec::<u8>::deserialize(deserializer)?;
    if matches!(
        bytes.len(),
        0 | crate::crypto::AES128_KEY_LEN | crate::crypto::AES256_KEY_LEN
    ) {
        return Ok(bytes);
    }
    Err(serde::de::Error::invalid_length(
        bytes.len(),
        &"0, 16, or 32 bytes",
    ))
}

/// Store-and-forward server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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
#[serde(default, deny_unknown_fields)]
pub struct TopologyConfig {
    /// How often to request a traceroute, in seconds.
    pub traceroute_interval_secs: u64,
    /// Seconds after which a node with no packets is considered stale.
    pub stale_node_timeout_secs: u64,
    /// Whether to request neighbor info packets from the radio.
    pub neighbor_info_enabled: bool,
    /// Node numbers manually designated as gateways.
    #[serde(default)]
    pub gateway_nodes: Vec<u32>,
    /// Ceiling SNR value used to derive Dijkstra edge cost as
    /// `cost = max(snr_ceiling - observed_snr, 0)`. Higher values flatten
    /// the cost function; lower values amplify the preference for strong
    /// links at the expense of hop count.
    #[serde(
        default = "default_snr_ceiling",
        deserialize_with = "deserialize_finite_snr_ceiling"
    )]
    pub snr_ceiling: f32,
}

#[expect(
    clippy::missing_const_for_fn,
    reason = "serde(default = \"...\") requires a named fn pointer, not a const item"
)]
fn default_snr_ceiling() -> f32 {
    30.0
}

// WHY: a non-finite snr_ceiling (NaN/inf) silently corrupts the Dijkstra
// edge-cost formula `max(snr_ceiling - observed_snr, 0)` — NaN comparisons
// are always false, poisoning every edge weight without an observable error.
// Reject at config deserialization instead of at routing time.
fn deserialize_finite_snr_ceiling<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = f32::deserialize(deserializer)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(format!(
            "snr_ceiling must be finite, got {value}"
        )))
    }
}

impl Default for TopologyConfig {
    fn default() -> Self {
        Self {
            traceroute_interval_secs: 3600,
            stale_node_timeout_secs: 7200,
            neighbor_info_enabled: true,
            gateway_nodes: Vec::new(),
            snr_ceiling: default_snr_ceiling(),
        }
    }
}

/// Gateway bridge health-monitor and failover configuration.
///
/// Controls how [`crate::bridge::GatewayBridge`] classifies gateway health,
/// decides when to fail over, and how often the background health monitor
/// ticks. None of these values affect the Meshtastic wire protocol — they
/// only shape when and how locally-tracked state transitions happen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BridgeConfig {
    /// Interval between background gateway health-check ticks, in seconds.
    pub health_check_interval_secs: u64,
    /// Response-time ceiling (in milliseconds) above which a gateway is
    /// considered degraded even if it is still responding.
    pub degraded_response_threshold_ms: u64,
    /// Packet-loss ratio (0.0..=1.0) above which a gateway is considered
    /// degraded.
    pub degraded_loss_threshold: f32,
    /// Number of consecutive failed checks before a gateway is marked offline.
    pub offline_check_threshold: u32,
    /// Minimum cooldown between failover events, in seconds. Prevents
    /// thrashing when multiple gateways flap simultaneously.
    pub failover_cooldown_secs: u64,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            health_check_interval_secs: 60,
            degraded_response_threshold_ms: 5_000,
            degraded_loss_threshold: 0.20,
            offline_check_threshold: 3,
            failover_cooldown_secs: 30,
        }
    }
}

impl BridgeConfig {
    /// Returns the health-check tick interval.
    #[must_use]
    pub const fn health_check_interval(&self) -> Duration {
        Duration::from_secs(self.health_check_interval_secs)
    }

    /// Returns the degraded-response threshold as a [`Duration`].
    #[must_use]
    pub const fn degraded_response_threshold(&self) -> Duration {
        Duration::from_millis(self.degraded_response_threshold_ms)
    }

    /// Returns the failover cooldown.
    #[must_use]
    pub const fn failover_cooldown(&self) -> Duration {
        Duration::from_secs(self.failover_cooldown_secs)
    }
}

/// Outbound queue and routing tuning parameters.
///
/// Governs how many messages may be inflight concurrently, how long to wait
/// for an ACK before retrying, how many retries are attempted, and how long
/// store-and-forward holds a message before discarding it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutboundConfig {
    /// Maximum number of concurrent inflight (awaiting-ACK) messages.
    pub max_inflight: usize,
    /// Maximum retry attempts per message before declaring delivery failure.
    pub max_retries: u8,
    /// Default ACK timeout for inflight messages, in seconds.
    pub ack_timeout_secs: u64,
    /// Default store-and-forward TTL, in seconds.
    pub store_forward_ttl_secs: u64,
    /// How long a delivery record is retained after it reaches a terminal
    /// state, and how long an unacknowledged record may stay active before it
    /// is expired, in seconds.
    ///
    /// WHY: without this bound the delivery tracker grows monotonically on a
    /// long-lived collector — terminal records are never released and records
    /// for packets that are never acknowledged stay active forever (#244).
    pub delivery_record_max_age_secs: u64,
}

impl Default for OutboundConfig {
    fn default() -> Self {
        Self {
            max_inflight: 8,
            max_retries: 5,
            ack_timeout_secs: 30,
            store_forward_ttl_secs: 3600,
            // WHY: longer than the store-and-forward TTL so a record outlives
            // the delivery attempt it describes, and still bounded so the
            // tracker cannot grow without limit.
            delivery_record_max_age_secs: 7200,
        }
    }
}

impl OutboundConfig {
    /// Returns the ACK timeout as a [`Duration`].
    #[must_use]
    pub const fn ack_timeout(&self) -> Duration {
        Duration::from_secs(self.ack_timeout_secs)
    }

    /// Returns the store-and-forward TTL as a [`Duration`].
    #[must_use]
    pub const fn store_forward_ttl(&self) -> Duration {
        Duration::from_secs(self.store_forward_ttl_secs)
    }

    /// Returns the delivery-record retention bound as a [`Duration`].
    #[must_use]
    pub const fn delivery_record_max_age(&self) -> Duration {
        Duration::from_secs(self.delivery_record_max_age_secs)
    }
}

/// Transport (TCP + serial) connect and reconnect tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TransportConfig {
    /// TCP `connect()` timeout in seconds.
    pub tcp_connect_timeout_secs: u64,
    /// Exponential-backoff ceiling for reconnection, in seconds. Applies to
    /// both TCP and serial transports.
    pub reconnect_max_backoff_secs: u64,
    /// Initial reconnection delay, in seconds. Each failed attempt doubles
    /// the delay up to [`TransportConfig::reconnect_max_backoff_secs`].
    pub reconnect_initial_delay_secs: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            tcp_connect_timeout_secs: 3,
            reconnect_max_backoff_secs: 30,
            reconnect_initial_delay_secs: 1,
        }
    }
}

impl TransportConfig {
    /// Returns the TCP connect timeout as a [`Duration`].
    #[must_use]
    pub const fn tcp_connect_timeout(&self) -> Duration {
        Duration::from_secs(self.tcp_connect_timeout_secs)
    }

    /// Returns the reconnection backoff ceiling as a [`Duration`].
    #[must_use]
    pub const fn reconnect_max_backoff(&self) -> Duration {
        Duration::from_secs(self.reconnect_max_backoff_secs)
    }

    /// Returns the initial reconnection delay as a [`Duration`].
    #[must_use]
    pub const fn reconnect_initial_delay(&self) -> Duration {
        Duration::from_secs(self.reconnect_initial_delay_secs)
    }
}

/// Config-dump handshake tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HandshakeConfig {
    /// Maximum time to wait for a complete config dump from the radio,
    /// in seconds.
    pub timeout_secs: u64,
}

impl Default for HandshakeConfig {
    fn default() -> Self {
        Self { timeout_secs: 10 }
    }
}

impl HandshakeConfig {
    /// Returns the handshake timeout as a [`Duration`].
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }
}

/// Keepalive heartbeat tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HeartbeatConfig {
    /// Interval between heartbeat transmissions, in seconds.
    pub interval_secs: u64,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self { interval_secs: 30 }
    }
}

impl HeartbeatConfig {
    /// Returns the heartbeat interval as a [`Duration`].
    #[must_use]
    pub const fn interval(&self) -> Duration {
        Duration::from_secs(self.interval_secs)
    }
}

/// Collector background-task tuning.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollectorConfig {
    /// Interval between router-flush ticks (drains outbound queue and
    /// processes timeouts), in seconds.
    pub router_flush_interval_secs: u64,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            router_flush_interval_secs: 1,
        }
    }
}

impl CollectorConfig {
    /// Returns the router-flush interval as a [`Duration`].
    #[must_use]
    pub const fn router_flush_interval(&self) -> Duration {
        Duration::from_secs(self.router_flush_interval_secs)
    }
}

/// Default values for [`crate::message::MessageBuilder`] output packets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MessageConfig {
    /// Default hop limit for outbound packets. Clamped to
    /// [`crate::types::MAX_HOP_LIMIT`] at build time — the protocol maximum
    /// is an invariant, not a tunable.
    pub default_hop_limit: u8,
}

impl Default for MessageConfig {
    fn default() -> Self {
        Self {
            default_hop_limit: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY(#229): the length rule was a doc comment and nothing enforced it, so
    /// a mistyped key reached AES, was rejected there, and the channel was
    /// skipped as though it had been unencrypted on purpose.
    #[test]
    fn a_psk_of_the_wrong_length_is_refused_at_the_config_boundary() {
        for len in [1usize, 7, 15, 17, 31, 33, 64] {
            let toml = format!(
                "[[channel_psk]]\nindex = 0\nname = \"x\"\npsk = [{}]\n",
                vec!["1"; len].join(", ")
            );
            let parsed: Result<MeshConfig, _> = toml::from_str(&toml);
            assert!(
                parsed.is_err(),
                "a {len}-byte PSK must be refused, not carried to AES"
            );
        }
    }

    /// Anti-vacuity: the documented lengths must still parse.
    #[test]
    fn the_documented_psk_lengths_parse() {
        for len in [0usize, 16, 32] {
            let toml = format!(
                "[[channel_psk]]\nindex = 0\nname = \"x\"\npsk = [{}]\n",
                vec!["1"; len].join(", ")
            );
            let parsed: Result<MeshConfig, _> = toml::from_str(&toml);
            assert!(parsed.is_ok(), "a {len}-byte PSK is documented as valid");
        }
    }

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

    // WHY: every config struct is `#[serde(default)]`, so before
    // `deny_unknown_fields` a mistyped key was silently dropped and the
    // default applied — the operator saw a running node with tuning they
    // never asked for and no diagnostic anywhere. These assert the typo is
    // now a load-time error naming the offending key.
    #[test]
    fn mistyped_key_in_sub_config_is_rejected() {
        let typo = r"
[topology]
stale_node_timeout_sec = 7200
";
        #[expect(
            clippy::expect_used,
            reason = "test-only: a successful parse here is the defect under test"
        )]
        let error = toml::from_str::<MeshConfig>(typo)
            .expect_err("a mistyped sub-config key must not deserialize");
        let rendered = error.to_string();
        assert!(
            rendered.contains("stale_node_timeout_sec"),
            "error must name the unknown key, got: {rendered}"
        );
    }

    #[test]
    fn mistyped_key_at_top_level_is_rejected() {
        let typo = r"
[topologgy]
stale_node_timeout_secs = 7200
";
        #[expect(
            clippy::expect_used,
            reason = "test-only: a successful parse here is the defect under test"
        )]
        let error = toml::from_str::<MeshConfig>(typo)
            .expect_err("a mistyped top-level section must not deserialize");
        assert!(
            error.to_string().contains("topologgy"),
            "error must name the unknown section, got: {error}"
        );
    }

    #[test]
    fn correctly_spelled_keys_still_deserialize() {
        #[expect(
            clippy::expect_used,
            reason = "test-only: SAMPLE_TOML is a known-valid fixture"
        )]
        let cfg = toml::from_str::<MeshConfig>(SAMPLE_TOML)
            .expect("the sample config must remain loadable under deny_unknown_fields");
        assert_eq!(cfg.topology.stale_node_timeout_secs, 7200);
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
        assert!((cfg.snr_ceiling - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn topology_config_rejects_non_finite_snr_ceiling() {
        // WHY: NaN/inf must be rejected at config load, not left to poison
        // Dijkstra edge-cost math (`max(snr_ceiling - observed_snr, 0)`)
        // silently at routing time.
        let nan_result: Result<TopologyConfig, _> = toml::from_str("snr_ceiling = nan");
        assert!(nan_result.is_err());

        let inf_result: Result<TopologyConfig, _> = toml::from_str("snr_ceiling = inf");
        assert!(inf_result.is_err());

        let neg_inf_result: Result<TopologyConfig, _> = toml::from_str("snr_ceiling = -inf");
        assert!(neg_inf_result.is_err());
    }

    #[test]
    fn default_bridge_config() {
        let cfg = BridgeConfig::default();
        assert_eq!(cfg.health_check_interval_secs, 60);
        assert_eq!(cfg.offline_check_threshold, 3);
        assert!((cfg.degraded_loss_threshold - 0.20).abs() < f32::EPSILON);
        assert_eq!(cfg.health_check_interval(), Duration::from_secs(60));
    }

    #[test]
    fn default_outbound_config() {
        let cfg = OutboundConfig::default();
        assert_eq!(cfg.max_inflight, 8);
        assert_eq!(cfg.max_retries, 5);
        assert_eq!(cfg.ack_timeout(), Duration::from_secs(30));
        assert_eq!(cfg.store_forward_ttl(), Duration::from_secs(3600));
    }

    #[test]
    fn default_transport_config() {
        let cfg = TransportConfig::default();
        assert_eq!(cfg.tcp_connect_timeout(), Duration::from_secs(3));
        assert_eq!(cfg.reconnect_max_backoff(), Duration::from_secs(30));
    }

    #[test]
    fn default_handshake_config() {
        let cfg = HandshakeConfig::default();
        assert_eq!(cfg.timeout(), Duration::from_secs(10));
    }

    #[test]
    fn default_heartbeat_config() {
        let cfg = HeartbeatConfig::default();
        assert_eq!(cfg.interval(), Duration::from_secs(30));
    }

    #[test]
    fn default_collector_config() {
        let cfg = CollectorConfig::default();
        assert_eq!(cfg.router_flush_interval(), Duration::from_secs(1));
    }

    #[test]
    fn default_message_config() {
        let cfg = MessageConfig::default();
        assert_eq!(cfg.default_hop_limit, 3);
    }

    #[test]
    fn mesh_config_full_serde_roundtrip() {
        // WHY: non-default values must survive TOML round-trip so agent-written
        // overrides are preserved across process restarts.
        let cfg = MeshConfig {
            connections: vec![],
            channel_psk: vec![],
            store_forward: StoreForwardConfig {
                enabled: true,
                max_queue_per_dest: 99,
                message_ttl_secs: 1234,
            },
            topology: TopologyConfig {
                traceroute_interval_secs: 111,
                stale_node_timeout_secs: 222,
                neighbor_info_enabled: false,
                gateway_nodes: vec![7, 8, 9],
                snr_ceiling: 17.5,
            },
            bridge: BridgeConfig {
                health_check_interval_secs: 15,
                degraded_response_threshold_ms: 250,
                degraded_loss_threshold: 0.33,
                offline_check_threshold: 9,
                failover_cooldown_secs: 5,
            },
            outbound: OutboundConfig {
                max_inflight: 2,
                max_retries: 1,
                ack_timeout_secs: 7,
                store_forward_ttl_secs: 11,
                delivery_record_max_age_secs: 13,
            },
            transport: TransportConfig {
                tcp_connect_timeout_secs: 8,
                reconnect_max_backoff_secs: 16,
                reconnect_initial_delay_secs: 2,
            },
            handshake: HandshakeConfig { timeout_secs: 4 },
            heartbeat: HeartbeatConfig { interval_secs: 45 },
            collector: CollectorConfig {
                router_flush_interval_secs: 3,
            },
            message: MessageConfig {
                default_hop_limit: 5,
            },
        };

        #[expect(clippy::unwrap_used, reason = "test-only: known-good values")]
        let toml_str = toml::to_string(&cfg).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only: just serialized")]
        let parsed: MeshConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.bridge.offline_check_threshold, 9);
        assert_eq!(parsed.outbound.max_inflight, 2);
        assert_eq!(parsed.transport.tcp_connect_timeout_secs, 8);
        assert_eq!(parsed.handshake.timeout_secs, 4);
        assert_eq!(parsed.heartbeat.interval_secs, 45);
        assert_eq!(parsed.collector.router_flush_interval_secs, 3);
        assert_eq!(parsed.message.default_hop_limit, 5);
        assert_eq!(parsed.topology.gateway_nodes, vec![7, 8, 9]);
        assert!((parsed.topology.snr_ceiling - 17.5).abs() < f32::EPSILON);
    }

    #[test]
    fn mesh_config_partial_toml_uses_defaults_for_unspecified_fields() {
        // WHY: agent-authored config may only override a handful of fields;
        // unspecified ones must fall through to defaults so the agent does
        // not need to know the whole schema.
        let partial = r"
[outbound]
max_inflight = 2

[bridge]
offline_check_threshold = 9
";
        #[expect(clippy::unwrap_used, reason = "test-only: known-good TOML")]
        let parsed: MeshConfig = toml::from_str(partial).unwrap();

        assert_eq!(parsed.outbound.max_inflight, 2);
        assert_eq!(
            parsed.outbound.max_retries,
            OutboundConfig::default().max_retries,
            "unspecified outbound field must default"
        );
        assert_eq!(parsed.bridge.offline_check_threshold, 9);
        assert_eq!(
            parsed.bridge.failover_cooldown_secs,
            BridgeConfig::default().failover_cooldown_secs,
            "unspecified bridge field must default"
        );
        assert_eq!(
            parsed.heartbeat.interval_secs,
            HeartbeatConfig::default().interval_secs,
            "unspecified sub-config must default wholesale"
        );
    }

    #[test]
    fn mesh_config_default_roundtrip() {
        // WHY: default config must always round-trip cleanly so agents can
        // bootstrap from an empty TOML file without errors.
        let cfg = MeshConfig::default();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let toml_str = toml::to_string(&cfg).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let parsed: MeshConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(
            parsed.bridge.health_check_interval_secs,
            cfg.bridge.health_check_interval_secs
        );
        assert_eq!(parsed.outbound.max_retries, cfg.outbound.max_retries);
    }
}
