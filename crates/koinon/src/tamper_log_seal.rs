//! Chain key and truncation-seal machinery for [`super::TamperLog`].
//!
//! Keying the hash chain closes the unkeyed, self-rooted forgery gap: with
//! the old scheme anyone holding the plaintext log could recompute a fully
//! valid chain rooted at the public `[0u8; 32]` genesis. [`ChainKey`] makes
//! that recomputation require a secret.
//!
//! The sidecar seal closes the complementary gap: trailing truncation. A
//! hash chain alone cannot detect a deleted tail — any prefix of a valid
//! chain is itself internally consistent, so reading to EOF looks
//! identical whether the file legitimately ends there or was cut short.
//! [`write_seal`] persists an authenticated entry count beside the log,
//! refreshed on every open and append; [`read_seal`] re-authenticates it
//! during verification so [`super::verify_chain`] can tell "ends here" from
//! "was made to look like it ends here."
//!
//! The seal also carries `segment_start_hash` (akroasis#211): the hash this
//! segment's own chain was linked from — the keyed genesis root for a
//! never-rotated log, or the terminal hash of the segment rotated out
//! immediately before this one. Authenticating it in the same MAC as the
//! count is what lets a deleted whole segment be detected: its successor's
//! seal still cryptographically commits to the deleted segment's terminal
//! hash, which no other surviving segment can supply (`tamper_log_segments`
//! walks the set and checks it). Content integrity for every entry up to
//! the sealed count is already the hash chain's job, not the seal's — the
//! seal exists for the two things a hash chain alone cannot see: where its
//! own file ends, and where it began.

use std::fmt;
use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use snafu::ResultExt;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::{IoSnafu, TamperLogError};

/// Length in bytes of a [`ChainKey`].
pub const CHAIN_KEY_LEN: usize = 32;

/// Domain-separation tag for the keyed genesis root.
const GENESIS_DOMAIN: &[u8] = b"koinon/tamper-log/genesis/v1";

/// Domain-separation tag for the seal MAC.
const SEAL_DOMAIN: &[u8] = b"koinon/tamper-log/seal/v1";

/// Length in bytes of the on-disk seal entry-count field.
const SEAL_COUNT_LEN: usize = 8;

/// Length in bytes of the on-disk seal's cross-segment link field.
const SEAL_LINK_LEN: usize = 32;

/// Length in bytes of the on-disk seal MAC.
const SEAL_MAC_LEN: usize = 32;

/// Total on-disk seal file length: count || `segment_start_hash` || MAC.
const SEAL_FILE_LEN: usize = SEAL_COUNT_LEN + SEAL_LINK_LEN + SEAL_MAC_LEN;

/// Domain separator for a seal that carries installation provenance.
///
/// Distinct from [`SEAL_DOMAIN`] so a MAC computed over an unsigned seal can
/// never authenticate a signed one or the reverse, whatever the field bytes
/// happen to be.
const SIGNED_SEAL_DOMAIN: &[u8] = b"koinon/tamper-log/seal-signed/v1";

/// Domain separator for the payload an installation signs.
///
/// WHY the signature covers a domain-separated string rather than the bare
/// terminal hash: a signature over a raw 32-byte value proves only that the
/// key signed *some* 32 bytes, so the same signature would be replayable
/// wherever else that key signs a hash. Binding the purpose makes the
/// signature a claim about this log's tip specifically.
pub(super) const TIP_SIGNING_DOMAIN: &[u8] = b"koinon/tamper-log/tip/v1";

/// Length of the short installation key identifier stored in a signed seal.
pub const KEY_ID_LEN: usize = 8;

/// Length of the tip signature stored in a signed seal.
pub const TIP_SIGNATURE_LEN: usize = 64;

/// Length of a seal that carries provenance: the unsigned fields, plus the
/// terminal hash the signature commits to, the key id, and the signature —
/// with the MAC last, as in the unsigned layout.
const SIGNED_SEAL_FILE_LEN: usize =
    SEAL_COUNT_LEN + SEAL_LINK_LEN + SEAL_LINK_LEN + KEY_ID_LEN + TIP_SIGNATURE_LEN + SEAL_MAC_LEN;

