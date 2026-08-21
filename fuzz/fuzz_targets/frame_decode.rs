//! Frame decoding against arbitrary bytes.
//!
//! The codec reads the 4-byte Meshtastic header off a radio link, so every byte
//! it sees is attacker-reachable. Its contract is that it either yields a frame,
//! asks for more input, or errors — never panics, and never spins without
//! consuming input.

#![no_main]

use bytes::BytesMut;
use kerykeion::codec::MeshCodec;
use libfuzzer_sys::fuzz_target;
use tokio_util::codec::Decoder;

/// Ceiling on decode calls per input.
///
/// WHY: `Framed` drives the decoder in a loop, so the interesting behaviour is a
/// sequence of decodes over one buffer rather than a single call. The bound
/// keeps a pathological input from turning one case into an unbounded loop; a
/// decoder that genuinely never terminates still trips libFuzzer's own timeout,
/// so this hides nothing.
const MAX_DECODES: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let mut codec = MeshCodec;
    let mut buf = BytesMut::from(data);

    for _ in 0..MAX_DECODES {
        let before = buf.len();
        match codec.decode(&mut buf) {
            Ok(Some(_frame)) => {
                // A yielded frame must have consumed input. Standing still while
                // reporting progress is how a decode loop becomes a hang.
                assert!(
                    buf.len() < before,
                    "decode yielded a frame without consuming input"
                );
            }
            Ok(None) | Err(_) => break,
        }
    }
});
