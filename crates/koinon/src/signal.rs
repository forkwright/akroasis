//! Signal kinds and the [`GeoSignal`] envelope that every collector produces.

use std::{collections::BTreeMap, fmt, net::IpAddr};

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

use crate::{Coordinates, DeviceId, Frequency, Power, SignalId, Timestamp};

// ---------------------------------------------------------------------------
// Supporting enums
// ---------------------------------------------------------------------------

/// Severity of a network security alert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AlertSeverity {
    /// Informational; no immediate action required.
    Low,
    /// Notable anomaly that warrants investigation.
    Medium,
    /// Significant threat requiring prompt response.
    High,
    /// Immediate action required; system or mission impact likely.
    Critical,
}

/// Manufacturer or ecosystem of a passive Bluetooth proximity tracker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TrackerKind {
    /// Apple `AirTag`.
    AirTag,
    /// Samsung `SmartTag`.
    SmartTag,
    /// Tile tracker.
    Tile,
    /// Unrecognised tracker vendor.
    Unknown,
}

// ---------------------------------------------------------------------------
// Domain detail enums
// ---------------------------------------------------------------------------

/// Detail payload for RF (radio frequency) domain signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RfDetail {
    /// An observed radio transmission.
    Transmission {
        /// Centre frequency of the transmission.
        frequency: Frequency,
        /// Measured signal power.
        power: Power,
        /// Modulation scheme (e.g. `"FM"`, `"AM"`, `"BPSK"`).
        modulation: CompactString,
        /// Channel bandwidth.
        bandwidth: Frequency,
    },
    /// Detected RF jamming activity.
    Jamming {
        /// Band or frequency range affected (e.g. `"2.4 GHz"`).
        affected_band: CompactString,
        /// Estimated jammer output power.
        estimated_power: Power,
    },
    /// Periodic RF beacon signal.
    Beacon {
        /// Beacon frequency.
        frequency: Frequency,
        /// Interval between beacon transmissions in milliseconds.
        interval_ms: u32,
    },
}

/// Detail payload for Meshtastic / `LoRa` mesh network signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum MeshDetail {
    /// A mesh node was observed.
    NodeSeen {
        /// Meshtastic node identifier.
        node_id: u32,
        /// Signal-to-noise ratio in dB.
        snr: f32,
        /// Number of hops from the observing node.
        hop_count: u8,
    },
    /// A mesh message was intercepted or relayed.
    Message {
        /// Originating node identifier.
        from_node: u32,
        /// Destination node identifier; `None` for broadcast.
        to_node: Option<u32>,
        /// Meshtastic channel index.
        channel: u8,
    },
    /// A position packet from a mesh node.
    Position {
        /// Node that reported the position.
        node_id: u32,
        /// Geographic coordinates reported by the node.
        coordinates: Coordinates,
    },
}

/// Detail payload for network defence signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NetworkDetail {
    /// An IP flow observed on the wire.
    Flow {
        /// Source IP address.
        src_ip: IpAddr,
        /// Destination IP address.
        dst_ip: IpAddr,
        /// Source port number.
        src_port: u16,
        /// Destination port number.
        dst_port: u16,
        /// IP protocol number (e.g. 6 = TCP, 17 = UDP).
        protocol: u8,
    },
    /// A DNS query observed on the wire.
    DnsQuery {
        /// Queried domain name.
        domain: CompactString,
    },
    /// An IDS/IPS alert triggered by a detection rule.
    Alert {
        /// Detection rule identifier.
        rule_id: u32,
        /// Alert severity level.
        severity: AlertSeverity,
    },
}

/// Detail payload for passive proximity scanning signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProximityDetail {
    /// A Wi-Fi access point or station observed during passive scan.
    Wifi {
        /// Network SSID, if present in the beacon.
        ssid: Option<CompactString>,
        /// Basic Service Set Identifier (MAC address of the AP).
        bssid: [u8; 6],
        /// Received Signal Strength Indicator in dBm.
        rssi: i8,
        /// Wi-Fi channel number.
        channel: u8,
    },
    /// A Bluetooth Low Energy advertisement observed during passive scan.
    Ble {
        /// Advertiser MAC address.
        mac: [u8; 6],
        /// Received Signal Strength Indicator in dBm.
        rssi: i8,
        /// Complete local name from the advertisement, if present.
        name: Option<CompactString>,
    },
    /// A detected proximity tracker advertisement.
    Tracker {
        /// Tracker vendor or ecosystem.
        kind: TrackerKind,
        /// Tracker MAC address.
        mac: [u8; 6],
    },
}

/// Detail payload for GPS receiver signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum GpsDetail {
    /// A valid GPS position fix was obtained.
    Fix {
        /// Number of satellites used in the fix.
        satellites: u8,
        /// Horizontal Dilution of Precision.
        hdop: f32,
        /// Ground speed in metres per second, if reported.
        speed_mps: Option<f32>,
    },
    /// GPS spoofing was detected or suspected.
    SpoofingSuspected {
        /// Human-readable reason for the suspicion (e.g. `"clock-jump"`).
        reason: CompactString,
    },
}