/// Secret key that binds a tamper log's hash chain to its owner.
///
/// Every link in the chain is keyed with [`blake3::Hasher::new_keyed`]
/// rather than the public unkeyed hasher, and the genesis root is
/// `blake3::keyed_hash` over a domain tag rather than a public constant.
/// Without this key an attacker who edits, deletes, or rewrites entries
/// cannot recompute a chain that re-validates.
///
/// Zeroized on drop. `Debug` output is redacted. Equality is
/// constant-time.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ChainKey {
    bytes: [u8; CHAIN_KEY_LEN],
}

impl ChainKey {
    /// Wraps raw key bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; CHAIN_KEY_LEN]) -> Self {
        Self { bytes }
    }

    /// Returns the raw key material.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CHAIN_KEY_LEN] {
        &self.bytes
    }
}

impl PartialEq for ChainKey {
    fn eq(&self, other: &Self) -> bool {
        self.bytes.ct_eq(&other.bytes).into()
    }
}

impl Eq for ChainKey {}

impl fmt::Debug for ChainKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChainKey([REDACTED])")
    }
}

/// Computes the keyed genesis root fed to the chain's first entry as
/// `prev_hash`.
///
/// Rooting this at a keyed value — rather than the public `[0u8; 32]` the
/// prior scheme used — means a verifier without `chain_key` cannot even
/// start a matching chain, let alone extend one.
pub(super) fn genesis_hash(chain_key: &ChainKey) -> [u8; 32] {
    blake3::keyed_hash(chain_key.as_bytes(), GENESIS_DOMAIN).into()
}

/// Signs a tamper log's terminal hash on behalf of an installation.
///
/// WHY a trait here rather than a concrete key type: koinon owns the chain and
/// its keyed hashing, not the fleet's choice of signature scheme. The identity
/// layer lives in `kryphos`, which depends on this crate — so a concrete
/// Ed25519 type in this signature would invert that dependency. Implementors
/// supply an opaque signature over whatever bytes they are handed.
pub trait TipSigner {
    /// Stable short identifier for the signing installation.
    ///
    /// A digest of the verifying key rather than the key itself, so a log left
    /// on disk names its origin without publishing material that identifies it
    /// to a reader who does not already hold the key.
    fn key_id(&self) -> [u8; KEY_ID_LEN];

    /// Signs `payload`, which is already domain-separated by the caller.
    fn sign_tip(&self, payload: &[u8]) -> [u8; TIP_SIGNATURE_LEN];
}

/// Verifies a tamper log's tip signature against a known installation.
///
/// The counterpart to [`TipSigner`], and deliberately a separate trait: a
/// verifier holds only public material, and nothing that verifies should need
/// a type that can also sign.
pub trait TipVerifier {
    /// Stable short identifier for the installation this verifier represents.
    fn key_id(&self) -> [u8; KEY_ID_LEN];

    /// Reports whether `signature` is a valid signature over `payload`.
    fn verify_tip(&self, payload: &[u8], signature: &[u8; TIP_SIGNATURE_LEN]) -> bool;
}

/// Installation provenance recorded alongside a seal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipProvenance {
    /// Short identifier of the installation that signed this tip.
    pub key_id: [u8; KEY_ID_LEN],
    /// The terminal chain hash the signature commits to.
    pub terminal_hash: [u8; SEAL_LINK_LEN],
    /// Signature over [`TIP_SIGNING_DOMAIN`] followed by `terminal_hash`.
    pub signature: [u8; TIP_SIGNATURE_LEN],
}

/// Builds the exact bytes a [`TipSigner`] signs and a [`TipVerifier`] checks.
///
/// One function so the two directions cannot drift: a verifier reconstructing
/// this by hand is a verifier that will one day reconstruct it differently.
pub(super) fn tip_payload(terminal_hash: &[u8; SEAL_LINK_LEN]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(TIP_SIGNING_DOMAIN.len() + SEAL_LINK_LEN);
    payload.extend_from_slice(TIP_SIGNING_DOMAIN);
    payload.extend_from_slice(terminal_hash);
    payload
}

