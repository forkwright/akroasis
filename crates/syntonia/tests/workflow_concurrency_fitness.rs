//! Fitness test: the merge-gate workflows must not carry a concurrency group
//! that collides across distinct push-to-main commits.
//!
//! WHY(#375): `.github/workflows/gate-attestation.yml`'s prior caller-level
//! `concurrency:` block and `.github/workflows/security.yml`'s block both
//! used `${{ github.event.pull_request.number || github.ref }}` -- constant
//! for `main` on a `push` event (`pull_request.number` is empty), so any two
//! main-push commits landing inside the `cancel-in-progress` window collided
//! and the loser's job reported `cancelled`, not red, silently dropping the
//! check for that commit. Confirmed live: `gh api
//! repos/forkwright/akroasis/commits/<sha>/check-runs` on `60f97de` and
//! `b5c8071` shows `cargo audit`/`cargo deny` `cancelled` on both.
//! `gate-attestation.yml` is fixed by removing its caller-level block
//! entirely (the reusable `hybrid-gate.yml` already supplies a sha-on-push
//! one, and a caller-level duplicate self-cancels that shared group);
//! `security.yml` has no reusable-workflow fallback to catch the same
//! defect, so it is fixed by sha-keying its own group on `push` instead.
//!
//! This asserts the shape stays fixed rather than merely documenting it in a
//! comment: before this test, "a second caller-level concurrency block
//! creeping back into `gate-attestation.yml`" or `security.yml`'s key
//! drifting off the sha-on-push form had no mechanical check -- only a
//! comment nobody re-reads under review pressure.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "integration test — panics are the correct failure mode"
)]

use std::path::{Path, PathBuf};

/// Workspace root, derived from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<name> is two levels below the workspace root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The value of the `group:` line nested two spaces under a top-level
/// `concurrency:` key, or `None` if the file has no top-level `concurrency:`
/// block at all.
///
/// WHY line-based, not a YAML parser: no YAML crate is a workspace
/// dependency, and both files' `concurrency:` blocks are a fixed two-line
/// shape -- adding a parser dependency for this would be more surface than
/// the thing it checks.
fn top_level_concurrency_group(text: &str) -> Option<String> {
    // WHY trim_end on the key line: a trailing space after `concurrency:`
    // (invisible in a diff, easy to leave behind editing this by hand) must
    // not make the guard blind to a real block -- fail-open on formatting
    // noise is exactly the shape of guard that misses its own regression.
    let mut lines = text.lines();
    lines.find(|l| l.trim_end() == "concurrency:")?;
    for line in lines {
        if let Some(rest) = line.strip_prefix("  group:") {
            return Some(rest.trim().to_string());
        }
        // WHY stop at the next top-level key: a `group:` line belonging to a
        // later, unrelated top-level block must never be picked up as this
        // block's value.
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
    }
    None
}

#[test]
fn gate_attestation_carries_no_caller_level_concurrency_block() {
    let text = read(".github/workflows/gate-attestation.yml");
    assert!(
        top_level_concurrency_group(&text).is_none(),
        "gate-attestation.yml must not declare its own concurrency: block -- \
         hybrid-gate.yml already declares a sha-on-push one, and a \
         caller-level duplicate self-cancels the shared group (see the \
         file's own WHY comment above the `on:` block)"
    );
}

#[test]
fn security_yml_concurrency_group_is_sha_keyed_on_push() {
    let text = read(".github/workflows/security.yml");
    let group = top_level_concurrency_group(&text)
        .expect("security.yml must declare a top-level concurrency: block");
    assert_eq!(
        group,
        "${{ github.workflow }}-${{ github.event_name == 'push' && github.sha || github.ref }}",
        "security.yml's concurrency group must be sha-keyed on push (matching \
         hybrid-gate.yml's own key) -- a ref-only key (e.g. \
         `github.event.pull_request.number || github.ref`) is constant across \
         every push to main and collides, cancelling one of two commits' \
         cargo audit/cargo deny runs instead of running both. Confirmed live \
         on 60f97de and b5c8071: both showed cargo audit/cargo deny \
         cancelled under the old key."
    );
}

#[test]
fn extraction_tolerates_a_trailing_space_on_the_key_line() {
    // WHY this test exists: the extractor originally matched `concurrency:`
    // by exact equality, so a trailing space on that line (invisible in a
    // diff) made it silently report "no block" for a file that has one --
    // the guard blind to its own regression. Pins the `trim_end` fix above.
    let text =
        "on:\n  push: {}\n\nconcurrency: \n  group: some-value\n  cancel-in-progress: true\n";
    assert_eq!(
        top_level_concurrency_group(text).as_deref(),
        Some("some-value"),
        "a trailing space after `concurrency:` must not blind the guard to a real block"
    );
}
