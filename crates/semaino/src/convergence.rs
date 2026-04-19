//! Spatial convergence detection across signal domains (REQ-06).
//!
//! Signals are placed into a quantized geographic grid. Cells that accumulate
//! observations from multiple distinct [`SignalKind`] domains within a sliding
//! time window are reported as [`Convergence`] events.
//!
//! # Resolution
//!
//! The default grid resolution of 10 000 maps one grid cell to approximately
//! 10 m on both axes at typical operating latitudes.

use std::collections::HashMap;

use koinon::{Coordinates, GeoSignal, Timestamp, signal::SignalKind};

// ---------------------------------------------------------------------------
// GridCell
// ---------------------------------------------------------------------------

/// A quantized latitude/longitude cell.
///
/// Each component is the coordinate multiplied by `resolution` and truncated
/// to an integer, so adjacent cells share an edge at `1 / resolution` degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell(pub i32, pub i32);

/// Convert a [`Coordinates`] to a [`GridCell`] at the given `resolution`.
///
/// `resolution = 10_000` → cells roughly 10 m wide at mid-latitudes.
/// The truncation is intentional: coordinates within the same cell map to the
/// same integer pair regardless of sub-cell offset.
#[must_use]
pub(crate) fn quantize(coords: &Coordinates, resolution: u32) -> GridCell {
    // WHY: cast from f64 to i32 is intentional; lat * 10_000 ≤ 900_000 and
    // lon * 10_000 ≤ 1_800_000, both within i32 range. The floor truncation
    // defines the grid cell lower-left corner.
    let res = f64::from(resolution);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "lat * resolution bounded by 900_000 for resolution ≤ 10_000; fits i32"
    )]
    let lat_cell = (coords.latitude * res) as i32; // SAFETY: lat*resolution bounded by 900_000 (resolution ≤ 10_000); fits i32
    #[expect(
        clippy::cast_possible_truncation,
        reason = "lon * resolution bounded by 1_800_000 for resolution ≤ 10_000; fits i32"
    )]
    let lon_cell = (coords.longitude * res) as i32; // SAFETY: lon*resolution bounded by 1_800_000 (resolution ≤ 10_000); fits i32
    GridCell(lat_cell, lon_cell)
}

// ---------------------------------------------------------------------------
// DomainHit
// ---------------------------------------------------------------------------

/// A single signal observation placed in a grid cell.
#[derive(Debug, Clone)]
pub struct DomainHit {
    /// The top-level signal domain discriminant.
    pub kind: SignalKind,
    /// Wall-clock time the signal was observed.
    pub timestamp: Timestamp,
}

// ---------------------------------------------------------------------------
// Convergence
// ---------------------------------------------------------------------------

/// A cell that accumulated observations from `>= min_domains` distinct signal
/// domains within the configured time window.
#[derive(Debug, Clone)]
pub struct Convergence {
    /// Approximate centre coordinates of the convergence cell.
    pub center: Coordinates,
    /// All domain observations inside this cell within the time window.
    pub hits: Vec<DomainHit>,
    /// Number of distinct [`SignalKind`] domains represented in `hits`.
    pub domain_count: usize,
}

// ---------------------------------------------------------------------------
// ConvergenceGrid
// ---------------------------------------------------------------------------

/// Spatial grid tracking per-cell domain observations.
///
/// Cells are keyed by [`GridCell`]. Each cell holds a [`Vec<DomainHit>`]
/// ordered by insertion time. Callers must periodically call [`evict`] to
/// prevent unbounded growth.
///
/// [`evict`]: ConvergenceGrid::evict
pub struct ConvergenceGrid {
    /// Per-cell observations.
    cells: HashMap<GridCell, Vec<DomainHit>>,
    /// Grid quantization factor (default 10 000 ≈ 10 m resolution).
    resolution: u32,
}

