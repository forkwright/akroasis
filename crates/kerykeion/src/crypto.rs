//! AES-CTR encryption and decryption for Meshtastic mesh packets.
//!
//! Meshtastic uses AES-CTR mode with a 16-byte nonce derived FROM the packet ID
//! and the sender's node number:
//!
//! ```text
//! ┌─ bytes 0..8  ─┬─ bytes 8..12 ─┬─ bytes 12..16 ─┐
//! │ packet_id u64 │  from_node u32 │   0x00000000    │
//! │  little-endian │ little-endian  │   (padding)     │
//! └───────────────┴────────────────┴─────────────────┘
//! ```
//!
//! PSK length determines cipher:
//! - 16 bytes → AES-128-CTR
//! - 32 bytes → AES-256-CTR
//!
//! Single-byte PSK VALUES (0x01–0x0A) are short-hand references to the default
//! key family: the byte value is placed at position 15 of [`DEFAULT_PSK`].

use aes::Aes128;
use aes::Aes256;
use ctr::Ctr128LE;
use ctr::cipher::{KeyIvInit as _, StreamCipher as _};
use prost::Message as _;

use crate::Error;
use crate::error::EncryptionSnafu;
use crate::proto::Data;

/// Default 16-byte AES-128 key used by Meshtastic's built-in `"LongFast"` channel
/// (the `[0x01]` short-hand resolves to this key).
pub const DEFAULT_PSK: [u8; 16] = [
    0xd4, 0xf1, 0xbb, 0x3a, 0x20, 0x29, 0x07, 0x59, 0xf0, 0xbc, 0xff, 0xab, 0xcf, 0x4e, 0x69, 0x01,
];

/// Build the 16-byte AES-CTR nonce FROM a packet ID and sender node number.
///
/// Layout: `[packet_id as u64 LE || from_node as u32 LE || 0x00000000]`
pub(crate) fn build_nonce(packet_id: u32, from_node: u32) -> [u8; 16] {
    let mut nonce = [0u8; 16];
    // WHY: split_at_mut over a fixed-size array yields disjoint sub-slices
    // without bracket-range indexing, so the 8/4-byte layout is expressed
    // without a panic-shaped `nonce[a..b]` access.
    let (packet_slot, rest) = nonce.split_at_mut(8);
    // WHY: Meshtastic firmware zero-extends packet_id to u64 before encoding.
    packet_slot.copy_from_slice(&u64::from(packet_id).to_le_bytes());
    let (node_slot, _reserved) = rest.split_at_mut(4);
    node_slot.copy_from_slice(&from_node.to_le_bytes());
    // Bytes 12..16 remain zero.
    nonce
}

/// Resolve a PSK to its full-length key bytes.
///
/// Length of an AES-128 key, in bytes.
///
/// WHY named here rather than written inline: the accepted PSK lengths are one
/// fact used by both this module and the config boundary that validates what an
/// operator writes. Two literals would be free to disagree.
pub(crate) const AES128_KEY_LEN: usize = 16;

/// Length of an AES-256 key, in bytes. See [`AES128_KEY_LEN`].
pub(crate) const AES256_KEY_LEN: usize = 32;

/// What a channel's PSK bytes resolve to.
///
/// WHY(#229) three states rather than an `Option`: a PSK of an undefined shape
/// used to be indistinguishable from a channel that carries no encryption. Both
/// ended in the same silent `continue`, so a misconfigured channel looked
/// exactly like a public one and nothing said otherwise.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PskResolution {
    /// The channel carries no encryption.
    Unencrypted,
    /// A usable AES key.
    Key(Vec<u8>),
    /// The bytes match no shape this protocol defines.
    Undefined {
        /// Length of the rejected material. The bytes themselves are key
        /// material and are not carried into a log line.
        len: usize,
    },
}

/// - Empty slice → [`PskResolution::Unencrypted`].
/// - Single byte `n` (1–10) → [`DEFAULT_PSK`] with byte 15 SET to `n`.
/// - 16 or 32 bytes → used as-is.
/// - Anything else → [`PskResolution::Undefined`].
pub(crate) fn resolve_psk(psk: &[u8]) -> PskResolution {
    match psk {
        [] => PskResolution::Unencrypted,
        [n] if *n >= 1 && *n <= 10 => {
            let mut key = DEFAULT_PSK;
            key[15] = *n; // kanon:ignore RUST/indexing-slicing -- key is fixed-size [u8; 16], index 15 is compile-time bounded
            PskResolution::Key(key.to_vec())
        }
        // WHY(#229) the two accepted key lengths are named rather than left to a
        // catch-all: the arm below used to be `_ => Some(psk.to_vec())`, which
        // handed AES whatever it was given. A single byte of 0, or of 11, or a
        // seven-byte blob, all became "keys" — the doc above said 16 or 32 and
        // the code accepted every length.
        [_, ..] if psk.len() == AES128_KEY_LEN || psk.len() == AES256_KEY_LEN => {
            PskResolution::Key(psk.to_vec())
        }
        other => PskResolution::Undefined { len: other.len() },
    }
}

