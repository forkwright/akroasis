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
    // WHY: `.floor()` before the cast is required — `as i32` truncates toward
    // zero, not toward negative infinity, which would double the width of the
    // cell straddling zero and desync from `cell_center`'s `+half` assumption
    // for negative coordinates. lat * 10_000 ≤ 900_000 and lon * 10_000 ≤
    // 1_800_000, both within i32 range after flooring.
    let res = f64::from(resolution);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "lat * resolution bounded by 900_000 for resolution ≤ 10_000; fits i32"
    )]
    let lat_cell = (coords.latitude * res).floor() as i32; // SAFETY: lat*resolution bounded by 900_000 (resolution ≤ 10_000); fits i32
    #[expect(
        clippy::cast_possible_truncation,
        reason = "lon * resolution bounded by 1_800_000 for resolution ≤ 10_000; fits i32"
    )]
    let lon_cell = (coords.longitude * res).floor() as i32; // SAFETY: lon*resolution bounded by 1_800_000 (resolution ≤ 10_000); fits i32
    GridCell(lat_cell, lon_cell)
}

// ---------------------------------------------------------------------------
// DomainHit
// ---------------------------------------------------------------------------

/// A single signal observation placed in a grid cell.
// WHY: pure data — an observation record with no derived invariant.
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
// WHY: pure data — a detection result bag with no derived invariant.
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

/// Number of domain-discriminant slots a cell can hold.
///
/// WHY(#223): [`kind_discriminant`] returns 0-6 for the seven known
/// [`SignalKind`] variants, plus sentinel 255 for any future
/// `#[non_exhaustive]` variant. Reserving one slot per known discriminant
/// (indices 0-6) and one shared slot for the sentinel (index 7) makes
/// per-cell storage exactly as expressive as `domain_count` ever reads --
/// distinctness of discriminant values -- while fixing its size at
/// compile time.
const DOMAIN_SLOTS: usize = 8;

/// Map a domain discriminant to its slot index in [`DOMAIN_SLOTS`].
const fn slot_index(discriminant: u8) -> usize {
    match discriminant {
        255 => DOMAIN_SLOTS - 1,
        d => d as usize,
    }
}

/// Bounded per-cell observation state: one retained hit per domain.
///
/// WHY(#223): the grid previously stored every ingested [`DomainHit`] in an
/// unbounded per-cell `Vec`, so an attacker who streams frames at one
/// coordinate grows that cell's memory at the input rate with no ceiling. No
/// consumer of [`Convergence`] reads more than one hit per domain --
/// `domain_count` only needs distinctness -- so retaining the single most
/// recent hit per discriminant carries the same detection semantics at fixed
/// size.
#[derive(Debug, Clone, Default)]
struct DomainSlots([Option<DomainHit>; DOMAIN_SLOTS]);

impl DomainSlots {
    /// Record `hit`, replacing any earlier hit for the same domain.
    fn record(&mut self, hit: DomainHit) {
        self.0[slot_index(kind_discriminant(&hit.kind))] = Some(hit);
    }

    /// Hits at or after `cutoff_ms`, at most one per domain.
    fn within_window(&self, cutoff_ms: i64) -> impl Iterator<Item = &DomainHit> {
        self.0
            .iter()
            .filter_map(Option::as_ref)
            .filter(move |h| h.timestamp.as_unix_millis() >= cutoff_ms)
    }

    /// Drop hits older than `cutoff_ms`. Returns `true` if any slot remains.
    fn evict(&mut self, cutoff_ms: i64) -> bool {
        let mut any_remaining = false;
        for slot in &mut self.0 {
            if slot
                .as_ref()
                .is_some_and(|h| h.timestamp.as_unix_millis() < cutoff_ms)
            {
                *slot = None;
            }
            any_remaining |= slot.is_some();
        }
        any_remaining
    }
}

