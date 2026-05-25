# Architecture: Akroasis

Single Rust binary. Five layers. The shared model supports all domains producing typed `GeoSignal` values into one pipeline; current live collection is kerykeion mesh, with the other domain collectors planned.

Run `cargo metadata --format-version 1 | jq '.workspace_members | length'` for current crate count.

## Layer structure

```
Interface:      opsis (TUI/web/native)
Orchestration:  praxis (automation, playbooks, PACE)
Analysis:       semaino (aggregation), ichneutes (correlation)
Collection:     syntonia, kerykeion, dektis, engys, aspis, skopos, peira
Foundation:     koinon (shared types), kryphos (encryption), lethe (privacy)
```

## Crate registry

| Crate | Layer | Purpose |
|-------|-------|---------|
| **koinon** | Foundation | Shared types, signal model, entity index, temporal baseline engine, tamper-evident logging |
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
| **pinax** | Knowledge | Offline knowledge repository, frequency databases, maps |
| **opsis** | Interface | TUI, Dioxus native app, web UI |
| **akroasis** | Binary | CLI entrypoint, subcommand routing |

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
| Mesh | Meshtastic official crate + own crypto | Transport via crate, encryption/topology/routing ourselves |

## Dependency philosophy

"Own the interface, depend on the implementation only when the implementation is trustworthy." When a crate is single-maintainer, pre-1.0, or provides a thin wrapper over a kernel/hardware interface we understand, we build our own. See `scope.md` -> Ownership Corrections for the full audit.

## References

- Planning docs (scope, roadmap, vision, research): live in the kanon repo
- Naming: `../standards/GNOMON.md`, `lexicon.md`
