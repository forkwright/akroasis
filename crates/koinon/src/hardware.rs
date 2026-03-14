//! Hardware asset registry — inventory of all physical devices managed by Akroasis.

use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;

use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

use crate::DeviceId;

// ── UsbId ────────────────────────────────────────────────────────────────────

/// USB vendor ID / product ID pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UsbId {
    /// USB vendor identifier.
    pub vid: u16,
    /// USB product identifier.
    pub pid: u16,
}

// ── HardwareKind ─────────────────────────────────────────────────────────────

/// High-level hardware category.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HardwareKind {
    /// VHF/UHF radio transceiver.
    Radio(RadioKind),
    /// Software-defined radio receiver.
    Sdr(SdrKind),
    /// `LoRa` mesh network node.
    MeshNode(MeshNodeKind),
    /// Passive antenna element.
    Antenna,
    /// GNSS/GPS receiver.
    Gps,
    /// Network interface adapter.
    NetworkAdapter,
    /// Environmental or proximity sensor.
    Sensor,
}

impl fmt::Display for HardwareKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Radio(k) => write!(f, "{k}"),
            Self::Sdr(k) => write!(f, "{k}"),
            Self::MeshNode(k) => write!(f, "{k}"),
            Self::Antenna => f.write_str("Antenna"),
            Self::Gps => f.write_str("GPS"),
            Self::NetworkAdapter => f.write_str("Network Adapter"),
            Self::Sensor => f.write_str("Sensor"),
        }
    }
}

// ── RadioKind ────────────────────────────────────────────────────────────────

/// Specific radio transceiver model.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RadioKind {
    /// Baofeng UV-5R handheld transceiver.
    BaofengUv5r,
    /// Baofeng BF-F8HP handheld transceiver.
    BaofengBfF8hp,
    /// Baofeng UV-5RM Plus handheld transceiver.
    BaofengUv5rmPlus,
    /// Yaesu FTM-510DR mobile transceiver.
    YaesuFtm510dr,
}

impl fmt::Display for RadioKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::BaofengUv5r => "Baofeng UV-5R",
            Self::BaofengBfF8hp => "Baofeng BF-F8HP",
            Self::BaofengUv5rmPlus => "Baofeng UV-5RM Plus",
            Self::YaesuFtm510dr => "Yaesu FTM-510DR",
        };
        f.write_str(name)
    }
}

// ── SdrKind ──────────────────────────────────────────────────────────────────

/// Specific SDR receiver model.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SdrKind {
    /// RTL-SDR Blog V4 dongle.
    RtlSdrV4,
    /// `HackRF` One wideband transceiver.
    HackRfOne,
}

impl fmt::Display for SdrKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::RtlSdrV4 => "RTL-SDR Blog V4",
            Self::HackRfOne => "HackRF One",
        };
        f.write_str(name)
    }
}

// ── MeshNodeKind ─────────────────────────────────────────────────────────────

/// Specific `LoRa` mesh node model.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MeshNodeKind {
    /// LILYGO T-Echo Meshtastic device.
    TEcho,
    /// LILYGO T-Deck Plus Meshtastic device.
    TDeckPlus,
    /// RAK2245 Pi HAT `LoRa` gateway module.
    Rak2245,
    /// RAK `WisBlock` modular `IoT` platform.
    WisBlock,
}

impl fmt::Display for MeshNodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::TEcho => "T-Echo",
            Self::TDeckPlus => "T-Deck Plus",
            Self::Rak2245 => "RAK2245",
            Self::WisBlock => "WisBlock",
        };
        f.write_str(name)
    }
}

// ── ConnectionType ────────────────────────────────────────────────────────────

