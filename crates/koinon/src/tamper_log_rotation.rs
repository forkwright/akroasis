//! Rotation path helpers for [`super`]; split out to keep the parent file under
//! the RUST/file-too-long 800-line threshold.

use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::{IoSnafu, TamperLogError};

/// Returns the stem of a log path, e.g. `"audit"` FROM `"audit.log"`.
pub(super) fn log_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Builds `{dir}/{stem}.{n}.log`.
pub(super) fn rotation_path(path: &Path, n: u32) -> PathBuf {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = log_stem(path);
    dir.join(format!("{stem}.{n}.log"))
}

/// Parses `{stem}.{n}.log` → `Some(n)`, returns `None` otherwise.
pub(super) fn parse_rotation_number(name: &str, stem: &str) -> Option<u32> {
    let prefix = format!("{stem}.");
    let suffix = ".log";
    let inner = name.strip_prefix(&prefix)?.strip_suffix(suffix)?;
    inner.parse::<u32>().ok()
}

/// Scans sibling files to find the next rotation number.
///
/// # Errors
///
/// Returns [`TamperLogError::Io`] if the log's directory cannot be scanned.
pub(super) fn next_rotation_number(path: &Path) -> Result<u32, TamperLogError> {
    let stem = log_stem(path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));

    // WHY: swallowing this error returned rotation number 1, and the caller
    // renames the live log onto that path — so an unreadable directory silently
    // overwrote <stem>.1, destroying an already-sealed segment of a
    // tamper-evident log. Refuse to rotate instead of guessing.
    let entries = std::fs::read_dir(dir).context(IoSnafu { path: dir })?;

    let mut max_n: u32 = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(n) = parse_rotation_number(&name, &stem)
            && n > max_n
        {
            max_n = n;
        }
    }
    // WARNING: saturating so an exhausted rotation space reuses the last slot
    // rather than wrapping to 0 and overwriting the oldest segment.
    Ok(max_n.saturating_add(1))
}
