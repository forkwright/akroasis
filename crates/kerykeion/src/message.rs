//! Outbound message construction for Meshtastic mesh packets.

use prost::Message as _;
use rand_core::{OsRng, RngCore as _};

use crate::crypto;
use crate::error::Error;
use crate::proto::mesh_packet::Priority;
use crate::proto::{AdminMessage, Data, MeshPacket, PortNum, Position, mesh_packet};
use crate::types::{ChannelIndex, MAX_HOP_LIMIT, NodeNum};

/// Default hop LIMIT for outbound packets.
const DEFAULT_HOP_LIMIT: u8 = 3;

/// Constructs outbound [`MeshPacket`] messages with a builder pattern.
///
/// # Examples
///
/// ```ignore
/// let packet = MessageBuilder::text(NodeNum(0x1234), "hello")
///     .with_ack()
///     .build(NodeNum(0xABCD), &[0x01])?;
/// ```
pub struct MessageBuilder {
    dest: NodeNum,
    portnum: PortNum,
    payload: Vec<u8>,
    channel: ChannelIndex,
    want_ack: bool,
    hop_limit: u8,
    priority: Priority,
}

impl MessageBuilder {
    /// Build a text message (`TEXT_MESSAGE_APP`).
    #[must_use]
    pub fn text(dest: NodeNum, text: &str) -> Self {
        Self {
            dest,
            portnum: PortNum::TextMessageApp,
            payload: text.as_bytes().to_vec(),
            channel: ChannelIndex(0),
            want_ack: false,
            hop_limit: DEFAULT_HOP_LIMIT,
            priority: Priority::Default,
        }
    }

