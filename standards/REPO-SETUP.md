# Repository setup

> What a well-configured forkwright project looks like and why. This is the setup reference -- not a rigid checklist but a set of principles with concrete defaults. Every deviation should be a deliberate decision, not an oversight.

---

## Guiding principles

**Single binary, zero ops.** A repo produces one artifact (a binary or a library). The fewer moving parts in deployment, the fewer things break. Prefer embedded over external, bundled over linked, static over dynamic.

**The compiler is the first reviewer.** Lints, type checks, and deny-level warnings catch bugs before any human or agent sees the code. Invest in compile-time guarantees over runtime checks. If the compiler can enforce it, it should.

**Convention over configuration.** Use the workspace defaults. Override only with justification. An agent or contributor reading any forkwright repo should find the same patterns in the same places. Predictability compounds.

**Own your dependencies.** Every dependency is a liability. Audit before adding. Pin unstable versions. Ban known-bad crates. The `deny.toml` is a policy document, not boilerplate.

**Ship with confidence.** Every merge to main should be releasable. CI gates enforce this -- format, lint, test, audit, scan. No manual "run the tests before pushing" discipline. The system enforces quality.

---

## Repository structure

A forkwright Rust project has a predictable layout. Not every project needs every item -- but when an item is present, it's in the standard location.

### Root files

| File | Purpose | When to omit |
|------|---------|-------------|
| `Cargo.toml` | Workspace root: version, edition, MSRV, lints, build profiles, shared deps | Never |
| `Cargo.lock` | Dependency lock | Omit for pure library crates (include for binaries) |
| `deny.toml` | Dependency policy: licenses, bans, advisories, sources | Never |
| `clippy.toml` | Workspace clippy configuration | Never |
| `rustfmt.toml` | Format configuration (default config is fine -- file signals "we format") | Never |
| `README.md` | What it is, quick start, configuration | Never |
| `CLAUDE.md` | Agent orientation: architecture, key types, common tasks, gotchas | Never |
| `SECURITY.md` | Vulnerability reporting (GitHub Security Advisories preferred) | Private repos with no external contributors |
| `.gitignore` | Build artifacts, secrets, editor files, instance data | Never |
| `flake.nix` | Nix dev shell (reproducible environment for all contributors + agents) | When Nix isn't used |
| `release-please-config.json` | Release automation config | When releases aren't cut (pure libraries) |
| `.release-please-manifest.json` | Current version tracker | Same |

**Notably absent:** `CHANGELOG.md`. Git history, release-please notes, and phase summaries provide the change record. A manually maintained changelog is redundant overhead.

**ARCHITECTURE.md** is required by the ARCHITECTURE.md standard for projects with multiple crates. For small projects, `CLAUDE.md` may cover the architecture sufficiently -- the principle is that the architecture is documented somewhere, not that a specific file exists.

### Directories

| Directory | Purpose | When to omit |
|-----------|---------|-------------|
| `crates/` (or `src/`) | All source code -- no code at root | Never |
| `.github/workflows/` | CI/CD workflows | Never (for GitHub-hosted repos) |
| `scripts/` | Build, deploy, maintenance scripts | When no scripts exist |
| `tests/` | Integration tests (unit tests are colocated with source) | When integration tests aren't needed yet |

**Not required at root:** `docs/`, `benches/`, `fuzz/`. Create them when they have content. Empty placeholder directories add noise.

---

## Workspace configuration

The workspace `Cargo.toml` is the most important configuration file. It sets the quality floor for the entire project.

### Version and edition

```toml
[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "AGPL-3.0-or-later"
```

- **Single workspace version.** All crates share one version. Avoids version desync across crates.
- **Edition 2024.** Enables `unsafe_op_in_unsafe_fn`, improved `use` semantics, and other safety improvements.
- **MSRV declared.** CI tests against it. Prevents accidental use of newer features.
- **License explicit.** AGPL-3.0-or-later is the forkwright default. MIT for tooling that benefits from permissive licensing (justify in CLAUDE.md).

### Workspace lints

```toml
[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "deny"
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"

[workspace.lints.rust]
unsafe_code = "deny"
```

All crates inherit via `[lints] workspace = true`. The principle: the workspace sets the quality floor, individual crates can only tighten (never loosen). See RUST.md for the full lint rationale.

### Build profiles

```toml
[profile.dev.package."*"]
opt-level = 2    # WHY: optimize deps for faster runtime during development

[profile.release]
lto = "thin"         # WHY: link-time optimization for binary size + speed
codegen-units = 1    # WHY: single codegen unit for maximum optimization
strip = true         # WHY: strip debug symbols from release binary
```

### Dependency management

All shared dependencies declared in `[workspace.dependencies]`. Crates reference them with `{ workspace = true }`. This ensures version consistency and makes auditing . See RUST.md for dependency evaluation criteria and the banned crate list.

---

## Dependency policy (deny.toml)

