# CLAUDE.md

Project conventions for AI coding agents working on this codebase.

## Standards

Universal: [standards/STANDARDS.md](standards/STANDARDS.md)
Rust: [standards/RUST.md](standards/RUST.md)
Shell: [standards/SHELL.md](standards/SHELL.md)
Writing: [standards/WRITING.md](standards/WRITING.md)

## Structure

17 crates, 10 capability domains. See README.md for the full domain map.

Foundation: `koinon` (shared types, signal model), `kryphos` (encryption, identity).

## Commands

```bash
cargo build                            # Debug build
cargo test --workspace                 # All tests
cargo test -p <crate>                  # Single crate
cargo clippy --workspace               # Lint (zero warnings)
```

## Key Patterns

- **Errors:** `snafu` with `.context()` propagation and `Location` tracking
- **IDs:** Newtypes for all domain IDs
- **Time:** `jiff` for time, `ulid` for IDs, `compact_str` for small strings
- **Async:** Tokio
- **Lints:** `#[expect(lint, reason = "...")]` over `#[allow]`
- **Visibility:** `pub(crate)` by default
- **Naming:** Greek names per [docs/gnomon.md](docs/gnomon.md), registry at [docs/lexicon.md](docs/lexicon.md)

## Before Submitting

1. `cargo test -p <affected-crate>` passes
2. `cargo clippy --workspace` — zero warnings
3. No `unwrap()` in library code
4. New errors use snafu with context

## Git

Conventional commits: `<type>(<scope>): <description>`. Scope is the crate name.
Branch from `main`. Rebase before pushing. Squash merge.
