//! κηρύκειον — Meshtastic mesh networking integration for Akroasis.
//!
//! This crate provides:
//! - Protobuf types generated from vendored Meshtastic `.proto` files
//! - Core mesh types: [`types::NodeNum`], [`types::PacketId`], [`types::ChannelIndex`]
//! - Configuration: [`config::MeshConfig`] with TOML deserialization
//! - Transport abstraction: [`connection::MeshConnection`] trait
//! - Frame codec: `codec::MeshCodec` (Meshtastic 4-byte header framing)
//! - Serial transport: [`transport::serial::SerialTransport`]
//! - TCP transport: [`transport::tcp::TcpTransport`]
//! - Config handshake: [`handshake::handshake`]
//! - AES-CTR encryption: [`crypto::encrypt`] / [`crypto::decrypt`]
//! - Heartbeat keepalive: [`heartbeat::run_heartbeat`]
//! - Node tracking: [`node_db::NodeDb`]
//! - Gateway bridge: [`bridge::GatewayBridge`] with multi-gateway failover
//! - MQTT parsing: [`mqtt`] for `ServiceEnvelope`, `MapReport` decoding
//! - Collection pipeline integration: [`collector::MeshCollector`]
//! - Mesh topology graph: [`topology::MeshTopology`]
//! - Packet dispatch: [`processor::PacketProcessor`]
//! - Routing ACK/NAK processing: [`processor::RoutingProcessor`]
//! - Node discovery: [`discovery::run_discovery`]
//! - Gateway detection: [`gateway::GatewayDetector`]
//! - Signal production: [`signals::MeshEvent`]
//! - Message construction: [`message::MessageBuilder`]
//! - Outbound queue: [`outbound::OutboundQueue`]
//! - Message routing: [`router::MeshRouter`]
//! - Delivery tracking: [`delivery::DeliveryTracker`]
//! - Store-and-forward: [`store_forward::StoreForward`]

pub mod bridge;
pub mod codec;
pub mod collector;
pub mod config;
pub mod connection;
pub mod crypto;
pub mod delivery;
pub mod discovery;
pub mod error;
pub mod gateway;
pub mod handshake;
pub mod heartbeat;
pub mod message;
pub mod mqtt;
pub mod node_db;
pub mod outbound;
pub mod processor;
pub mod router;
pub mod signals;
pub mod store_forward;
pub mod topology;
pub mod transport;
pub mod types;

// WHY: generated protobuf code cannot be annotated; allow all clippy/doc
// lints on this module. #[expect] cannot be used because some lints in the
// bundle (dead_code, unused) do not fire on every build of the generated
// output, which would trigger unfulfilled_lint_expectations.
#[allow( // kanon:ignore RUST/allow-not-expect -- generated code; expect would warn on unfulfilled bundle lints
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    missing_docs,
    dead_code,
    unused,
    reason = "generated protobuf module — bundle lints may not all fire so expect cannot be used"
)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}

pub use bridge::{GatewayBridge, GatewayEvent, GatewayHealth, GatewayState};
pub use collector::{Collector, MeshCollector};
pub use config::{ChannelPsk, ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};
pub use connection::MeshConnection;
pub use crypto::{DEFAULT_PSK, decrypt, encrypt};
pub use delivery::{DeliveryFailure, DeliveryStatus, DeliveryTracker, DestStats};
pub use discovery::{NodeState, build_traceroute_request, classify_node_state, run_discovery};
pub use error::Error;
pub use gateway::GatewayDetector;
pub use handshake::{HandshakeResult, handshake};
pub use message::MessageBuilder;
pub use mqtt::{GatewayInfo, ParsedMapReport};
pub use node_db::{DeviceMetrics, MeshNode, NodeDb, NodePosition, UserInfo};
pub use outbound::{InflightMessage, OutboundQueue, PendingMessage};
pub use processor::{PacketProcessor, RoutingProcessor, RoutingResult};
pub use proto::{FromRadio, ToRadio};
pub use router::{MeshRouter, SendOptions};
pub use signals::{MeshEvent, mesh_event_to_signal};
pub use store_forward::{StoreForward, StoredMessage};
pub use topology::{LinkQuality, MeshTopology, TopologySnapshot};
pub use types::{
    BROADCAST_ADDR, ChannelIndex, FRAME_MAGIC, MAX_CHANNELS, MAX_HOP_LIMIT, MAX_PACKET_SIZE,
    NodeNum, PacketId,
};

#[cfg(test)]
mod tests {
    use prost::Message as _;

    use crate::proto::{Data, FromRadio, MeshPacket, ToRadio, from_radio, mesh_packet, to_radio};

    fn make_mesh_packet() -> MeshPacket {
        MeshPacket {
            from: 0xDEAD_BEEF,
            to: 0xFFFF_FFFF,
            channel: 0,
            id: 0x1234_5678,
            hop_limit: 3,
            want_ack: false,
            via_mqtt: false,
            rx_snr: 4.5,
            rx_rssi: -90,
            hop_start: 3,
            priority: 0,
            rx_time: 0,
            payload_variant: Some(mesh_packet::PayloadVariant::Decoded(Data {
                portnum: 1, // TEXT_MESSAGE_APP
                payload: b"hello mesh".to_vec(),
                want_response: false,
                dest: 0,
                source: 0,
                request_id: 0,
                reply_id: 0,
                emoji: vec![],
            })),
        }
    }

    #[test]
    fn mesh_packet_encode_decode_roundtrip() {
        let original = make_mesh_packet();
        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid packet")]
        original.encode(&mut buf).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded bytes")]
        let decoded = MeshPacket::decode(buf.as_slice()).unwrap();
        assert_eq!(decoded.from, original.from);
        assert_eq!(decoded.to, original.to);
        assert_eq!(decoded.id, original.id);
    }

    #[test]
    fn to_radio_encode_decode_roundtrip() {
        let original = ToRadio {
            payload_variant: Some(to_radio::PayloadVariant::Packet(make_mesh_packet())),
        };
        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid packet")]
        original.encode(&mut buf).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded bytes")]
        let decoded = ToRadio::decode(buf.as_slice()).unwrap();
        assert!(decoded.payload_variant.is_some());
    }

    #[test]
    fn from_radio_encode_decode_roundtrip() {
        let original = FromRadio {
            id: 42,
            payload_variant: Some(from_radio::PayloadVariant::Packet(make_mesh_packet())),
        };
        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid packet")]
        original.encode(&mut buf).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded bytes")]
        let decoded = FromRadio::decode(buf.as_slice()).unwrap();
        assert_eq!(decoded.id, 42);
        assert!(decoded.payload_variant.is_some());
    }
}
