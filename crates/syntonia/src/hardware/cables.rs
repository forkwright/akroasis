//! Programming cable chip identification.

use std::fmt;

/// USB-to-serial chipset found in radio programming cables.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CableChip {
    /// Prolific PL2303 — most common Baofeng cable. Clones are rampant.
    Pl2303,
    /// `WinChipHead` CH340 — second most common. Generally reliable.
    Ch340,
    /// Silicon Labs CP2102 — higher-end cables.
    Cp2102,
    /// FTDI FT232R — rare for Baofeng, common for other radios.
    Ftdi,
    /// Unrecognized USB-to-serial adapter.
    Unknown {
        /// USB vendor ID.
        vid: u16,
        /// USB product ID.
        pid: u16,
    },
}

impl fmt::Display for CableChip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pl2303 => f.write_str("PL2303"),
            Self::Ch340 => f.write_str("CH340"),
            Self::Cp2102 => f.write_str("CP2102"),
            Self::Ftdi => f.write_str("FT232R"),
            Self::Unknown { vid, pid } => write!(f, "Unknown ({vid:04X}:{pid:04X})"),
        }
    }
}

/// A known programming cable entry.
#[derive(Debug, Clone, Copy)]
pub struct KnownCable {
    /// USB vendor ID.
    pub vid: u16,
    /// USB product ID.
    pub pid: u16,
    /// Chipset type.
    pub chip: CableChip,
    /// Human-readable name.
    pub name: &'static str,
}

/// Known USB-to-serial chips used in radio programming cables.
pub const KNOWN_CABLES: &[KnownCable] = &[
    KnownCable {
        vid: 0x067B,
        pid: 0x2303,
        chip: CableChip::Pl2303,
        name: "Prolific PL2303",
    },
    KnownCable {
        vid: 0x1A86,
        pid: 0x7523,
        chip: CableChip::Ch340,
        name: "WinChipHead CH340",
    },
    KnownCable {
        vid: 0x10C4,
        pid: 0xEA60,
        chip: CableChip::Cp2102,
        name: "Silicon Labs CP2102",
    },
    KnownCable {
        vid: 0x0403,
        pid: 0x6001,
        chip: CableChip::Ftdi,
        name: "FTDI FT232R",
    },
];

/// Classify a USB device by VID:PID into a cable chip type.
#[must_use]
pub fn classify_cable(vid: u16, pid: u16) -> CableChip {
    KNOWN_CABLES
        .iter()
        .find(|c| c.vid == vid && c.pid == pid)
        .map_or(CableChip::Unknown { vid, pid }, |c| c.chip)
}

/// Look up a known cable entry by VID:PID.
#[must_use]
pub fn lookup_cable(vid: u16, pid: u16) -> Option<&'static KnownCable> {
    KNOWN_CABLES.iter().find(|c| c.vid == vid && c.pid == pid)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn pl2303_vid_pid_classifies_correctly() {
        assert_eq!(classify_cable(0x067B, 0x2303), CableChip::Pl2303);
    }

    #[test]
    fn ch340_vid_pid_classifies_correctly() {
        assert_eq!(classify_cable(0x1A86, 0x7523), CableChip::Ch340);
    }

    #[test]
    fn cp2102_vid_pid_classifies_correctly() {
        assert_eq!(classify_cable(0x10C4, 0xEA60), CableChip::Cp2102);
    }

    #[test]
    fn ftdi_vid_pid_classifies_correctly() {
        assert_eq!(classify_cable(0x0403, 0x6001), CableChip::Ftdi);
    }

    #[test]
    fn unknown_vid_pid_classifies_as_unknown() {
        assert_eq!(
            classify_cable(0xDEAD, 0xBEEF),
            CableChip::Unknown {
                vid: 0xDEAD,
                pid: 0xBEEF,
            }
        );
    }

    #[test]
    fn all_known_cables_resolve_via_lookup() {
        let entries = [
            (0x067B_u16, 0x2303_u16, CableChip::Pl2303),
            (0x1A86, 0x7523, CableChip::Ch340),
            (0x10C4, 0xEA60, CableChip::Cp2102),
            (0x0403, 0x6001, CableChip::Ftdi),
        ];
        for (vid, pid, expected_chip) in &entries {
            let cable = lookup_cable(*vid, *pid).expect("cable must be in table");
            assert_eq!(cable.chip, *expected_chip);
        }
    }

    #[test]
    fn vid_pid_match_is_exact_no_false_positives() {
        assert!(lookup_cable(0x067B, 0x2304).is_none());
        assert!(lookup_cable(0x067C, 0x2303).is_none());
        assert!(lookup_cable(0x1A86, 0x7524).is_none());
        assert!(lookup_cable(0x0000, 0x0000).is_none());
    }

    #[test]
    fn display_shows_chip_names() {
        assert_eq!(CableChip::Pl2303.to_string(), "PL2303");
        assert_eq!(CableChip::Ch340.to_string(), "CH340");
        assert_eq!(CableChip::Cp2102.to_string(), "CP2102");
        assert_eq!(CableChip::Ftdi.to_string(), "FT232R");
        assert_eq!(
            CableChip::Unknown {
                vid: 0xABCD,
                pid: 0x1234
            }
            .to_string(),
            "Unknown (ABCD:1234)"
        );
    }
}