/// Physical or logical connection path to a device.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionType {
    /// USB serial adapter (programming cable).
    UsbSerial {
        /// USB vendor identifier of the serial adapter chipset.
        vid: u16,
        /// USB product identifier of the serial adapter chipset.
        pid: u16,
        /// Serial baud rate in bits per second.
        baud: u32,
    },
    /// TCP/IP network connection.
    Tcp {
        /// Remote socket address.
        addr: SocketAddr,
    },
    /// Bluetooth Low Energy connection.
    Ble {
        /// 48-bit MAC address in big-endian byte order.
        mac: [u8; 6],
    },
    /// I²C bus connection.
    I2c {
        /// I²C bus number.
        bus: u8,
        /// 7-bit I²C device address.
        addr: u8,
    },
    /// SPI bus connection.
    Spi {
        /// SPI bus number.
        bus: u8,
        /// Chip select line index.
        cs: u8,
    },
    /// USB device with no serial abstraction (raw USB).
    UsbDirect {
        /// USB vendor identifier.
        vid: u16,
        /// USB product identifier.
        pid: u16,
    },
}

impl fmt::Display for ConnectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsbSerial { vid, pid, baud } => {
                write!(f, "USB serial {vid:04X}:{pid:04X} @ {baud} baud")
            }
            Self::Tcp { addr } => write!(f, "TCP {addr}"),
            Self::Ble {
                mac: [b0, b1, b2, b3, b4, b5],
            } => write!(
                f,
                "BLE {b0:02X}:{b1:02X}:{b2:02X}:{b3:02X}:{b4:02X}:{b5:02X}"
            ),
            Self::I2c { bus, addr } => write!(f, "I2C bus {bus} addr 0x{addr:02X}"),
            Self::Spi { bus, cs } => write!(f, "SPI bus {bus} CS {cs}"),
            Self::UsbDirect { vid, pid } => write!(f, "USB direct {vid:04X}:{pid:04X}"),
        }
    }
}

// ── AssetStatus ───────────────────────────────────────────────────────────────

/// Operational status of a hardware asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetStatus {
    /// Registered but not currently detected.
    Offline,
    /// Detected and available for use.
    Available,
    /// Currently in active use by a subsystem.
    InUse,
    /// Error state — device may be malfunctioning.
    Error,
}

impl fmt::Display for AssetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Offline => "Offline",
            Self::Available => "Available",
            Self::InUse => "In Use",
            Self::Error => "Error",
        };
        f.write_str(name)
    }
}

// ── HardwareAsset ─────────────────────────────────────────────────────────────

/// A single physical device managed by Akroasis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareAsset {
    /// Unique device identifier.
    pub device_id: DeviceId,
    /// Hardware category and model.
    pub kind: HardwareKind,
    /// Human-readable name for the device.
    pub name: CompactString,
    /// Manufacturer serial number, if known.
    pub serial_number: Option<CompactString>,
    /// USB VID:PID pair for the device itself (not the cable adapter), if applicable.
    pub usb_vid_pid: Option<UsbId>,
    /// How the device is connected to the host.
    pub connection: ConnectionType,
    /// Current operational status.
    pub status: AssetStatus,
}

// ── Known USB device table ─────────────────────────────────────────────────────

/// A USB device chipset with known vendor/product identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownUsbDevice {
    /// USB vendor identifier.
    pub vid: u16,
    /// USB product identifier.
    pub pid: u16,
    /// Human-readable device name.
    pub name: &'static str,
    /// Chipset name.
    pub chip: &'static str,
    /// Whether clones of this chipset are prevalent and may misbehave.
    pub is_clone_risk: bool,
}

/// USB devices commonly encountered when identifying Akroasis hardware.
pub const KNOWN_USB_DEVICES: &[KnownUsbDevice] = &[
    // Serial adapters (programming cables)
    KnownUsbDevice {
        vid: 0x067B,
        pid: 0x2303,
        name: "Prolific PL2303",
        chip: "PL2303",
        is_clone_risk: true,
    },
    KnownUsbDevice {
        vid: 0x1A86,
        pid: 0x7523,
        name: "QinHeng CH340",
        chip: "CH340",
        is_clone_risk: false,
    },
    KnownUsbDevice {
        vid: 0x10C4,
        pid: 0xEA60,
        name: "Silicon Labs CP2102",
        chip: "CP2102",
        is_clone_risk: false,
    },
    KnownUsbDevice {
        vid: 0x0403,
        pid: 0x6001,
        name: "FTDI FT232R",
        chip: "FT232R",
        is_clone_risk: false,
    },
    // SDR devices
    KnownUsbDevice {
        vid: 0x0BDA,
        pid: 0x2838,
        name: "RTL-SDR",
        chip: "RTL2832U",
        is_clone_risk: false,
    },
    // Meshtastic devices (common ESP32-S3 USB)
    KnownUsbDevice {
        vid: 0x303A,
        pid: 0x1001,
        name: "Espressif ESP32-S3",
        chip: "ESP32-S3",
        is_clone_risk: false,
    },
];