/// Encrypt or decrypt `data` in-place using AES-CTR.
///
/// AES-CTR is its own inverse, so this function handles both directions.
///
/// # Errors
///
/// Returns [`Error::Encryption`] if `key` is not 16 or 32 bytes, or if the
/// cipher cannot be constructed FROM the given key/nonce pair.
pub(crate) fn apply_aes_ctr(
    data: &mut [u8], // kanon:ignore RUST/indexing-slicing -- function parameter &mut [u8], not indexing
    packet_id: u32,
    from_node: u32,
    key: &[u8],
) -> Result<(), Error> {
    let nonce = build_nonce(packet_id, from_node);

    match key.len() {
        16 => {
            let mut cipher = Ctr128LE::<Aes128>::new_from_slices(key, &nonce).map_err(|e| {
                Error::Encryption {
                    detail: format!("AES-128 init failed: {e}"),
                    location: snafu::location!(),
                }
            })?;
            cipher.apply_keystream(data);
        }
        32 => {
            let mut cipher = Ctr128LE::<Aes256>::new_from_slices(key, &nonce).map_err(|e| {
                Error::Encryption {
                    detail: format!("AES-256 init failed: {e}"),
                    location: snafu::location!(),
                }
            })?;
            cipher.apply_keystream(data);
        }
        n => {
            return EncryptionSnafu {
                detail: format!("invalid PSK length {n}: must be 16 or 32 bytes"),
            }
            .fail();
        }
    }

    Ok(())
}

/// Encrypt a plaintext payload and return the ciphertext.
///
/// # Errors
///
/// Propagates errors FROM .
pub fn encrypt(
    plaintext: &[u8],
    packet_id: u32,
    from_node: u32,
    psk: &[u8],
) -> Result<Vec<u8>, Error> {
    let key = match resolve_psk(psk) {
        PskResolution::Key(key) => key,
        // Unencrypted channel: return plaintext unchanged.
        PskResolution::Unencrypted => return Ok(plaintext.to_vec()),
        // WHY(#229) an error rather than a pass-through: sending is the
        // dangerous direction. A misconfigured PSK previously became a bad key,
        // and returning the plaintext instead would transmit in the clear on a
        // channel the operator believes is encrypted.
        PskResolution::Undefined { len } => {
            return EncryptionSnafu {
                detail: format!("channel PSK is {len} bytes, which defines no key"),
            }
            .fail();
        }
    };
    let mut buf = plaintext.to_vec();
    apply_aes_ctr(&mut buf, packet_id, from_node, &key)?;
    Ok(buf)
}

