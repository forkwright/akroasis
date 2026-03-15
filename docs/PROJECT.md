# Akroasis - Project Overview

Communications sovereignty, RF intelligence, and operational awareness platform. 17 crates, 10 capability domains, one shared signal model.

## Current State

Phase 0 (Foundation) in progress. 2 PRs merged (koinon scaffold + tamper-evident log). 5 hygiene PRs merged.

## Architecture

See `ARCHITECTURE.md` for the crate map, layer structure, and key decisions.

## Phases

| Phase | Domain | Status |
|-------|--------|--------|
| 0 | Foundation + research | In progress |
| 1 | Radio management (syntonia) | Queued |
| 2 | Mesh core (kerykeion) | Queued |
| 3 | SDR foundation (dektis) | Queued |
| 4 | Signal intelligence (semaino + ichneutes) | Queued |
| 5-14 | Network defense through Aletheia integration | Planned |

Full phase details: `/media/theke/projects/akroasis/planning/roadmap.md`

## Related Projects

| Project | Relationship |
|---------|-------------|
| [Aletheia](https://github.com/forkwright/aletheia) | Akroasis plugs into Aletheia as a thesauros domain pack (Phase 14). Standalone otherwise. |
| [Harmonia](https://github.com/forkwright/harmonia) | Sibling project — same toolchain and patterns, different domain. |

## References

| Document | Purpose |
|----------|---------|
| `ARCHITECTURE.md` | Crate map, layer structure, key decisions |
| `gnomon.md` | Greek naming methodology |
| `lexicon.md` | Domain terms and name registry |
| `../standards/STANDARDS.md` | Universal coding standards |
| `../standards/RUST.md` | Rust conventions |
