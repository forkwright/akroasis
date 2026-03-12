# Akroasis

*ἀκρόασις — attentive reception*

RF intelligence, mesh networking, and communications sovereignty. Rust-first, no compromise.

---

## Philosophy

Akroasis is the act of listening — not passive hearing but disciplined, attentive reception. The kind of listening that brings understanding to what is received.

This is a communications sovereignty and RF intelligence platform. Every protocol it touches is owned, understood, and controlled by the operator. No cloud dependencies. No subscription services. No trust in infrastructure you don't hold. Grid-up or grid-down, the system works.

**Security first. Privacy first. Sovereignty first.**

Standalone by design. Runs without an LLM, without internet, without anything but the hardware in front of you. Plugs into [Aletheia](https://github.com/forkwright/aletheia) when the full stack is available — an agent gains awareness of RF spectrum, mesh topology, network defense, and communications sovereignty. But Aletheia is an upgrade, not a requirement.

## Architecture

14 crates. 7 capability domains. One shared signal model.

```
akroasis/
├── Cargo.toml                # Workspace root
├── crates/
│   ├── akroasis/             # Binary — CLI entrypoint
│   │
│   │  ── Foundation ──
│   ├── koinon/               # Commons — signal model, entity index, temporal engine
│   ├── kryphos/              # Encryption — key management, forward secrecy
│   ├── lethe/                # Privacy — VPN, proxy, anonymization, DNS filtering
│   │
│   │  ── Collection ──
│   ├── syntonia/             # Radio management — serial protocols, frequency plans
│   ├── kerykeion/            # Mesh networking — Meshtastic, topology, routing
│   ├── dektis/               # SDR reception — I/Q pipeline, spectrum, demodulation
│   ├── aspis/                # Network defense — IDS/IPS, Suricata/Zeek, active response
│   ├── skopos/               # OSINT — feeds, recon, asset discovery, threat intel
│   ├── peira/                # Offensive security — pentesting, vuln scanning, probing
│   │
│   │  ── Processing ──
│   ├── semaino/              # Signal aggregation — convergence, anomaly baselines
│   ├── ichneutes/            # Intelligence analysis — entity correlation, threat scoring
│   │
│   │  ── Model + Interface ──
│   ├── chorografia/          # Geographic model — RF coverage, cascade analysis
│   └── opsis/                # Frontend — TUI, native app (Tauri), web UI
└── docs/
    └── gnomon.md             # Name registry and rationale
```

## Domains

| Domain | Crate(s) | Capability |
|--------|----------|-----------|
| **Radio Management** | `syntonia` | Frequency plans, channel programming, radio profiles — clean-room CHIRP replacement |
| **Mesh Networking** | `kerykeion` | Meshtastic protocol stack, node management, topology awareness, message routing |
| **SDR / Reception** | `dektis` | Spectrum monitoring, signal demodulation, scanner mode, I/Q recording |
| **Signal Intelligence** | `semaino` + `ichneutes` | Signal fusion, convergence detection, entity correlation, focal points, threat scoring |
| **Network Defense** | `aspis` | Full IDS/IPS — Suricata/Zeek orchestration, active response, flow analysis |
| **OSINT** | `skopos` | Feed aggregation, threat intel (STIX/TAXII), asset discovery, dark web monitoring |
| **Offensive Security** | `peira` | Penetration testing, vulnerability scanning, RF security, scope-locked with audit trail |
| **Communications** | `kryphos` | Encrypted messaging, email, Winlink, protocol bridges |
| **Privacy** | `lethe` | VPN/proxy management, DNS filtering, anonymization, metadata scrubbing, identity segregation |
| **Geographic** | `chorografia` | RF coverage modeling, infrastructure dependency graph, cascade analysis |
| **Interface** | `opsis` | TUI (ratatui), native app (Tauri), web UI (Axum over Tailscale) |

## Status

Scaffolded. Research phase. Not yet under active development — Aletheia cutover comes first.

---

*Named via [gnomon](https://github.com/forkwright/aletheia/blob/main/docs/gnomon.md) — the system of names that reveal essential natures.*
*Lethe (λήθη) is the etymological complement to Aletheia (ἀ-λήθεια). Same root, opposite directions. Unconcealment for understanding. Concealment for sovereignty.*
