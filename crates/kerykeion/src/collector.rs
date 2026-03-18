//! `MeshCollector` — kerykeion integration point for the Akroasis collection pipeline.
//!
//! `koinon::Collector` does not yet exist as a trait; `Collector` is defined locally
//! here until koinon is extended. See the Observations section in the P2-01 PR.

use crate::{config::MeshConfig, error::Error};

/// Trait for Akroasis data collectors.
///
/// Defines the minimal lifecycle interface shared across all collector crates.
/// This is a local definition pending the addition of `koinon::Collector`.
pub trait Collector: Send + Sync {
    /// Returns the canonical name of this collector.
    fn name(&self) -> &'static str;

    /// Returns `true` if the collector's hardware or network target is reachable.
    ///
    /// Used during startup to decide whether to activate the collector.
    fn probe(&self) -> impl std::future::Future<Output = bool> + Send;

    /// Starts the collection loop.
    ///
    /// Returns when the collector has shut down cleanly.
    ///
    /// # Errors
    ///
    /// Returns an error if the collector encounters a fatal, unrecoverable failure.
    fn run(&mut self) -> impl std::future::Future<Output = Result<(), Error>> + Send;
}

/// Meshtastic mesh networking collector.
///
/// Manages connections to one or more Meshtastic radios, receives mesh packets,
/// maintains the node database, and forwards observations into the Akroasis pipeline.
pub struct MeshCollector {
    config: MeshConfig,
}

impl MeshCollector {
    /// Creates a new `MeshCollector` with the given configuration.
    #[must_use]
    pub const fn new(config: MeshConfig) -> Self {
        Self { config }
    }
}

impl Collector for MeshCollector {
    fn name(&self) -> &'static str {
        "kerykeion"
    }

    async fn probe(&self) -> bool {
        // WHY: stub — actual USB enumeration and TCP probe implemented in P2-02.
        // Return true only if at least one connection is configured.
        !self.config.connections.is_empty()
    }

    async fn run(&mut self) -> Result<(), Error> {
        // WHY: stub — full implementation in P2-02 (serial/TCP/BLE transports).
        tracing::info!(
            collector = self.name(),
            "run() called (stub — not yet implemented)"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ConnectionConfig, MeshConfig, StoreForwardConfig, TopologyConfig};

    fn make_config(connections: Vec<ConnectionConfig>) -> MeshConfig {
        MeshConfig {
            connections,
            channel_psk: vec![],
            store_forward: StoreForwardConfig::default(),
            topology: TopologyConfig::default(),
        }
    }

    #[test]
    fn name_is_kerykeion() {
        let c = MeshCollector::new(make_config(vec![]));
        assert_eq!(c.name(), "kerykeion");
    }

    #[tokio::test]
    async fn probe_false_when_no_connections() {
        let c = MeshCollector::new(make_config(vec![]));
        assert!(!c.probe().await);
    }

    #[tokio::test]
    async fn probe_true_when_connections_configured() {
        let c = MeshCollector::new(make_config(vec![ConnectionConfig::Serial {
            port: "/dev/ttyUSB0".into(),
            baud: 115_200,
        }]));
        assert!(c.probe().await);
    }

    #[tokio::test]
    async fn run_returns_ok() {
        let mut c = MeshCollector::new(make_config(vec![]));
        assert!(c.run().await.is_ok());
    }
}
