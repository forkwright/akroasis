//! στοιχεῖον — the elements the Akroasis workspace is composed from.
//!
//! A stoicheion is a primary constituent, and also a letter of the alphabet:
//! the units that combine into everything written above them. This crate holds
//! that layer — what a signal, a frequency, a coordinate, a piece of hardware
//! and a moment in time *are* — and nothing that reasons about them.
//!
//! Evidence about those things (the tamper-evident log, caller authority,
//! effect receipts) lives in `tekmerion`, which depends on this crate and not
//! the reverse.

#![deny(missing_docs)]

pub mod baseline;
pub mod coordinates;
pub mod entity;
pub mod frequency;
pub mod hardware;
pub mod id;
pub mod power;
pub mod signal;
pub mod timestamp;

pub use baseline::{AnomalyScore, Baseline, ScoringConfig, TemporalBucketedBaseline};
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
pub use timestamp::{Timestamp, TimestampError};
