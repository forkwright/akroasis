//! Radio identification for the Baofeng UV-5R family.
//!
//! After entering programming mode the host requests identification. The radio
//! responds with 8 or 12 raw bytes terminated by `0xDD`. Twelve-byte responses
//! are normalized to 8 bytes for consistent variant matching.

/// Parsed radio identification FROM the UV-5R clone handshake.
#[derive(Debug, Clone)]
pub struct RadioIdent {
    /// Raw bytes received FROM the radio (before normalization, without `0xDD`).
    pub raw_bytes: Vec<u8>,

    /// Canonical 8-byte identification used for variant matching.
    pub normalized: [u8; 8],

    /// First 6 characters of the firmware string (e.g. `"BFB291"`).
    pub firmware_prefix: String,
}

impl RadioIdent {
    /// Build a `RadioIdent` FROM raw identification bytes (without terminator).
    ///
    /// If the response is 12 bytes it is collapsed to 8:
    /// `[resp[0], resp[3], resp[5], resp[7], resp[8], resp[9], resp[10], resp[11]]`.
    /// Eight-byte responses are used as-is.
    pub fn from_raw(raw: &[u8]) -> Option<Self> {
        let normalized = match raw.len() {
            8 => {
                let mut n = [0u8; 8];
                n.copy_from_slice(raw);
                n
            }
            // INVARIANT: we just checked raw.len() == 12, so all indices are in bounds.
            12 => [
                raw.first().copied().unwrap_or_default(),
                raw.get(3).copied().unwrap_or_default(),
                raw.get(5).copied().unwrap_or_default(),
                raw.get(7).copied().unwrap_or_default(),
                raw.get(8).copied().unwrap_or_default(),
                raw.get(9).copied().unwrap_or_default(),
                raw.get(10).copied().unwrap_or_default(),
                raw.get(11).copied().unwrap_or_default(),
            ],
            _ => return None,
        };

        let firmware_prefix = normalized
            .iter()
            .take(6)
            .map(|&b| {
                if b.is_ascii_graphic() {
                    char::from(b)
                } else {
                    '?'
                }
            })
            .collect();

        Some(Self {
            raw_bytes: raw.to_vec(),
            normalized,
            firmware_prefix,
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::RadioIdent;

    #[test]
    fn from_raw_uses_eight_byte_response_as_is() {
        let ident = RadioIdent::from_raw(b"BFB2910\xFF").unwrap();
        assert_eq!(ident.normalized, *b"BFB2910\xFF");
    }

    #[test]
    fn from_raw_collapses_twelve_byte_response_to_eight() {
        // WHY: collapse indices per the module doc: [0, 3, 5, 7, 8, 9, 10, 11].
        let raw: [u8; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let ident = RadioIdent::from_raw(&raw).unwrap();
        assert_eq!(ident.normalized, [0, 3, 5, 7, 8, 9, 10, 11]);
    }

    #[test]
    fn from_raw_replaces_non_graphic_bytes_with_placeholder() {
        let ident = RadioIdent::from_raw(b"BF\x00297XY").unwrap();
        assert_eq!(ident.firmware_prefix, "BF?297");
    }

    #[test]
    fn from_raw_rejects_lengths_other_than_eight_or_twelve() {
        assert!(RadioIdent::from_raw(&[1, 2, 3]).is_none());
        assert!(RadioIdent::from_raw(&[0; 7]).is_none());
        assert!(RadioIdent::from_raw(&[0; 13]).is_none());
    }
}