impl ConvergenceGrid {
    /// Create a new grid with the given quantization `resolution`.
    #[must_use]
    pub fn new(resolution: u32) -> Self {
        Self {
            cells: HashMap::new(),
            resolution,
        }
    }

    /// Place a signal into the grid.
    ///
    /// Signals without a [`GeoSignal::location`] are silently skipped —
    /// location is required for grid placement.
    pub fn ingest(&mut self, signal: &GeoSignal) {
        let Some(coords) = signal.location else {
            return;
        };
        let cell = quantize(&coords, self.resolution);
        self.cells.entry(cell).or_default().push(DomainHit {
            kind: signal.kind.clone(),
            timestamp: signal.timestamp,
        });
    }

    /// Return all cells where `>= min_domains` distinct [`SignalKind`] domains
    /// appear within the most recent `window` duration.
    ///
    /// Only hits whose `timestamp` is `>= cutoff` (where `cutoff = now - window`)
    /// are considered. The caller supplies `now` so the function is pure and
    /// testable without a real clock.
    #[must_use]
    pub fn detect(
        &self,
        min_domains: usize,
        window: std::time::Duration,
        now: Timestamp,
    ) -> Vec<Convergence> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "Duration::as_millis() returns u128; converting to i64 is safe for any reasonable window (i64::MAX ms >> practical window sizes)"
        )]
        let window_ms = window.as_millis() as i64; // SAFETY: Duration::as_millis() returns u128 but real windows are bounded; fits i64
        let cutoff_ms = now.as_unix_millis() - window_ms;

        let mut result = Vec::new();

        for (&cell, hits) in &self.cells {
            // Filter to hits within the time window.
            let window_hits: Vec<&DomainHit> = hits
                .iter()
                .filter(|h| h.timestamp.as_unix_millis() >= cutoff_ms)
                .collect();

            // Count distinct domain discriminants.
            let distinct = domain_count(&window_hits);
            if distinct >= min_domains {
                let center = cell_center(cell, self.resolution);
                result.push(Convergence {
                    center,
                    hits: window_hits.iter().map(|h| (*h).clone()).collect(),
                    domain_count: distinct,
                });
            }
        }

        result
    }

    /// Remove all hits older than `older_than` from every cell.
    ///
    /// Cells that become empty after eviction are removed from the grid.
    pub fn evict(&mut self, older_than: Timestamp) {
        let cutoff = older_than.as_unix_millis();
        self.cells.retain(|_, hits| {
            hits.retain(|h| h.timestamp.as_unix_millis() >= cutoff);
            !hits.is_empty()
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count distinct top-level domain discriminants in a slice of [`DomainHit`] refs.
fn domain_count(hits: &[&DomainHit]) -> usize {
    use std::collections::HashSet;

    let discriminants: HashSet<u8> = hits.iter().map(|h| kind_discriminant(&h.kind)).collect();
    discriminants.len()
}

/// Map a [`SignalKind`] to a stable integer discriminant for domain counting.
///
/// New [`SignalKind`] variants should be assigned a unique value here.
const fn kind_discriminant(kind: &SignalKind) -> u8 {
    match kind {
        SignalKind::Rf(_) => 0,
        SignalKind::Mesh(_) => 1,
        SignalKind::Network(_) => 2,
        SignalKind::Proximity(_) => 3,
        SignalKind::Gps(_) => 4,
        SignalKind::Environmental(_) => 5,
        SignalKind::Osint(_) => 6,
        // WHY: SignalKind is #[non_exhaustive]; unknown future variants are
        // grouped under sentinel 255 so they do not silently inflate domain counts.
        _ => 255,
    }
}

/// Reconstruct approximate centre [`Coordinates`] from a [`GridCell`] and resolution.
///
/// Adds 0.5 cell-widths to each axis to return the cell midpoint.
fn cell_center(cell: GridCell, resolution: u32) -> Coordinates {
    let res = f64::from(resolution);
    let half = 0.5 / res;
    let lat = f64::from(cell.0) / res + half;
    let lon = f64::from(cell.1) / res + half;
    // Clamp to valid coordinate ranges before constructing. Coordinates::new
    // returns Err only for out-of-range values; clamping here prevents that.
    let lat = lat.clamp(-90.0, 90.0);
    let lon = lon.clamp(-180.0, 180.0);
    // WHY: clamped values are always in range; use unwrap_or_else to fall back
    // to origin rather than propagating a Result through a helper function.
    Coordinates::new(lat, lon, None).unwrap_or_else(|_| {
        // Safety: (0.0, 0.0) is always valid.
        #[expect(
            clippy::unwrap_used,
            reason = "literal (0,0) coordinates cannot fail validation"
        )]
        Coordinates::new(0.0, 0.0, None).unwrap()
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use koinon::{
        Frequency, Power, Timestamp,
        signal::{
            EnvironmentalDetail, GpsDetail, MeshDetail, NetworkDetail, ProximityDetail, RfDetail,
        },
    };

    use super::*;

    fn coords(lat: f64, lon: f64) -> Coordinates {
        Coordinates::new(lat, lon, None).unwrap()
    }

    fn ts_now() -> Timestamp {
        Timestamp::now()
    }

    fn signal_at(kind: SignalKind, location: Coordinates) -> GeoSignal {
        GeoSignal::new(kind, ts_now(), Some(location))
    }

    fn rf_kind() -> SignalKind {
        SignalKind::Rf(RfDetail::Transmission {
            frequency: Frequency::mhz(146),
            power: Power::dbm(-30.0),
            modulation: "FM".into(),
            bandwidth: Frequency::khz(25),
        })
    }

    fn mesh_kind() -> SignalKind {
        SignalKind::Mesh(MeshDetail::NodeSeen {
            node_id: 1,
            snr: 5.0,
            hop_count: 1,
        })
    }

    fn network_kind() -> SignalKind {
        SignalKind::Network(NetworkDetail::DnsQuery {
            domain: "test.local".into(),
        })
    }

    fn proximity_kind() -> SignalKind {
        SignalKind::Proximity(ProximityDetail::Wifi {
            ssid: None,
            bssid: [0u8; 6],
            rssi: -65,
            channel: 6,
        })
    }

    fn gps_kind() -> SignalKind {
        SignalKind::Gps(GpsDetail::Fix {
            satellites: 8,
            hdop: 1.2,
            speed_mps: None,
        })
    }

    fn env_kind() -> SignalKind {
        SignalKind::Environmental(EnvironmentalDetail::Temperature { celsius: 22.5 })
    }

    // ── quantize ─────────────────────────────────────────────────────────────

    #[test]
    fn quantize_groups_nearby_coordinates() {
        // Two coordinates ~1 m apart at 10m resolution should map to the same cell.
        // 1 m in degrees latitude ≈ 0.000009°.
        let a = coords(51.500_00, 0.0);
        let b = coords(51.500_005, 0.0); // ~0.5 m north
        let cell_a = quantize(&a, 10_000);
        let cell_b = quantize(&b, 10_000);
        assert_eq!(cell_a, cell_b, "nearby coordinates should share a cell");
    }

    #[test]
    fn quantize_separates_distant_coordinates() {
        let a = coords(51.5, 0.0);
        let b = coords(51.6, 0.0); // ~11 km north
        let cell_a = quantize(&a, 10_000);
        let cell_b = quantize(&b, 10_000);
        assert_ne!(
            cell_a, cell_b,
            "distant coordinates must be in different cells"
        );
    }

    // ── single domain below threshold ─────────────────────────────────────────

    #[test]
    fn single_domain_below_threshold() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);
        grid.ingest(&signal_at(rf_kind(), loc));
        let now = Timestamp::now();
        let found = grid.detect(2, std::time::Duration::from_secs(30), now);
        assert!(
            found.is_empty(),
            "one domain should not trigger convergence"
        );
    }

    // ── multi-domain triggers convergence ────────────────────────────────────

    #[test]
    fn multi_domain_triggers_convergence() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);
        grid.ingest(&signal_at(rf_kind(), loc));
        grid.ingest(&signal_at(mesh_kind(), loc));
        grid.ingest(&signal_at(proximity_kind(), loc));
        let now = Timestamp::now();
        let found = grid.detect(3, std::time::Duration::from_secs(30), now);
        assert_eq!(found.len(), 1, "three domains should trigger convergence");
        assert_eq!(found.first().expect("checked len above").domain_count, 3);
    }

    // ── different cells no convergence ───────────────────────────────────────

    #[test]
    fn different_cells_no_convergence() {
        let mut grid = ConvergenceGrid::new(10_000);
        // Place each signal in a clearly different cell (~100 km apart).
        grid.ingest(&signal_at(rf_kind(), coords(51.0, 0.0)));
        grid.ingest(&signal_at(mesh_kind(), coords(52.0, 0.0)));
        grid.ingest(&signal_at(proximity_kind(), coords(53.0, 0.0)));
        let now = Timestamp::now();
        let found = grid.detect(2, std::time::Duration::from_secs(30), now);
        assert!(
            found.is_empty(),
            "signals in separate cells must not converge"
        );
    }

    // ── eviction removes old entries ─────────────────────────────────────────

    #[test]
    fn eviction_removes_old_entries() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);

        // Inject signals with an old timestamp.
        let stale_millis = Timestamp::now().as_unix_millis() - 10_000; // 10 seconds ago
        let stale_ts = Timestamp::from_unix_millis(stale_millis).unwrap();
        let stale_rf = GeoSignal::new(rf_kind(), stale_ts, Some(loc));
        let stale_mesh = GeoSignal::new(mesh_kind(), stale_ts, Some(loc));
        let stale_prox = GeoSignal::new(proximity_kind(), stale_ts, Some(loc));

        grid.ingest(&stale_rf);
        grid.ingest(&stale_mesh);
        grid.ingest(&stale_prox);

        // Evict everything older than 5 seconds ago.
        let cutoff_millis = Timestamp::now().as_unix_millis() - 5_000;
        let evict_before = Timestamp::from_unix_millis(cutoff_millis).unwrap();
        grid.evict(evict_before);

        let now = Timestamp::now();
        let found = grid.detect(2, std::time::Duration::from_secs(30), now);
        assert!(
            found.is_empty(),
            "evicted signals must not trigger convergence"
        );
    }

    // ── time-window filtering ─────────────────────────────────────────────────

    #[test]
    fn time_window_filters_old_hits() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);

        // Aged signals (60 s ago) — outside a 30 s window.
        let aged_millis = Timestamp::now().as_unix_millis() - 60_000;
        let aged_ts = Timestamp::from_unix_millis(aged_millis).unwrap();
        for kind in [rf_kind(), mesh_kind(), proximity_kind()] {
            grid.ingest(&GeoSignal::new(kind, aged_ts, Some(loc)));
        }

        let now = Timestamp::now();
        let found = grid.detect(2, std::time::Duration::from_secs(30), now);
        assert!(
            found.is_empty(),
            "signals outside the time window must not trigger convergence"
        );
    }

    // ── full domain set ───────────────────────────────────────────────────────

    #[test]
    fn six_distinct_domains_all_detected() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(40.0, -74.0);
        for kind in [
            rf_kind(),
            mesh_kind(),
            network_kind(),
            proximity_kind(),
            gps_kind(),
            env_kind(),
        ] {
            grid.ingest(&signal_at(kind, loc));
        }
        let now = Timestamp::now();
        let found = grid.detect(6, std::time::Duration::from_secs(30), now);
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().expect("checked len above").domain_count, 6);
    }
}
