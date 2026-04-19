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
#[expect(
    clippy::unwrap_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
#[path = "signal_tests.rs"]
mod tests;
