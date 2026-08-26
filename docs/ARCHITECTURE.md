# Architecture: Akroasis

Single Rust binary. Five layers. The shared model supports all domains producing typed `GeoSignal` values into one pipeline; current live collection is kerykeion mesh, with the other domain collectors planned.

Run `cargo metadata --format-version 1 | jq '.workspace_members | length'` for current crate count.

## Layer structure

```
Interface:      opsis (desktop-first via theatron); akroasis-server (axum HTTP/SSE backend)
Orchestration:  praxis (automation, playbooks, PACE)
Analysis:       semaino (aggregation), ichneutes (correlation)
Collection:     syntonia, kerykeion, dektis, engys, aspis, skopos, peira
Foundation:     stoicheion (vocabulary), tekmerion (evidence), kryphos (encryption), lethe (privacy)
```

## Crate registry

| Crate | Layer | Purpose |
|-------|-------|---------|
| **stoicheion** | Foundation | Shared types, signal model, entity index, temporal baseline engine |
| **tekmerion** | Foundation | Validated callers and authority, effect receipts, tamper-evident logging |
| **kryphos** | Foundation | Encryption, key management, forward secrecy, credential vault, identity management |
| **lethe** | Foundation | Privacy infrastructure, VPN, anonymization, OPSEC scoring |
| **syntonia** | Collection | Radio management, frequency plans, serial protocols, hardware programming |
| **kerykeion** | Collection | Meshtastic mesh stack, DTN, multi-path routing, PACE failover |
| **dektis** | Collection | SDR reception, spectrum analysis, demodulation, EW detection |
| **engys** | Collection | Proximity protocols (WiFi/BLE/Zigbee/NFC/RFID) |
| **aspis** | Collection | Network defense, IDS/IPS, active response |
| **skopos** | Collection | OSINT collection, recon, asset discovery |
| **peira** | Collection | Offensive security, penetration testing |
| **semaino** | Processing | Signal aggregation, convergence detection, anomaly baselines |
| **ichneutes** | Analysis | Entity correlation, focal points, threat scoring, intelligence synthesis |
| **praxis** | Orchestration | Automation engine, playbooks, event triggers, state machines |
| **chorografia** | Model | Geographic model, RF propagation, navigation, terrain |
| **opsis** | Interface | Operator surfaces: desktop-first via theatron (akroasis-desktop), consumed through the `akroasis-server` HTTP API. #118 resolved. |
| **akroasis** | Binary | CLI entrypoint, subcommand routing, and library interface for akroasis-server |
| **akroasis-server** | Interface | Canonical durable programmatic surface: typed axum HTTP backend (`/api/v1/*`) called by akroasis-desktop and agent clients. Mirrors schema-versioned CLI `--json` report contracts for shipped non-interactive surfaces. |

The planned reference-library application has not earned an application or
crate name. Akroasis owns its domain model and envelope policy; standalone
[`forkwright/pinax`](https://github.com/forkwright/pinax) exclusively owns the
relational engine, including transactions, typed schemas, and page-at-rest
encryption. A local `pinax` crate would be a second authority, not an
application layer. Sphragis recipient wrapping is separate from Pinax page
encryption.

## Key decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust | Single binary, no runtime deps, grid-down capable |
| Async | Tokio | Real threads, I/O multiplexing |
| Errors | snafu | Context selectors, location tracking |
| IDs | Newtypes | AgentId, SignalId, DeviceId, etc. |
| Time | jiff | Ergonomic temporal types |
| Signal model | `GeoSignal` enum | Typed signals from heterogeneous sources into shared pipeline |
| Temporal baseline | Welford's algorithm | Online mean/variance without storing history |
| Tamper logging | BLAKE3 hash chain | Append-only with cryptographic integrity |
| SDR | Own dataflow engine | FutureSDR dependency risk too high (single maintainer, 0.0.x) |
| Mesh | Clean-room stack over `prost` | The official `meshtastic` crate is GPL-3 and covers ~15% of the protocol; framing, crypto, routing and topology are ours |

## Dependency philosophy

"Own the interface, depend on the implementation only when the implementation is trustworthy." When a crate is single-maintainer, pre-1.0, or provides a thin wrapper over a kernel/hardware interface we understand, we build our own. See `scope.md` -> Ownership Corrections for the full audit.

## References

- Planning docs (scope, roadmap, vision, research): live in the kanon repo
- Naming: `../standards/GNOMON.md`, `lexicon.md`
- Reference store layout: `reference-store.md`
- Reference-library encryption authority boundary: `fjall-column-encryption.md`
- PQ content-key wrapping boundary: `pq-content-key-wrapping.md`
