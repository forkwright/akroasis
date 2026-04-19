//! Tracked entity types: anything the system observes across time and domains.

use std::{collections::BTreeMap, fmt};

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

use crate::{EntityId, Timestamp};

/// The category of a tracked entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EntityKind {
    /// A hardware device (radio, sensor, computer, phone, etc.).
    Device,
    /// A human individual.
    Person,
    /// A ground, air, or maritime vehicle.
    Vehicle,
    /// An IP network or logical network segment.
    Network,
    /// A geographic place or area of interest.
    Location,
    /// An organisation, group, or institution.
    Organization,
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Device => "Device",
            Self::Person => "Person",
            Self::Vehicle => "Vehicle",
            Self::Network => "Network",
            Self::Location => "Location",
            Self::Organization => "Organization",
        };
        f.write_str(name)
    }
}

/// An entity that accumulates attributes from multiple signal sources over time.
///
/// Entities represent anything the system tracks: devices, people, vehicles,
/// networks, locations, or organisations. Collectors identify entities from
/// raw signals and update them via [`Entity::update_seen`] and
/// [`Entity::set_attribute`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier for this entity.
    pub entity_id: EntityId,
    /// Category of the tracked entity.
    pub kind: EntityKind,
    /// Optional human-readable name or label.
    pub name: Option<CompactString>,
    /// Timestamp when this entity was first observed.
    pub first_seen: Timestamp,
    /// Timestamp when this entity was most recently observed.
    pub last_seen: Timestamp,
    /// Arbitrary attributes accumulated from signal sources.
    pub attributes: BTreeMap<CompactString, serde_json::Value>,
}

impl Entity {
    /// Construct a new [`Entity`] with a freshly generated [`EntityId`].
    ///
    /// Both `first_seen` and `last_seen` are set to the current time.
    #[must_use]
    pub fn new(kind: EntityKind) -> Self {
        let now = Timestamp::now();
        Self {
            entity_id: EntityId::new(),
            kind,
            name: None,
            first_seen: now,
            last_seen: now,
            attributes: BTreeMap::new(),
        }
    }

    /// Advance `last_seen` to the current time.
    pub fn update_seen(&mut self) {
        self.last_seen = Timestamp::now();
    }

    /// Insert or replace an attribute by key.
    pub fn set_attribute(&mut self, key: impl Into<CompactString>, value: serde_json::Value) {
        self.attributes.insert(key.into(), value);
    }
}

impl fmt::Display for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.kind, self.entity_id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    #[test]
    fn entity_new_generates_unique_ids() {
        let a = Entity::new(EntityKind::Device);
        let b = Entity::new(EntityKind::Device);
        assert_ne!(a.entity_id, b.entity_id);
    }

    #[test]
    fn entity_new_first_seen_equals_last_seen() {
        let e = Entity::new(EntityKind::Person);
        assert_eq!(e.first_seen, e.last_seen);
    }

    #[test]
    fn entity_update_seen_advances_last_seen() {
        let mut e = Entity::new(EntityKind::Vehicle);
        let original = e.last_seen;
        std::thread::sleep(std::time::Duration::from_millis(5));
        e.update_seen();
        assert!(e.last_seen > original);
    }

    #[test]
    fn entity_set_attribute_stores_and_retrieves() {
        let mut e = Entity::new(EntityKind::Network);
        e.set_attribute("ip", serde_json::json!("192.0.2.1"));
        assert_eq!(
            e.attributes.get("ip"),
            Some(&serde_json::json!("192.0.2.1"))
        );
    }

    #[test]
    fn entity_set_attribute_overwrites_existing() {
        let mut e = Entity::new(EntityKind::Device);
        e.set_attribute("mac", serde_json::json!("aa:bb:cc:dd:ee:ff"));
        e.set_attribute("mac", serde_json::json!("11:22:33:44:55:66"));
        assert_eq!(
            e.attributes.get("mac"),
            Some(&serde_json::json!("11:22:33:44:55:66"))
        );
    }

    #[test]
    fn entity_serde_roundtrip_with_attributes() {
        let mut e = Entity::new(EntityKind::Organization);
        e.set_attribute("url", serde_json::json!("https://corp.test"));
        e.set_attribute("active", serde_json::json!(true));
        let json = serde_json::to_string(&e).expect("serialize");
        let back: Entity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(e, back);
    }

    #[test]
    fn entity_display_starts_with_kind() {
        let e = Entity::new(EntityKind::Device);
        let s = e.to_string();
        assert!(s.starts_with("Device "), "got: {s}");
    }

    // --- Behavioral tests ---

    /// Adding multiple distinct attributes stores all of them.
    #[test]
    fn entity_accumulates_attributes() {
        let mut e = Entity::new(EntityKind::Device);
        e.set_attribute("mac", serde_json::json!("aa:bb:cc:dd:ee:ff"));
        e.set_attribute("vendor", serde_json::json!("Acme"));
        e.set_attribute("firmware", serde_json::json!("1.2.3"));
        assert_eq!(e.attributes.len(), 3, "expected 3 distinct attributes");
        assert_eq!(
            e.attributes.get("mac"),
            Some(&serde_json::json!("aa:bb:cc:dd:ee:ff"))
        );
        assert_eq!(e.attributes.get("vendor"), Some(&serde_json::json!("Acme")));
        assert_eq!(
            e.attributes.get("firmware"),
            Some(&serde_json::json!("1.2.3"))
        );
    }

    /// Setting the same attribute key twice keeps only one entry (last value wins).
    #[test]
    fn entity_deduplicates_same_attribute() {
        let mut e = Entity::new(EntityKind::Device);
        e.set_attribute("status", serde_json::json!("online"));
        e.set_attribute("status", serde_json::json!("offline"));
        assert_eq!(
            e.attributes.len(),
            1,
            "duplicate key must not create a second entry"
        );
        assert_eq!(
            e.attributes.get("status"),
            Some(&serde_json::json!("offline")),
            "last write must win"
        );
    }

    #[test]
    fn entity_kind_display_device() {
        assert_eq!(EntityKind::Device.to_string(), "Device");
    }

    #[test]
    fn entity_kind_display_person() {
        assert_eq!(EntityKind::Person.to_string(), "Person");
    }

    #[test]
    fn entity_kind_display_vehicle() {
        assert_eq!(EntityKind::Vehicle.to_string(), "Vehicle");
    }

    #[test]
    fn entity_kind_display_network() {
        assert_eq!(EntityKind::Network.to_string(), "Network");
    }

    #[test]
    fn entity_kind_display_location() {
        assert_eq!(EntityKind::Location.to_string(), "Location");
    }

    #[test]
    fn entity_kind_display_organization() {
        assert_eq!(EntityKind::Organization.to_string(), "Organization");
    }
}
