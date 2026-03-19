//! κηρύκειον — Meshtastic mesh networking integration for Akroasis.
//!
//! This crate provides:
//! - Protobuf types generated from vendored Meshtastic `.proto` files
//! - Core mesh types: [`types::NodeNum`], [`types::PacketId`], [`types::ChannelIndex`]
//! - Configuration: [`config::MeshConfig`] with TOML deserialization
//! - Transport abstraction: [`connection::MeshConnection`] trait
//! - Node tracking: [`node_db::NodeDb`]
//! - Collection pipeline integration: [`collector::MeshCollector`]
//!
//! Protocol implementations (serial, TCP, BLE) are added in P2-02.

pub mod collector;
pub mod config;
pub mod connection;
pub mod error;
pub mod node_db;
pub mod types;

// WHY: generated protobuf code cannot be annotated; allow all lints on this module.
#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    missing_docs,
    dead_code,
    unused
)]
pub(crate) mod proto {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}

pub use collector::{Collector, MeshCollector};
pub use config::{ChannelPsk, ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};
pub use error::Error;
pub use node_db::{DeviceMetrics, MeshNode, NodeDb, NodePosition, UserInfo};
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