/// Return the matching entry from [`KNOWN_USB_DEVICES`] for the given VID:PID pair.
#[must_use]
pub fn lookup_usb_device(vid: u16, pid: u16) -> Option<&'static KnownUsbDevice> {
    KNOWN_USB_DEVICES
        .iter()
        .find(|d| d.vid == vid && d.pid == pid)
}

// ── RegistryError ─────────────────────────────────────────────────────────────

/// Errors produced by [`AssetRegistry`] operations.
#[derive(Debug, Snafu)]
pub enum RegistryError {
    /// A device with the same ID was already registered.
    #[snafu(display("device {device_id} already registered"))]
    AlreadyRegistered {
        /// The conflicting device ID.
        device_id: DeviceId,
    },
    /// The requested device ID does not exist in the registry.
    #[snafu(display("device {device_id} not found"))]
    NotFound {
        /// The missing device ID.
        device_id: DeviceId,
    },
}

// ── AssetRegistry ─────────────────────────────────────────────────────────────

/// In-memory inventory of all hardware assets known to Akroasis.
#[derive(Debug, Default)]
pub struct AssetRegistry {
    assets: BTreeMap<DeviceId, HardwareAsset>,
}

impl AssetRegistry {
    /// Create an empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            assets: BTreeMap::new(),
        }
    }

    /// Add a new asset.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::AlreadyRegistered`] if an asset with the same [`DeviceId`]
    /// already exists.
    pub fn register(&mut self, asset: HardwareAsset) -> Result<(), RegistryError> {
        if self.assets.contains_key(&asset.device_id) {
            return AlreadyRegisteredSnafu {
                device_id: asset.device_id,
            }
            .fail();
        }
        self.assets.insert(asset.device_id, asset);
        Ok(())
    }

    /// Remove an asset and return it, or `None` if the ID was not registered.
    pub fn unregister(&mut self, device_id: &DeviceId) -> Option<HardwareAsset> {
        self.assets.remove(device_id)
    }

    /// Retrieve a registered asset by ID.
    #[must_use]
    pub fn get(&self, device_id: &DeviceId) -> Option<&HardwareAsset> {
        self.assets.get(device_id)
    }

    /// Retrieve a mutable reference to a registered asset by ID.
    #[must_use]
    pub fn get_mut(&mut self, device_id: &DeviceId) -> Option<&mut HardwareAsset> {
        self.assets.get_mut(device_id)
    }

    /// Return all assets whose top-level hardware kind matches the discriminant of `kind_filter`.
    ///
    /// Inner variant values are ignored — `Radio(BaofengUv5r)` and `Radio(YaesuFtm510dr)`
    /// both match a filter of `Radio(_)`.
    #[must_use]
    pub fn find_by_kind(&self, kind_filter: &HardwareKind) -> Vec<&HardwareAsset> {
        let target = std::mem::discriminant(kind_filter);
        self.assets
            .values()
            .filter(|a| std::mem::discriminant(&a.kind) == target)
            .collect()
    }

    /// Return all assets with a USB VID:PID matching the given pair.
    #[must_use]
    pub fn find_by_usb(&self, vid: u16, pid: u16) -> Vec<&HardwareAsset> {
        self.assets
            .values()
            .filter(|a| a.usb_vid_pid.is_some_and(|u| u.vid == vid && u.pid == pid))
            .collect()
    }

    /// Return all assets with the given status.
    #[must_use]
    pub fn find_by_status(&self, status: AssetStatus) -> Vec<&HardwareAsset> {
        self.assets
            .values()
            .filter(|a| a.status == status)
            .collect()
    }

    /// Iterate over all registered assets.
    pub fn all(&self) -> impl Iterator<Item = &HardwareAsset> {
        self.assets.values()
    }

    /// Return the number of registered assets.
    #[must_use]
    pub fn count(&self) -> usize {
        self.assets.len()
    }

    /// Update the status of a registered asset.
    ///
    /// # Errors
    ///
    /// Returns [`RegistryError::NotFound`] if the device ID is not in the registry.
    pub fn set_status(
        &mut self,
        device_id: &DeviceId,
        status: AssetStatus,
    ) -> Result<(), RegistryError> {
        match self.assets.get_mut(device_id) {
            Some(asset) => {
                asset.status = status;
                Ok(())
            }
            None => NotFoundSnafu {
                device_id: *device_id,
            }
            .fail(),
        }
    }

    /// Probe for USB devices currently connected to the host.
    ///
    /// Returns an empty list. Real detection requires udev/nusb integration.
    // NOTE: USB enumeration stub — udev/nusb hotplug integration lands in Phase 1.
    #[must_use]
    pub const fn detect_usb_stub() -> Vec<UsbId> {
        Vec::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::missing_docs_in_private_items
)]
mod tests {
    use super::*;

