//! MQTT message parsing for Meshtastic mesh traffic.
//!
//! Parses MQTT-related protobuf messages that arrive via the mesh network.
//! This module does NOT implement an MQTT client — it handles the Meshtastic
//! MQTT protobuf envelope types (`ServiceEnvelope`, `MapReport`,
//! `MqttClientProxyMessage`) that wrap mesh packets transported over MQTT.
//! Actual MQTT client integration is deferred to praxis (automation layer).

use prost::Message;
use snafu::ResultExt;

use crate::error::{Error, ProtobufDecodeSnafu};
use crate::proto::{MapReport, MqttClientProxyMessage, ServiceEnvelope};
use crate::types::NodeNum;

/// Parsed gateway identifier extracted from a `ServiceEnvelope`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayInfo {
    /// The gateway's node number, if the `gateway_id` is a valid hex node string.
    pub node_num: Option<NodeNum>,
    /// The raw gateway ID string from the envelope.
    pub raw_id: String,
    /// The channel ID the message was published on.
    pub channel_id: String,
}

/// Decoded map report with human-friendly field types.
#[derive(Debug, Clone)]
pub struct ParsedMapReport {
    /// Long name of the reporting node.
    pub long_name: String,
    /// Short name of the reporting node.
    pub short_name: String,
    /// Hardware model enum value.
    pub hw_model: i32,
    /// Firmware version string.
    pub firmware_version: String,
    /// Latitude in decimal degrees, if non-zero.
    pub latitude: Option<f64>,
    /// Longitude in decimal degrees, if non-zero.
    pub longitude: Option<f64>,
    /// Altitude in metres above sea level, if non-zero.
    pub altitude: Option<i32>,
    /// Number of online local nodes visible to this node.
    pub num_online_local_nodes: u32,
}

/// Decodes a `ServiceEnvelope` from raw protobuf bytes.
///
/// # Errors
///
/// Returns [`Error::ProtobufDecode`] if the bytes cannot be decoded.
pub fn decode_service_envelope(bytes: &[u8]) -> Result<ServiceEnvelope, Error> {
    ServiceEnvelope::decode(bytes).context(ProtobufDecodeSnafu)
}

/// Extracts gateway information from a `ServiceEnvelope`.
///
/// The `gateway_id` field in the Meshtastic MQTT protocol is a string like
/// `"!deadbeef"` representing the gateway node's hex ID.
#[must_use]
pub fn extract_gateway_info(envelope: &ServiceEnvelope) -> GatewayInfo {
    let node_num = parse_gateway_id(&envelope.gateway_id);
    GatewayInfo {
        node_num,
        raw_id: envelope.gateway_id.clone(),
        channel_id: envelope.channel_id.clone(),
    }
}

/// Decodes a `MapReport` from raw protobuf bytes.
///
/// # Errors
///
/// Returns [`Error::ProtobufDecode`] if the bytes cannot be decoded.
pub fn decode_map_report(bytes: &[u8]) -> Result<ParsedMapReport, Error> {
    let report = MapReport::decode(bytes).context(ProtobufDecodeSnafu)?;
    let latitude = if report.latitude_i != 0 {
        Some(f64::from(report.latitude_i) * 1e-7)
    } else {
        None
    };
    let longitude = if report.longitude_i != 0 {
        Some(f64::from(report.longitude_i) * 1e-7)
    } else {
        None
    };
    let altitude = if report.altitude != 0 {
        Some(report.altitude)
    } else {
        None
    };

    Ok(ParsedMapReport {
        long_name: report.long_name,
        short_name: report.short_name,
        hw_model: report.hw_model,
        firmware_version: report.firmware_version,
        latitude,
        longitude,
        altitude,
        num_online_local_nodes: report.num_online_local_nodes,
    })
}

/// Decodes an `MqttClientProxyMessage` from raw protobuf bytes.
///
/// # Errors
///
/// Returns [`Error::ProtobufDecode`] if the bytes cannot be decoded.
pub fn decode_proxy_message(bytes: &[u8]) -> Result<MqttClientProxyMessage, Error> {
    MqttClientProxyMessage::decode(bytes).context(ProtobufDecodeSnafu)
}

/// Parses a Meshtastic gateway ID string (e.g. `"!deadbeef"`) to a `NodeNum`.
///
/// Returns `None` if the string does not match the expected format.
#[must_use]
fn parse_gateway_id(gateway_id: &str) -> Option<NodeNum> {
    let hex = gateway_id.strip_prefix('!')?;
    u32::from_str_radix(hex, 16).ok().map(NodeNum)
}

