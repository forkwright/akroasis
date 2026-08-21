//! κοινόν — shared foundational types for the Akroasis workspace.

#![deny(missing_docs)]

pub mod baseline;
pub mod caller;
mod caller_error;
mod caller_ref;
pub mod coordinates;
pub mod effect_receipt;
mod effect_receipt_state;
pub mod entity;
pub mod frequency;
pub mod hardware;
pub mod id;
pub mod power;
pub mod signal;
pub mod tamper_log;
pub mod timestamp;

pub use baseline::{AnomalyScore, Baseline, ScoringConfig, TemporalBucketedBaseline};
pub use caller::{
    AuthorityClaims, AuthorityClaimsBuilder, AuthorityDecision, AuthorityGrant,
    AuthorizationDenial, AuthorizationRequirement, AuthorizedCaller, CALLER_CONTEXT_VERSION,
    CALLER_RESOLVER_VERSION, CallerAuthority, CallerContractError, CallerRef, CallerResolver,
    CapabilityRef, EffectRef, EvidenceDigest, LocalPeerCredentials, PersonaRef, PolicyEpoch,
    PrincipalSource, ReceiptDigest, RevocationState, SchemaEpoch, ScopeRef, TrustState,
    ValidatedCaller, authorize_caller,
};
pub use coordinates::{Coordinates, CoordinatesError, Datum};
pub use effect_receipt::{
    EFFECT_RECEIPT_VERSION, EffectDescriptor, EffectOutcome, EffectReceipt, EffectReceiptError,
    ReceiptContext, ReceiptEvent, RecoveryRelation, ValidatedReceiptLink,
};
pub use effect_receipt_state::{
    EffectReceiptSink, OutcomeAppendError, PendingIntent, PendingOutcome, PersistedIntent,
    PersistedOutcome, ReceiptTransitionError, RecoveryAuthorization, RecoveryTicket,
};
pub use entity::{Entity, EntityKind};
pub use frequency::Frequency;
pub use hardware::{
    AssetRegistry, AssetStatus, ConnectionType, HardwareAsset, HardwareKind, KNOWN_USB_DEVICES,
    KnownUsbDevice, MeshNodeKind, RadioKind, RegistryError, SdrKind, UsbId, lookup_usb_device,
};
pub use id::{DeviceId, EntityId, FrequencyId, SignalId};
pub use power::Power;
pub use signal::{Confidence, GeoSignal, SignalKind};
pub use tamper_log::{
    CHAIN_KEY_LEN, ChainKey, ChainStatus, DEFAULT_MAX_FILE_BYTES, KEY_ID_LEN, LogEntry,
    LogEntryKind, MAX_ENTRY_BYTES, SegmentChainStatus, TIP_SIGNATURE_LEN, TamperLog,
    TamperLogConfig, TamperLogError, TipProvenance, TipSigner, TipStatus, TipVerifier,
    VerificationResult, verify_chain, verify_segment_chain, verify_tip_provenance,
};
pub use timestamp::{Timestamp, TimestampError};
