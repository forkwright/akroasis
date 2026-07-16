//! Error types for kerykeion mesh networking operations.

use snafu::Snafu;

/// All errors produced by kerykeion operations.
// WHY: pub(crate) visibility on context selectors is required so that
// transport, codec, handshake, and crypto modules can construct errors via
// the snafu context-selector pattern.  snafu 0.8 defaults to private selectors.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    /// Failed to open a serial port connection.
    #[snafu(display("failed to open serial port {port}: {source}"))]
    SerialConnect {
        /// Underlying I/O error.
        source: std::io::Error,
        /// Serial port path (e.g. `/dev/ttyUSB0`).
        port: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serial read or write failed.
    #[snafu(display("serial I/O error: {source}"))]
    SerialIo {
        /// Underlying I/O error.
        source: std::io::Error,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// TCP connection to a Meshtastic node failed.
    #[snafu(display("TCP connection to {addr} failed: {source}"))]
    TcpConnect {
        /// Underlying I/O error.
        source: std::io::Error,
        /// Address that was dialed.
        addr: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// BLE connection to a named device failed.
    #[snafu(display("BLE connection to device '{device}' failed"))]
    BleConnect {
        /// Device name or address.
        device: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Received a frame with an invalid magic header or out-of-range length.
    #[snafu(display("invalid frame: {detail}"))]
    FrameDecode {
        /// Human-readable description of the decode failure.
        detail: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Protobuf deserialization failed.
    #[snafu(display("protobuf decode error: {source}"))]
    ProtobufDecode {
        /// Underlying prost error.
        source: prost::DecodeError,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Protobuf serialization failed.
    #[snafu(display("protobuf encode error: {source}"))]
    ProtobufEncode {
        /// Underlying prost error.
        source: prost::EncodeError,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// AES-CTR encryption or decryption failed.
    #[snafu(display("encryption/decryption error: {detail}"))]
    Encryption {
        /// Human-readable description of the failure.
        detail: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Channel index is outside the valid range `0..MAX_CHANNELS`.
    #[snafu(display("channel index {index} is out of range (max {})", crate::types::MAX_CHANNELS - 1))]
    InvalidChannel {
        /// The invalid index that was provided.
        index: u8,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Hop limit exceeds the protocol maximum.
    #[snafu(display(
        "hop LIMIT {hop_limit} exceeds maximum {}",
        crate::types::MAX_HOP_LIMIT
    ))]
    InvalidHopLimit {
        /// The invalid hop limit that was provided.
        hop_limit: u8,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The requested node number is not in the `NodeDb`.
    #[snafu(display("node {node_num:#010x} not found in NodeDb"))]
    NodeNotFound {
        /// The node number that was looked up.
        node_num: u32,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Config handshake with the radio did not complete.
    #[snafu(display("config handshake failed: {detail}"))]
    HandshakeFailed {
        /// Human-readable description of the failure.
        detail: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The transport connection dropped unexpectedly.
    #[snafu(display("connection lost: {detail}"))]
    ConnectionLost {
        /// Human-readable description of why the connection was lost.
        detail: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Packet send failed due to a routing error.
    #[snafu(display("packet send failed: {detail}"))]
    SendFailed {
        /// Human-readable description of the routing error.
        detail: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Protobuf payload could not be decoded for the given portnum.
    #[snafu(display("failed to decode {portnum} payload: {source}"))]
    PayloadDecode {
        /// Port number name for diagnostics.
        portnum: String,
        /// Underlying prost decode error.
        source: prost::DecodeError,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Topology snapshot deserialization failed.
    #[snafu(display("topology snapshot error: {source}"))]
    TopologySnapshot {
        /// Underlying JSON error.
        source: serde_json::Error,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Message delivery failed after exhausting all retry attempts.
    #[snafu(display("delivery failed for packet {packet_id}: {reason}"))]
    DeliveryFailed {
        /// The packet that could not be delivered.
        packet_id: u32,
        /// Human-readable failure reason.
        reason: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A routing NAK was received from the mesh.
    #[snafu(display("routing NAK for packet {packet_id}: {error_code}"))]
    RoutingNak {
        /// The packet that was NAK'd.
        packet_id: u32,
        /// Meshtastic routing error code name.
        error_code: String,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Store-and-forward queue is full for the destination node.
    #[snafu(display("store-forward queue full for node {dest:#010x}"))]
    QueueFull {
        /// The destination node whose queue is at capacity.
        dest: u32,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Message has expired past its TTL.
    #[snafu(display("message {packet_id} expired (TTL exceeded)"))]
    MessageExpired {
        /// The expired packet's ID.
        packet_id: u32,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serialization or deserialization of store-forward state failed.
    #[snafu(display("store-forward serialization error: {source}"))]
    StoreForwardSerde {
        /// Underlying `serde_json` error.
        source: serde_json::Error,
        /// Source location for diagnostics.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

// WHY: tokio_util::codec::Decoder::Error and Encoder::Error both require
// From<io::Error> so that Framed can wrap transport-layer I/O errors.
// We map them to ConnectionLost since an I/O failure on the transport means
// the connection can no longer be used.
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::ConnectionLost {
            detail: e.to_string(),
            location: snafu::location!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use snafu::ResultExt as _;

    use super::*;

    fn make_io_error() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe broken")
    }

    #[test]
    fn serial_connect_error_chain() {
        let result: Result<(), Error> = Err(make_io_error()).context(SerialConnectSnafu {
            port: "/dev/ttyUSB0",
        });
        #[expect(clippy::unwrap_used, reason = "test-only: we expect an error")]
        let err = result.unwrap_err();
        assert!(err.to_string().contains("/dev/ttyUSB0"));
    }

    #[test]
    fn invalid_channel_message() {
        let err = Error::InvalidChannel {
            index: 9,
            location: snafu::location!(),
        };
        assert!(err.to_string().contains('9'));
    }

    #[test]
    fn node_not_found_message() {
        let err = Error::NodeNotFound {
            node_num: 0xDEAD_BEEF,
            location: snafu::location!(),
        };
        assert!(err.to_string().contains("0xdeadbeef"));
    }
}