/// Result of reading and authenticating a sidecar seal file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SealState {
    /// No seal file exists at the sidecar path.
    Absent,
    /// A seal file exists but its MAC does not authenticate under
    /// `chain_key` — corrupted, forged, or written under a different key.
    Invalid,
    /// A seal file exists and its MAC authenticates; carries the sealed
    /// entry count and the segment's authenticated chain-start hash (see
    /// [`super::rotation`] — genesis for a never-rotated segment, or the
    /// prior segment's terminal hash for one that carries a rotation
    /// boundary).
    Valid {
        /// Authenticated entry count.
        entry_count: u64,
        /// Authenticated chain-start hash this segment's first entry was
        /// linked from.
        segment_start_hash: [u8; 32],
        /// Installation provenance, when this seal carries it.
        ///
        /// `None` on a log written before provenance existed, or by a writer
        /// with no identity. Present means the MAC authenticated it along with
        /// everything else, so a reader can trust the key id and terminal hash
        /// as far as it trusts the count.
        provenance: Option<TipProvenance>,
    },
}

/// Derives the sidecar seal path for a log file: `{path}.seal`.
pub(super) fn seal_path(log_path: &Path) -> PathBuf {
    let mut name = log_path.as_os_str().to_owned();
    name.push(".seal");
    PathBuf::from(name)
}

/// Computes the authenticated MAC over `entry_count` and `segment_start_hash`,
/// domain-separated from entry and genesis hashing.
///
/// Authenticating `segment_start_hash` alongside the count is what makes the
/// cross-segment link tamper-evident: without the key, an attacker who
/// deletes a rotated segment cannot forge a replacement seal for its
/// successor that still claims the deleted segment's terminal hash as its
/// start (see `tamper_log_segments`).
fn seal_mac(chain_key: &ChainKey, entry_count: u64, segment_start_hash: &[u8; 32]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(chain_key.as_bytes());
    hasher.update(SEAL_DOMAIN);
    hasher.update(&entry_count.to_le_bytes());
    hasher.update(segment_start_hash);
    hasher.finalize().into()
}

/// Computes the MAC over a seal that carries provenance.
///
/// WHY the provenance fields go under the seal's own MAC rather than relying
/// on the signature alone: the signature proves an installation committed to a
/// terminal hash, and says nothing about the count or link beside it. Covering
/// everything with one MAC keyed by `chain_key` means a reader that trusts the
/// count trusts the key id and terminal hash to exactly the same degree, and an
/// editor of any one field invalidates the whole seal rather than leaving a
/// self-consistent record with one value swapped.
fn signed_seal_mac(
    chain_key: &ChainKey,
    entry_count: u64,
    segment_start_hash: &[u8; 32],
    provenance: &TipProvenance,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(chain_key.as_bytes());
    hasher.update(SIGNED_SEAL_DOMAIN);
    hasher.update(&entry_count.to_le_bytes());
    hasher.update(segment_start_hash);
    hasher.update(&provenance.terminal_hash);
    hasher.update(&provenance.key_id);
    hasher.update(&provenance.signature);
    hasher.finalize().into()
}

/// Reads and authenticates the seal sidecar for `log_path`.
pub(super) fn read_seal(log_path: &Path, chain_key: &ChainKey) -> SealState {
    let Ok(bytes) = fs::read(seal_path(log_path)) else {
        return SealState::Absent;
    };
    // WHY length discriminates the two layouts rather than a version byte: a
    // seal is a fixed-width record with no header, so its length already
    // distinguishes them unambiguously, and each layout's MAC is
    // domain-separated from the other. Anything that is neither length is not a
    // seal this code wrote.
    match bytes.len() {
        SEAL_FILE_LEN => read_unsigned_seal(&bytes, chain_key),
        SIGNED_SEAL_FILE_LEN => read_signed_seal(&bytes, chain_key),
        _ => SealState::Invalid,
    }
}

