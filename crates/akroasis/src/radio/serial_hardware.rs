//! Feature-gated live serial radio detection backend.

use koinon::RadioKind;
use syntonia::hardware::{self, CableChip, DetectedRadio as SyntoniaDetectedRadio, UsbCable};

use super::errors::RadioError;
use super::{DetectedRadio, Hardware, RadioVariant, Session};

pub(super) struct SerialHardware;

impl Hardware for SerialHardware {
    fn detect_radios(&self) -> Result<Vec<DetectedRadio>, RadioError> {
        let detected =
            hardware::detect_radios().map_err(|source| RadioError::HardwareDetect { source })?;
        Ok(detected.into_iter().filter_map(to_cli_radio).collect())
    }

    fn open(&self, _port: &str) -> Result<Box<dyn Session>, RadioError> {
        Err(RadioError::HardwareNotAvailable)
    }
}

fn to_cli_radio(detected: SyntoniaDetectedRadio) -> Option<DetectedRadio> {
    let warnings = cable_warnings(&detected.cable);
    Some(DetectedRadio {
        variant: to_cli_variant(detected.variant.kind)?,
        port: detected.cable.serial_port,
        firmware: detected.ident.firmware,
        warnings,
    })
}

const fn to_cli_variant(kind: RadioKind) -> Option<RadioVariant> {
    match kind {
        RadioKind::BaofengUv5r => Some(RadioVariant::Uv5r),
        RadioKind::BaofengBfF8hp => Some(RadioVariant::BfF8hp),
        RadioKind::BaofengUv5rmPlus => Some(RadioVariant::Uv5rmPlus),
        _ => None,
    }
}

fn cable_warnings(cable: &UsbCable) -> Vec<String> {
    let mut warnings = Vec::new();

    if cable.is_clone == Some(true) {
        warnings.push(format!(
            "PL2303 clone detected on {}. Works on Linux but may fail on Windows.",
            cable.serial_port
        ));
    }

    if let CableChip::Unknown { vid, pid } = cable.chip {
        warnings.push(format!(
            "Unknown USB serial device {vid:04X}:{pid:04X} on {}. It might work; try --port {} to use it directly.",
            cable.serial_port, cable.serial_port
        ));
    }

    warnings
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
mod tests {
    use super::*;
    use syntonia::hardware::{RadioIdent, VariantConfig};

    fn cable(chip: CableChip) -> UsbCable {
        UsbCable {
            vid: 0x1234,
            pid: 0x5678,
            chip,
            serial_port: "/dev/ttyUSB0".to_string(),
            manufacturer: None,
            product: None,
            serial_number: None,
            is_clone: None,
        }
    }

    #[test]
    fn maps_baofeng_detected_radio_to_cli_descriptor() {
        let detected = SyntoniaDetectedRadio {
            cable: cable(CableChip::Ch340),
            variant: VariantConfig {
                kind: RadioKind::BaofengBfF8hp,
                baud_rate: 9600,
                memory_size: 0x1808,
            },
            ident: RadioIdent {
                firmware: "BFF800".to_string(),
                raw_response: b"BFF800".to_vec(),
            },
        };

        let cli = to_cli_radio(detected).unwrap();

        assert_eq!(cli.variant, RadioVariant::BfF8hp);
        assert_eq!(cli.port, "/dev/ttyUSB0");
        assert_eq!(cli.firmware, "BFF800");
        assert!(cli.warnings.is_empty());
    }

    #[test]
    fn cable_warnings_include_clone_and_unknown_adapter() {
        let mut usb = cable(CableChip::Unknown {
            vid: 0x9999,
            pid: 0x0001,
        });
        usb.is_clone = Some(true);

        let warnings = cable_warnings(&usb);

        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.contains("PL2303 clone")));
        assert!(warnings.iter().any(|w| w.contains("9999:0001")));
    }
}
