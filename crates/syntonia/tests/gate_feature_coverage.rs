//! Fitness test: every optional feature this crate declares must be compiled
//! and exercised by the gate.
//!
//! WHY(#335): `hardware-serial` was declared, off by default, and enabled by no
//! gate stage. Every fmt/check/clippy/nextest run — local and hosted — skipped
//! `baofeng::{protocol,detect,variant}` and ran none of the 202 tests in
//! `protocol_tests.rs`. Nothing reported a gap, because a `#[cfg]`-disabled
//! module and a correct one render identically to a green stage. This test
//! asserts the coverage the stage list cannot assert about itself, for every
//! feature rather than only the one that was missed.

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

/// Feature names declared in this crate's `[features]` table, excluding
/// `default` (which selects among the others rather than adding a code path).
fn declared_features() -> Vec<String> {
    let manifest: toml::Table = read("crates/syntonia/Cargo.toml")
        .parse()
        .expect("syntonia manifest should parse as TOML");
    let features = manifest
        .get("features")
        .and_then(toml::Value::as_table)
        .expect("syntonia declares a [features] table");
    features
        .keys()
        .filter(|k| k.as_str() != "default")
        .cloned()
        .collect()
}

/// The `cmd` string of one `.kanon-ci.toml` stage.
fn stage_cmd(stage: &str) -> String {
    let ci: toml::Table = read(".kanon-ci.toml")
        .parse()
        .expect(".kanon-ci.toml should parse as TOML");
    ci.get("stages")
        .and_then(toml::Value::as_table)
        .and_then(|s| s.get(stage))
        .and_then(toml::Value::as_table)
        .and_then(|s| s.get("cmd"))
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("`.kanon-ci.toml` declares a `{stage}` stage with a cmd"))
        .to_owned()
}

/// The value of a `<key>: "<value>"` input in the gate workflow.
fn workflow_input(key: &str) -> String {
    let yaml = read(".github/workflows/gate-attestation.yml");
    let needle = format!("{key}: \"");
    let line = yaml
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with(&needle))
        .unwrap_or_else(|| panic!("gate-attestation.yml sets `{key}`"));
    line[needle.len()..]
        .strip_suffix('"')
        .expect("workflow input value is a closed double-quoted scalar")
        .to_owned()
}

/// A cargo invocation enables `feature` if it names it package-qualified or
/// turns on every feature wholesale.
fn enables(cmd: &str, feature: &str) -> bool {
    cmd.contains("--all-features") || cmd.contains(&format!("syntonia/{feature}"))
}

#[test]
fn every_declared_feature_is_compiled_and_tested_by_the_gate() {
    let features = declared_features();
    // Anti-vacuity: an empty feature list would satisfy the loop below trivially.
    assert!(
        !features.is_empty(),
        "syntonia declares no features — this test would pass vacuously"
    );

    let check = stage_cmd("cargo check");
    let nextest = stage_cmd("cargo nextest");

    for feature in &features {
        assert!(
            enables(&check, feature),
            "the `cargo check` stage never compiles `syntonia/{feature}`; its code is \
             invisible to the gate. Stage cmd: {check}"
        );
        assert!(
            enables(&nextest, feature),
            "the `cargo nextest` stage never runs the tests behind `syntonia/{feature}`. \
             Stage cmd: {nextest}"
        );
    }
}

#[test]
fn the_gate_workflow_mirrors_the_local_stage_commands() {
    // WHY(#335): gate-attestation.yml states that its command strings mirror
    // .kanon-ci.toml verbatim, so that a Gate-Passed trailer and a green
    // full-gate-build attest identical stages. Drift between them makes one of
    // the two a false green. The claim is checked here rather than trusted.
    for (stage, input) in [
        ("cargo fmt", "fmt_cmd"),
        ("cargo check", "check_cmd"),
        ("cargo clippy", "clippy_cmd"),
        ("cargo nextest", "nextest_cmd"),
    ] {
        assert_eq!(
            stage_cmd(stage),
            workflow_input(input),
            "`.kanon-ci.toml` stage `{stage}` and gate-attestation.yml `{input}` have drifted; \
             the local gate and the hosted gate would attest different work"
        );
    }
}
