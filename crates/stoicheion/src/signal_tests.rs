//! Tests for [`super`]; split out to keep the parent file under the
//! RUST/file-too-long 800-line threshold.

use std::net::{IpAddr, Ipv4Addr};

use super::*;

fn sample_timestamp() -> Timestamp {
    Timestamp::now()
}

fn sample_coords() -> Coordinates {
    Coordinates::new(51.5074, -0.1278, None).unwrap()
}

// --- SignalKind construction ---

#[test]
fn rf_transmission_constructs_and_matches() {
    let kind = SignalKind::Rf(RfDetail::Transmission {
        frequency: Frequency::mhz(146),
        power: Power::dbm(-30.0),
        modulation: "FM".into(),
        bandwidth: Frequency::khz(25),
    });
    assert!(matches!(
        kind,
        SignalKind::Rf(RfDetail::Transmission { .. })
    ));
}

#[test]
fn rf_jamming_constructs_and_matches() {
    let kind = SignalKind::Rf(RfDetail::Jamming {
        affected_band: "2.4 GHz".into(),
        estimated_power: Power::dbm(20.0),
    });
    assert!(matches!(kind, SignalKind::Rf(RfDetail::Jamming { .. })));
}

#[test]
fn rf_beacon_constructs_and_matches() {
    let kind = SignalKind::Rf(RfDetail::Beacon {
        frequency: Frequency::mhz(433),
        interval_ms: 1_000,
    });
    assert!(matches!(kind, SignalKind::Rf(RfDetail::Beacon { .. })));
}

#[test]
fn mesh_node_seen_constructs_and_matches() {
    let kind = SignalKind::Mesh(MeshDetail::NodeSeen {
        node_id: 42,
        snr: 7.5,
        hop_count: 2,
    });
    assert!(matches!(
        kind,
        SignalKind::Mesh(MeshDetail::NodeSeen { .. })
    ));
}

#[test]
fn mesh_message_constructs_and_matches() {
    let kind = SignalKind::Mesh(MeshDetail::Message {
        from_node: 1,
        to_node: Some(2),
        channel: 0,
    });
    assert!(matches!(kind, SignalKind::Mesh(MeshDetail::Message { .. })));
}

#[test]
fn mesh_position_constructs_and_matches() {
    let kind = SignalKind::Mesh(MeshDetail::Position {
        node_id: 10,
        coordinates: sample_coords(),
    });
    assert!(matches!(
        kind,
        SignalKind::Mesh(MeshDetail::Position { .. })
    ));
}

#[test]
fn network_flow_constructs_and_matches() {
    let kind = SignalKind::Network(NetworkDetail::Flow {
        src_ip: IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
        dst_ip: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)),
        src_port: 12_345,
        dst_port: 443,
        protocol: 6,
    });
    assert!(matches!(
        kind,
        SignalKind::Network(NetworkDetail::Flow { .. })
    ));
}

#[test]
fn network_dns_query_constructs_and_matches() {
    let kind = SignalKind::Network(NetworkDetail::DnsQuery {
        domain: "test.local".into(),
    });
    assert!(matches!(
        kind,
        SignalKind::Network(NetworkDetail::DnsQuery { .. })
    ));
}

#[test]
fn network_alert_constructs_and_matches() {
    let kind = SignalKind::Network(NetworkDetail::Alert {
        rule_id: 9_001,
        severity: AlertSeverity::High,
    });
    assert!(matches!(
        kind,
        SignalKind::Network(NetworkDetail::Alert { .. })
    ));
}

#[test]
fn proximity_wifi_constructs_and_matches() {
    let kind = SignalKind::Proximity(ProximityDetail::Wifi {
        ssid: Some("corp-net".into()),
        bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
        rssi: -65,
        channel: 6,
    });
    assert!(matches!(
        kind,
        SignalKind::Proximity(ProximityDetail::Wifi { .. })
    ));
}

#[test]
fn proximity_ble_constructs_and_matches() {
    let kind = SignalKind::Proximity(ProximityDetail::Ble {
        mac: [0x01, 0x02, 0x03, 0x04, 0x05, 0x06],
        rssi: -80,
        name: None,
    });
    assert!(matches!(
        kind,
        SignalKind::Proximity(ProximityDetail::Ble { .. })
    ));
}

#[test]
fn proximity_tracker_constructs_and_matches() {
    let kind = SignalKind::Proximity(ProximityDetail::Tracker {
        kind: TrackerKind::AirTag,
        mac: [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01],
    });
    assert!(matches!(
        kind,
        SignalKind::Proximity(ProximityDetail::Tracker { .. })
    ));
}

#[test]
fn gps_fix_constructs_and_matches() {
    let kind = SignalKind::Gps(GpsDetail::Fix {
        satellites: 8,
        hdop: 1.2,
        speed_mps: Some(5.0),
    });
    assert!(matches!(kind, SignalKind::Gps(GpsDetail::Fix { .. })));
}

