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
    pub(crate) fn from_raw(raw: &[u8]) -> Option<Self> {
        let normalized = match raw.len() {
            8 => {
                let mut n = [0u8; 8];
                n.copy_from_slice(raw);
                n
            }
            // INVARIANT: we just checked raw.len() == 12, so all indices are in bounds.
            12 => [
                raw.get(0).copied().unwrap_or_default(), raw.get(3).copied().unwrap_or_default(), raw.get(5).copied().unwrap_or_default(), raw.get(7).copied().unwrap_or_default(), raw.get(8).copied().unwrap_or_default(), raw.get(9).copied().unwrap_or_default(), raw.get(10).copied().unwrap_or_default(), raw.get(11).copied().unwrap_or_default(),
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
