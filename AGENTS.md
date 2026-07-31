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

<!-- kanon:auto-start -->
<!--
scope: akroasis repo cross-tool agent guide (Claude Code, Kimi, Codex, Cursor, Windsurf, Copilot)
generated_by: kanon docs sync
defers_to: CLAUDE.md for Claude Code-specific behavior; ~/menos-ops/CLAUDE.md for machine + service topology
tightens: workflow/AGENTS-mcp-tools.md catalog routing; crates/basanos/standards/AGENT-DOCS.md authoring rules
-->

# akroasis

Kanon-managed forkwright repository `akroasis`.

## Commands

Run `kanon --help` for all kanon-managed workflow commands. Run project-local
build, test, and lint commands from this repository root.

- `kanon gate` - full local gate for kanon-managed PRs
- `kanon lint --fix` - deterministic standards fixes
- `kanon lint --explain <RULE>` - rule rationale and fix guidance
- `kanon pr open <head_ref> --title "..."` - open a forge PR
- `kanon pr merge <N> [--strategy squash|ff|rebase]` - merge after CI and gate checks
- `kanon docs sync --check --repo akroasis` - verify derived bootstrap docs
- `kanon docs sync --apply --repo akroasis` - regenerate derived bootstrap docs

For agent-native operations, prefer the `mcp__kanon__*` tool family. See
[workflow/AGENTS-mcp-tools.md](workflow/AGENTS-mcp-tools.md) for routing and fallback rules.

## Standards

Read `crates/basanos/standards/STANDARDS.md` § Philosophy before writing code. Key principles:
no workarounds, define once, reference everywhere, no shortcuts, no compromise on quality.
Rust work also reads `crates/basanos/standards/RUST.md` before editing Rust code.

## Rules

- Structured comment tags only: WHY, NOTE, WARNING, PERF, SAFETY, INVARIANT, TODO(#NNN), FIXME(#NNN)
- Conventional commits: `type(scope): description`
- Add `Gate-Passed: kanon 0.1.0` to validated commit bodies
- Never add `#[allow]` suppressions; use `#[expect(lint, reason = "...")]` only when justified
- Prefer MCP tools first; CLI commands are resilience fallbacks

## Architecture

- Registry name: `akroasis`
- Forge repo: `forkwright/akroasis`
- Kanon prefix: `ak`
- Config source: `workflow/kanon.toml [projects.akroasis]`

## Boundaries

Always: run the applicable gate before pushing, stay inside the declared blast radius.
Ask first: workflow, service, credential, schema, or deployment changes.
Never: bypass CI, push to protected upstream refs, commit secrets, or suppress warnings.

## Blast zone

- Paths explicitly named by the rendered prompt, role, or template input.

## Acceptance verifier

```bash
kanon gate
```
<!-- kanon:auto-end -->
