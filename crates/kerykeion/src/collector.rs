//! Mesh network collector — integrates with koinon's asset registry.

use koinon::{AssetRegistry, HardwareKind, MeshNodeKind};
use tracing::info;

/// Mesh network data collector.
///
/// Discovers Meshtastic devices via koinon's `AssetRegistry` and collects
/// mesh traffic. Full implementation arrives in P2-02+.
#[derive(Debug)]
pub struct MeshCollector {
    _private: (),
}

impl MeshCollector {
    /// Create a new mesh collector.
    #[must_use]
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Collector name used for logging and identification.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        "kerykeion"
    }

    /// Probe for available Meshtastic devices in the asset registry.
    #[must_use]
    pub fn probe(registry: &AssetRegistry) -> Vec<koinon::DeviceId> {
        registry
            .all()
            .filter(|asset| {
                matches!(
                    &asset.kind,
                    HardwareKind::MeshNode(MeshNodeKind::TEcho | MeshNodeKind::TDeckPlus)
                )
            })
            .map(|asset| asset.device_id)
            .collect()
    }

    /// Run the collector loop (stub — actual implementation in P2-02+).
    ///
    /// # Errors
    ///
    /// Returns `Error` if the collector encounters an unrecoverable fault.
    pub fn run(&self) -> Result<(), crate::error::Error> {
        info!("kerykeion collector started (stub)");
        Ok(())
    }
}

impl Default for MeshCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn collector_name_is_kerykeion() {
        let c = MeshCollector::new();
        assert_eq!(c.name(), "kerykeion");
    }

    #[test]
    fn default_equals_new() {
        let a = MeshCollector::new();
        let b = MeshCollector::default();
        assert_eq!(a.name(), b.name());
    }

    #[test]
    fn probe_empty_registry_yields_nothing() {
        let registry = AssetRegistry::new();
        let devices = MeshCollector::probe(&registry);
        assert!(devices.is_empty());
    }

    #[test]
    fn run_stub_succeeds() {
        let c = MeshCollector::new();
        assert!(c.run().is_ok());
    }
}