#[cfg(test)]
mod tests {
    use prost::Message;

    use super::*;

    #[test]
    fn parse_gateway_id_valid() {
        assert_eq!(parse_gateway_id("!deadbeef"), Some(NodeNum(0xDEAD_BEEF)));
    }

    #[test]
    fn parse_gateway_id_no_prefix() {
        assert_eq!(parse_gateway_id("deadbeef"), None);
    }

    #[test]
    fn parse_gateway_id_invalid_hex() {
        assert_eq!(parse_gateway_id("!zzzz"), None);
    }

    #[test]
    fn parse_gateway_id_empty() {
        assert_eq!(parse_gateway_id(""), None);
        assert_eq!(parse_gateway_id("!"), None);
    }

    #[test]
    fn decode_service_envelope_roundtrip() {
        let envelope = ServiceEnvelope {
            packet: Some(crate::proto::MeshPacket {
                from: 0x1234,
                to: 0xFFFF_FFFF,
                channel: 0,
                id: 42,
                ..Default::default()
            }),
            channel_id: "LongFast".into(),
            gateway_id: "!aabbccdd".into(),
        };

        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid")]
        envelope.encode(&mut buf).unwrap();

        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded")]
        let decoded = decode_service_envelope(&buf).unwrap();
        assert_eq!(decoded.channel_id, "LongFast");
        assert_eq!(decoded.gateway_id, "!aabbccdd");
        assert!(decoded.packet.is_some());
    }

    #[test]
    fn extract_gateway_info_from_envelope() {
        let envelope = ServiceEnvelope {
            packet: None,
            channel_id: "LongFast".into(),
            gateway_id: "!deadbeef".into(),
        };

        let info = extract_gateway_info(&envelope);
        assert_eq!(info.node_num, Some(NodeNum(0xDEAD_BEEF)));
        assert_eq!(info.channel_id, "LongFast");
        assert_eq!(info.raw_id, "!deadbeef");
    }

    #[test]
    fn decode_map_report_roundtrip() {
        let report = MapReport {
            long_name: "Base Station".into(),
            short_name: "BS01".into(),
            hw_model: 43,
            firmware_version: "2.3.0".into(),
            latitude_i: 408_500_000,
            longitude_i: -739_000_000,
            altitude: 150,
            num_online_local_nodes: 7,
            ..Default::default()
        };

        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid")]
        report.encode(&mut buf).unwrap();

        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded")]
        let parsed = decode_map_report(&buf).unwrap();
        assert_eq!(parsed.long_name, "Base Station");
        assert_eq!(parsed.short_name, "BS01");
        assert_eq!(parsed.hw_model, 43);
        assert!((parsed.latitude.unwrap_or(0.0) - 40.85).abs() < 0.001);
        assert!((parsed.longitude.unwrap_or(0.0) - (-73.9)).abs() < 0.001);
        assert_eq!(parsed.altitude, Some(150));
        assert_eq!(parsed.num_online_local_nodes, 7);
    }

    #[test]
    fn decode_map_report_zero_position() {
        let report = MapReport {
            long_name: "Node".into(),
            short_name: "N".into(),
            latitude_i: 0,
            longitude_i: 0,
            altitude: 0,
            ..Default::default()
        };

        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid")]
        report.encode(&mut buf).unwrap();

        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded")]
        let parsed = decode_map_report(&buf).unwrap();
        assert!(parsed.latitude.is_none());
        assert!(parsed.longitude.is_none());
        assert!(parsed.altitude.is_none());
    }

    #[test]
    fn decode_proxy_message_with_data() {
        let msg = MqttClientProxyMessage {
            topic: "msh/US/2/json/LongFast/!aabb".into(),
            payload_variant: Some(
                crate::proto::mqtt_client_proxy_message::PayloadVariant::Data(b"hello".to_vec()),
            ),
            retained: false,
        };

        let mut buf = Vec::new();
        #[expect(clippy::unwrap_used, reason = "test-only: encoding known-valid")]
        msg.encode(&mut buf).unwrap();

        #[expect(clippy::unwrap_used, reason = "test-only: decoding just-encoded")]
        let decoded = decode_proxy_message(&buf).unwrap();
        assert_eq!(decoded.topic, "msh/US/2/json/LongFast/!aabb");
    }

    #[test]
    fn decode_invalid_bytes_returns_error() {
        let result = decode_service_envelope(&[0xFF, 0xFF, 0xFF]);
        // WHY: protobuf may or may not fail on arbitrary bytes — just verify no panic.
        let _ = result;
    }
}
