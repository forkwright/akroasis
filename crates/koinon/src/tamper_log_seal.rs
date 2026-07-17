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
//! "was made to look like it ends here." The MAC covers the count alone —
//! content integrity for every entry up to that count is already the hash
//! chain's job, not the seal's.

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

/// Length in bytes of the on-disk seal MAC.
const SEAL_MAC_LEN: usize = 32;

/// Total on-disk seal file length: count || MAC.
const SEAL_FILE_LEN: usize = SEAL_COUNT_LEN + SEAL_MAC_LEN;

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

/// Result of reading and authenticating a sidecar seal file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SealState {
    /// No seal file exists at the sidecar path.
    Absent,
    /// A seal file exists but its MAC does not authenticate under
    /// `chain_key` — corrupted, forged, or written under a different key.
    Invalid,
    /// A seal file exists and its MAC authenticates; carries the sealed
    /// entry count.
    Valid(u64),
}

/// Derives the sidecar seal path for a log file: `{path}.seal`.
pub(super) fn seal_path(log_path: &Path) -> PathBuf {
    let mut name = log_path.as_os_str().to_owned();
    name.push(".seal");
    PathBuf::from(name)
}

/// Computes the authenticated MAC over `entry_count`, domain-separated
/// from entry and genesis hashing.
fn seal_mac(chain_key: &ChainKey, entry_count: u64) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(chain_key.as_bytes());
    hasher.update(SEAL_DOMAIN);
    hasher.update(&entry_count.to_le_bytes());
    hasher.finalize().into()
}

/// Reads and authenticates the seal sidecar for `log_path`.
pub(super) fn read_seal(log_path: &Path, chain_key: &ChainKey) -> SealState {
    let Ok(bytes) = fs::read(seal_path(log_path)) else {
        return SealState::Absent;
    };
    let Ok(raw) = <[u8; SEAL_FILE_LEN]>::try_from(bytes.as_slice()) else {
        return SealState::Invalid;
    };

    let mut count_bytes = [0u8; SEAL_COUNT_LEN];
    count_bytes.copy_from_slice(&raw[..SEAL_COUNT_LEN]);
    let count = u64::from_le_bytes(count_bytes);

    let mut stored_mac = [0u8; SEAL_MAC_LEN];
    stored_mac.copy_from_slice(&raw[SEAL_COUNT_LEN..]);

    let expected_mac = seal_mac(chain_key, count);
    if bool::from(expected_mac.ct_eq(&stored_mac)) {
        SealState::Valid(count)
    } else {
        SealState::Invalid
    }
}

/// Writes the seal sidecar for `log_path`, authenticating `entry_count`.
///
/// Writes to a `.tmp` sibling and renames into place so a concurrent
/// reader (or a crash mid-write) never observes a partially-written seal.
pub(super) fn write_seal(
    log_path: &Path,
    chain_key: &ChainKey,
    entry_count: u64,
) -> Result<(), TamperLogError> {
    let target = seal_path(log_path);
    let mut tmp_name = target.clone().into_os_string();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);

    let mac = seal_mac(chain_key, entry_count);
    let mut payload = Vec::with_capacity(SEAL_FILE_LEN);
    payload.extend_from_slice(&entry_count.to_le_bytes());
    payload.extend_from_slice(&mac);

    {
        let mut file = File::create(&tmp).context(IoSnafu { path: tmp.clone() })?;
        file.write_all(&payload)
            .context(IoSnafu { path: tmp.clone() })?;
        file.sync_all().context(IoSnafu { path: tmp.clone() })?;
    }
    fs::rename(&tmp, &target).context(IoSnafu { path: target })?;

    Ok(())
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
    reason = "test code: panics and unwraps acceptable in assertions"
)]
mod tests {
    use super::*;

    fn key(byte: u8) -> ChainKey {
        ChainKey::from_bytes([byte; CHAIN_KEY_LEN])
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

        write_seal(&log_path, &k, 7).unwrap();
        let state = read_seal(&log_path, &k);
        assert_eq!(state, SealState::Valid(7));
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

        write_seal(&log_path, &key(0xAA), 3).unwrap();
        let state = read_seal(&log_path, &key(0xBB));
        assert_eq!(state, SealState::Invalid);
    }

    #[test]
    fn read_seal_tampered_mac_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("audit.log");
        let k = key(0x77);

        write_seal(&log_path, &k, 4).unwrap();
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

        write_seal(&log_path, &k, 4).unwrap();
        let mut bytes = fs::read(seal_path(&log_path)).unwrap();
        bytes[0] = 99; // low byte of the little-endian count
        fs::write(seal_path(&log_path), &bytes).unwrap();

        let state = read_seal(&log_path, &k);
        assert_eq!(state, SealState::Invalid);
    }

    #[test]
    fn rename_seal_moves_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a.log");
        let to = dir.path().join("a.1.log");
        write_seal(&from, &key(0x01), 1).unwrap();

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
