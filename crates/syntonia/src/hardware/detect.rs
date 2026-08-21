//! Radio detection via serial port probing.

use std::io::{self, Read, Write};
use std::time::Duration;

use snafu::{ResultExt, Snafu};
use stoicheion::RadioKind;

use crate::baofeng::variant::{
    BF_F8HP_PREFIXES, MAGIC_UV5R_PROBE, UV5R_PREFIXES, UV5RM_PLUS_PREFIXES,
};
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
// WHY: pure data — a raw identification response with no derived invariant.
#[derive(Debug, Clone)]
pub struct RadioIdent {
    /// Firmware version string.
    pub firmware: String,
    /// Raw identification bytes FROM the radio.
    pub raw_response: Vec<u8>,
}

/// A detected radio with its cable and identification info.
// WHY: pure data — a detection result bag with no derived invariant.
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
#[non_exhaustive]
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
    magic: &'static [u8], // kanon:ignore RUST/indexing-slicing -- function parameter &'static [u8], not indexing
    parse: fn(&[u8]) -> Option<VariantConfig>,
}

// WHY: Baofeng radios enter programming mode via a specific magic byte handshake.
// Different radio families use different magic sequences.
const MAGIC_SEQUENCES: &[MagicSequence] = &[MagicSequence {
    magic: &MAGIC_UV5R_PROBE,
    parse: parse_uv5r_ident,
}];

fn parse_uv5r_ident(ident: &[u8]) -> Option<VariantConfig> {
    // WHY: the firmware-prefix tables are owned by `baofeng::variant`. This
    // module used to keep a private three-letter copy which had drifted from
    // it, so a BF-F8HP answering `BFP3V3 F` — the prefix the owning table
    // lists first — was reported as no radio at all.
    let kind = classify_ident(ident)?;
    Some(VariantConfig {
        kind,
        baud_rate: BAUD_RATE,
        memory_size: 0x1808,
    })
}

