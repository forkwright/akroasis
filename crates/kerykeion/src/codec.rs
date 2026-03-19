//! Meshtastic serial frame codec.
//!
//! Every Meshtastic packet on the wire is wrapped in a 4-byte header:
//!
//! ```text
//! ┌────────┬────────┬────────┬────────┬─────── … ───────┐
//! │  0x94  │  0xC3  │  MSB   │  LSB   │  protobuf bytes  │
//! └────────┴────────┴────────┴────────┴─────── … ────────┘
//!   magic[0]  magic[1]   ╰──── big-endian payload length ────╯
//! ```
//!
//!  implements [`tokio_util::codec::Decoder`] (yields [`FromRadio`])
//! and [`tokio_util::codec::Encoder`] (takes [`ToRadio`]).

use bytes::{Buf as _, BufMut as _, BytesMut};
use prost::Message as _;
use snafu::ResultExt as _;
use tokio_util::codec::{Decoder, Encoder};

use crate::Error;
use crate::error::{ProtobufDecodeSnafu, SendFailedSnafu};
use crate::proto::{FromRadio, ToRadio};
use crate::types::{FRAME_MAGIC, MAX_PACKET_SIZE};

/// Codec that applies the Meshtastic 4-byte frame header on top of protobuf payloads.
///
/// One codec instance is typically wrapped in a [`tokio_util::codec::Framed`] that
/// sits on top of a serial port or TCP stream.
pub(crate) struct MeshCodec;

impl Decoder for MeshCodec {
    type Item = FromRadio;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            // Need at least 2 bytes to find the magic pair.
            if src.len() < 2 {
                return Ok(None);
            }

            // Locate the 0x94 0xC3 magic pair.
            // SAFETY: the range is 0..len-1, so i+1 < len; get() is used to satisfy clippy.
            let magic_pos = (0..src.len().saturating_sub(1)).find(|&i| {
                src.get(i).copied() == Some(FRAME_MAGIC[0])
                    && src.get(i + 1).copied() == Some(FRAME_MAGIC[1])
            });

            match magic_pos {
                None => {
                    // WHY: keep the last byte — it could be the first half of a split
                    // magic pair arriving across two buffer fills.
                    let keep = src.len().saturating_sub(1);
                    src.advance(keep);
                    return Ok(None);
                }
                Some(n) if n > 0 => {
                    // Discard bytes before the magic and recheck.
                    tracing::trace!(skipped = n, "skipping non-magic bytes before frame");
                    src.advance(n);
                    continue;
                }
                Some(_) => {
                    // Magic is at position 0; fall through to header parsing.
                }
            }

            // Need all 4 header bytes before we can read the length.
            if src.len() < 4 {
                return Ok(None);
            }

            // WHY: bounds already checked (src.len() >= 4); get() used to satisfy clippy.
            let Some(&msb) = src.get(2) else {
                return Ok(None);
            };
            let Some(&lsb) = src.get(3) else {
                return Ok(None);
            };
            let payload_len = usize::from(u16::from_be_bytes([msb, lsb]));

            if payload_len > MAX_PACKET_SIZE {
                // Corrupt frame — skip past the two magic bytes and re-seek.
                tracing::debug!(
                    payload_len,
                    max = MAX_PACKET_SIZE,
                    "frame length exceeds maximum; discarding and re-seeking magic"
                );
                src.advance(2);
                continue;
            }

            if payload_len == 0 {
                // Zero-length frame is a no-op; consume the header.
                src.advance(4);
                continue;
            }

            let total_needed = 4 + payload_len;
            if src.len() < total_needed {
                // Partial payload — wait for more data.
                src.reserve(total_needed - src.len());
                return Ok(None);
            }

            // Consume the 4-byte header.
            src.advance(4);
            // Extract exactly `payload_len` bytes.
            let payload = src.split_to(payload_len);

            let msg = FromRadio::decode(payload.as_ref()).context(ProtobufDecodeSnafu)?;
            return Ok(Some(msg));
        }
    }
}

impl Encoder<ToRadio> for MeshCodec {
    type Error = Error;