#[test]
fn gps_spoofing_suspected_constructs_and_matches() {
    let kind = SignalKind::Gps(GpsDetail::SpoofingSuspected {
        reason: "clock-jump".into(),
    });
    assert!(matches!(
        kind,
        SignalKind::Gps(GpsDetail::SpoofingSuspected { .. })
    ));
}

#[test]
fn environmental_temperature_constructs_and_matches() {
    let kind = SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 22.5 });
    assert!(matches!(
        kind,
        SignalKind::Environmental(EnvironmentalDetail::Temperature { .. })
    ));
}

#[test]
fn environmental_humidity_constructs_and_matches() {
    let kind = SignalKind::Environmental(EnvironmentalDetail::Humidity { percent: 60.0 });
    assert!(matches!(
        kind,
        SignalKind::Environmental(EnvironmentalDetail::Humidity { .. })
    ));
}

#[test]
fn environmental_barometric_constructs_and_matches() {
    let kind = SignalKind::Environmental(EnvironmentalDetail::Barometric { hpa: 1_013.25 });
    assert!(matches!(
        kind,
        SignalKind::Environmental(EnvironmentalDetail::Barometric { .. })
    ));
}

#[test]
fn osint_feed_item_constructs_and_matches() {
    let kind = SignalKind::Osint(OsintDetail::FeedItem {
        source: "threatfeed.test".into(),
        title: "APT-42 IOC UPDATE".into(),
    });
    assert!(matches!(
        kind,
        SignalKind::Osint(OsintDetail::FeedItem { .. })
    ));
}

#[test]
fn osint_threat_indicator_constructs_and_matches() {
    let kind = SignalKind::Osint(OsintDetail::ThreatIndicator {
        indicator_type: "ip".into(),
        value: "198.51.100.42".into(),
    });
    assert!(matches!(
        kind,
        SignalKind::Osint(OsintDetail::ThreatIndicator { .. })
    ));
}

// --- GeoSignal ---

#[test]
fn geo_signal_new_generates_unique_ids() {
    let kind = SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 20.0 });
    let a = GeoSignal::new(kind.clone(), sample_timestamp(), None);
    let b = GeoSignal::new(kind, sample_timestamp(), None);
    assert_ne!(a.signal_id, b.signal_id);
}

#[test]
fn builder_chain_sets_fields() {
    let device = DeviceId::new();
    let kind = SignalKind::Environmental(EnvironmentalDetail::Humidity { percent: 55.0 });
    let signal = GeoSignal::new(kind, sample_timestamp(), None)
        .with_device(device)
        .with_confidence(Confidence::new(0.9))
        .with_metadata("sensor", serde_json::json!("top-floor"));
    assert_eq!(signal.source_device, Some(device));
    assert!((signal.confidence.as_f32() - 0.9).abs() < f32::EPSILON);
    assert!(signal.metadata.contains_key("sensor"));
}

// --- Confidence ---

#[test]
fn confidence_clamps_below_zero() {
    assert!((Confidence::new(-5.0).as_f32() - 0.0).abs() < f32::EPSILON);
}

#[test]
fn confidence_clamps_above_one() {
    assert!((Confidence::new(2.0).as_f32() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn confidence_display_shows_percentage() {
    assert_eq!(Confidence::new(0.85).to_string(), "85%");
}

// --- GeoSignal behavioral tests ---

/// A signal constructed with coordinates must report Some location.
#[test]
fn geosignal_with_coordinates_has_location() {
    let coords = Coordinates::new(51.5074, -0.1278, None).unwrap();
    let signal = GeoSignal::new(
        SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 20.0 }),
        sample_timestamp(),
        Some(coords),
    );
    assert!(
        signal.location.is_some(),
        "signal constructed with coordinates must have Some(location)"
    );
    let loc = signal.location.unwrap();
    assert!((loc.latitude - 51.5074).abs() < 1e-9);
    assert!((loc.longitude - (-0.1278)).abs() < 1e-9);
}

/// Metadata keys from multiple `with_metadata` calls all appear in the map.
#[test]
fn geosignal_metadata_merge() {
    let signal = GeoSignal::new(
        SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 22.0 }),
        sample_timestamp(),
        None,
    )
    .with_metadata("sensor", serde_json::json!("roof"))
    .with_metadata("building", serde_json::json!("HQ"))
    .with_metadata("floor", serde_json::json!(3));

    assert_eq!(signal.metadata.len(), 3);
    assert_eq!(
        signal.metadata.get("sensor"),
        Some(&serde_json::json!("roof"))
    );
    assert_eq!(
        signal.metadata.get("building"),
        Some(&serde_json::json!("HQ"))
    );
    assert_eq!(signal.metadata.get("floor"), Some(&serde_json::json!(3)));
}

