//! USB cable scanning and enumeration.

use snafu::{ResultExt, Snafu};

use crate::hardware::cables::{CableChip, classify_cable};

/// A detected USB programming cable.
// WHY: pure data — a detection result bag with no derived invariant.
#[derive(Debug, Clone)]
pub struct UsbCable {
    /// USB vendor ID.
    pub vid: u16,
    /// USB product ID.
    pub pid: u16,
    /// Identified chipset.
    pub chip: CableChip,
    /// Serial port path (e.g., "/dev/ttyUSB0").
    pub serial_port: String,
    /// USB manufacturer string, if available.
    pub manufacturer: Option<String>,
    /// USB product string, if available.
    pub product: Option<String>,
    /// USB serial number, if available.
    pub serial_number: Option<String>,
    /// Whether this is a PL2303 clone (`None` if not checked or not PL2303).
    pub is_clone: Option<bool>,
}

/// Errors from USB cable scanning.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum ScanError {
    /// Failed to enumerate available serial ports.
    #[snafu(display("failed to enumerate serial ports"))]
    EnumeratePorts {
        /// The underlying serialport error.
        source: serialport::Error,
        /// Source location.
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Scan for USB programming cables connected to the system.
///
/// Returns a list of detected cables with their VID:PID, chipset, and serial port path.
/// PL2303 cables are checked for clone indicators via USB descriptors.
///
/// # Errors
///
/// Returns [`ScanError::EnumeratePorts`] if serial port enumeration fails.
pub fn scan_usb_cables() -> Result<Vec<UsbCable>, ScanError> {
    let ports = serialport::available_ports().context(EnumeratePortsSnafu)?;
    let mut cables = cables_from_ports(&ports);
    check_pl2303_clones(&mut cables);
    Ok(cables)
}

/// Build cable list from serialport info (testable without hardware).
pub(crate) fn cables_from_ports(ports: &[serialport::SerialPortInfo]) -> Vec<UsbCable> {
    ports
        .iter()
        .filter_map(|port| {
            if let serialport::SerialPortType::UsbPort(info) = &port.port_type {
                Some(UsbCable {
                    vid: info.vid,
                    pid: info.pid,
                    chip: classify_cable(info.vid, info.pid),
                    serial_port: port.port_name.clone(),
                    manufacturer: info.manufacturer.clone(),
                    product: info.product.clone(),
                    serial_number: info.serial_number.clone(),
                    is_clone: None,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Check PL2303 cables for clone indicators via USB descriptors.
fn check_pl2303_clones(cables: &mut [UsbCable]) {
    let Ok(devices) = rusb::devices() else {
        return;
    };

    for cable in cables.iter_mut() {
        if cable.chip == CableChip::Pl2303 {
            cable.is_clone = Some(is_pl2303_clone(&devices, cable.vid, cable.pid));
        }
    }
}

/// Detect PL2303 clones via `bcdDevice` version in USB descriptors.
///
/// Genuine PL2303 chips (TA, TB, RA series) report `bcdDevice` >= 0x0400.
/// Clones (HX, HXA) typically report `bcdDevice` 0x0300.
fn is_pl2303_clone(devices: &rusb::DeviceList<rusb::GlobalContext>, vid: u16, pid: u16) -> bool {
    // WHY: PL2303 clones work on Linux but fail on modern Windows drivers.
    // Checking bcdDevice distinguishes genuine chips from counterfeits.
    for dev in devices.iter() {
        let Ok(desc) = dev.device_descriptor() else {
            continue;
        };
        if desc.vendor_id() == vid && desc.product_id() == pid {
            return desc.device_version() < rusb::Version(0, 4, 0);
        }
    }
    false
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

    fn make_usb_port(name: &str, vid: u16, pid: u16) -> serialport::SerialPortInfo {
        serialport::SerialPortInfo {
            port_name: name.to_string(),
            port_type: serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
                vid,
                pid,
                serial_number: None,
                manufacturer: None,
                product: None,
            }),
        }
    }

    #[test]
    fn cables_from_empty_port_list_returns_empty() {
        assert!(cables_from_ports(&[]).is_empty());
    }

    #[test]
    fn cables_from_usb_ports_identifies_chips() {
        let ports = vec![
            make_usb_port("/dev/ttyUSB0", 0x067B, 0x2303),
            make_usb_port("/dev/ttyUSB1", 0x1A86, 0x7523),
        ];
        let cables = cables_from_ports(&ports);
        assert_eq!(cables.len(), 2);
        assert_eq!(cables[0].chip, CableChip::Pl2303);
        assert_eq!(cables[0].serial_port, "/dev/ttyUSB0");
        assert_eq!(cables[1].chip, CableChip::Ch340);
        assert_eq!(cables[1].serial_port, "/dev/ttyUSB1");
    }

    #[test]
    fn non_usb_ports_are_filtered_out() {
        let ports = vec![
            make_usb_port("/dev/ttyUSB0", 0x067B, 0x2303),
            serialport::SerialPortInfo {
                port_name: "/dev/ttyS0".to_string(),
                port_type: serialport::SerialPortType::PciPort,
            },
            serialport::SerialPortInfo {
                port_name: "/dev/rfcomm0".to_string(),
                port_type: serialport::SerialPortType::BluetoothPort,
            },
        ];
        let cables = cables_from_ports(&ports);
        assert_eq!(cables.len(), 1);
        assert_eq!(cables[0].serial_port, "/dev/ttyUSB0");
    }

    #[test]
    fn unknown_usb_device_classified_as_unknown() {
        let ports = vec![make_usb_port("/dev/ttyUSB0", 0xDEAD, 0xBEEF)];
        let cables = cables_from_ports(&ports);
        assert_eq!(cables.len(), 1);
        assert_eq!(
            cables[0].chip,
            CableChip::Unknown {
                vid: 0xDEAD,
                pid: 0xBEEF
            }
        );
    }
}
