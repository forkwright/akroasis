<!--
scope: akroasis repo conventions (SIGINT/comms crates: koinon, kryphos)
defers_to: ~/menos-ops/CLAUDE.md for machine topology; ~/.claude/CLAUDE.md for operator principles
tightens: per-crate CLAUDE.md files may narrow within their layer
-->

# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

## Standards

Universal: `~/dev/kanon/crates/basanos/standards/STANDARDS.md`
Rust: `~/dev/kanon/crates/basanos/standards/RUST.md`
Shell: `~/dev/kanon/crates/basanos/standards/SHELL.md`
Writing: `~/dev/kanon/crates/basanos/standards/WRITING.md`

## Structure

Foundation layer: `koinon` (shared types, signal model), `kryphos` (encryption, identity). See README.md for the full domain map with status markers and `docs/ARCHITECTURE.md` for layer structure.

## Commands

```bash
cargo build                            # Debug build
cargo test --workspace                 # All tests
cargo test -p <crate>                  # Single crate
cargo clippy --workspace               # Lint (zero warnings)
```

## Key patterns

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio
- **Lints:** `#[expect(lint, reason = "...")]` over `#[allow]`
- **Visibility:** `pub(crate)` by default
- **Naming:** Greek names per `~/dev/kanon/crates/basanos/standards/GNOMON.md`, registry at [docs/lexicon.md](docs/lexicon.md)

## Before submitting

1. `cargo test -p <affected-crate>` passes
2. `cargo clippy --workspace` passes with zero warnings
3. No `unwrap()` in library code
4. New errors use snafu with context

## Git

Conventional commits: `<type>(<scope>): <description>`. Scope is the crate name.
Branch from `main`. Rebase before pushing. Squash merge.