/// Signals with distinct unix-millisecond timestamps sort in chronological order.
#[test]
fn geosignal_timestamp_ordering() {
    let kind = || SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 20.0 });
    let t1 = Timestamp::from_unix_millis(1_700_000_000_000).unwrap();
    let t2 = Timestamp::from_unix_millis(1_700_000_001_000).unwrap();
    let t3 = Timestamp::from_unix_millis(1_700_000_002_000).unwrap();

    let s1 = GeoSignal::new(kind(), t1, None);
    let s2 = GeoSignal::new(kind(), t2, None);
    let s3 = GeoSignal::new(kind(), t3, None);

    let mut signals = [s3, s1, s2];
    signals.sort_by_key(|s| s.timestamp);

    assert_eq!(signals.first().map(|s| s.timestamp), Some(t1));
    assert_eq!(signals.get(1).map(|s| s.timestamp), Some(t2));
    assert_eq!(signals.get(2).map(|s| s.timestamp), Some(t3));
}

// --- GeoSignal serde roundtrips ---

#[test]
fn geo_signal_serde_roundtrip_with_all_fields() {
    let signal = GeoSignal::new(
        SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 21.0 }),
        sample_timestamp(),
        Some(sample_coords()),
    )
    .with_device(DeviceId::new())
    .with_confidence(Confidence::new(0.75))
    .with_metadata("floor", serde_json::json!(3));
    let json = serde_json::to_string(&signal).unwrap();
    let back: GeoSignal = serde_json::from_str(&json).unwrap();
    assert_eq!(signal, back);
}

#[test]
fn geo_signal_serde_roundtrip_no_optional_fields() {
    let signal = GeoSignal::new(
        SignalKind::Gps(GpsDetail::Fix {
            satellites: 6,
            hdop: 2.0,
            speed_mps: None,
        }),
        sample_timestamp(),
        None,
    );
    let json = serde_json::to_string(&signal).unwrap();
    let back: GeoSignal = serde_json::from_str(&json).unwrap();
    assert_eq!(signal, back);
}

// --- SignalKind serde roundtrips (one per variant) ---

#[test]
fn signal_kind_rf_serde_roundtrip() {
    let kind = SignalKind::Rf(RfDetail::Transmission {
        frequency: Frequency::mhz(146),
        power: Power::dbm(-30.0),
        modulation: "FM".into(),
        bandwidth: Frequency::khz(25),
    });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn signal_kind_mesh_serde_roundtrip() {
    let kind = SignalKind::Mesh(MeshDetail::Message {
        from_node: 1,
        to_node: None,
        channel: 0,
    });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn signal_kind_network_serde_roundtrip() {
    let kind = SignalKind::Network(NetworkDetail::Alert {
        rule_id: 100,
        severity: AlertSeverity::Critical,
    });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn signal_kind_proximity_serde_roundtrip() {
    let kind = SignalKind::Proximity(ProximityDetail::Tracker {
        kind: TrackerKind::Tile,
        mac: [0x11, 0x22, 0x33, 0x44, 0x55, 0x66],
    });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn signal_kind_gps_serde_roundtrip() {
    let kind = SignalKind::Gps(GpsDetail::SpoofingSuspected {
        reason: "clock-jump".into(),
    });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn signal_kind_environmental_serde_roundtrip() {
    let kind = SignalKind::Environmental(EnvironmentalDetail::Barometric { hpa: 1_013.25 });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn signal_kind_osint_serde_roundtrip() {
    let kind = SignalKind::Osint(OsintDetail::ThreatIndicator {
        indicator_type: "domain".into(),
        value: "evil.test.invalid".into(),
    });
    let json = serde_json::to_string(&kind).unwrap();
    let back: SignalKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, back);
}

#[test]
fn confidence_maps_nan_to_zero_rather_than_storing_it() {
    // WHY: NaN compares false against both bounds, so the ordered if/else chain
    // fell through to the `else` arm and stored NaN unclamped. Every downstream
    // comparison against that value then silently answered false.
    let c = Confidence::new(f32::NAN);

    assert!(!c.as_f32().is_nan(), "NaN survived construction");
    assert!((c.as_f32() - 0.0).abs() < f32::EPSILON);
}

#[test]
fn confidence_clamps_values_outside_the_unit_interval() {
    assert!((Confidence::new(-3.0).as_f32() - 0.0).abs() < f32::EPSILON);
    assert!((Confidence::new(7.5).as_f32() - 1.0).abs() < f32::EPSILON);
    assert!((Confidence::new(0.25).as_f32() - 0.25).abs() < f32::EPSILON);
}

#[test]
fn deserializing_a_confidence_cannot_bypass_the_clamp() {
    // WHY: the derived Deserialize rebuilt the private tuple field directly, so
    // a stored 5.0 or NaN entered the type without passing new().
    let out_of_range: Confidence = serde_json::from_str("5.0").unwrap();
    assert!((out_of_range.as_f32() - 1.0).abs() < f32::EPSILON);

    let negative: Confidence = serde_json::from_str("-2.0").unwrap();
    assert!((negative.as_f32() - 0.0).abs() < f32::EPSILON);
    // NOTE: JSON has no NaN literal, so the NaN path is covered by
    // confidence_maps_nan_to_zero_rather_than_storing_it against new(), which
    // #[serde(from = "f32")] now routes every deserialized value through.
}
