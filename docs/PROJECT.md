# Akroasis - Project Overview

Communications sovereignty, RF intelligence, and operational awareness platform. Capability domains span radio, mesh, SDR, proximity, network defense, OSINT, offensive security, and signal intelligence, unified by one shared signal model.

## Current state

See the status markers in [../README.md](../README.md) for shipped vs planned crates. Phase and wave status lives in the kanon repo roadmap  -  single source of truth.

## Architecture

See `ARCHITECTURE.md` for the crate map, layer structure, and key decisions.

## Phases

Phase index lives in the kanon repo roadmap. Wave status is reflected in merged PRs (`gh pr list --state merged --repo forkwright/akroasis`) and the shipped/planned markers in README.md. Duplicating phase tables here produces stale content  -  intentionally omitted.

## Related projects

| Project | Relationship |
|---------|-------------|
| [Aletheia](https://github.com/forkwright/aletheia) | Akroasis plugs into Aletheia as a thesauros domain pack (Phase 14). Standalone otherwise. |
| [Harmonia](https://github.com/forkwright/harmonia) | Sibling project: same toolchain and patterns, different domain. |

## References

| Document | Purpose |
|----------|---------|
| `ARCHITECTURE.md` | Crate map, layer structure, key decisions |
| `../standards/GNOMON.md` | Greek naming methodology |
| `lexicon.md` | Domain terms and name registry |
| `../standards/STANDARDS.md` | Universal coding standards |
| `../standards/RUST.md` | Rust conventions |