    fn radio_asset(device_id: DeviceId, kind: RadioKind) -> HardwareAsset {
        HardwareAsset {
            device_id,
            kind: HardwareKind::Radio(kind),
            name: CompactString::new("Test Radio"),
            serial_number: None,
            usb_vid_pid: Some(UsbId {
                vid: 0x1A86,
                pid: 0x7523,
            }),
            connection: ConnectionType::UsbSerial {
                vid: 0x1A86,
                pid: 0x7523,
                baud: 9_600,
            },
            status: AssetStatus::Offline,
        }
    }

    #[test]
    fn hardware_asset_constructs_with_all_fields() {
        let id = DeviceId::new();
        let asset = HardwareAsset {
            device_id: id,
            kind: HardwareKind::Radio(RadioKind::BaofengUv5r),
            name: CompactString::new("UV-5R Unit 1"),
            serial_number: Some(CompactString::new("SN001")),
            usb_vid_pid: Some(UsbId {
                vid: 0x067B,
                pid: 0x2303,
            }),
            connection: ConnectionType::UsbSerial {
                vid: 0x067B,
                pid: 0x2303,
                baud: 9_600,
            },
            status: AssetStatus::Offline,
        };
        assert_eq!(asset.device_id, id);
        assert_eq!(asset.name, "UV-5R Unit 1");
        assert_eq!(asset.serial_number.as_deref(), Some("SN001"));
        assert_eq!(asset.status, AssetStatus::Offline);
    }

