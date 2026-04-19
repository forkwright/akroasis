//! Core mesh networking type primitives.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Node number: the last four bytes of the node's MAC address, encoded as a `u32`.
///
/// `0xFFFF_FFFF` is the broadcast address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeNum(pub u32);

/// Packet identifier: a random 32-bit value assigned by the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PacketId(pub u32);

/// Channel index in the range `0..MAX_CHANNELS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct ChannelIndex(pub u8);

impl TryFrom<u8> for ChannelIndex {
    type Error = crate::error::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ChannelIndex> for u8 {
    fn from(value: ChannelIndex) -> Self {
        value.0
    }
}

impl NodeNum {
    /// Returns the broadcast node number (`0xFFFF_FFFF`).
    #[must_use]
    pub const fn broadcast() -> Self {
        BROADCAST_ADDR
    }

    /// Returns `true` if this is the broadcast address.
    #[must_use]
    pub const fn is_broadcast(self) -> bool {
        self.0 == BROADCAST_ADDR.0
    }
}

impl ChannelIndex {
    /// Constructs a `ChannelIndex`, returning an error if `index >= MAX_CHANNELS`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::InvalidChannel`] if `index >= MAX_CHANNELS`.
    pub fn new(index: u8) -> Result<Self, crate::error::Error> {
        if index < MAX_CHANNELS {
            Ok(Self(index))
        } else {
            Err(crate::error::Error::InvalidChannel {
                index,
                location: snafu::Location::new(file!(), line!(), column!()),
            })
        }
    }
}

impl fmt::Display for NodeNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#010x}", self.0)
    }
}

impl fmt::Display for PacketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#010x}", self.0)
    }
}

impl fmt::Display for ChannelIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Broadcast destination: all nodes on the mesh.
pub const BROADCAST_ADDR: NodeNum = NodeNum(0xFFFF_FFFF);

/// Maximum number of channels a Meshtastic device supports.
pub const MAX_CHANNELS: u8 = 8;

/// Maximum hop limit for a mesh packet.
pub const MAX_HOP_LIMIT: u8 = 7;

/// Maximum protobuf payload size enforced by Meshtastic firmware.
pub const MAX_PACKET_SIZE: usize = 512;

/// Two-byte magic header that begins every Meshtastic serial frame.
pub const FRAME_MAGIC: [u8; 2] = [0x94, 0xC3];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcast_addr_value() {
        assert_eq!(BROADCAST_ADDR.0, 0xFFFF_FFFF);
    }

    #[test]
    fn node_num_is_broadcast() {
        assert!(BROADCAST_ADDR.is_broadcast());
        assert!(!NodeNum(0x1234_5678).is_broadcast());
    }

    #[test]
    fn node_num_display() {
        assert_eq!(NodeNum(0xABCD_1234).to_string(), "0xabcd1234");
    }

    #[test]
    fn packet_id_display() {
        assert_eq!(PacketId(1).to_string(), "0x00000001");
    }

    #[test]
    fn channel_index_valid() {
        assert!(ChannelIndex::new(0).is_ok());
        assert!(ChannelIndex::new(7).is_ok());
    }

    #[test]
    fn channel_index_out_of_range() {
        assert!(ChannelIndex::new(8).is_err());
        assert!(ChannelIndex::new(255).is_err());
    }

    #[test]
    fn channel_index_display() {
        #[expect(clippy::unwrap_used, reason = "test-only: value is known valid")]
        let s = ChannelIndex::new(3).unwrap().to_string();
        assert_eq!(s, "3");
    }

    #[test]
    fn frame_constants() {
        assert_eq!(FRAME_MAGIC, [0x94, 0xC3]);
        assert_eq!(MAX_PACKET_SIZE, 512);
        assert_eq!(MAX_CHANNELS, 8);
        assert_eq!(MAX_HOP_LIMIT, 7);
    }

    // ── Property tests ──────────────────────────────────────────────────────

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(256))]

        /// `PacketId` serialises to JSON and back without loss for any `u32` value.
        ///
        /// WHY: `PacketId` is stored in GreptimeDB and round-tripped through JSON signal
        /// payloads; data loss on any value (including 0 and `u32::MAX`) would silently
        /// corrupt delivery-tracking state.
        #[test]
        fn packet_id_round_trips(raw in 0_u32..=u32::MAX) {
            #[expect(clippy::unwrap_used, reason = "test-only: PacketId serialisation is infallible")]
            let serialised = serde_json::to_string(&PacketId(raw)).unwrap();
            #[expect(clippy::unwrap_used, reason = "test-only: deserialising just-serialised value")]
            let restored: PacketId = serde_json::from_str(&serialised).unwrap();
            proptest::prop_assert_eq!(PacketId(raw), restored);
        }

        /// `NodeNum` serialises to JSON and back without loss for any `u32` value.
        ///
        /// WHY: node numbers are MAC-derived u32 values used as primary keys in `NodeDb`
        /// and emitted in mesh topology signals; a round-trip mismatch would cause nodes
        /// to be mis-identified or duplicated in the topology graph.
        #[test]
        fn node_id_round_trips(raw in 0_u32..=u32::MAX) {
            #[expect(clippy::unwrap_used, reason = "test-only: NodeNum serialisation is infallible")]
            let serialised = serde_json::to_string(&NodeNum(raw)).unwrap();
            #[expect(clippy::unwrap_used, reason = "test-only: deserialising just-serialised value")]
            let restored: NodeNum = serde_json::from_str(&serialised).unwrap();
            proptest::prop_assert_eq!(NodeNum(raw), restored);
        }
    }
}
