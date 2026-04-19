//! σημαίνω — signal aggregation, convergence detection, and alert pipeline.
//!
//! semaino is the intelligence processing core of akroasis. It consumes typed
//! [`GeoSignal`] events from collectors, maintains per-signal-type temporal
//! baselines, detects spatial convergence across signal domains, and produces
//! deduplicated severity-classified alerts.
//!
//! # Architecture
//!
//! ```text
//! broadcast::Receiver<GeoSignal>
//!         │
//!         ▼
//!   ┌─────────────────┐
//!   │ SignalAggregator │  per-kind TemporalBucketedBaseline → AnomalyScore
//!   └────────┬────────┘
//!            │  AggregatedSignal
//!            ▼
//!   ┌──────────────────┐
//!   │ ConvergenceGrid  │  spatial grid → multi-domain Convergence events
//!   └────────┬─────────┘
//!            │  Convergence
//!            ▼
//!   ┌───────────────────┐
//!   │  AlertPipeline    │  dedup + classify → Alert via AlertSink
//!   └───────────────────┘
//! ```
//!
//! # Modules
//!
//! - [`aggregator`] — baseline scoring per signal kind (REQ-05)
//! - [`convergence`] — spatial grid and multi-domain convergence detection (REQ-06)
//! - [`alert`] — deduplication, severity classification, routing (REQ-07)
//! - [`pipeline`] — top-level async orchestrator

#![deny(missing_docs)]

pub mod aggregator;
pub mod alert;
pub mod convergence;
pub mod pipeline;

pub use aggregator::{AggregatedSignal, SignalAggregator};
pub use alert::{Alert, AlertFingerprint, AlertPipeline, AlertSink, TracingSink};
pub use convergence::{Convergence, ConvergenceGrid, DomainHit, GridCell};
pub use pipeline::{SemainoConfig, SemainoPipeline};
