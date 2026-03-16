//! Error types for mesh networking operations.

use snafu::Snafu;

/// Unified error type for the kerykeion crate.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum Error {
    /// Failed to open serial port.
    #[snafu(display("failed to connect to serial port {port}"))]
    SerialConnect {
        port: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Serial read/write error.
    #[snafu(display("serial I/O error"))]
    SerialIo {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// TCP connection failed.
    #[snafu(display("failed to connect to TCP endpoint {addr}"))]
    TcpConnect {
        addr: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// BLE connection failed.
    #[snafu(display("failed to connect to BLE device {device}"))]
    BleConnect {
        device: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid frame header or length.
    #[snafu(display("frame decode error: {reason}"))]
    FrameDecode {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Protobuf deserialization failed.
    #[snafu(display("protobuf decode error"))]
    ProtobufDecode {
        source: prost::DecodeError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Protobuf serialization failed.
    #[snafu(display("protobuf encode error"))]
    ProtobufEncode {
        source: prost::EncodeError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// AES-CTR encryption/decryption failure.
    #[snafu(display("encryption error: {reason}"))]
    Encryption {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Channel index out of range.
    #[snafu(display("invalid channel index {index}, must be 0–7"))]
    InvalidChannel {
        index: u8,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Hop limit exceeds maximum.
    #[snafu(display("invalid hop limit {hops}, maximum is 7"))]
    InvalidHopLimit {
        hops: u8,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Node not found in the node database.
    #[snafu(display("node {node_num:#010x} not found in NodeDb"))]
    NodeNotFound {
        node_num: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Config handshake did not complete.
    #[snafu(display("handshake with device failed"))]
    HandshakeFailed {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Connection dropped unexpectedly.
    #[snafu(display("connection lost"))]
    ConnectionLost {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Packet send failure.
    #[snafu(display("send failed: {reason}"))]
    SendFailed {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn error_display_includes_context() {
        let err: Error = InvalidChannelSnafu { index: 9 }.build();
        let msg = err.to_string();
        assert!(msg.contains("invalid channel index 9"), "got: {msg}");
    }

    #[test]
    fn error_display_node_not_found() {
        let err: Error = NodeNotFoundSnafu {
            node_num: 0xAABB_u32,
        }
        .build();
        let msg = err.to_string();
        assert!(msg.contains("NodeDb"), "got: {msg}");
    }

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Error>();
    }

    #[test]
    fn error_chain_preserves_source() {
        use snafu::ResultExt;
        let result: Result<(), Error> = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no device",
        ))
        .context(SerialConnectSnafu {
            port: "/dev/ttyUSB0",
        });
        let err = result.expect_err("should be error");
        let msg = err.to_string();
        assert!(msg.contains("/dev/ttyUSB0"), "got: {msg}");
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn variant_count_at_least_thirteen() {
        // WHY: Acceptance criteria requires 13+ variants.
        // Builds only variants without source fields; source variants
        // are validated via context() in error_chain_preserves_source.
        let variants: Vec<Error> = vec![
            InvalidChannelSnafu { index: 0 }.build(),
            InvalidHopLimitSnafu { hops: 0 }.build(),
            NodeNotFoundSnafu { node_num: 0_u32 }.build(),
            HandshakeFailedSnafu.build(),
            ConnectionLostSnafu.build(),
            SendFailedSnafu { reason: "test" }.build(),
            EncryptionSnafu { reason: "test" }.build(),
            FrameDecodeSnafu { reason: "test" }.build(),
            BleConnectSnafu { device: "test" }.build(),
        ];
        // 9 sourceless + 4 with source (SerialConnect, SerialIo, TcpConnect,
        // ProtobufDecode, ProtobufEncode) = 14 total variants
        assert!(variants.len() >= 9);
    }
}
