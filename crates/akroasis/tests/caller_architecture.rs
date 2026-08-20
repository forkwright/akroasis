//! Semantic architecture guard for the application-owned caller resolver.

#![expect(
    clippy::expect_used,
    reason = "workspace architecture fixtures fail immediately on unreadable Rust source"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};

const RESTRICTED: [&str; 6] = [
    "ApplicationCallerResolver",
    "AuthorityClaims",
    "AuthorityClaimsBuilder",
    "AuthorityDecision",
    "CallerAuthority",
    "CallerResolver",
];

#[derive(Default)]
struct RestrictedUse {
    identifiers: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for RestrictedUse {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        for segment in &path.segments {
            let identifier = segment.ident.to_string();
            if RESTRICTED.contains(&identifier.as_str()) {
                self.identifiers.insert(identifier);
            }
        }
        visit::visit_path(self, path);
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        for token in node
            .tokens
            .to_string()
            .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        {
            if RESTRICTED.contains(&token) {
                self.identifiers.insert(token.to_owned());
            }
        }
        visit::visit_macro(self, node);
    }

    fn visit_use_tree(&mut self, node: &'ast syn::UseTree) {
        let identifier = match node {
            syn::UseTree::Name(name) => Some(&name.ident),
            syn::UseTree::Rename(rename) => Some(&rename.ident),
            syn::UseTree::Path(path) => Some(&path.ident),
            syn::UseTree::Glob(_) | syn::UseTree::Group(_) => None,
        };
        if let Some(identifier) = identifier {
            let identifier = identifier.to_string();
            if RESTRICTED.contains(&identifier.as_str()) {
                self.identifiers.insert(identifier);
            }
        }
        visit::visit_use_tree(self, node);
    }
}

fn restricted_identifiers(source: &str) -> BTreeSet<String> {
    let syntax = syn::parse_file(source).expect("parse Rust source fixture");
    let mut visitor = RestrictedUse::default();
    visitor.visit_file(&syntax);
    visitor.identifiers
}

#[test]
fn caller_construction_aliases_are_detected() {
    let source = r#"
        use koinon::{
            AuthorityClaimsBuilder as Claims,
            CallerAuthority as Authority,
            CallerResolver as Resolver,
        };

        fn bypass<A: Authority>(authority: A) {
            let _claims = Claims::new();
            let _resolver = Resolver::<A>::try_new(1, authority);
        }
    "#;
    assert_eq!(
        restricted_identifiers(source),
        BTreeSet::from([
            "AuthorityClaimsBuilder".to_owned(),
            "CallerAuthority".to_owned(),
            "CallerResolver".to_owned(),
        ]),
        "renamed imports and turbofish calls must not bypass the boundary"
    );
}

#[test]
fn caller_construction_stays_at_the_application_boundary() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    collect_rust_files(&workspace.join("crates"), &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&workspace)
            .expect("workspace-relative source path");
        if is_allowed(relative) {
            continue;
        }
        let source = fs::read_to_string(&file).expect("read Rust source");
        let identifiers = restricted_identifiers(&source);
        if !identifiers.is_empty() {
            violations.push(format!(
                "{} references application-only caller construction: {:?}",
                relative.display(),
                identifiers
            ));
        }
    }

    assert_eq!(
        violations,
        Vec::<String>::new(),
        "domain crates must consume ValidatedCaller, never mint principals"
    );
}

fn is_allowed(path: &Path) -> bool {
    path == Path::new("crates/akroasis/src/caller.rs")
        || path == Path::new("crates/akroasis/src/caller_tests.rs")
        || path == Path::new("crates/akroasis/tests/caller_contract.rs")
        || path == Path::new("crates/akroasis/tests/caller_recovery.rs")
        || path == Path::new("crates/akroasis/tests/caller_receipt_wire.rs")
        || path == Path::new("crates/koinon/src/caller.rs")
        || path == Path::new("crates/koinon/src/lib.rs")
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path).expect("read workspace source directory") {
        let path = entry.expect("workspace source entry").path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
            files.push(path);
        }
    }
}