/// Detail payload for environmental sensor readings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EnvironmentalDetail {
    /// Ambient temperature reading.
    Temperature {
        /// Temperature in degrees Celsius.
        celsius: f32,
    },
    /// Relative humidity reading.
    Humidity {
        /// Relative humidity as a percentage (0–100).
        percent: f32,
    },
    /// Barometric pressure reading.
    Barometric {
        /// Pressure in hectopascals.
        hpa: f32,
    },
}

/// Detail payload for OSINT feed signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OsintDetail {
    /// An item ingested from an open-source intelligence feed.
    FeedItem {
        /// Name or URL of the originating feed.
        source: CompactString,
        /// Article or indicator title.
        title: CompactString,
    },
    /// A structured threat indicator (IP, domain, hash, etc.).
    ThreatIndicator {
        /// Type of indicator (e.g. `"ip"`, `"domain"`, `"sha256"`).
        indicator_type: CompactString,
        /// The indicator value.
        value: CompactString,
    },
}

// ---------------------------------------------------------------------------
// SignalKind
// ---------------------------------------------------------------------------

/// Top-level discriminant that identifies the domain a signal belongs to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum SignalKind {
    /// Radio frequency domain signal.
    Rf(RfDetail),
    /// Mesh network domain signal.
    Mesh(MeshDetail),
    /// Network defence domain signal.
    Network(NetworkDetail),
    /// Passive proximity scan signal.
    Proximity(ProximityDetail),
    /// GPS receiver signal.
    Gps(GpsDetail),
    /// Environmental sensor signal.
    Environmental(EnvironmentalDetail),
    /// Open-source intelligence signal.
    Osint(OsintDetail),
}

// ---------------------------------------------------------------------------
// Confidence
// ---------------------------------------------------------------------------

/// A confidence score clamped to the closed interval \[0.0, 1.0\].
///
/// Values passed to [`Confidence::new`] outside this range are silently
/// clamped rather than rejected, preventing panics on out-of-range input
/// from imprecise sensors or computation.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    /// Construct a [`Confidence`], clamping `value` to \[0.0, 1.0\].
    ///
    /// Values below 0.0 become 0.0; values above 1.0 become 1.0.
    #[must_use]
    pub const fn new(value: f32) -> Self {
        // WHY: clamp() is a const fn since Rust 1.83; clamping avoids returning
        // an error type for a simple bounds check on a scalar field.
        let clamped = if value < 0.0 {
            0.0_f32
        } else if value > 1.0 {
            1.0_f32
        } else {
            value
        };
        Self(clamped)
    }

    /// Return the underlying score as an `f32`.
    #[must_use]
    pub const fn as_f32(self) -> f32 {
        self.0
    }
}

impl Default for Confidence {
    /// Returns a confidence of 1.0 (full certainty).
    fn default() -> Self {
        Self(1.0)
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0}%", self.0 * 100.0)
    }
}

// ---------------------------------------------------------------------------
// GeoSignal
// ---------------------------------------------------------------------------

/// A geo-located, timestamped signal event produced by any collector.
///
/// Every collector in the Akroasis system produces `GeoSignal` values that
/// flow into the aggregation pipeline. Use [`GeoSignal::new`] and then chain
/// builder methods for optional fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoSignal {
    /// Unique identifier for this signal event.
    pub signal_id: SignalId,
    /// Domain-specific signal payload.
    pub kind: SignalKind,
    /// Wall-clock time at which the signal was observed.
    pub timestamp: Timestamp,
    /// Geographic location at which the signal was observed, if known.
    pub location: Option<Coordinates>,
    /// Hardware device that captured the signal, if known.
    pub source_device: Option<DeviceId>,
    /// Confidence in the validity or accuracy of this signal.
    pub confidence: Confidence,
    /// Arbitrary key-value metadata from the originating collector.
    pub metadata: BTreeMap<CompactString, serde_json::Value>,
}

impl GeoSignal {
    /// Construct a [`GeoSignal`] with a freshly generated [`SignalId`],
    /// default confidence of 1.0, and empty metadata.
    #[must_use]
    pub fn new(kind: SignalKind, timestamp: Timestamp, location: Option<Coordinates>) -> Self {
        Self {
            signal_id: SignalId::new(),
            kind,
            timestamp,
            location,
            source_device: None,
            confidence: Confidence::default(),
            metadata: BTreeMap::new(),
        }
    }

    /// Attach a source device identifier to this signal.
    #[must_use]
    pub const fn with_device(mut self, device: DeviceId) -> Self {
        self.source_device = Some(device);
        self
    }

    /// Override the confidence score.
    #[must_use]
    pub const fn with_confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }

    /// Insert a metadata key-value pair, replacing any existing value for `key`.
    #[must_use]
    pub fn with_metadata(
        mut self,
        key: impl Into<CompactString>,
        value: serde_json::Value,
    ) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_docs_in_private_items
)]
mod tests {
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
}
