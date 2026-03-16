//! Kerykeion — Meshtastic mesh networking for Akroasis.

pub mod collector;
pub mod config;
pub mod connection;
pub mod error;
pub mod node_db;
pub mod types;

/// Generated Meshtastic protobuf types (from build.rs / prost-build).
#[allow(
    missing_docs,
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::derive_partial_eq_without_eq
)]
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/meshtastic.rs"));
}

pub use collector::MeshCollector;
pub use config::{ChannelPsk, ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};
pub use connection::MeshConnection;
pub use error::Error;
pub use node_db::{DeviceMetrics, MeshNode, NodeDb, NodePosition, UserInfo};
pub use types::{
    BROADCAST_ADDR, ChannelIndex, FRAME_MAGIC, MAX_CHANNELS, MAX_HOP_LIMIT, MAX_PACKET_SIZE,
    NodeNum, PacketId,
};

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use prost::Message;

    use super::*;

    #[test]
    fn protobuf_roundtrip_mesh_packet() {
        let packet = proto::MeshPacket {
            from: 0xAABB_CCDD,
            to: 0xFFFF_FFFF,
            channel: 0,
            id: 12345,
            hop_limit: 3,
            want_ack: true,
            ..Default::default()
        };

        let encoded = packet.encode_to_vec();
        let decoded = proto::MeshPacket::decode(encoded.as_slice()).expect("decode");

        assert_eq!(decoded.from, packet.from);
        assert_eq!(decoded.to, packet.to);
        assert_eq!(decoded.id, packet.id);
        assert_eq!(decoded.hop_limit, packet.hop_limit);
        assert!(decoded.want_ack);
    }

    #[test]
    fn protobuf_roundtrip_to_radio() {
        use proto::to_radio::PayloadVariant;

        let to_radio = proto::ToRadio {
            payload_variant: Some(PayloadVariant::WantConfigId(42)),
        };

        let encoded = to_radio.encode_to_vec();
        let decoded = proto::ToRadio::decode(encoded.as_slice()).expect("decode");

        match decoded.payload_variant {
            Some(PayloadVariant::WantConfigId(id)) => assert_eq!(id, 42),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn protobuf_roundtrip_from_radio() {
        use proto::from_radio::PayloadVariant;

        let from_radio = proto::FromRadio {
            id: 7,
            payload_variant: Some(PayloadVariant::ConfigCompleteId(42)),
        };

        let encoded = from_radio.encode_to_vec();
        let decoded = proto::FromRadio::decode(encoded.as_slice()).expect("decode");

        assert_eq!(decoded.id, 7);
        match decoded.payload_variant {
            Some(PayloadVariant::ConfigCompleteId(id)) => assert_eq!(id, 42),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn collector_name() {
        let collector = MeshCollector::new();
        assert_eq!(collector.name(), "kerykeion");
    }

    #[test]
    fn collector_run_stub_succeeds() {
        let collector = MeshCollector::new();
        collector.run().expect("stub should succeed");
    }

    #[test]
    fn collector_probe_empty_registry() {
        let registry = koinon::AssetRegistry::new();
        let devices = MeshCollector::probe(&registry);
        assert!(devices.is_empty());
    }

    #[test]
    fn mesh_node_serde_roundtrip() {
        let node = MeshNode {
            num: NodeNum(0x1234),
            user: Some(UserInfo {
                long_name: "Alice Node".into(),
                short_name: "ALIC".into(),
                hw_model: 42,
            }),
            position: Some(NodePosition {
                latitude: 51.5074,
                longitude: -0.1278,
                altitude: Some(11.0),
            }),
            metrics: Some(DeviceMetrics {
                battery_level: 85,
                voltage: 3.7,
                uptime_secs: 3600,
            }),
            last_heard: None,
            snr: Some(9.5),
            hop_count: Some(2),
        };

        let json = serde_json::to_string(&node).expect("serialize");
        let reparsed: MeshNode = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(reparsed.num, node.num);
        assert_eq!(
            reparsed.user.as_ref().expect("user").long_name,
            "Alice Node"
        );
        assert_eq!(reparsed.snr, Some(9.5));
    }
}