/// Decrypt a ciphertext by trying each PSK in `channel_psks` in ORDER.
///
/// For each PSK, the ciphertext is decrypted and the result is tested as a valid
/// protobuf [`Data`] message. The first successful decode wins.
///
/// Returns `(plaintext_bytes, channel_index)`.
///
/// # Errors
///
/// Returns [`Error::Encryption`] if no PSK produces a valid [`Data`] decode.
pub fn decrypt(
    ciphertext: &[u8],
    packet_id: u32,
    from_node: u32,
    channel_psks: &[(usize, Vec<u8>)],
) -> Result<(Vec<u8>, usize), Error> {
    for (channel_idx, psk) in channel_psks {
        let key = match resolve_psk(psk) {
            PskResolution::Key(key) => key,
            // Skip unencrypted-channel PSKs.
            PskResolution::Unencrypted => continue,
            // WHY(#229) logged rather than skipped silently: this used to be
            // indistinguishable from an unencrypted channel, so a channel that
            // simply could not decrypt anything looked like one that was never
            // meant to.
            PskResolution::Undefined { len } => {
                tracing::warn!(
                    channel = channel_idx,
                    psk_len = len,
                    "skipping channel whose PSK defines no key"
                );
                continue;
            }
        };

        let mut candidate = ciphertext.to_vec();
        // WHY(#229) surfaced rather than skipped: this arm used to swallow the
        // error, which was defensible while a bad-length key could reach here
        // and fail init routinely. Since #436 it cannot — `resolve_psk` yields
        // `Key` only at 16 or 32 bytes, and `apply_aes_ctr` rejects every other
        // length before touching the cipher — so a failure here is a fault in
        // the AES implementation or the machine under it, not a wrong guess
        // about which channel this packet belongs to.
        //
        // Still `continue` rather than `fail`: a later channel may decrypt, and
        // refusing the whole packet on one channel's fault would turn a local
        // problem into dropped traffic. What changes is that it can no longer
        // happen quietly.
        if let Err(error) = apply_aes_ctr(&mut candidate, packet_id, from_node, &key) {
            tracing::warn!(
                channel = channel_idx,
                %error,
                "AES initialisation failed for a key of valid length; skipping channel"
            );
            continue;
        }

        // Accept if the bytes decode as a valid Data protobuf.
        if Data::decode(candidate.as_slice()).is_ok() {
            return Ok((candidate, *channel_idx));
        }
    }

    EncryptionSnafu {
        detail: "no channel PSK decrypted the payload to a valid Data message",
    }
    .fail()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key a PSK resolves to, if it resolves to one.
    ///
    /// Returns an `Option` rather than panicking so the call sites keep their
    /// existing `unwrap` — and with it the `#[expect(clippy::unwrap_used)]`
    /// attributes that would otherwise go unfulfilled, which is itself an error
    /// under this workspace's lints.
    fn key_of(psk: &[u8]) -> Option<Vec<u8>> {
        match resolve_psk(psk) {
            PskResolution::Key(key) => Some(key),
            PskResolution::Unencrypted | PskResolution::Undefined { .. } => None,
        }
    }

    // ── Nonce construction ──────────────────────────────────────────────────

    #[test]
    fn nonce_layout_known_values() {
        // packet_id = 1, from_node = 2
        let nonce = build_nonce(1, 2);
        // bytes 0..8: u64::from(1u32) = 1 as LE = [1, 0, 0, 0, 0, 0, 0, 0]
        assert_eq!(
            nonce.get(0..8).unwrap_or_default(),
            &[1, 0, 0, 0, 0, 0, 0, 0]
        );
        // bytes 8..12: 2u32 LE = [2, 0, 0, 0]
        assert_eq!(nonce.get(8..12).unwrap_or_default(), &[2, 0, 0, 0]);
        // bytes 12..16: zero
        assert_eq!(nonce.get(12..16).unwrap_or_default(), &[0, 0, 0, 0]);
    }

    #[test]
    fn nonce_max_values() {
        let nonce = build_nonce(u32::MAX, u32::MAX);
        // u64::from(u32::MAX) = 0x00000000_FFFFFFFF in LE
        assert_eq!(
            nonce.get(0..4).unwrap_or_default(),
            &[0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            nonce.get(4..8).unwrap_or_default(),
            &[0x00, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            nonce.get(8..12).unwrap_or_default(),
            &[0xFF, 0xFF, 0xFF, 0xFF]
        );
        assert_eq!(
            nonce.get(12..16).unwrap_or_default(),
            &[0x00, 0x00, 0x00, 0x00]
        );
    }

    // ── PSK expansion ───────────────────────────────────────────────────────

    #[test]
    fn psk_empty_returns_none() {
        assert_eq!(resolve_psk(&[]), PskResolution::Unencrypted);
    }

    /// WHY(#229): the arm these fall into used to be `_ => Some(psk.to_vec())`,
    /// so each of these became an AES "key" of its own length. A single byte of
    /// 0 or 11 is an index this protocol does not define, not a one-byte key.
    #[test]
    fn a_psk_of_no_defined_shape_is_not_a_key() {
        for psk in [
            vec![0x00],     // index 0
            vec![0x0B],     // index 11, one past the defined range
            vec![0xFF],     // index 255
            vec![0xAA; 7],  // neither key length
            vec![0xAA; 15], // one short of AES-128
            vec![0xAA; 31], // one short of AES-256
            vec![0xAA; 33], // one over AES-256
        ] {
            assert_eq!(
                resolve_psk(&psk),
                PskResolution::Undefined { len: psk.len() },
                "a {}-byte PSK defines no key",
                psk.len()
            );
        }
    }

    /// Anti-vacuity: the shapes the protocol does define must still resolve, or
    /// the case above would pass against a function that rejects everything.
    #[test]
    fn the_defined_psk_shapes_still_resolve() {
        assert_eq!(resolve_psk(&[]), PskResolution::Unencrypted);
        for index in 1..=10u8 {
            assert!(
                key_of(&[index]).is_some(),
                "index {index} is within the defined range"
            );
        }
        assert!(key_of(&[0xAA; AES128_KEY_LEN]).is_some());
        assert!(key_of(&[0xAA; AES256_KEY_LEN]).is_some());
    }

    #[test]
    fn psk_0x01_resolves_to_default_key() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = key_of(&[0x01]).unwrap();
        assert_eq!(key, DEFAULT_PSK.to_vec());
    }

    #[test]
    fn psk_0x05_sets_last_byte() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = key_of(&[0x05]).unwrap();
        assert_eq!(key.len(), 16);
        assert_eq!(key.last().copied(), Some(0x05u8));
        // key and DEFAULT_PSK are both 16 bytes; get(..15) never returns None.
        assert_eq!(
            key.get(..15).unwrap_or_default(),
            DEFAULT_PSK.get(..15).unwrap_or_default()
        );
    }

    #[test]
    fn psk_0x0a_sets_last_byte() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = key_of(&[0x0A]).unwrap();
        assert_eq!(key.last().copied(), Some(0x0Au8));
    }

    #[test]
    fn psk_16_bytes_used_as_is() {
        let raw = [0xAAu8; 16];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = key_of(&raw).unwrap();
        assert_eq!(key, raw.to_vec());
    }

    #[test]
    fn psk_32_bytes_used_as_is() {
        let raw = [0xBBu8; 32];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = key_of(&raw).unwrap();
        assert_eq!(key, raw.to_vec());
    }

    // ── Encrypt / decrypt roundtrips ────────────────────────────────────────

    #[test]
    fn encrypt_decrypt_roundtrip_default_psk() {
        let plaintext = b"hello meshtastic";
        let packet_id = 0x1234_5678u32;
        let from_node = 0xDEAD_BEEFu32;
        let psk = &[0x01u8];

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let ciphertext = encrypt(plaintext, packet_id, from_node, psk).unwrap();
        assert_ne!(ciphertext, plaintext);

        // Decrypt by encrypting again (AES-CTR is self-inverse).
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let recovered = encrypt(&ciphertext, packet_id, from_node, psk).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn encrypt_decrypt_roundtrip_16_byte_psk() {
        let plaintext = b"custom-key-test!";
        let psk = [0x01u8; 16];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let ct = encrypt(plaintext, 1, 2, &psk).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pt = encrypt(&ct, 1, 2, &psk).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn encrypt_decrypt_roundtrip_32_byte_psk() {
        let plaintext = b"256-bit key test data goes here!";
        let psk = [0x02u8; 32];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let ct = encrypt(plaintext, 5, 10, &psk).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let pt = encrypt(&ct, 5, 10, &psk).unwrap();
        assert_eq!(pt, plaintext);
    }

    #[test]
    fn encrypt_empty_psk_returns_plaintext_unchanged() {
        let plaintext = b"unencrypted";
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let out = encrypt(plaintext, 0, 0, &[]).unwrap();
        assert_eq!(out, plaintext);
    }

    #[test]
    fn encrypt_invalid_psk_length_returns_error() {
        let result = encrypt(b"data", 0, 0, &[0xAA; 17]);
        assert!(result.is_err());
    }

    // ── Multi-channel decryption ─────────────────────────────────────────────

    #[test]
    fn multi_channel_decrypt_finds_correct_channel() {
        use crate::proto::{Data, PortNum};

        // Build a valid Data protobuf payload.
        let data = Data {
            portnum: i32::from(PortNum::TextMessageApp),
            payload: b"test".to_vec(),
            ..Default::default()
        };
        let plaintext = data.encode_to_vec();

        let packet_id = 0xABCDu32;
        let from_node = 0x1111u32;
        let correct_psk = [0x10u8; 16];

        // Encrypt with the correct key.
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let ciphertext = encrypt(&plaintext, packet_id, from_node, &correct_psk).unwrap();

        // Three channels: only index 1 has the RIGHT PSK.
        let channel_psks: Vec<(usize, Vec<u8>)> = vec![
            (0, vec![0x01u8; 16]), // wrong key
            (1, correct_psk.to_vec()),
            (2, vec![0x02u8; 16]), // wrong key
        ];

        #[expect(clippy::unwrap_used, reason = "test-only")]
        let (recovered, ch_idx) =
            decrypt(&ciphertext, packet_id, from_node, &channel_psks).unwrap();

        assert_eq!(ch_idx, 1);
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn multi_channel_no_matching_psk_returns_error() {
        let ciphertext = vec![0xFFu8; 32];
        let channel_psks: Vec<(usize, Vec<u8>)> =
            vec![(0, vec![0x01u8; 16]), (1, vec![0x02u8; 16])];
        let result = decrypt(&ciphertext, 1, 1, &channel_psks);
        assert!(result.is_err());
    }

    #[test]
    fn different_packet_id_produces_different_ciphertext() {
        let plaintext = b"same message";
        let psk = [0x01u8];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let ct1 = encrypt(plaintext, 100, 1, &psk).unwrap();
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let ct2 = encrypt(plaintext, 200, 1, &psk).unwrap();
        assert_ne!(ct1, ct2);
    }
}
