//! Hardware warnings generated during cable scanning and radio detection.

use std::fmt;

use crate::hardware::cables::CableChip;
use crate::hardware::detect::DetectedRadio;
use crate::hardware::usb::UsbCable;

/// Warnings produced during hardware detection.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareWarning {
    /// PL2303 clone detected — works on Linux but may fail on Windows.
    Pl2303Clone {
        /// Serial port path.
        port: String,
    },
    /// Multiple radios detected — user should specify which one.
    MultipleRadiosDetected {
        /// Number of radios found.
        count: usize,
    },
    /// Serial port access denied — likely a permissions issue.
    PortAccessDenied {
        /// Serial port path.
        port: String,
    },
    /// Unknown USB serial device — might still work.
    UnknownCable {
        /// USB vendor ID.
        vid: u16,
        /// USB product ID.
        pid: u16,
        /// Serial port path.
        port: String,
    },
}

impl fmt::Display for HardwareWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pl2303Clone { port } => write!(
                f,
                "PL2303 clone detected on {port}. Works on Linux but may fail on Windows."
            ),
            Self::MultipleRadiosDetected { count } => write!(
                f,
                "Multiple radios detected ({count}). Use --port to specify which one."
            ),
            Self::PortAccessDenied { port } => write!(
                f,
                "Cannot access {port}. Add your user to the 'dialout' GROUP: \
                 `sudo usermod -aG dialout $USER`"
            ),
            Self::UnknownCable { vid, pid, port } => write!(
                f,
                "Unknown USB serial device {vid:04X}:{pid:04X} on {port}. \
                 It might work \u{2014} try --port {port} to use it directly."
            ),
        }
    }
}

/// Collect warnings from a cable scan result.
#[must_use]
pub fn collect_scan_warnings(cables: &[UsbCable]) -> Vec<HardwareWarning> {
    let mut warnings = Vec::new();
    for cable in cables {
        if cable.is_clone == Some(true) {
            warnings.push(HardwareWarning::Pl2303Clone {
                port: cable.serial_port.clone(),
            });
        }
        if let CableChip::Unknown { vid, pid } = cable.chip {
            warnings.push(HardwareWarning::UnknownCable {
                vid,
                pid,
                port: cable.serial_port.clone(),
            });
        }
    }
    warnings
}

/// Collect warnings from radio detection results.
#[must_use]
pub fn collect_detection_warnings(detected: &[DetectedRadio]) -> Vec<HardwareWarning> {
    let mut warnings = Vec::new();
    if detected.len() > 1 {
        warnings.push(HardwareWarning::MultipleRadiosDetected {
            count: detected.len(),
        });
    }
    warnings
}

/// Create a port-access-denied warning.
#[must_use]
pub fn port_access_denied(port: &str) -> HardwareWarning {
    HardwareWarning::PortAccessDenied {
        port: port.to_string(),
    }
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

    fn make_cable(port: &str, chip: CableChip, is_clone: Option<bool>) -> UsbCable {
        let (vid, pid) = match chip {
            CableChip::Pl2303 => (0x067B, 0x2303),
            CableChip::Ch340 => (0x1A86, 0x7523),
            CableChip::Unknown { vid, pid } => (vid, pid),
            CableChip::Cp2102 => (0x10C4, 0xEA60),
            CableChip::Ftdi => (0x0403, 0x6001),
        };
        UsbCable {
            vid,
            pid,
            chip,
            serial_port: port.to_string(),
            manufacturer: None,
            product: None,
            serial_number: None,
            is_clone,
        }
    }

    #[test]
    fn pl2303_clone_generates_warning() {
        let cables = vec![make_cable("/dev/ttyUSB0", CableChip::Pl2303, Some(true))];
        let warnings = collect_scan_warnings(&cables);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings.get(0).copied().unwrap_or_default(),
            HardwareWarning::Pl2303Clone { port } if port == "/dev/ttyUSB0"
        ));
    }

    #[test]
    fn genuine_pl2303_generates_no_warning() {
        let cables = vec![make_cable("/dev/ttyUSB0", CableChip::Pl2303, Some(false))];
        let warnings = collect_scan_warnings(&cables);
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_cable_generates_warning() {
        let cables = vec![make_cable(
            "/dev/ttyUSB0",
            CableChip::Unknown {
                vid: 0xDEAD,
                pid: 0xBEEF,
            },
            None,
        )];
        let warnings = collect_scan_warnings(&cables);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings.get(0).copied().unwrap_or_default(),
            HardwareWarning::UnknownCable {
                vid: 0xDEAD,
                pid: 0xBEEF,
                ..
            }
        ));
    }

    #[test]
    fn port_access_denied_generates_actionable_message() {
        let warning = port_access_denied("/dev/ttyUSB0");
        let msg = warning.to_string();
        assert!(msg.contains("/dev/ttyUSB0"));
        assert!(msg.contains("dialout"));
    }

    #[test]
    fn multiple_radios_generates_warning() {
        use crate::hardware::detect::{RadioIdent, VariantConfig};
        use koinon::RadioKind;

        let detected = vec![
            DetectedRadio {
                cable: make_cable("/dev/ttyUSB0", CableChip::Pl2303, None),
                variant: VariantConfig {
                    kind: RadioKind::BaofengUv5r,
                    baud_rate: 9600,
                    memory_size: 0x1808,
                },
                ident: RadioIdent {
                    firmware: "BFB297".to_string(),
                    raw_response: vec![],
                },
            },
            DetectedRadio {
                cable: make_cable("/dev/ttyUSB1", CableChip::Ch340, None),
                variant: VariantConfig {
                    kind: RadioKind::BaofengBfF8hp,
                    baud_rate: 9600,
                    memory_size: 0x1808,
                },
                ident: RadioIdent {
                    firmware: "BFF800".to_string(),
                    raw_response: vec![],
                },
            },
        ];
        let warnings = collect_detection_warnings(&detected);
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            &warnings.get(0).copied().unwrap_or_default(),
            HardwareWarning::MultipleRadiosDetected { count: 2 }
        ));
    }

    #[test]
    fn single_radio_generates_no_detection_warning() {
        use crate::hardware::detect::{RadioIdent, VariantConfig};
        use koinon::RadioKind;

        let detected = vec![DetectedRadio {
            cable: make_cable("/dev/ttyUSB0", CableChip::Ch340, None),
            variant: VariantConfig {
                kind: RadioKind::BaofengUv5r,
                baud_rate: 9600,
                memory_size: 0x1808,
            },
            ident: RadioIdent {
                firmware: "BFB297".to_string(),
                raw_response: vec![],
            },
        }];
        let warnings = collect_detection_warnings(&detected);
        assert!(warnings.is_empty());
    }
}
