# Akroasis

*ἀκρόασις — attentive reception*

RF intelligence, mesh networking, and communications sovereignty. Rust-first, no compromise.

---

## Philosophy

Akroasis is the act of listening — not passive hearing but disciplined, attentive reception. The kind of listening that brings understanding to what is received.

This is a communications sovereignty platform. Every protocol it touches is owned, understood, and controlled by the operator. No cloud dependencies. No subscription services. No trust in infrastructure you don't hold. Grid-up or grid-down, the system works.

**Security first. Privacy first. Sovereignty first.**

Standalone by design. Runs without an LLM, without internet, without anything but the hardware in front of you. Plugs into [Aletheia](https://github.com/forkwright/aletheia) when the full stack is available — an agent gains awareness of RF spectrum, mesh topology, and communication channels beyond the internet. But Aletheia is an upgrade, not a requirement.

## Domains

| Domain | Crate | Capability |
|--------|-------|-----------|
| **Radio Management** | `syntonia` | Frequency plans, channel programming, radio profiles — clean-room Rust-native CHIRP replacement |
| **Mesh Networking** | `kerykeion` | Meshtastic protocol stack, node management, topology awareness, message routing |
| **SDR / Reception** | `dektis` | Spectrum monitoring, signal demodulation, scanner mode, I/Q recording |
| **Signal Intelligence** | `semaino` + `ichneutes` | Protocol decoding (APRS, ADS-B, P25), activity monitoring, signal fusion, threat assessment |
| **Communications** | `kryphos` | Encrypted messaging, email, Winlink, protocol bridges |
| **Geographic** | `chorografia` | Coverage modeling, infrastructure dependencies, cascade analysis |
| **Interface** | `opsis` | TUI first, web later — spectrum waterfall, mesh topology, intelligence dashboard |

## Architecture

```
akroasis/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── akroasis/           # Binary entrypoint — CLI subcommands
│   ├── koinon/             # Shared types, signal model, geographic primitives
│   ├── syntonia/           # Radio management — serial protocols, memory maps
│   ├── kerykeion/          # Meshtastic mesh stack — protobuf, connections, routing
│   ├── dektis/             # SDR hardware abstraction — device management, I/Q pipeline
│   ├── semaino/            # Signal processing + intelligence — aggregation, baselines, convergence
│   ├── ichneutes/          # Analysis + correlation — entity tracking, focal points, threat scoring
│   ├── kryphos/            # Encryption — key management, forward secrecy
│   ├── chorografia/        # Geographic model — coverage, cascade, infrastructure graph
│   └── opsis/              # Frontend — TUI (ratatui), future web
└── docs/
    └── gnomon.md           # Name registry and rationale
```

## Status

Scaffolded. Research phase. Not yet under active development — Aletheia cutover comes first.

---

*Named via [gnomon](https://github.com/forkwright/aletheia/blob/main/docs/gnomon.md) — the system of names that reveal essential natures.*
