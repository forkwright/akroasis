//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

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
        .unwrap_or_default();
    assert_eq!(registry.count(), 1);
}

#[test]
fn registry_register_fails_for_duplicate_device_id() {
    let mut registry = AssetRegistry::new();
    let id = DeviceId::new();
    registry
        .register(radio_asset(id, RadioKind::BaofengUv5r))
        .unwrap_or_default();
    let err = registry.register(radio_asset(id, RadioKind::BaofengBfF8hp));
    assert!(matches!(err, Err(RegistryError::AlreadyRegistered { .. })));
}

#[test]
fn registry_unregister_removes_and_returns_asset() {
    let mut registry = AssetRegistry::new();
    let id = DeviceId::new();
    registry
        .register(radio_asset(id, RadioKind::BaofengUv5r))
        .unwrap_or_default();
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
        .unwrap_or_default();
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
        .unwrap_or_default();
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
        .unwrap_or_default();
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
        .unwrap_or_default();

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
        .unwrap_or_default();

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
        .unwrap_or_default();
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
        .unwrap_or_default();

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
        .unwrap_or_default();
    registry
        .set_status(&id, AssetStatus::Available)
        .unwrap_or_default();
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
        .unwrap_or_default();
    assert_eq!(registry.count(), 1);
    registry
        .register(radio_asset(id2, RadioKind::BaofengBfF8hp))
        .unwrap_or_default();
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
    let device = lookup_usb_device(0x067B, 0x2303).unwrap();
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
    let json = serde_json::to_string(&asset).unwrap();
    let back: HardwareAsset = serde_json::from_str(&json).unwrap();
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
            addr: "127.0.0.1:8080".parse().unwrap(),
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
        let json = serde_json::to_string(variant).unwrap();
        let back: ConnectionType = serde_json::from_str(&json).unwrap();
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
        let dev = lookup_usb_device(*vid, *pid).unwrap();
        assert_eq!(dev.chip, *chip);
    }
}