/// Spatial grid tracking per-cell domain observations.
///
/// Cells are keyed by [`GridCell`]. Each cell holds a bounded [`DomainSlots`]
/// (at most one retained hit per domain, independent of input volume — see
/// [`DomainSlots`]). Callers must periodically call [`evict`] to reclaim
/// cells whose hits have all aged out.
///
/// [`evict`]: ConvergenceGrid::evict
pub struct ConvergenceGrid {
    /// Per-cell observations.
    cells: HashMap<GridCell, DomainSlots>,
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
        self.cells.entry(cell).or_default().record(DomainHit {
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
    ///
    /// WHY(#223): work here is bounded by `cells.len() * DOMAIN_SLOTS`
    /// regardless of how many signals were ingested — no cell can hold more
    /// than [`DOMAIN_SLOTS`] hits, so a flood at one coordinate no longer
    /// makes this scan more expensive.
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

        for (&cell, slots) in &self.cells {
            let window_hits: Vec<&DomainHit> = slots.within_window(cutoff_ms).collect();

            // Count distinct domain discriminants.
            let distinct = domain_count(&window_hits);
            if distinct >= min_domains {
                let center = cell_center(cell, self.resolution);
                result.push(Convergence {
                    center,
                    hits: window_hits.into_iter().cloned().collect(),
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
        self.cells.retain(|_, slots| slots.evict(cutoff));
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
        Coordinates::new(0.0, 0.0, None).unwrap() // SAFETY: (0.0, 0.0) is within valid lat/lon range; CoordinatesError cannot fire
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
            EnvironmentalDetail, GpsDetail, MeshDetail, NetworkDetail, OsintDetail,
            ProximityDetail, RfDetail,
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

    fn osint_kind() -> SignalKind {
        SignalKind::Osint(OsintDetail::FeedItem {
            source: "test-feed".into(),
            title: "synthetic indicator".into(),
        })
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

    // ── floor semantics for negative coordinates ──────────────────────────────

    #[test]
    fn quantize_floors_negative_coordinates_not_truncates_toward_zero() {
        // -0.00015 * 10_000 = -1.5. Truncation-toward-zero (`as i32` alone)
        // gives -1; floor must give -2, matching the doc's floor contract.
        let loc = coords(-0.00015, -0.00015);
        let cell = quantize(&loc, 10_000);
        assert_eq!(
            cell,
            GridCell(-2, -2),
            "floor(-1.5) must be -2, not -1 (truncation-toward-zero)"
        );
    }

    #[test]
    fn cell_center_reconstructs_within_true_cell_range_for_negative_coords() {
        let resolution = 10_000;
        let res = f64::from(resolution);
        let loc = coords(-0.00015, -0.00015);
        let cell = quantize(&loc, resolution);
        let center = cell_center(cell, resolution);

        // True cell range under floor semantics: [cell / res, (cell + 1) / res).
        let lat_lo = f64::from(cell.0) / res;
        let lat_hi = f64::from(cell.0 + 1) / res;
        let lon_lo = f64::from(cell.1) / res;
        let lon_hi = f64::from(cell.1 + 1) / res;

        assert!(
            center.latitude >= lat_lo && center.latitude < lat_hi,
            "reconstructed centre latitude {} must lie within cell range [{lat_lo}, {lat_hi})",
            center.latitude
        );
        assert!(
            center.longitude >= lon_lo && center.longitude < lon_hi,
            "reconstructed centre longitude {} must lie within cell range [{lon_lo}, {lon_hi})",
            center.longitude
        );
    }

    #[test]
    fn equator_and_prime_meridian_cell_is_not_double_width() {
        // The cell straddling (0, 0) must be the same width as every other
        // cell: a coordinate just below zero and one just above zero must
        // fall in different cells, not share one double-width cell.
        let resolution = 10_000;
        let just_below = coords(-0.00001, -0.00001);
        let just_above = coords(0.00001, 0.00001);
        assert_ne!(
            quantize(&just_below, resolution),
            quantize(&just_above, resolution),
            "the cell straddling zero must not be double-width"
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

    // ── #223: per-cell storage is bounded under a same-domain flood ─────────

    #[test]
    fn single_domain_flood_does_not_grow_cell_storage() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);

        for _ in 0..10_000 {
            grid.ingest(&signal_at(rf_kind(), loc));
        }

        let cell = quantize(&loc, 10_000);
        let slots = grid
            .cells
            .get(&cell)
            .expect("cell must exist after at least one ingest");
        let occupied = slots.0.iter().filter(|s| s.is_some()).count();
        assert_eq!(
            occupied, 1,
            "10,000 same-domain signals at one coordinate must retain exactly \
             one slot, not grow with input volume"
        );
    }

    #[test]
    fn multi_domain_flood_stays_within_domain_slot_bound() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);
        let kinds = [
            rf_kind(),
            mesh_kind(),
            network_kind(),
            proximity_kind(),
            gps_kind(),
            env_kind(),
            osint_kind(),
        ];

        for i in 0..10_000 {
            grid.ingest(&signal_at(kinds[i % kinds.len()].clone(), loc));
        }

        let cell = quantize(&loc, 10_000);
        let slots = grid.cells.get(&cell).expect("cell must exist");
        let occupied = slots.0.iter().filter(|s| s.is_some()).count();
        assert!(
            occupied <= DOMAIN_SLOTS,
            "occupied slots ({occupied}) must never exceed the fixed bound \
             ({DOMAIN_SLOTS}) regardless of input volume"
        );
        assert_eq!(
            occupied,
            kinds.len(),
            "all seven distinct domains from the flood should still be represented"
        );
    }

    #[test]
    fn a_later_hit_for_the_same_domain_replaces_the_earlier_one() {
        // WHY(#223): the whole point of bounding storage to one slot per
        // domain is that a later hit for a domain already present replaces
        // the earlier one rather than accumulating -- verify the *content*
        // that survives is the latest, not just the count.
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(51.5, -0.1);

        let old_ts = Timestamp::from_unix_millis(Timestamp::now().as_unix_millis() - 5_000)
            .expect("valid timestamp");
        grid.ingest(&GeoSignal::new(rf_kind(), old_ts, Some(loc)));

        let now = Timestamp::now();
        grid.ingest(&GeoSignal::new(rf_kind(), now, Some(loc)));

        let cell = quantize(&loc, 10_000);
        let slots = grid.cells.get(&cell).expect("cell must exist");
        let rf_slot = slots.0[slot_index(kind_discriminant(&rf_kind()))]
            .as_ref()
            .expect("rf slot must be occupied");
        assert_eq!(
            rf_slot.timestamp.as_unix_millis(),
            now.as_unix_millis(),
            "the surviving hit must be the latest one, not the first"
        );
    }

    #[test]
    fn seven_distinct_domains_all_detected() {
        let mut grid = ConvergenceGrid::new(10_000);
        let loc = coords(40.0, -74.0);
        for kind in [
            rf_kind(),
            mesh_kind(),
            network_kind(),
            proximity_kind(),
            gps_kind(),
            env_kind(),
            osint_kind(),
        ] {
            grid.ingest(&signal_at(kind, loc));
        }
        let now = Timestamp::now();
        let found = grid.detect(7, std::time::Duration::from_secs(30), now);
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().expect("checked len above").domain_count, 7);
    }
}
