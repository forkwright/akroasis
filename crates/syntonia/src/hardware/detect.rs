//! Radio detection via serial port probing.

use std::io::{Read, Write};
use std::time::Duration;

use koinon::RadioKind;
use snafu::{ResultExt, Snafu};

use crate::hardware::cables::{CableChip, classify_cable};
use crate::hardware::usb::{ScanError, UsbCable, scan_usb_cables};

// ── Types ────────────────────────────────────────────────────────────────────

/// Detected radio variant configuration.
#[derive(Debug, Clone)]
pub struct VariantConfig {
    /// Radio model.
    pub kind: RadioKind,
    /// Baud rate for programming communication.
    pub baud_rate: u32,
    /// Memory size in bytes.
    pub memory_size: u32,
}

/// Radio identification response FROM the auto-detect probe.
#[derive(Debug, Clone)]
pub struct RadioIdent {
    /// Firmware version string.
    pub firmware: String,
    /// Raw identification bytes FROM the radio.
    pub raw_response: Vec<u8>,
}

/// A detected radio with its cable and identification info.
#[derive(Debug, Clone)]
pub struct DetectedRadio {
    /// USB cable connecting the radio.
    pub cable: UsbCable,
    /// Detected radio variant configuration.
    pub variant: VariantConfig,
    /// Radio identification response.
    pub ident: RadioIdent,
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors FROM radio detection.
#[derive(Debug, Snafu)]
pub enum DetectError {
    /// Failed to scan USB ports.
    #[snafu(display("failed to scan USB ports"))]
    ScanPorts {
        /// Source scan error.
        source: ScanError,
        /// Source location.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Failed to open a serial port.
    #[snafu(display("failed to open serial port {port}"))]
    OpenPort {
        /// Port path that could not be opened.
        port: String,
        /// The underlying serialport error.
        source: serialport::Error,
        /// Source location.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Serial I/O error during probing.
    #[snafu(display("serial I/O failed on {port}"))]
    SerialIo {
        /// Port path WHERE I/O failed.
        port: String,
        /// The underlying I/O error.
        source: std::io::Error,
        /// Source location.
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Failed to enumerate serial ports for cable lookup.
    #[snafu(display("failed to enumerate ports for cable lookup"))]
    EnumerateForLookup {
        /// The underlying serialport error.
        source: serialport::Error,
        /// Source location.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

// ── Magic sequences ──────────────────────────────────────────────────────────

const ACK: u8 = 0x06;
const IDENT_REQUEST: u8 = 0x02;
const IDENT_LENGTH: usize = 8;
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
const BAUD_RATE: u32 = 9600;

struct MagicSequence {
    magic: &'static [u8],
    parse: fn(&[u8]) -> Option<VariantConfig>,
}

// WHY: Baofeng radios enter programming mode via a specific magic byte handshake.
// Different radio families use different magic sequences.
const MAGIC_SEQUENCES: &[MagicSequence] = &[MagicSequence {
    magic: &[0x50, 0xBB, 0xFF, 0x20, 0x12, 0x07, 0x25],
    parse: parse_uv5r_ident,
}];

fn parse_uv5r_ident(ident: &[u8]) -> Option<VariantConfig> {
    let prefix = ident.get(..3)?;
    match prefix {
        b"BFB" => Some(VariantConfig {
            kind: RadioKind::BaofengUv5r,
            baud_rate: BAUD_RATE,
            memory_size: 0x1808,
        }),
        b"BFF" => Some(VariantConfig {
            kind: RadioKind::BaofengBfF8hp,
            baud_rate: BAUD_RATE,
            memory_size: 0x1808,
        }),
        b"BFU" => Some(VariantConfig {
            kind: RadioKind::BaofengUv5rmPlus,
            baud_rate: BAUD_RATE,
            memory_size: 0x1808,
        }),
        _ => None,
    }
}

// ── Prober trait ─────────────────────────────────────────────────────────────

/// Abstraction over serial port probing for testability.
pub trait RadioProber {
    /// Probe a serial port for a connected radio.
    ///
    /// # Errors
    ///
    /// Returns [`DetectError`] if the port cannot be opened or communication fails.
    fn probe(&self, port_path: &str) -> Result<Option<(VariantConfig, RadioIdent)>, DetectError>;
}

/// Default prober that opens real serial ports.
struct DefaultProber;

impl RadioProber for DefaultProber {
    fn probe(&self, port_path: &str) -> Result<Option<(VariantConfig, RadioIdent)>, DetectError> {
        let mut port = serialport::new(port_path, BAUD_RATE)
            .timeout(PROBE_TIMEOUT)
            .open()
            .context(OpenPortSnafu {
                port: port_path.to_string(),
            })?;

        for seq in MAGIC_SEQUENCES {
            if let Some(result) = try_magic_sequence(&mut *port, seq) {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }
}

fn try_magic_sequence(
    port: &mut (impl Read + Write + ?Sized),
    seq: &MagicSequence,
) -> Option<(VariantConfig, RadioIdent)> {
    port.write_all(seq.magic).ok()?;

    let mut ack = [0u8; 1];
    port.read_exact(&mut ack).ok()?;
    if ack.get(0).copied().unwrap_or_default() != ACK {
        return None;
    }

    port.write_all(&[IDENT_REQUEST]).ok()?;

    let mut ident_buf = [0u8; IDENT_LENGTH];
    port.read_exact(&mut ident_buf).ok()?;

    let _ = port.write_all(&[ACK]);

    let variant = (seq.parse)(&ident_buf)?;
    let firmware = String::from_utf8_lossy(&ident_buf)
        .trim_end_matches('\0')
        .to_string();
    let ident = RadioIdent {
        firmware,
        raw_response: ident_buf.to_vec(),
    };
    Some((variant, ident))
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Scan for USB cables and probe each for a connected radio.
///
/// Skips ports that fail to open or don't respond to any magic sequence.
/// Returns an empty list (not an error) if no radios are found.
///
/// # Errors
///
/// Returns [`DetectError::ScanPorts`] if USB port enumeration fails.
pub fn detect_radios() -> Result<Vec<DetectedRadio>, DetectError> {
    let cables = scan_usb_cables().context(ScanPortsSnafu)?;
    Ok(detect_radios_impl(cables, &DefaultProber))
}

/// Probe a specific serial port for a connected radio.
///
/// # Errors
///
/// Returns errors if port enumeration or serial communication fails.
pub fn detect_radio_on_port(port_path: &str) -> Result<Option<DetectedRadio>, DetectError> {
    detect_radio_on_port_impl(port_path, &DefaultProber)
}

pub(crate) fn detect_radios_impl(
    cables: Vec<UsbCable>,
    prober: &dyn RadioProber,
) -> Vec<DetectedRadio> {
    let mut detected = Vec::new();

    for cable in cables {
        match prober.probe(&cable.serial_port) {
            Ok(Some((variant, ident))) => {
                detected.push(DetectedRadio {
                    cable,
                    variant,
                    ident,
                });
            }
            Ok(None) => {
                tracing::debug!(port = %cable.serial_port, "no radio detected");
            }
            Err(e) => {
                tracing::warn!(port = %cable.serial_port, error = %e, "failed to probe port");
            }
        }
    }

    detected
}

fn detect_radio_on_port_impl(
    port_path: &str,
    prober: &dyn RadioProber,
) -> Result<Option<DetectedRadio>, DetectError> {
    let cable = find_cable_for_port(port_path)?;
    match prober.probe(port_path)? {
        Some((variant, ident)) => Ok(Some(DetectedRadio {
            cable,
            variant,
            ident,
        })),
        None => Ok(None),
    }
}

fn find_cable_for_port(port_path: &str) -> Result<UsbCable, DetectError> {
    let ports = serialport::available_ports().context(EnumerateForLookupSnafu)?;
    for port in &ports {
        if port.port_name == port_path {
            if let serialport::SerialPortType::UsbPort(info) = &port.port_type {
                return Ok(UsbCable {
                    vid: info.vid,
                    pid: info.pid,
                    chip: classify_cable(info.vid, info.pid),
                    serial_port: port_path.to_string(),
                    manufacturer: info.manufacturer.clone(),
                    product: info.product.clone(),
                    serial_number: info.serial_number.clone(),
                    is_clone: None,
                });
            }
        }
    }

    Ok(UsbCable {
        vid: 0,
        pid: 0,
        chip: CableChip::Unknown { vid: 0, pid: 0 },
        serial_port: port_path.to_string(),
        manufacturer: None,
        product: None,
        serial_number: None,
        is_clone: None,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items
)]
mod tests {
    use super::*;

    // ── Mock serial port ─────────────────────────────────────────────────

    struct MockSerial {
        read_data: std::io::Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl MockSerial {
        fn new(read_data: Vec<u8>) -> Self {
            Self {
                read_data: std::io::Cursor::new(read_data),
                written: Vec::new(),
            }
        }
    }

    impl Read for MockSerial {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.read_data.read(buf)
        }
    }

    impl Write for MockSerial {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // ── Mock prober ──────────────────────────────────────────────────────

    struct MockProber {
        responses: Vec<(String, Option<(VariantConfig, RadioIdent)>)>,
    }

    impl RadioProber for MockProber {
        fn probe(
            &self,
            port_path: &str,
        ) -> Result<Option<(VariantConfig, RadioIdent)>, DetectError> {
            Ok(self
                .responses
                .iter()
                .find(|(p, _)| p == port_path)
                .and_then(|(_, r)| r.clone()))
        }
    }

    fn make_variant(kind: RadioKind) -> VariantConfig {
        VariantConfig {
            kind,
            baud_rate: 9600,
            memory_size: 0x1808,
        }
    }

    fn make_ident(firmware: &str) -> RadioIdent {
        RadioIdent {
            firmware: firmware.to_string(),
            raw_response: firmware.as_bytes().to_vec(),
        }
    }

    fn make_cable(port: &str) -> UsbCable {
        UsbCable {
            vid: 0x067B,
            pid: 0x2303,
            chip: CableChip::Pl2303,
            serial_port: port.to_string(),
            manufacturer: None,
            product: None,
            serial_number: None,
            is_clone: None,
        }
    }

    // ── Magic sequence tests ─────────────────────────────────────────────

    #[test]
    fn try_magic_returns_variant_on_valid_uv5r_response() {
        let response = [&[ACK][..], b"BFB297\x00\x00"].concat();
        let mut port = MockSerial::new(response);

        let result = try_magic_sequence(&mut port, &MAGIC_SEQUENCES.get(0).copied().unwrap_or_default());
        assert!(result.is_some());

        let (variant, ident) = result.unwrap();
        assert_eq!(variant.kind, RadioKind::BaofengUv5r);
        assert_eq!(ident.firmware, "BFB297");
    }

    #[test]
    fn try_magic_returns_variant_for_f8hp() {
        let response = [&[ACK][..], b"BFF800\x00\x00"].concat();
        let mut port = MockSerial::new(response);

        let result = try_magic_sequence(&mut port, &MAGIC_SEQUENCES.get(0).copied().unwrap_or_default());
        assert!(result.is_some());
        assert_eq!(result.unwrap().0.kind, RadioKind::BaofengBfF8hp);
    }

    #[test]
    fn try_magic_returns_none_on_no_ack() {
        let response = vec![0xFF]; // Not an ACK
        let mut port = MockSerial::new(response);
        assert!(try_magic_sequence(&mut port, &MAGIC_SEQUENCES.get(0).copied().unwrap_or_default()).is_none());
    }

    #[test]
    fn try_magic_returns_none_on_empty_response() {
        let mut port = MockSerial::new(vec![]);
        assert!(try_magic_sequence(&mut port, &MAGIC_SEQUENCES.get(0).copied().unwrap_or_default()).is_none());
    }

    #[test]
    fn try_magic_returns_none_for_unrecognized_ident() {
        let response = [&[ACK][..], b"ZZZ999\x00\x00"].concat();
        let mut port = MockSerial::new(response);
        assert!(try_magic_sequence(&mut port, &MAGIC_SEQUENCES.get(0).copied().unwrap_or_default()).is_none());
    }

    #[test]
    fn parse_uv5r_ident_recognizes_known_prefixes() {
        assert!(parse_uv5r_ident(b"BFB297\x00\x00").is_some());
        assert!(parse_uv5r_ident(b"BFF800\x00\x00").is_some());
        assert!(parse_uv5r_ident(b"BFU100\x00\x00").is_some());
        assert!(parse_uv5r_ident(b"ZZZ000\x00\x00").is_none());
        assert!(parse_uv5r_ident(b"BF").is_none());
        assert!(parse_uv5r_ident(b"").is_none());
    }

    // ── Detection integration tests (mocked) ────────────────────────────

    #[test]
    fn detect_with_mock_returns_responding_radio() {
        let cables = vec![make_cable("/dev/ttyUSB0")];
        let prober = MockProber {
            responses: vec![(
                "/dev/ttyUSB0".to_string(),
                Some((make_variant(RadioKind::BaofengUv5r), make_ident("BFB297"))),
            )],
        };

        let results = detect_radios_impl(cables, &prober);
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).copied().unwrap_or_default().variant.kind, RadioKind::BaofengUv5r);
        assert_eq!(results.get(0).copied().unwrap_or_default().cable.serial_port, "/dev/ttyUSB0");
    }

    #[test]
    fn detect_skips_unresponsive_ports() {
        let cables = vec![make_cable("/dev/ttyUSB0")];
        let prober = MockProber {
            responses: vec![("/dev/ttyUSB0".to_string(), None)],
        };

        let results = detect_radios_impl(cables, &prober);
        assert!(results.is_empty());
    }

    #[test]
    fn detect_multiple_ports_returns_only_responding() {
        let cables = vec![
            make_cable("/dev/ttyUSB0"),
            UsbCable {
                vid: 0x1A86,
                pid: 0x7523,
                chip: CableChip::Ch340,
                serial_port: "/dev/ttyUSB1".to_string(),
                manufacturer: None,
                product: None,
                serial_number: None,
                is_clone: None,
            },
        ];
        let prober = MockProber {
            responses: vec![
                ("/dev/ttyUSB0".to_string(), None),
                (
                    "/dev/ttyUSB1".to_string(),
                    Some((make_variant(RadioKind::BaofengBfF8hp), make_ident("BFF800"))),
                ),
            ],
        };

        let results = detect_radios_impl(cables, &prober);
        assert_eq!(results.len(), 1);
        assert_eq!(results.get(0).copied().unwrap_or_default().cable.serial_port, "/dev/ttyUSB1");
        assert_eq!(results.get(0).copied().unwrap_or_default().variant.kind, RadioKind::BaofengBfF8hp);
    }

    #[test]
    fn no_cables_returns_empty_detection() {
        let prober = MockProber { responses: vec![] };
        let results = detect_radios_impl(vec![], &prober);
        assert!(results.is_empty());
    }
}
