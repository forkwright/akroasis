//! κοινόν — shared foundational types for the Akroasis workspace.

pub mod baseline;
pub mod coordinates;
pub mod frequency;
pub mod hardware;
pub mod id;
pub mod power;
pub mod tamper_log;
pub mod timestamp;

pub use baseline::{
    AnomalyScore, Baseline, ScoringConfig, TemporalBucketedBaseline, TimeWindowedBaseline,
};
pub use coordinates::{Coordinates, CoordinatesError, Datum};
pub use frequency::Frequency;
pub use hardware::{
    AssetRegistry, AssetStatus, ConnectionType, HardwareAsset, HardwareKind, KNOWN_USB_DEVICES,
    KnownUsbDevice, MeshNodeKind, RadioKind, RegistryError, SdrKind, UsbId, lookup_usb_device,
};
pub use id::{DeviceId, EntityId, FrequencyId, SignalId};
pub use power::Power;
pub use tamper_log::{
    ChainStatus, LogEntry, LogEntryKind, TamperLog, TamperLogError, VerificationResult,
    verify_chain,
};
pub use timestamp::{Timestamp, TimestampError};
