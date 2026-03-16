//! Core mesh networking newtypes and constants.

use std::fmt;

use serde::{Deserialize, Serialize};
use snafu::ensure;

use crate::error::{InvalidChannelSnafu, InvalidHopLimitSnafu};

/// 4-byte node number (last 4 bytes of device MAC address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeNum(pub u32);

/// Random 32-bit packet identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PacketId(pub u32);

/// Channel index (0–7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelIndex(u8);

/// Broadcast address — all nodes on the mesh.
pub const BROADCAST_ADDR: NodeNum = NodeNum(0xFFFF_FFFF);

/// Maximum number of channels supported by Meshtastic firmware.
pub const MAX_CHANNELS: u8 = 8;

/// Maximum hop count for mesh packet routing.
pub const MAX_HOP_LIMIT: u8 = 7;

/// Maximum packet payload size in bytes.
pub const MAX_PACKET_SIZE: usize = 512;

/// Serial/TCP frame header magic bytes.
pub const FRAME_MAGIC: [u8; 2] = [0x94, 0xC3];

impl ChannelIndex {
    /// Create a validated channel index.
    ///
    /// # Errors
    ///
    /// Returns `InvalidChannel` if `index >= MAX_CHANNELS`.
    pub fn new(index: u8) -> Result<Self, crate::error::Error> {
        ensure!(index < MAX_CHANNELS, InvalidChannelSnafu { index });
        Ok(Self(index))
    }

    /// Return the raw index value.
    #[must_use]
    pub const fn value(self) -> u8 {
        self.0
    }
}

impl fmt::Display for NodeNum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "!{:08x}", self.0)
    }
}

impl fmt::Display for PacketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pkt-{:08x}", self.0)
    }
}

impl fmt::Display for ChannelIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ch{}", self.0)
    }
}

/// Validate a hop limit value.
///
/// # Errors
///
/// Returns `InvalidHopLimit` if `hops > MAX_HOP_LIMIT`.
pub fn validate_hop_limit(hops: u8) -> Result<u8, crate::error::Error> {
    ensure!(hops <= MAX_HOP_LIMIT, InvalidHopLimitSnafu { hops });
    Ok(hops)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn node_num_display_formats_hex() {
        let node = NodeNum(0xDEAD_BEEF);
        assert_eq!(node.to_string(), "!deadbeef");
    }

    #[test]
    fn packet_id_display_formats_hex() {
        let pkt = PacketId(0x0000_00FF);
        assert_eq!(pkt.to_string(), "pkt-000000ff");
    }

    #[test]
    fn channel_index_valid_range() {
        for i in 0..MAX_CHANNELS {
            let ch = ChannelIndex::new(i).expect("valid channel");
            assert_eq!(ch.value(), i);
        }
    }

    #[test]
    fn channel_index_rejects_out_of_range() {
        assert!(ChannelIndex::new(8).is_err());
        assert!(ChannelIndex::new(255).is_err());
    }

    #[test]
    fn channel_index_display() {
        let ch = ChannelIndex::new(3).expect("valid");
        assert_eq!(ch.to_string(), "ch3");
    }

    #[test]
    fn validate_hop_limit_accepts_valid() {
        for h in 0..=MAX_HOP_LIMIT {
            assert_eq!(validate_hop_limit(h).expect("valid"), h);
        }
    }

    #[test]
    fn validate_hop_limit_rejects_excess() {
        assert!(validate_hop_limit(8).is_err());
        assert!(validate_hop_limit(255).is_err());
    }

    #[test]
    fn broadcast_addr_is_all_ones() {
        assert_eq!(BROADCAST_ADDR.0, 0xFFFF_FFFF);
    }

    #[test]
    fn frame_magic_bytes() {
        assert_eq!(FRAME_MAGIC, [0x94, 0xC3]);
    }

    #[test]
    fn max_packet_size_is_512() {
        assert_eq!(MAX_PACKET_SIZE, 512);
    }

    #[test]
    fn node_num_equality_and_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(NodeNum(1));
        set.insert(NodeNum(1));
        set.insert(NodeNum(2));
        assert_eq!(set.len(), 2);
    }
}
