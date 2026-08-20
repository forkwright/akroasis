//! Opaque references and epochs used by caller and receipt contracts.

use std::fmt;
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};
use snafu::OptionExt;

use crate::caller_error::{CallerContractError, ZeroEpochSnafu};

macro_rules! define_opaque_ref {
    ($name:ident, $domain:literal, $doc:literal) => {
        #[doc = $doc]
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Derive an opaque reference from a canonical value.
            ///
            /// The original value is not retained. References identify a
            /// policy object; they are not proof that the object was trusted.
            #[must_use]
            pub fn from_canonical(value: &[u8]) -> Self {
                let mut hasher = blake3::Hasher::new();
                hasher.update($domain);
                hasher.update(&[0]);
                hasher.update(value);
                Self(hasher.finalize().into())
            }

            /// Return the opaque reference bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }
        }

        impl AsRef<[u8; 32]> for $name {
            fn as_ref(&self) -> &[u8; 32] {
                self.as_bytes()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<opaque>)"))
            }
        }
    };
}

define_opaque_ref!(
    CallerRef,
    b"akroasis.caller.v1",
    "Opaque caller identity reference."
);
define_opaque_ref!(
    PersonaRef,
    b"akroasis.persona.v1",
    "Opaque caller persona reference."
);
define_opaque_ref!(
    CapabilityRef,
    b"akroasis.capability.v1",
    "Opaque domain-capability reference."
);
define_opaque_ref!(
    ScopeRef,
    b"akroasis.scope.v1",
    "Opaque domain-owned scope reference."
);
define_opaque_ref!(
    EffectRef,
    b"akroasis.effect.v1",
    "Opaque protected-effect reference."
);
define_opaque_ref!(
    EvidenceDigest,
    b"akroasis.evidence.v1",
    "Digest of allowlisted effect evidence."
);
define_opaque_ref!(
    ReceiptDigest,
    b"akroasis.receipt.v1",
    "Digest of a durable minimized receipt."
);

macro_rules! define_epoch {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[repr(transparent)]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize,
        )]
        pub struct $name(NonZeroU64);

        impl $name {
            /// Construct a non-zero epoch.
            ///
            /// # Errors
            ///
            /// Returns [`CallerContractError::ZeroEpoch`] when `value` is zero.
            pub fn try_from_u64(value: u64) -> Result<Self, CallerContractError> {
                NonZeroU64::new(value).map(Self).context(ZeroEpochSnafu {
                    field: stringify!($name),
                })
            }

            /// Return the epoch as an integer.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

define_epoch!(PolicyEpoch, "Version of the policy that granted authority.");
define_epoch!(
    SchemaEpoch,
    "Version of the domain schema governing an effect."
);