/// Match an ident response against the owning prefix tables, most specific
/// family first.
fn classify_ident(ident: &[u8]) -> Option<RadioKind> {
    let matches = |table: &[&str]| {
        table
            .iter()
            .any(|prefix| ident.starts_with(prefix.as_bytes()))
    };

    // WHY: ordered as `baofeng::variant::identify_variant` orders them, so the
    // two paths cannot disagree about a prefix listed in more than one table.
    if matches(BF_F8HP_PREFIXES) {
        Some(RadioKind::BaofengBfF8hp)
    } else if matches(UV5R_PREFIXES) {
        Some(RadioKind::BaofengUv5r)
    } else if matches(UV5RM_PLUS_PREFIXES) {
        Some(RadioKind::BaofengUv5rmPlus)
    } else {
        None
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

#[rustfmt::skip]
impl RadioProber for DefaultProber { // kanon:ignore ARCHITECTURE/trait-impl-colocation -- RadioProber trait exists for testability; DefaultProber is the production path
    fn probe(&self, port_path: &str) -> Result<Option<(VariantConfig, RadioIdent)>, DetectError> {
        let mut port = serialport::new(port_path, BAUD_RATE)
            .timeout(PROBE_TIMEOUT)
            .open()
            .context(OpenPortSnafu {
                port: port_path.to_string(),
            })?;

        probe_port(&mut *port, port_path)
    }
}

/// Try every magic sequence against an already-open port.
///
/// # Errors
///
/// Returns [`DetectError::SerialIo`] if the port itself fails; a port that
/// merely stays silent yields `Ok(None)`.
fn probe_port(
    port: &mut (impl Read + Write + ?Sized),
    port_path: &str,
) -> Result<Option<(VariantConfig, RadioIdent)>, DetectError> {
    for seq in MAGIC_SEQUENCES {
        match try_magic_sequence(&mut *port, seq) {
            Ok(Some(result)) => return Ok(Some(result)),
            Ok(None) => {}
            Err(err) if is_silence(&err) => {}
            Err(err) => {
                return Err(err).context(SerialIoSnafu {
                    port: port_path.to_string(),
                });
            }
        }
    }
    Ok(None)
}

/// Whether an I/O error means the radio said nothing, as opposed to the port
/// itself failing.
///
/// WHY: a probe that swallows every I/O error reports a permission-denied or
/// disconnected port as "no radio on this cable", which sends the operator
/// looking for a radio fault. Only silence — a read that times out, or a port
/// that closes mid-handshake — means "not this variant, try the next".
fn is_silence(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::UnexpectedEof | io::ErrorKind::WouldBlock
    )
}

/// Run one magic-sequence handshake.
///
/// `Ok(None)` means the radio answered but is not this variant; `Err` is a
/// port-level I/O failure, which the caller separates from silence via
/// [`is_silence`].
fn try_magic_sequence(
    port: &mut (impl Read + Write + ?Sized),
    seq: &MagicSequence,
) -> io::Result<Option<(VariantConfig, RadioIdent)>> {
    port.write_all(seq.magic)?;

    let mut ack = [0u8; 1];
    port.read_exact(&mut ack)?;
    if ack.first().copied().unwrap_or_default() != ACK {
        return Ok(None);
    }

    port.write_all(&[IDENT_REQUEST])?;

    let mut ident_buf = [0u8; IDENT_LENGTH];
    port.read_exact(&mut ident_buf)?;

    // WHY: the ident bytes are already read at this point, so a failure to
    // write the closing ACK does not invalidate the identification — log it
    // rather than failing a handshake that otherwise succeeded.
    if let Err(error) = port.write_all(&[ACK]) {
        tracing::debug!(%error, "failed to write closing ACK after ident read");
    }

    let Some(variant) = (seq.parse)(&ident_buf) else {
        return Ok(None);
    };
    let firmware = String::from_utf8_lossy(&ident_buf)
        .trim_end_matches('\0')
        .to_string();
    let ident = RadioIdent {
        firmware,
        raw_response: ident_buf.to_vec(),
    };
    Ok(Some((variant, ident)))
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
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::type_complexity,
    clippy::missing_docs_in_private_items,
    reason = "test code: panics and unwraps acceptable in assertions"
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

    /// Serial port that fails every read with a fixed error kind.
    struct FailingSerial {
        kind: std::io::ErrorKind,
    }

    impl Read for FailingSerial {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.kind, "mock port failure"))
        }
    }

    impl Write for FailingSerial {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn try_magic_returns_variant_on_valid_uv5r_response() {
        let response = [&[ACK][..], b"BFB297\x00\x00"].concat();
        let mut port = MockSerial::new(response);

        let result = try_magic_sequence(&mut port, MAGIC_SEQUENCES.first().unwrap()).unwrap();
        assert!(result.is_some());

        let (variant, ident) = result.unwrap();
        assert_eq!(variant.kind, RadioKind::BaofengUv5r);
        assert_eq!(ident.firmware, "BFB297");
    }

    #[test]
    fn try_magic_returns_variant_for_f8hp() {
        let response = [&[ACK][..], b"BFF800\x00\x00"].concat();
        let mut port = MockSerial::new(response);

        let result = try_magic_sequence(&mut port, MAGIC_SEQUENCES.first().unwrap()).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().0.kind, RadioKind::BaofengBfF8hp);
    }

    #[test]
    fn try_magic_returns_none_on_no_ack() {
        let response = vec![0xFF]; // Not an ACK
        let mut port = MockSerial::new(response);
        let result = try_magic_sequence(&mut port, MAGIC_SEQUENCES.first().unwrap()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn try_magic_treats_empty_response_as_silence() {
        let mut port = MockSerial::new(vec![]);
        let err = try_magic_sequence(&mut port, MAGIC_SEQUENCES.first().unwrap()).unwrap_err();
        assert!(
            is_silence(&err),
            "empty read should read as silence: {err:?}"
        );
    }

    #[test]
    fn try_magic_returns_none_for_unrecognized_ident() {
        let response = [&[ACK][..], b"ZZZ999\x00\x00"].concat();
        let mut port = MockSerial::new(response);
        let result = try_magic_sequence(&mut port, MAGIC_SEQUENCES.first().unwrap()).unwrap();
        assert!(result.is_none());
    }

    // ── Prefix-table alignment ───────────────────────────────────────────

    // WHY: `BFP3V3 F` is the prefix `baofeng::variant::BF_F8HP_PREFIXES` lists
    // first for the BF-F8HP. The private three-letter table this module used
    // to carry did not contain it, so a real F8HP probed as no radio at all.
    #[test]
    fn f8hp_ident_from_the_owning_table_is_classified() {
        assert_eq!(
            classify_ident(b"BFP3V3 F").unwrap(),
            RadioKind::BaofengBfF8hp
        );
        assert_eq!(
            classify_ident(b"N5R-3\x00\x00\x00").unwrap(),
            RadioKind::BaofengBfF8hp
        );
        assert_eq!(
            classify_ident(b"BFT297\x00\x00").unwrap(),
            RadioKind::BaofengBfF8hp
        );
    }

    // WHY: the falsifying sibling — a UV-5R prefix must not be swept into the
    // F8HP arm by the wider table.
    #[test]
    fn uv5r_idents_from_the_owning_table_stay_uv5r() {
        assert_eq!(
            classify_ident(b"BFB297\x00\x00").unwrap(),
            RadioKind::BaofengUv5r
        );
        assert_eq!(
            classify_ident(b"BTS123\x00\x00").unwrap(),
            RadioKind::BaofengUv5r
        );
        assert_eq!(
            classify_ident(b"N5R-2\x00\x00\x00").unwrap(),
            RadioKind::BaofengUv5r
        );
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

    // ── Probe failure classification ─────────────────────────────────────

    // WHY: the pair. A port that fails to read is a port fault and must
    // surface; a port that stays silent is a cable with no radio on it and
    // must not. Before this split both arrived as `None`.
    #[test]
    fn probe_reports_a_port_level_io_failure() {
        let mut port = FailingSerial {
            kind: std::io::ErrorKind::PermissionDenied,
        };
        let err = probe_port(&mut port, "/dev/ttyUSB0").unwrap_err();
        assert!(
            matches!(err, DetectError::SerialIo { .. }),
            "expected SerialIo, got: {err:?}"
        );
    }

    #[test]
    fn probe_reports_a_silent_port_as_no_radio() {
        let mut port = FailingSerial {
            kind: std::io::ErrorKind::TimedOut,
        };
        let result = probe_port(&mut port, "/dev/ttyUSB0").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn probe_returns_the_radio_when_one_answers() {
        let response = [&[ACK][..], b"BFP3V3 F"].concat();
        let mut port = MockSerial::new(response);
        let (variant, _) = probe_port(&mut port, "/dev/ttyUSB0").unwrap().unwrap();
        assert_eq!(variant.kind, RadioKind::BaofengBfF8hp);
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
        assert_eq!(
            results.first().unwrap().variant.kind,
            RadioKind::BaofengUv5r
        );
        assert_eq!(results.first().unwrap().cable.serial_port, "/dev/ttyUSB0");
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
        assert_eq!(results.first().unwrap().cable.serial_port, "/dev/ttyUSB1");
        assert_eq!(
            results.first().unwrap().variant.kind,
            RadioKind::BaofengBfF8hp
        );
    }

    #[test]
    fn no_cables_returns_empty_detection() {
        let prober = MockProber { responses: vec![] };
        let results = detect_radios_impl(vec![], &prober);
        assert!(results.is_empty());
    }

    // ── Single-port detection tests (detect_radio_on_port_impl) ──────────

    #[test]
    fn detect_radio_on_port_returns_responding_radio() {
        let prober = MockProber {
            responses: vec![(
                "/dev/ttyUSB-TEST-A".to_string(),
                Some((make_variant(RadioKind::BaofengUv5r), make_ident("BFB297"))),
            )],
        };

        let radio = detect_radio_on_port_impl("/dev/ttyUSB-TEST-A", &prober)
            .unwrap()
            .unwrap();
        assert_eq!(radio.variant.kind, RadioKind::BaofengUv5r);
    }

    #[test]
    fn detect_radio_on_port_returns_none_when_unresponsive() {
        let prober = MockProber {
            responses: vec![("/dev/ttyUSB-TEST-B".to_string(), None)],
        };

        let result = detect_radio_on_port_impl("/dev/ttyUSB-TEST-B", &prober).unwrap();
        assert!(result.is_none());
    }

    // WHY: "/dev/ttyUSB-TEST-C" is absent from any real host's port list, so
    // find_cable_for_port deterministically takes the not-found fallback
    // (vid/pid 0, CableChip::Unknown) rather than a live OS port lookup.
    #[test]
    fn detect_radio_on_port_not_found_falls_back_to_unknown_cable() {
        let prober = MockProber {
            responses: vec![(
                "/dev/ttyUSB-TEST-C".to_string(),
                Some((make_variant(RadioKind::BaofengBfF8hp), make_ident("BFF800"))),
            )],
        };

        let radio = detect_radio_on_port_impl("/dev/ttyUSB-TEST-C", &prober)
            .unwrap()
            .unwrap();
        assert_eq!(radio.cable.chip, CableChip::Unknown { vid: 0, pid: 0 });
        assert_eq!(radio.cable.vid, 0);
        assert_eq!(radio.cable.pid, 0);
    }
}