    #[test]
    fn registry_register_succeeds_for_new_device() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        registry
            .register(radio_asset(id, RadioKind::BaofengUv5r))
            .expect("register");
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn registry_register_fails_for_duplicate_device_id() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        registry
            .register(radio_asset(id, RadioKind::BaofengUv5r))
            .expect("first register");
        let err = registry.register(radio_asset(id, RadioKind::BaofengBfF8hp));
        assert!(matches!(err, Err(RegistryError::AlreadyRegistered { .. })));
    }

    #[test]
    fn registry_unregister_removes_and_returns_asset() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        registry
            .register(radio_asset(id, RadioKind::BaofengUv5r))
            .expect("register");
        let removed = registry.unregister(&id);
        assert!(removed.is_some());
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn registry_unregister_returns_none_for_missing_device() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        assert!(registry.unregister(&id).is_none());
    }

    #[test]
    fn registry_get_retrieves_registered_asset() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        registry
            .register(radio_asset(id, RadioKind::BaofengUv5r))
            .expect("register");
        let retrieved = registry.get(&id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().device_id, id);
    }

    #[test]
    fn find_by_kind_filters_by_hardware_kind_discriminant() {
        let mut registry = AssetRegistry::new();
        let radio_id = DeviceId::new();
        let sdr_id = DeviceId::new();
        let node_id = DeviceId::new();

        registry
            .register(radio_asset(radio_id, RadioKind::BaofengUv5r))
            .expect("radio");
        registry
            .register(HardwareAsset {
                device_id: sdr_id,
                kind: HardwareKind::Sdr(SdrKind::RtlSdrV4),
                name: CompactString::new("RTL-SDR"),
                serial_number: None,
                usb_vid_pid: Some(UsbId {
                    vid: 0x0BDA,
                    pid: 0x2838,
                }),
                connection: ConnectionType::UsbDirect {
                    vid: 0x0BDA,
                    pid: 0x2838,
                },
                status: AssetStatus::Available,
            })
            .expect("sdr");
        registry
            .register(HardwareAsset {
                device_id: node_id,
                kind: HardwareKind::MeshNode(MeshNodeKind::TEcho),
                name: CompactString::new("T-Echo 1"),
                serial_number: None,
                usb_vid_pid: None,
                connection: ConnectionType::Ble {
                    mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
                },
                status: AssetStatus::Offline,
            })
            .expect("node");

        // Filter by Radio discriminant — should return only the radio, not SDR or MeshNode.
        let radios = registry.find_by_kind(&HardwareKind::Radio(RadioKind::BaofengUv5r));
        assert_eq!(radios.len(), 1);
        assert_eq!(radios.first().unwrap().device_id, radio_id);

        let sdrs = registry.find_by_kind(&HardwareKind::Sdr(SdrKind::RtlSdrV4));
        assert_eq!(sdrs.len(), 1);
        assert_eq!(sdrs.first().unwrap().device_id, sdr_id);
    }

    #[test]
    fn find_by_usb_matches_vid_pid() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        registry
            .register(HardwareAsset {
                device_id: id,
                kind: HardwareKind::Sdr(SdrKind::RtlSdrV4),
                name: CompactString::new("RTL-SDR"),
                serial_number: None,
                usb_vid_pid: Some(UsbId {
                    vid: 0x0BDA,
                    pid: 0x2838,
                }),
                connection: ConnectionType::UsbDirect {
                    vid: 0x0BDA,
                    pid: 0x2838,
                },
                status: AssetStatus::Available,
            })
            .expect("register");

        let found = registry.find_by_usb(0x0BDA, 0x2838);
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().unwrap().device_id, id);

        assert!(registry.find_by_usb(0xFFFF, 0xFFFF).is_empty());
    }

    #[test]
    fn find_by_status_filters_by_asset_status() {
        let mut registry = AssetRegistry::new();
        let online_id = DeviceId::new();
        let offline_id = DeviceId::new();

        registry
            .register(HardwareAsset {
                device_id: online_id,
                kind: HardwareKind::Radio(RadioKind::BaofengUv5r),
                name: CompactString::new("Radio Online"),
                serial_number: None,
                usb_vid_pid: None,
                connection: ConnectionType::UsbSerial {
                    vid: 0x1A86,
                    pid: 0x7523,
                    baud: 9_600,
                },
                status: AssetStatus::Available,
            })
            .expect("online");
        registry
            .register(HardwareAsset {
                device_id: offline_id,
                kind: HardwareKind::Radio(RadioKind::BaofengBfF8hp),
                name: CompactString::new("Radio Offline"),
                serial_number: None,
                usb_vid_pid: None,
                connection: ConnectionType::UsbSerial {
                    vid: 0x1A86,
                    pid: 0x7523,
                    baud: 9_600,
                },
                status: AssetStatus::Offline,
            })
            .expect("offline");

        let available = registry.find_by_status(AssetStatus::Available);
        assert_eq!(available.len(), 1);
        assert_eq!(available.first().unwrap().device_id, online_id);

        assert!(registry.find_by_status(AssetStatus::Error).is_empty());
    }

    #[test]
    fn set_status_updates_existing_asset() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        registry
            .register(radio_asset(id, RadioKind::BaofengUv5r))
            .expect("register");
        registry
            .set_status(&id, AssetStatus::Available)
            .expect("set status");
        assert_eq!(registry.get(&id).unwrap().status, AssetStatus::Available);
    }

    #[test]
    fn set_status_fails_for_unknown_device() {
        let mut registry = AssetRegistry::new();
        let id = DeviceId::new();
        let err = registry.set_status(&id, AssetStatus::Available);
        assert!(matches!(err, Err(RegistryError::NotFound { .. })));
    }

    #[test]
    fn count_tracks_additions_and_removals() {
        let mut registry = AssetRegistry::new();
        assert_eq!(registry.count(), 0);

        let id1 = DeviceId::new();
        let id2 = DeviceId::new();
        registry
            .register(radio_asset(id1, RadioKind::BaofengUv5r))
            .expect("register 1");
        assert_eq!(registry.count(), 1);
        registry
            .register(radio_asset(id2, RadioKind::BaofengBfF8hp))
            .expect("register 2");
        assert_eq!(registry.count(), 2);
        registry.unregister(&id1);
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn lookup_usb_device_finds_pl2303() {
        let device = lookup_usb_device(0x067B, 0x2303);
        assert!(device.is_some());
        assert_eq!(device.unwrap().chip, "PL2303");
    }

    #[test]
    fn lookup_usb_device_finds_ch340() {
        let device = lookup_usb_device(0x1A86, 0x7523);
        assert!(device.is_some());
        assert_eq!(device.unwrap().chip, "CH340");
    }

    #[test]
    fn lookup_usb_device_returns_none_for_unknown_vid_pid() {
        assert!(lookup_usb_device(0xFFFF, 0xFFFF).is_none());
    }

    #[test]
    fn pl2303_is_flagged_as_clone_risk() {
        let device = lookup_usb_device(0x067B, 0x2303).expect("PL2303 must be in table");
        assert!(device.is_clone_risk);
    }

    #[test]
    fn serde_roundtrip_hardware_asset() {
        let id = DeviceId::new();
        let asset = HardwareAsset {
            device_id: id,
            kind: HardwareKind::Radio(RadioKind::YaesuFtm510dr),
            name: CompactString::new("FTM-510DR"),
            serial_number: Some(CompactString::new("SN-YAESU-001")),
            usb_vid_pid: Some(UsbId {
                vid: 0x10C4,
                pid: 0xEA60,
            }),
            connection: ConnectionType::UsbSerial {
                vid: 0x10C4,
                pid: 0xEA60,
                baud: 38_400,
            },
            status: AssetStatus::Offline,
        };
        let json = serde_json::to_string(&asset).expect("serialize");
        let back: HardwareAsset = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(asset, back);
    }

    #[test]
    fn serde_roundtrip_connection_type_variants() {
        let variants = [
            ConnectionType::UsbSerial {
                vid: 0x1A86,
                pid: 0x7523,
                baud: 9_600,
            },
            ConnectionType::Tcp {
                addr: "127.0.0.1:8080".parse().expect("valid addr"),
            },
            ConnectionType::Ble {
                mac: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
            },
            ConnectionType::I2c { bus: 1, addr: 0x42 },
            ConnectionType::Spi { bus: 0, cs: 1 },
            ConnectionType::UsbDirect {
                vid: 0x0BDA,
                pid: 0x2838,
            },
        ];
        for variant in &variants {
            let json = serde_json::to_string(variant).expect("serialize");
            let back: ConnectionType = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*variant, back);
        }
    }

    #[test]
    fn display_radio_kind_shows_human_readable_names() {
        assert_eq!(RadioKind::BaofengUv5r.to_string(), "Baofeng UV-5R");
        assert_eq!(RadioKind::BaofengBfF8hp.to_string(), "Baofeng BF-F8HP");
        assert_eq!(
            RadioKind::BaofengUv5rmPlus.to_string(),
            "Baofeng UV-5RM Plus"
        );
        assert_eq!(RadioKind::YaesuFtm510dr.to_string(), "Yaesu FTM-510DR");
    }

    #[test]
    fn detect_usb_stub_returns_empty_vec() {
        assert!(AssetRegistry::detect_usb_stub().is_empty());
    }

    #[test]
    fn all_six_known_usb_devices_resolve_via_lookup() {
        // PL2303, CH340, CP2102, FTDI, RTL-SDR, ESP32-S3
        let entries = [
            (0x067B_u16, 0x2303_u16, "PL2303"),
            (0x1A86, 0x7523, "CH340"),
            (0x10C4, 0xEA60, "CP2102"),
            (0x0403, 0x6001, "FT232R"),
            (0x0BDA, 0x2838, "RTL2832U"),
            (0x303A, 0x1001, "ESP32-S3"),
        ];
        for (vid, pid, chip) in &entries {
            let dev = lookup_usb_device(*vid, *pid).expect("device must be in table");
            assert_eq!(dev.chip, *chip);
        }
    }
}