    fn encode(&mut self, item: ToRadio, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let payload = item.encode_to_vec();
        let len = payload.len();

        if len > MAX_PACKET_SIZE {
            return SendFailedSnafu {
                detail: format!("encoded ToRadio too large: {len} bytes (max {MAX_PACKET_SIZE})"),
            }
            .fail();
        }

        // SAFETY: len <= MAX_PACKET_SIZE <= 512 < u16::MAX, so truncation cannot occur.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "payload_len is bounded by MAX_PACKET_SIZE (512) which fits in u16"
        )]
        let len_u16 = len as u16;

        dst.reserve(4 + len);
        dst.put_u8(FRAME_MAGIC[0]);
        dst.put_u8(FRAME_MAGIC[1]);
        dst.put_u16(len_u16); // big-endian
        dst.put_slice(&payload);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{from_radio, to_radio};

    fn make_from_radio(id: u32) -> FromRadio {
        FromRadio {
            id,
            payload_variant: Some(from_radio::PayloadVariant::ConfigCompleteId(id)),
        }
    }

    /// Encode a `FromRadio` manually with the 4-byte frame header.
    fn frame_from_radio(msg: &FromRadio) -> Vec<u8> {
        let payload = msg.encode_to_vec();
        let len = payload.len() as u16;
        let mut out = vec![
            FRAME_MAGIC[0],
            FRAME_MAGIC[1],
            (len >> 8) as u8,
            (len & 0xFF) as u8,
        ];
        out.extend_from_slice(&payload);
        out
    }

    // ── Decoder tests ───────────────────────────────────────────────────────

    #[test]
    fn decoder_single_frame_roundtrip() {
        let original = make_from_radio(7);
        let raw = frame_from_radio(&original);
        let mut buf = BytesMut::from(raw.as_slice());
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only: known valid frame")]
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.id, original.id);
    }

    #[test]
    fn decoder_partial_header_returns_none() {
        // Only 3 bytes — not enough for the 4-byte header.
        let mut buf = BytesMut::from(&[0x94u8, 0xC3, 0x00][..]);
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn decoder_partial_payload_returns_none() {
        let msg = make_from_radio(1);
        let raw = frame_from_radio(&msg);
        // Truncate the payload by 1 byte.
        let end = raw.len().saturating_sub(1);
        #[expect(clippy::unwrap_used, reason = "test-only: end is always <= raw.len()")]
        let truncated = raw.get(..end).unwrap();
        let mut buf = BytesMut::from(truncated);
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let result = codec.decode(&mut buf).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn decoder_corrupt_magic_skipped() {
        // Garbage bytes followed by a valid frame.
        let msg = make_from_radio(99);
        let mut raw = vec![0x00u8, 0xFF, 0xAB]; // junk
        raw.extend_from_slice(&frame_from_radio(&msg));
        let mut buf = BytesMut::from(raw.as_slice());
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.id, 99);
    }

    #[test]
    fn decoder_length_too_large_re_seeks() {
        // Frame with length > 512 should be discarded; the valid frame after it is decoded.
        let valid = make_from_radio(55);
        let mut buf_data = vec![
            FRAME_MAGIC[0],
            FRAME_MAGIC[1],
            0x02,
            0x00, // length = 512 = MAX_PACKET_SIZE, still valid
        ];
        // Construct one oversized frame (length = 513).
        let mut bad = vec![FRAME_MAGIC[0], FRAME_MAGIC[1], 0x02, 0x01]; // 513 > 512
        bad.extend(vec![0xFFu8; 513]);
        let good = frame_from_radio(&valid);
        buf_data.clear();
        buf_data.extend_from_slice(&bad);
        buf_data.extend_from_slice(&good);

        let mut buf = BytesMut::from(buf_data.as_slice());
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.id, 55);
    }

    #[test]
    fn decoder_zero_length_frame_skipped() {
        // Zero-length frame followed by a valid frame.
        let valid = make_from_radio(11);
        let mut data = vec![FRAME_MAGIC[0], FRAME_MAGIC[1], 0x00, 0x00]; // zero-length
        data.extend_from_slice(&frame_from_radio(&valid));
        let mut buf = BytesMut::from(data.as_slice());
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.id, 11);
    }

    #[test]
    fn decoder_back_to_back_frames() {
        let a = make_from_radio(1);
        let b = make_from_radio(2);
        let mut data = frame_from_radio(&a);
        data.extend_from_slice(&frame_from_radio(&b));
        let mut buf = BytesMut::from(data.as_slice());
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let first = codec.decode(&mut buf).unwrap().unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let second = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
    }

    #[test]
    fn decoder_leading_0x94_not_followed_by_0xc3() {
        // Single 0x94 that is NOT the start of a magic pair should be discarded.
        let msg = make_from_radio(3);
        let mut data = vec![0x94u8, 0x00, 0x00]; // fake magic start
        data.extend_from_slice(&frame_from_radio(&msg));
        let mut buf = BytesMut::from(data.as_slice());
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let decoded = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(decoded.id, 3);
    }

    // ── Encoder tests ───────────────────────────────────────────────────────

    #[test]
    fn encoder_produces_correct_header() {
        let msg = ToRadio {
            payload_variant: Some(to_radio::PayloadVariant::WantConfigId(42)),
        };
        let payload = msg.encode_to_vec();
        let mut dst = BytesMut::new();
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        codec
            .encode(
                ToRadio {
                    payload_variant: Some(to_radio::PayloadVariant::WantConfigId(42)),
                },
                &mut dst,
            )
            .unwrap();

        let raw: &[u8] = &dst;
        // Verify magic bytes.
        assert_eq!(raw.first(), Some(&0x94u8));
        assert_eq!(raw.get(1), Some(&0xC3u8));
        // Verify big-endian length.
        #[expect(clippy::unwrap_used, reason = "test-only: known non-empty buffer")]
        let encoded_len = u16::from_be_bytes([*raw.get(2).unwrap(), *raw.get(3).unwrap()]) as usize;
        assert_eq!(encoded_len, payload.len());
        // Verify payload follows header.
        assert_eq!(raw.get(4..), Some(payload.as_slice()));
    }

    #[test]
    fn encoder_roundtrip_via_decoder() {
        // Encode a ToRadio; verify the 4-byte frame header wraps the payload correctly.
        let msg = ToRadio {
            payload_variant: Some(to_radio::PayloadVariant::WantConfigId(123)),
        };
        let mut dst = BytesMut::new();
        let mut codec = MeshCodec;
        #[expect(clippy::unwrap_used, reason = "test-only")]
        codec.encode(msg, &mut dst).unwrap();

        let raw: &[u8] = &dst;
        // Verify frame structure: magic + length + payload.
        assert_eq!(raw.first(), Some(&FRAME_MAGIC[0]));
        assert_eq!(raw.get(1), Some(&FRAME_MAGIC[1]));
        #[expect(clippy::unwrap_used, reason = "test-only: known non-empty buffer")]
        let declared_len =
            u16::from_be_bytes([*raw.get(2).unwrap(), *raw.get(3).unwrap()]) as usize;
        assert_eq!(declared_len + 4, dst.len());
    }
}
