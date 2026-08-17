//! Exclusive single-writer lock for [`super::TamperLog`]; split out to keep
//! the parent file under the RUST/file-too-long 800-line threshold.
//!
//! The lock lives at a fixed sidecar path (`{log}.lock`) rather than on the
//! log's own data-file descriptor, so it survives rotation untouched:
//! [`super::TamperLog::rotate`] renames the data file and reopens a fresh one
//! at the same logical path, but never touches this sidecar, so a lock held
//! across `TamperLog::open` needs no re-acquisition mid-rotation — closing
//! the race a self-relocking design would have in the window between the
//! rename and the reopen of a brand-new, as-yet-unlocked file descriptor.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use snafu::ResultExt;

use super::{IoSnafu, LockedSnafu, TamperLogError};

/// Derives the sidecar lock path for a log file: `{path}.lock`.
pub(super) fn lock_path(log_path: &Path) -> PathBuf {
    let mut name = log_path.as_os_str().to_owned();
    name.push(".lock");
    PathBuf::from(name)
}

/// Acquires the exclusive single-writer lock for `log_path`.
///
/// Fails fast rather than blocking: `TamperLog::open` may be called from a
/// short-lived process (a CLI invocation, a health check) that has no
/// business waiting on a long-lived daemon's writer, so a second writer
/// loses the race outright instead of queueing.
///
/// The returned [`File`] must be kept alive for as long as the lock should
/// be held — the OS releases an advisory lock the moment every descriptor
/// referencing it closes, including on process exit or crash, so a writer
/// that dies mid-append simply frees the lock for the next opener, which
/// then recovers state from whatever was durably written (see
/// [`super::TamperLog::open_with_config`] and akroasis#285).
///
/// # Errors
///
/// Returns [`TamperLogError::Locked`] if another writer already holds the
/// lock. Returns [`TamperLogError::Io`] if the lock file cannot be created.
pub(super) fn acquire(log_path: &Path) -> Result<File, TamperLogError> {
    let path = lock_path(log_path);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .context(IoSnafu { path: path.clone() })?;

    file.try_lock_exclusive().map_err(|_| {
        LockedSnafu {
            path: log_path.to_owned(),
        }
        .build()
    })?;

    Ok(file)
}
