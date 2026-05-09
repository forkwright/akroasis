# akroasis

Communications sovereignty and RF intelligence platform. Rust workspace, single binary, grid-down capable.

## Commands

- `cargo build`  -  debug build
- `cargo test --workspace`  -  all tests
- `cargo test -p <crate>`  -  single crate
- `cargo clippy --workspace --all-targets -- -D warnings`  -  lint (zero warnings required)
- `cargo fmt --all -- --check`  -  format gate
- `kanon lint . --summary`  -  project-specific lint rules (standards enforcement)

## Rules

- Use `snafu` with `.context()` and `Location` tracking for errors  -  not `thiserror`, not `anyhow`; snafu carries source + location without boilerplate
- Use `jiff` for time, `ulid` for IDs, `compact_str` for small strings  -  chrono is banned; ulid sorts lexicographically; compact_str avoids heap for short strings
- Newtypes for all domain IDs (`SignalId`, `DeviceId`, `NodeId`)  -  raw `String`/`u64` lets mismatches pass the type checker
- Use `#[expect(lint, reason = "...")]` not `#[allow]`  -  `#[expect]` warns when the suppression becomes stale
- Conventional commits: `type(scope): description` where scope is the crate name  -  release-please parses these
- `pub(crate)` by default, `pub` only for workspace-public API  -  narrow visibility keeps refactors local
- No `unwrap()`, `expect()`, or `panic!()` in library code  -  workspace lints deny these; use `?` with snafu context

## Architecture

- Foundation: `koinon` (shared types, signal model), `kryphos` (crypto, identity)
- Collection crates produce typed `GeoSignal` into the shared pipeline  -  add a domain, add a crate, signals flow automatically
- Async: tokio, native async traits
- Mesh: clean-room Meshtastic stack via `prost` protobuf  -  not the official `meshtastic` crate (GPL-3, ~15% coverage)

## Where to add things

- New crate: `crates/<greek-name>/`, register in root `Cargo.toml` members, follow `~/dev/kanon/crates/basanos/standards/GNOMON.md` for naming, add entry to `docs/lexicon.md`
- New signal type: extend `GeoSignal` enum in `koinon`; downstream crates match exhaustively
- New standard or convention: propose it in `~/dev/kanon/crates/basanos/standards/`; cross-link from this repo's `standards/README.md`

## Boundaries

- Always: run `cargo fmt` + `cargo clippy -D warnings` before pushing, rebase onto main (linear history), squash merge
- Ask first: changes to `GeoSignal` enum, crypto primitives in `kryphos`, workspace-wide dependency bumps
- Never: push to upstream with `--force`, bypass CI with `--admin`, commit secrets (see `.gitleaks.toml`)
