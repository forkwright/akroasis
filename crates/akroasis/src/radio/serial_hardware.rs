//! Feature-gated live serial radio detection and protocol session backend.

use koinon::RadioKind;
use syntonia::baofeng::{
    codec::{CodecError, decode_all_channels, encode_all_channels},
    image::MemoryImage,
    protocol::Uv5rProtocol,
    variant::{MAGIC_SETS, VariantConfig, identify_variant},
};
use syntonia::hardware::{self, CableChip, DetectedRadio as SyntoniaDetectedRadio, UsbCable};
use syntonia::serial::HardwareSerialPort;
use syntonia::{Channel, FrequencyPlan};

use super::errors::RadioError;
use super::{DetectedRadio, Hardware, RadioVariant, Session};

pub(super) struct SerialHardware;

impl Hardware for SerialHardware {
    fn detect_radios(&self) -> Result<Vec<DetectedRadio>, RadioError> {
        let detected =
            hardware::detect_radios().map_err(|source| RadioError::HardwareDetect { source })?;
        Ok(detected.into_iter().filter_map(to_cli_radio).collect())
    }

    fn detect_radio_on_port(&self, port: &str) -> Result<Option<DetectedRadio>, RadioError> {
        let detected = hardware::detect_radio_on_port(port)
            .map_err(|source| RadioError::HardwareDetect { source })?;
        Ok(detected.and_then(to_cli_radio))
    }

    fn open(&self, port: &str) -> Result<Box<dyn Session>, RadioError> {
        BaofengProtocolSession::open(port).map(|s| Box::new(s) as Box<dyn Session>)
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

// ---------------------------------------------------------------------------
// Baofeng protocol session — wraps Uv5rProtocol<HardwareSerialPort>
// ---------------------------------------------------------------------------

/// An open Baofeng UV-5R family protocol session on a real serial port.
struct BaofengProtocolSession {
    protocol: Uv5rProtocol<HardwareSerialPort>,
    config: VariantConfig,
    variant: RadioVariant,
}

impl BaofengProtocolSession {
    /// Open a Baofeng session: open the port, try all magic sequences, identify, enter clone mode.
    ///
    /// # Errors
    ///
    /// Returns `RadioError` if the port cannot be opened, the radio does not
    /// respond to any magic sequence, or the firmware string is unrecognized.
    fn open(port_path: &str) -> Result<Self, RadioError> {
        // WHY: UV-5R programming sessions run at 9600 baud 8N1 regardless of variant.
        const BAUD_RATE: u32 = 9600;
        let hw_port =
            HardwareSerialPort::open(port_path, BAUD_RATE).map_err(|e| RadioError::PermissionDenied {
                port: format!("{port_path}: {e}"),
            })?;

        let mut protocol = Uv5rProtocol::new(hw_port);

        // Try every magic sequence in priority order (UV5R-291, BF-F8HP, UV5R-orig).
        let ident = {
            let mut found = None;
            for magic in MAGIC_SETS {
                if protocol.enter_programming_mode(magic).is_ok() {
                    match protocol.identify() {
                        Ok(id) => {
                            found = Some(id);
                            break;
                        }
                        Err(_) => continue,
                    }
                }
            }
            found.ok_or_else(|| RadioError::SerialTimeout {
                port: port_path.to_string(),
            })?
        };

        // Map the raw ident bytes to a VariantConfig.
        let config = identify_variant(&ident).map_err(|_| RadioError::WrongBaudRate {
            port: port_path.to_string(),
        })?;

        let variant = variant_from_config(&config).ok_or(RadioError::WrongBaudRate {
            port: port_path.to_string(),
        })?;

        Ok(Self {
            protocol,
            config,
            variant,
        })
    }
}

fn variant_from_config(config: &VariantConfig) -> Option<RadioVariant> {
    use syntonia::baofeng::variant::RadioVariant as SynVariant;
    match config.variant {
        SynVariant::Uv5r | SynVariant::Uv5rOriginal => Some(RadioVariant::Uv5r),
        SynVariant::BfF8hp => Some(RadioVariant::BfF8hp),
        SynVariant::Uv5rmPlus => Some(RadioVariant::Uv5rmPlus),
    }
}

fn codec_to_radio_error(e: CodecError) -> RadioError {
    RadioError::Plan {
        message: e.to_string(),
    }
}

impl Session for BaofengProtocolSession {
    fn variant(&self) -> RadioVariant {
        self.variant
    }

    fn download_image(&mut self, on_block: &dyn Fn(u16, u16)) -> Result<Vec<u8>, RadioError> {
        // WHY: UV-5R main memory is 0x0000–0x1800 in 64-byte blocks = 96 blocks.
        // With aux block (BF-F8HP) adds ~32 more blocks. We emit progress in 64-byte
        // increments; the caller's progress bar expects (done, total) block counts.
        let mem_image =
            self.protocol
                .download_image()
                .map_err(|e| RadioError::SerialTimeout {
                    port: format!("download failed: {e}"),
                })?;

        // Signal completion to progress bar.
        on_block(128, 128);
        Ok(mem_image.as_slice().to_vec())
    }

    fn upload_image(&mut self, data: &[u8], on_block: &dyn Fn(u16, u16)) -> Result<(), RadioError> {
        let image = MemoryImage::from_bytes(data.to_vec());
        self.protocol
            .upload_image(&image, &mut |done, total| {
                on_block(
                    u16::try_from(done).unwrap_or(u16::MAX),
                    u16::try_from(total).unwrap_or(u16::MAX),
                );
            })
            .map_err(|e| RadioError::VerificationFailed {
                message: e.to_string(),
            })
    }

    fn decode_channels(&self, image: &[u8]) -> Result<Vec<Channel>, RadioError> {
        let mem_image = MemoryImage::from_bytes(image.to_vec());
        let plan = decode_all_channels(&mem_image).map_err(codec_to_radio_error)?;
        Ok(plan.channels)
    }

    fn encode_channels(&self, channels: &[Channel]) -> Result<Vec<u8>, RadioError> {
        // WHY: image size must cover the full EEPROM including aux block (0x2000 = 8192).
        // Use a large enough buffer; the protocol upload uses only safe write ranges.
        const IMAGE_SIZE: usize = 0x2000;
        let plan = FrequencyPlan {
            name: "program".to_string(),
            radio_model: None,
            channels: channels.to_vec(),
            created: None,
        };
        let mut image = MemoryImage::new(IMAGE_SIZE);
        encode_all_channels(&plan, &mut image).map_err(codec_to_radio_error)?;
        Ok(image.as_slice().to_vec())
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions use unwrap for clarity")]
mod tests {
    use syntonia::hardware::{RadioIdent, VariantConfig};

    use super::*;

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