/// Reads a seal written without installation provenance.
fn read_unsigned_seal(bytes: &[u8], chain_key: &ChainKey) -> SealState {
    let Ok(raw) = <[u8; SEAL_FILE_LEN]>::try_from(bytes) else {
        return SealState::Invalid;
    };

    let mut count_bytes = [0u8; SEAL_COUNT_LEN];
    count_bytes.copy_from_slice(&raw[..SEAL_COUNT_LEN]);
    let count = u64::from_le_bytes(count_bytes);

    let mut segment_start_hash = [0u8; SEAL_LINK_LEN];
    segment_start_hash.copy_from_slice(&raw[SEAL_COUNT_LEN..SEAL_COUNT_LEN + SEAL_LINK_LEN]);

    let mut stored_mac = [0u8; SEAL_MAC_LEN];
    stored_mac.copy_from_slice(&raw[SEAL_COUNT_LEN + SEAL_LINK_LEN..]);

    let expected_mac = seal_mac(chain_key, count, &segment_start_hash);
    if bool::from(expected_mac.ct_eq(&stored_mac)) {
        SealState::Valid {
            entry_count: count,
            segment_start_hash,
            provenance: None,
        }
    } else {
        SealState::Invalid
    }
}

/// Reads a seal that carries installation provenance.
fn read_signed_seal(bytes: &[u8], chain_key: &ChainKey) -> SealState {
    let Ok(raw) = <[u8; SIGNED_SEAL_FILE_LEN]>::try_from(bytes) else {
        return SealState::Invalid;
    };

    let mut cursor = 0;
    let mut take = |len: usize| -> &[u8] {
        let start = cursor;
        cursor += len;
        raw.get(start..cursor).unwrap_or(&[])
    };

    let mut count_bytes = [0u8; SEAL_COUNT_LEN];
    count_bytes.copy_from_slice(take(SEAL_COUNT_LEN));
    let count = u64::from_le_bytes(count_bytes);

    let mut segment_start_hash = [0u8; SEAL_LINK_LEN];
    segment_start_hash.copy_from_slice(take(SEAL_LINK_LEN));

    let mut terminal_hash = [0u8; SEAL_LINK_LEN];
    terminal_hash.copy_from_slice(take(SEAL_LINK_LEN));

    let mut key_id = [0u8; KEY_ID_LEN];
    key_id.copy_from_slice(take(KEY_ID_LEN));

    let mut signature = [0u8; TIP_SIGNATURE_LEN];
    signature.copy_from_slice(take(TIP_SIGNATURE_LEN));

    let mut stored_mac = [0u8; SEAL_MAC_LEN];
    stored_mac.copy_from_slice(take(SEAL_MAC_LEN));

    let provenance = TipProvenance {
        key_id,
        terminal_hash,
        signature,
    };

    let expected_mac = signed_seal_mac(chain_key, count, &segment_start_hash, &provenance);
    if bool::from(expected_mac.ct_eq(&stored_mac)) {
        SealState::Valid {
            entry_count: count,
            segment_start_hash,
            provenance: Some(provenance),
        }
    } else {
        SealState::Invalid
    }
}

/// Test-only failure injection for [`write_seal`]'s individual stages.
///
/// WHY a seam rather than manipulating the filesystem from outside: the
/// stages of this two-file commit have different recovery properties, and on
/// POSIX they cannot be told apart from outside the function. `File::create`
/// and `fs::rename` share one precondition — write permission on the
/// directory — so a permissions-based injection that reaches the rename has
/// already blocked the create, and the only external way to fail the rename
/// alone is to replace its target with a directory, which destroys the very
/// stale seal whose survival is the property worth testing. `write_all` and
/// `sync_all` have no external trigger at all short of filling the device.
///
/// The selector is thread-local, so tests running concurrently cannot arm
/// each other's failures, and it is one-shot, so an armed stage cannot leak
/// into a later call. The whole module compiles out of any build that is not
/// `cfg(test)`.
#[cfg(test)]
pub(super) mod inject {
    use std::cell::Cell;

    /// A fallible stage of [`super::write_seal`].
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(crate) enum SealStage {
        /// Creating the `.tmp` sibling.
        Create,
        /// Writing the payload into it.
        Write,
        /// Flushing that payload to the device.
        Sync,
        /// Renaming the `.tmp` over the live seal.
        Rename,
        /// Making the rename itself durable.
        DirSync,
    }

