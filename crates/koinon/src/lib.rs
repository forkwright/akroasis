//! κοινόν — shared foundational types for the Akroasis workspace.

#![deny(missing_docs)]

pub mod baseline;
pub mod coordinates;
pub mod entity;
pub mod frequency;
pub mod hardware;
pub mod id;
pub mod power;
pub mod signal;
pub mod tamper_log;
pub mod timestamp;

pub use baseline::{
    AnomalyScore, Baseline, ScoringConfig, TemporalBucketedBaseline, TimeWindowedBaseline,
};
pub use coordinates::{Coordinates, CoordinatesError, Datum};
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
    ChainStatus, DEFAULT_MAX_FILE_BYTES, LogEntry, LogEntryKind, MAX_ENTRY_BYTES, TamperLog,
    TamperLogConfig, TamperLogError, VerificationResult, verify_chain,
};
pub use timestamp::{Timestamp, TimestampError};