```toml
[advisories]
vulnerability = "deny"
unmaintained = "warn"
yanked = "deny"

[licenses]
unlicensed = "deny"
allow = [
    "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause",
    "ISC", "Unicode-3.0", "Zlib", "MPL-2.0", "AGPL-3.0-or-later",
]
confidence-threshold = 0.8

[bans]
multiple-versions = "warn"
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

The principle: deny by default, allow explicitly. Every allowed license is a policy decision. Every skipped advisory is a risk acceptance. Document both.

---

## CI gates

Every merge to main requires these checks to pass. The gates are ordered by speed -- fast checks fail first, saving CI minutes.

| Gate | What it catches | Speed |
|------|----------------|-------|
| Format (`cargo fmt --check`) | Style inconsistency | Seconds |
| Lint (`cargo clippy -D warnings`) | Code quality, common bugs | Seconds-minutes |
| Commit lint | Non-conventional commit messages | Seconds |
| Credential scan (gitleaks) | Accidentally committed secrets | Seconds |
| Tests (`cargo test --workspace`) | Behavior regressions | Minutes |
| Doc tests (`cargo test --doc`) | Stale documentation examples | Minutes |
| Rustdoc (`-D warnings`) | Broken doc links, missing docs | Minutes |
| Dependency audit (`cargo deny`) | License violations, known CVEs, banned crates | Seconds |

**The principle:** if the gates pass, the code is releasable. No "it passed CI but we need to manually check X." If X matters, it's a gate.

---

## Library crate requirements

Every library crate enforces documentation:

```rust
#![deny(missing_docs)]
```

The principle: public API is a commitment. If it's `pub`, it has documentation. If it shouldn't have documentation, it shouldn't be `pub`.

---

## Release automation

See RELEASES.md for the full versioning policy. Key points:

- release-please runs hourly (not per-push) to avoid CI noise during batch merges
- Release PRs are never auto-merged -- operator reviews and approves
- Tags trigger binary builds for musl Linux (static, any distro) and macOS aarch64 (Apple Silicon)
- `feat`, `fix`, `perf`, `refactor`, `docs` appear in changelog. `test`, `chore`, `ci`, `style` are hidden.

---

## Git discipline

- Branch protection on `main`: require PR, require CI pass, squash merge default, auto-delete branches
- Conventional commits: `type(scope): description`
- One logical change per commit
- Rebase before pushing (linear history)
- Never commit directly to main

---

## Verification

Run from the repo root. Adapt to the repo's context (public vs private, binary vs library).

```bash
#!/usr/bin/env bash
set -euo pipefail

FAIL=0
ok()   { echo "  OK: $1"; }
fail() { echo "  FAIL: $1"; FAIL=1; }
skip() { echo "  SKIP: $1"; }

echo "=== Required files ==="
for f in Cargo.toml deny.toml clippy.toml rustfmt.toml README.md CLAUDE.md .gitignore; do
    [ -f "$f" ] && ok "$f" || fail "$f missing"
done

# Conditional files
[ -f Cargo.lock ] && ok "Cargo.lock" || skip "Cargo.lock (omit for pure libraries)"
[ -f SECURITY.md ] && ok "SECURITY.md" || skip "SECURITY.md (required for public repos only)"
[ -f flake.nix ] && ok "flake.nix" || skip "flake.nix (required when using Nix)"

echo ""
echo "=== Required directories ==="
[ -d crates ] || [ -d src ] && ok "source code (crates/ or src/)" || fail "no source directory"
if gh api repos/$(git remote get-url origin | sed 's|.*github.com/||;s|\.git||')/  --jq '.private' 2>/dev/null | grep -q false; then
    [ -d .github/workflows ] && ok ".github/workflows/" || fail "CI workflows missing (public repo)"
else
    skip ".github/workflows/ (private repo -- CI runs locally)"
fi

echo ""
echo "=== Build gates ==="
cargo fmt --all -- --check 2>/dev/null && ok "cargo fmt" || fail "cargo fmt"
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -q "Finished" && ok "cargo clippy" || fail "cargo clippy"
cargo test --workspace 2>&1 | grep -q "FAILED" && fail "cargo test" || ok "cargo test"
cargo deny check 2>&1 | grep -q "FAILED" && fail "cargo deny" || ok "cargo deny"

echo ""
echo "=== Library crate docs ==="
for lib in $(find crates -name "lib.rs" 2>/dev/null); do
    grep -q "deny(missing_docs)" "$lib" && ok "$lib" || fail "$lib missing deny(missing_docs)"
done

echo ""
echo "=== Workspace lint inheritance ==="
for toml in $(find crates -name "Cargo.toml" -not -path "*/target/*"); do
    grep -q "workspace = true" "$toml" && ok "$(basename $(dirname $toml))" || fail "$(basename $(dirname $toml)) missing [lints] workspace"
done

echo ""
echo "=== Root hygiene ==="
# Every root file must be required by its consuming tool
for f in $(find . -maxdepth 1 -type f -not -name ".*" | sort); do
    name=$(basename "$f")
    case "$name" in
        Cargo.toml|Cargo.lock|CLAUDE.md|README.md|SECURITY.md) ;; # GitHub/Cargo required
        clippy.toml|rustfmt.toml|deny.toml|flake.nix) ;;          # Tool configs
        .gitignore|.gitleaks.toml|.kanon-lint-ignore) ;;           # Dotfile configs
        release-please-config.json|.release-please-manifest.json) ;; # Release automation
        *) fail "unjustified root file: $name" ;;
    esac
done

# No empty directories
find . -maxdepth 2 -type d -empty -not -path "./.git/*" -not -path "*/target/*" 2>/dev/null | while read d; do
    fail "empty directory: $d"
done

echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "All checks passed."
else
    echo "Issues found. Fix before merge."
    exit 1
fi
```

The script tests concrete requirements. The principles in this document guide judgment on everything the script can't test.
