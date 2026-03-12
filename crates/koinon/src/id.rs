//! Newtype IDs wrapping [`ulid::Ulid`] for domain entity identification.

use std::fmt;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

macro_rules! define_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize,
        )]
        pub struct $name(Ulid);

        impl $name {
            /// Generate a new ID backed by a fresh ULID.
            #[must_use]
            pub fn new() -> Self {
                Self(Ulid::new())
            }

            /// Reconstruct an ID from an existing [`Ulid`].
            #[must_use]
            pub const fn from_ulid(ulid: Ulid) -> Self {
                Self(ulid)
            }

            /// Return the underlying [`Ulid`].
            #[must_use]
            pub const fn as_ulid(&self) -> Ulid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(SignalId, "Unique identifier for a captured signal.");
define_id!(EntityId, "Unique identifier for a tracked entity.");
define_id!(DeviceId, "Unique identifier for a hardware device.");
define_id!(FrequencyId, "Unique identifier for a frequency plan entry.");

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn signal_id_new_is_unique() {
        let a = SignalId::new();
        let b = SignalId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn entity_id_from_ulid_round_trips() {
        let ulid = Ulid::new();
        let id = EntityId::from_ulid(ulid);
        assert_eq!(id.as_ulid(), ulid);
    }

    #[test]
    fn device_id_display_is_nonempty() {
        let id = DeviceId::new();
        assert!(!id.to_string().is_empty());
    }

    #[test]
    fn frequency_id_default_is_new() {
        let id = FrequencyId::default();
        assert!(!id.to_string().is_empty());
    }

    #[test]
    fn signal_id_serde_round_trip() {
        let id = SignalId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        let back: SignalId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, back);
    }
}
