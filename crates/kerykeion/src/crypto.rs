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
    // WHY: Meshtastic firmware zero-extends packet_id to u64 before encoding.
    nonce[0..8].copy_from_slice(&u64::from(packet_id).to_le_bytes()); // SAFETY: fixed-size [u8; 16], not a string. kanon:ignore RUST/indexing-slicing -- compile-time bounded
    nonce[8..12].copy_from_slice(&from_node.to_le_bytes());
    // Bytes 12..16 remain zero.
    nonce
}

/// Resolve a PSK to its full-length key bytes.
///
/// - Empty slice → `None` (channel has no encryption).
/// - Single byte `n` (1–10) → [`DEFAULT_PSK`] with byte 15 SET to `n`.
/// - 16 or 32 bytes → used as-is.
///
/// Returns `None` if the PSK is empty (unencrypted channel), otherwise `Some(key)`.
pub(crate) fn resolve_psk(psk: &[u8]) -> Option<Vec<u8>> {
    match psk {
        [] => None,
        [n] if *n >= 1 && *n <= 10 => {
            let mut key = DEFAULT_PSK;
            key[15] = *n; // kanon:ignore RUST/indexing-slicing -- key is fixed-size [u8; 16], index 15 is compile-time bounded
            Some(key.to_vec())
        }
        _ => Some(psk.to_vec()),
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
    let Some(key) = resolve_psk(psk) else {
        // Unencrypted channel: return plaintext unchanged.
        return Ok(plaintext.to_vec());
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
        let Some(key) = resolve_psk(psk) else {
            // Skip unencrypted-channel PSKs.
            continue;
        };

        let mut candidate = ciphertext.to_vec();
        if apply_aes_ctr(&mut candidate, packet_id, from_node, &key).is_err() {
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
        assert!(resolve_psk(&[]).is_none());
    }

    #[test]
    fn psk_0x01_resolves_to_default_key() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = resolve_psk(&[0x01]).unwrap();
        assert_eq!(key, DEFAULT_PSK.to_vec());
    }

    #[test]
    fn psk_0x05_sets_last_byte() {
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = resolve_psk(&[0x05]).unwrap();
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
        let key = resolve_psk(&[0x0A]).unwrap();
        assert_eq!(key.last().copied(), Some(0x0Au8));
    }

    #[test]
    fn psk_16_bytes_used_as_is() {
        let raw = [0xAAu8; 16];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = resolve_psk(&raw).unwrap();
        assert_eq!(key, raw.to_vec());
    }

    #[test]
    fn psk_32_bytes_used_as_is() {
        let raw = [0xBBu8; 32];
        #[expect(clippy::unwrap_used, reason = "test-only")]
        let key = resolve_psk(&raw).unwrap();
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
