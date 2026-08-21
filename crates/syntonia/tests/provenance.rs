//! Enforces `provenance.toml` against the tree it describes.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
struct Ledger {
    upstream: Vec<Upstream>,
}

#[derive(Deserialize)]
struct Upstream {
    name: String,
    license: String,
    #[serde(default)]
    derived: Vec<String>,
    #[serde(default)]
    interop: Vec<String>,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ledger() -> Ledger {
    let path = crate_root().join("provenance.toml");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()))
}

/// Every `.rs` file under `src/`, as crate-relative paths with `/` separators.
fn source_files() -> Vec<String> {
    let src = crate_root().join("src");
    let mut found = Vec::new();
    walk(&src, &mut found);
    found
        .iter()
        .filter_map(|p| p.strip_prefix(crate_root()).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn mentions(path: &str, needle: &str) -> bool {
    let full = crate_root().join(path);
    fs::read_to_string(&full)
        .map(|text| text.to_lowercase().contains(&needle.to_lowercase()))
        .unwrap_or(false)
}

/// The case the ledger exists to catch: a file that names an upstream and has
/// been classified as neither derived nor interop.
///
/// A new derived file is added by someone who knows why it is fine, and the
/// knowing does not survive them. Failing here is what forces the answer into
/// the ledger while it is still known.
#[test]
fn every_file_naming_an_upstream_is_classified() {
    for upstream in ledger().upstream {
        let classified: BTreeSet<&str> = upstream
            .derived
            .iter()
            .chain(upstream.interop.iter())
            .map(String::as_str)
            .collect();

        let unclassified: Vec<String> = source_files()
            .into_iter()
            .filter(|path| mentions(path, &upstream.name))
            .filter(|path| !classified.contains(path.as_str()))
            .collect();

        assert!(
            unclassified.is_empty(),
            "these files name '{}' but provenance.toml classifies them as neither \
             derived nor interop: {unclassified:?}\n\
             Add each to `derived` if its CONTENT came from the upstream (the \
             licence obligation reaches it), or to `interop` if it only reads or \
             writes the upstream's file formats.",
            upstream.name
        );
    }
}

/// The other direction: a ledger that lists files which no longer mention the
/// upstream is describing a tree that no longer exists, and it will keep
/// passing the check above while doing so.
#[test]
fn every_classified_path_exists_and_still_names_its_upstream() {
    for upstream in ledger().upstream {
        for path in upstream.derived.iter().chain(upstream.interop.iter()) {
            let full = crate_root().join(path);
            assert!(
                full.exists(),
                "provenance.toml lists '{path}' for upstream '{}', but that file \
                 does not exist",
                upstream.name
            );
            assert!(
                mentions(path, &upstream.name),
                "provenance.toml lists '{path}' for upstream '{}', but that file \
                 no longer mentions it — remove the row, or restore the \
                 attribution the row is asserting",
                upstream.name
            );
        }
    }
}

/// The licence is stated once and the derived files point at it.
///
/// WHY as a test rather than a convention: the label was wrong here in the
/// permissive direction, and it was wrong at four sites at once. One copy can
/// be corrected; four copies is a place where three stay wrong and nothing
/// reports the disagreement. Requiring exactly one occurrence is what makes
/// "point at the module header" enforceable instead of aspirational.
#[test]
fn each_upstream_licence_is_named_exactly_once_and_in_a_derived_file() {
    for upstream in ledger().upstream {
        let naming: Vec<String> = source_files()
            .into_iter()
            .filter(|path| mentions(path, &upstream.license))
            .collect();

        assert_eq!(
            naming.len(),
            1,
            "the licence '{}' for upstream '{}' must be stated in exactly one \
             source file, with the other derived files pointing at it. Found it \
             in: {naming:?}",
            upstream.license,
            upstream.name
        );

        let sole = naming.first().expect("length asserted as 1 above");
        assert!(
            upstream.derived.contains(sole),
            "the licence '{}' is stated in '{sole}', which provenance.toml does \
             not list as derived from '{}' — the statement belongs with the code \
             it governs",
            upstream.license,
            upstream.name
        );
    }
}