    /// Build a position message (`POSITION_APP`).
    ///
    /// Latitude and longitude are in decimal degrees; they are converted to
    /// Meshtastic's `i32` representation (`value * 1e7`).
    #[must_use]
    pub fn position(dest: NodeNum, lat: f64, lon: f64) -> Self {
        // WHY: Meshtastic firmware stores lat/lon as fixed-point i32 = degrees * 1e7.
        #[expect(
            clippy::as_conversions,
            reason = "f64→i32 via multiplication is the Meshtastic wire format convention"
        )]
        let pos = Position {
            latitude_i: (lat * 1e7) as i32,
            longitude_i: (lon * 1e7) as i32,
            ..Default::default()
        };
        Self {
            dest,
            portnum: PortNum::PositionApp,
            payload: pos.encode_to_vec(),
            channel: ChannelIndex(0),
            want_ack: false,
            hop_limit: DEFAULT_HOP_LIMIT,
            priority: Priority::Default,
        }
    }

    /// Build an admin message (`ADMIN_APP`).
    #[must_use]
    pub fn admin(dest: NodeNum, admin_msg: &AdminMessage) -> Self {
        Self {
            dest,
            portnum: PortNum::AdminApp,
            payload: admin_msg.encode_to_vec(),
            channel: ChannelIndex(0),
            want_ack: true,
            hop_limit: DEFAULT_HOP_LIMIT,
            priority: Priority::Reliable,
        }
    }

    /// Build a traceroute request (`TRACEROUTE_APP`).
    #[must_use]
    pub const fn traceroute(dest: NodeNum) -> Self {
        Self {
            dest,
            portnum: PortNum::TracerouteApp,
            payload: Vec::new(),
            channel: ChannelIndex(0),
            want_ack: true,
            hop_limit: MAX_HOP_LIMIT,
            priority: Priority::Reliable,
        }
    }

    /// Set the channel index for this message.
    #[must_use]
    pub const fn channel(mut self, ch: ChannelIndex) -> Self {
        self.channel = ch;
        self
    }

    /// Request an acknowledgement for this message.
    #[must_use]
    pub const fn with_ack(mut self) -> Self {
        self.want_ack = true;
        self
    }

    /// Set the hop LIMIT (clamped to [`MAX_HOP_LIMIT`]).
    #[must_use]
    pub const fn hop_limit(mut self, limit: u8) -> Self {
        if limit > MAX_HOP_LIMIT {
            self.hop_limit = MAX_HOP_LIMIT;
        } else {
            self.hop_limit = limit;
        }
        self
    }

    /// Set the delivery priority.
    #[must_use]
    pub const fn priority(mut self, p: Priority) -> Self {
        self.priority = p;
        self
    }

    /// Consume the builder and produce an encrypted [`MeshPacket`].
    ///
    /// A random `packet_id` is assigned. The payload is encrypted using
    /// AES-CTR with the provided PSK.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Encryption`] if encryption fails (e.g. invalid PSK length).
    pub fn build(self, from: NodeNum, psk: &[u8]) -> Result<MeshPacket, Error> {
        let packet_id = OsRng.next_u32();

        let data = Data {
            portnum: i32::from(self.portnum),
            payload: self.payload,
            want_response: false,
            dest: 0,
            source: 0,
            request_id: 0,
            reply_id: 0,
            emoji: vec![],
        };
        let plaintext = data.encode_to_vec();

        let encrypted = crypto::encrypt(&plaintext, packet_id, from.0, psk)?;

        // WHY: if PSK is empty, the channel is unencrypted  -  send as Decoded.
        let payload_variant = if psk.is_empty() {
            Some(mesh_packet::PayloadVariant::Decoded(data))
        } else {
            Some(mesh_packet::PayloadVariant::Encrypted(encrypted))
        };

        Ok(MeshPacket {
            from: from.0,
            to: self.dest.0,
            channel: u32::from(self.channel.0),
            id: packet_id,
            rx_time: 0,
            rx_snr: 0.0,
            hop_limit: u32::from(self.hop_limit),
            want_ack: self.want_ack,
            priority: i32::from(self.priority),
            rx_rssi: 0,
            via_mqtt: false,
            hop_start: u32::from(self.hop_limit),
            payload_variant,
        })
    }

    /// Consume the builder and produce an encrypted [`MeshPacket`] with a deterministic packet ID.
    ///
    /// Useful for testing. Production code should use [`build`](Self::build).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Encryption`] if encryption fails.
    #[cfg(test)]
    pub fn build_with_id(
        self,
        from: NodeNum,
        psk: &[u8],
        packet_id: u32,
    ) -> Result<MeshPacket, Error> {
        let data = Data {
            portnum: i32::from(self.portnum),
            payload: self.payload,
            want_response: false,
            dest: 0,
            source: 0,
            request_id: 0,
            reply_id: 0,
            emoji: vec![],
        };
        let plaintext = data.encode_to_vec();
        let encrypted = crypto::encrypt(&plaintext, packet_id, from.0, psk)?;

        let payload_variant = if psk.is_empty() {
            Some(mesh_packet::PayloadVariant::Decoded(data))
        } else {
            Some(mesh_packet::PayloadVariant::Encrypted(encrypted))
        };

        Ok(MeshPacket {
            from: from.0,
            to: self.dest.0,
            channel: u32::from(self.channel.0),
            id: packet_id,
            rx_time: 0,
            rx_snr: 0.0,
            hop_limit: u32::from(self.hop_limit),
            want_ack: self.want_ack,
            priority: i32::from(self.priority),
            rx_rssi: 0,
            via_mqtt: false,
            hop_start: u32::from(self.hop_limit),
            payload_variant,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::mesh_packet::PayloadVariant;

    const FROM_NODE: NodeNum = NodeNum(0xDEAD_BEEF);
    const DEST: NodeNum = NodeNum(0x1234_5678);

    #[test]
    fn text_message_sets_portnum_and_encrypts() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::text(DEST, "hello mesh")
            .build(FROM_NODE, &[0x01])
            .unwrap();

        assert_eq!(pkt.from, FROM_NODE.0);
        assert_eq!(pkt.to, DEST.0);
        assert!(pkt.payload_variant.is_some());
        // With a non-empty PSK the payload should be encrypted.
        assert!(
            matches!(&pkt.payload_variant, Some(PayloadVariant::Encrypted(_))),
            "expected encrypted payload"
        );
    }

    #[test]
    fn text_message_unencrypted_when_empty_psk() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::text(DEST, "cleartext")
            .build(FROM_NODE, &[])
            .unwrap();

        assert!(
            matches!(&pkt.payload_variant, Some(PayloadVariant::Decoded(d)) if d.portnum == i32::from(PortNum::TextMessageApp)),
            "expected decoded TEXT_MESSAGE_APP payload"
        );
    }

    #[test]
    fn position_message_encodes_lat_lon() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::position(DEST, 37.7749, -122.4194)
            .build(FROM_NODE, &[])
            .unwrap();

        let Some(PayloadVariant::Decoded(data)) = &pkt.payload_variant else {
            unreachable!("expected decoded position payload");
        };
        assert_eq!(data.portnum, i32::from(PortNum::PositionApp));
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pos = Position::decode(data.payload.as_slice()).unwrap();
        // 37.7749 * 1e7 ≈ 377749000
        assert!(
            (pos.latitude_i - 377_749_000).unsigned_abs() < 10,
            "latitude_i={} expected ~377749000",
            pos.latitude_i
        );
    }

    #[test]
    fn admin_message_sets_reliable_priority() {
        let admin = AdminMessage {
            payload_variant: None,
        };
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::admin(DEST, &admin)
            .build(FROM_NODE, &[])
            .unwrap();
        assert_eq!(pkt.priority, i32::from(Priority::Reliable));
        assert!(pkt.want_ack, "admin messages should request ACK");
    }

    #[test]
    fn traceroute_uses_max_hop_limit() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::traceroute(DEST)
            .build(FROM_NODE, &[])
            .unwrap();
        assert_eq!(pkt.hop_limit, u32::from(MAX_HOP_LIMIT));
        assert!(pkt.want_ack, "traceroute should request ACK");
    }

    #[test]
    fn builder_chain_methods() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::text(DEST, "test")
            .channel(ChannelIndex(2))
            .with_ack()
            .hop_limit(5)
            .priority(Priority::Reliable)
            .build(FROM_NODE, &[])
            .unwrap();

        assert_eq!(pkt.channel, 2);
        assert!(pkt.want_ack);
        assert_eq!(pkt.hop_limit, 5);
        assert_eq!(pkt.priority, i32::from(Priority::Reliable));
    }

    #[test]
    fn hop_limit_clamped_to_max() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::text(DEST, "test")
            .hop_limit(100)
            .build(FROM_NODE, &[])
            .unwrap();
        assert_eq!(pkt.hop_limit, u32::from(MAX_HOP_LIMIT));
    }

    #[test]
    fn build_with_id_deterministic() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pkt = MessageBuilder::text(DEST, "test")
            .build_with_id(FROM_NODE, &[], 0xCAFE)
            .unwrap();
        assert_eq!(pkt.id, 0xCAFE);
    }
}