    thread_local! {
        static FAIL_AT: Cell<Option<SealStage>> = const { Cell::new(None) };
    }

    /// Arms a one-shot failure at `stage` on this thread.
    pub(crate) fn fail_at(stage: SealStage) {
        FAIL_AT.with(|cell| cell.set(Some(stage)));
    }

    /// Disarms any pending injection on this thread.
    pub(crate) fn clear() {
        FAIL_AT.with(|cell| cell.set(None));
    }

    /// Consumes a pending injection when it names `stage`.
    pub(crate) fn should_fail(stage: SealStage) -> bool {
        FAIL_AT.with(|cell| {
            let armed = cell.get() == Some(stage);
            if armed {
                cell.set(None);
            }
            armed
        })
    }
}

/// Fails with an ordinary I/O error when this thread has armed `stage`.
#[cfg(test)]
fn injected(stage: inject::SealStage, path: &Path) -> Result<(), TamperLogError> {
    if inject::should_fail(stage) {
        return Err::<(), std::io::Error>(std::io::Error::other(format!(
            "injected seal-stage failure at {stage:?}"
        )))
        .context(IoSnafu {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// Fsyncs the directory holding `path`, making the rename that placed the
/// file there durable rather than merely visible.
///
/// WHY this is not redundant with the seal file's own `sync_all`: they cover
/// two different objects. `sync_all` on the temporary file makes its
/// *contents* durable. The rename that gives those contents their name is a
/// modification of the *directory*, and on ext4, XFS and btrfs a crash
/// between the rename and a directory fsync can lose the name while keeping
/// the data. The seal would then read as stale — or absent — after a power
/// loss that the append it describes survived, which is the log-ahead state
/// this module exists to make recoverable, reached by a route that leaves no
/// trace of how.
///
/// The cost is one extra fsync per append, on top of the seal file's own. For
/// a log whose entire purpose is that its record survives the event it
/// records, that is the correct side to spend on.
fn sync_parent_dir(path: &Path) -> Result<(), TamperLogError> {
    // WHY unix-gated: opening a directory as a file and fsyncing the handle
    // is a POSIX guarantee, not a portable one — Windows refuses the open
    // outright. The fleet is Unix; elsewhere the rename's durability falls
    // back to whatever the platform provides, which is what the code did
    // everywhere before this.
    #[cfg(unix)]
    {
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        // An empty parent means a bare relative filename, whose directory is
        // the process working directory.
        let dir = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        File::open(dir)
            .and_then(|handle| handle.sync_all())
            .context(IoSnafu {
                path: dir.to_path_buf(),
            })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Writes the seal sidecar for `log_path`, authenticating `entry_count` and
/// `segment_start_hash` together.
///
/// Writes to a `.tmp` sibling and renames into place so a concurrent reader
/// (or a crash mid-write) never observes a partially-written seal.
///
/// # Durability boundary
///
/// The commit is four ordered steps, and what survives a crash depends on
/// which one it interrupts:
///
/// - **create / write** — the live seal is untouched, so the log may end up
///   ahead of a still-valid seal. That is the recoverable `Unsealed` state:
///   the next open verifies the extra entries and reseals.
/// - **sync** — same, with the added case that the `.tmp` may hold
///   unflushed bytes. It is never read: only the rename publishes it, and a
///   later call truncates it.
/// - **rename** — atomic by POSIX guarantee. A reader sees either the old
///   seal or the new one, never a splice of the two.
/// - **directory sync** — see [`sync_parent_dir`]. Without it the rename can
///   be visible now and lost after a power cut.
///
/// Every one of those leaves either the previous valid seal or the new one,
/// which is why a log ahead of a *validly authenticated* seal is treated as
/// recoverable while an absent or unauthenticated seal stays fail-closed: an
/// interrupted commit cannot produce the latter, but an attacker can.
pub(super) fn write_seal(
    log_path: &Path,
    chain_key: &ChainKey,
    entry_count: u64,
    segment_start_hash: &[u8; 32],
    provenance: Option<&TipProvenance>,
) -> Result<(), TamperLogError> {
    let target = seal_path(log_path);
    let mut tmp_name = target.clone().into_os_string();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);

    let payload = match provenance {
        None => {
            let mac = seal_mac(chain_key, entry_count, segment_start_hash);
            let mut payload = Vec::with_capacity(SEAL_FILE_LEN);
            payload.extend_from_slice(&entry_count.to_le_bytes());
            payload.extend_from_slice(segment_start_hash);
            payload.extend_from_slice(&mac);
            payload
        }
        Some(tip) => {
            let mac = signed_seal_mac(chain_key, entry_count, segment_start_hash, tip);
            let mut payload = Vec::with_capacity(SIGNED_SEAL_FILE_LEN);
            payload.extend_from_slice(&entry_count.to_le_bytes());
            payload.extend_from_slice(segment_start_hash);
            payload.extend_from_slice(&tip.terminal_hash);
            payload.extend_from_slice(&tip.key_id);
            payload.extend_from_slice(&tip.signature);
            payload.extend_from_slice(&mac);
            payload
        }
    };

    {
        #[cfg(test)]
        injected(inject::SealStage::Create, &tmp)?;
        let mut file = File::create(&tmp).context(IoSnafu { path: tmp.clone() })?;

        #[cfg(test)]
        injected(inject::SealStage::Write, &tmp)?;
        file.write_all(&payload)
            .context(IoSnafu { path: tmp.clone() })?;

        #[cfg(test)]
        injected(inject::SealStage::Sync, &tmp)?;
        file.sync_all().context(IoSnafu { path: tmp.clone() })?;
    }

    #[cfg(test)]
    injected(inject::SealStage::Rename, &target)?;
    fs::rename(&tmp, &target).context(IoSnafu {
        path: target.clone(),
    })?;

    #[cfg(test)]
    injected(inject::SealStage::DirSync, &target)?;
    sync_parent_dir(&target)
}

/// Renames the seal sidecar alongside a rotated log file.
///
/// Best-effort on the source side: a log rotated before its first append
/// ever refreshed a seal has none to preserve, which is not an error.
pub(super) fn rename_seal(from_log: &Path, to_log: &Path) -> Result<(), TamperLogError> {
    let from = seal_path(from_log);
    if !from.exists() {
        return Ok(());
    }
    let to = seal_path(to_log);
    fs::rename(&from, &to).context(IoSnafu { path: to })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    fn key(byte: u8) -> ChainKey {
        ChainKey::from_bytes([byte; CHAIN_KEY_LEN])
    }

    fn link(byte: u8) -> [u8; SEAL_LINK_LEN] {
        [byte; SEAL_LINK_LEN]
    }

    #[test]
    fn chain_key_from_bytes_round_trips() {
        let bytes = [0x42; CHAIN_KEY_LEN];
        let k = ChainKey::from_bytes(bytes);
        assert_eq!(k.as_bytes(), &bytes);
    }

    #[test]
    fn chain_key_debug_is_redacted() {
        let k = key(0x11);
        assert_eq!(format!("{k:?}"), "ChainKey([REDACTED])");
    }

    #[test]
    fn chain_key_eq_compares_by_value() {
        let a = ChainKey::from_bytes([0xAA; CHAIN_KEY_LEN]);
        let b = ChainKey::from_bytes([0xAA; CHAIN_KEY_LEN]);
        let c = ChainKey::from_bytes([0xBB; CHAIN_KEY_LEN]);
        assert_eq!(a, b, "keys built from identical bytes must compare equal");
        assert_ne!(
            a, c,
            "keys built from different bytes must not compare equal"
        );
    }

    #[test]
    fn genesis_hash_is_deterministic() {
        let k = key(0x01);
        let first_call = genesis_hash(&k);
        let second_call = genesis_hash(&k);
        assert_eq!(
            first_call, second_call,
            "genesis_hash must be a pure function of the key"
        );
    }

    #[test]
    fn genesis_hash_differs_per_key() {
        assert_ne!(genesis_hash(&key(0x01)), genesis_hash(&key(0x02)));
    }

    #[test]
    fn seal_path_appends_dot_seal() {
        let p = seal_path(Path::new("/tmp/foo/tamper.log"));
        assert_eq!(p, Path::new("/tmp/foo/tamper.log.seal"));
    }

    #[test]
    fn write_then_read_seal_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let k = key(0x55);
        let start = link(0x01);

        write_seal(&log_path, &k, 7, &start, None).unwrap();
        let state = read_seal(&log_path, &k);
        assert_eq!(
            state,
            SealState::Valid {
                entry_count: 7,
                segment_start_hash: start,
                provenance: None,
            }
        );
    }

    #[test]
    fn write_then_read_seal_round_trips_segment_start_hash() {
        // WHY: entry_count round-tripping alone wouldn't catch the link
        // field being dropped, truncated, or silently zeroed — assert it
        // explicitly rather than just checking equality against the whole
        // struct (which the prior test already does).
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("linked.log");
        let k = key(0x59);
        let start = link(0xAB);

        write_seal(&log_path, &k, 2, &start, None).unwrap();
        let state = read_seal(&log_path, &k);
        match state {
            SealState::Valid {
                segment_start_hash, ..
            } => assert_eq!(segment_start_hash, start),
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn read_seal_missing_file_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("no-seal.log");
        let state = read_seal(&log_path, &key(0x01));
        assert_eq!(state, SealState::Absent);
    }

    #[test]
    fn read_seal_wrong_key_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        write_seal(&log_path, &key(0xAA), 3, &link(0x01), None).unwrap();
        let state = read_seal(&log_path, &key(0xBB));
        assert_eq!(state, SealState::Invalid);
    }

    #[test]
    fn read_seal_tampered_mac_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let k = key(0x77);

        write_seal(&log_path, &k, 4, &link(0x01), None).unwrap();
        let mut bytes = fs::read(seal_path(&log_path)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(seal_path(&log_path), &bytes).unwrap();

        let state = read_seal(&log_path, &k);
        assert_eq!(state, SealState::Invalid);
    }

    #[test]
    fn read_seal_tampered_count_is_invalid() {
        // WHY: the MAC covers the count field itself — an attacker who
        // edits just the count (without the key) cannot produce a
        // matching MAC, so a hand-edited count is caught too, not just a
        // hand-edited MAC.
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let k = key(0x66);

        write_seal(&log_path, &k, 4, &link(0x01), None).unwrap();
        let mut bytes = fs::read(seal_path(&log_path)).unwrap();
        bytes[0] = 99; // low byte of the little-endian count
        fs::write(seal_path(&log_path), &bytes).unwrap();

        let state = read_seal(&log_path, &k);
        assert_eq!(state, SealState::Invalid);
    }

    #[test]
    fn read_seal_tampered_segment_start_hash_is_invalid() {
        // WHY (#211): the MAC must cover segment_start_hash too — an
        // attacker who hand-edits only the link field (leaving count and
        // MAC untouched) must still be caught, or a deleted segment's
        // successor could be re-pointed at an arbitrary start hash without
        // the key.
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let k = key(0x68);

        write_seal(&log_path, &k, 4, &link(0x01), None).unwrap();
        let mut bytes = fs::read(seal_path(&log_path)).unwrap();
        bytes[SEAL_COUNT_LEN] ^= 0xFF; // first byte of segment_start_hash
        fs::write(seal_path(&log_path), &bytes).unwrap();

        let state = read_seal(&log_path, &k);
        assert_eq!(state, SealState::Invalid);
    }

    #[test]
    fn rename_seal_moves_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.log");
        let to = dir.path().join("a.1.log");
        write_seal(&from, &key(0x01), 1, &link(0x02), None).unwrap();

        rename_seal(&from, &to).unwrap();

        assert!(!seal_path(&from).exists());
        assert!(seal_path(&to).exists());
    }

    #[test]
    fn rename_seal_is_a_no_op_without_a_source_seal() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.log");
        let to = dir.path().join("a.1.log");

        rename_seal(&from, &to).unwrap();

        assert!(!seal_path(&to).exists());
    }
}
