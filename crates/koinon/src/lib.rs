//! κοινόν — shared foundational types for the Akroasis workspace.

pub mod coordinates;
pub mod frequency;
pub mod id;
pub mod power;
pub mod tamper_log;
pub mod timestamp;

pub use coordinates::{Coordinates, CoordinatesError, Datum};
pub use frequency::Frequency;
pub use id::{DeviceId, EntityId, FrequencyId, SignalId};
pub use power::Power;
pub use tamper_log::{
    ChainStatus, LogEntry, LogEntryKind, TamperLog, TamperLogError, VerificationResult,
    verify_chain,
};
pub use timestamp::{Timestamp, TimestampError};
