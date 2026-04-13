# Akroasis

*ἀκρόασις - attentive reception*

---

Every tool for radio, mesh networking, spectrum monitoring, network security, or communications is a separate thing. Separate interfaces, separate data models, separate mental contexts. A mesh node goes offline while frequency activity spikes nearby and a network IDS fires an alert. Three tools. Three windows. No one connecting the dots.

Akroasis is the attempt to fix that.

One system. One signal model. Every domain produces typed signals into the same pipeline. Radio anomalies correlate with network threats correlate with proximity intelligence correlate with OSINT. The convergence is where the intelligence lives - not in any single domain but in the relationships between them.

17 crates. 10 capability domains. Rust from the ground up.

---

## What it does

| Domain | Crate | What |
|--------|-------|------|
| **Radio Management** | syntonia | Frequency plans, channel programming, serial protocols - clean-room CHIRP replacement. Programs Baofeng UV-5R family and Yaesu FTM-510DR directly. |
| **Mesh Networking** | kerykeion | Full Meshtastic protocol stack. Node management, topology awareness, message routing. Delay-tolerant networking - messages survive hours-long network partitions. PACE communications with automated failover. |
| **SDR / Reception** | dektis | Spectrum monitoring, FM/AM/SSB demodulation, protocol decoding (APRS, ADS-B, P25). Jamming detection, direction finding, emitter fingerprinting. The electromagnetic environment as a contested space. |
| **Proximity Intelligence** | engys | WiFi, BLE, Zigbee, Z-Wave, NFC, RFID. Everything broadcasting within range - every phone, beacon, smart lock, tracker. Presence analytics, rogue device detection, counter-surveillance input. |
| **Network Defense** | aspis | Full IDS/IPS - Suricata and Zeek orchestration with active response. CAN bus security for vehicle networks. IoT device monitoring. |
| **OSINT** | skopos | Feed aggregation, threat intelligence (STIX/TAXII), asset discovery, web scraping, dark web monitoring. All collection routed through anonymization infrastructure. |
| **Offensive Security** | peira | Penetration testing, vulnerability scanning, wireless security testing. Every operation scope-locked with full audit trail. |
| **Signal Intelligence** | semaino + ichneutes | Signal aggregation, convergence detection, anomaly baselines (Welford's algorithm), entity correlation, focal point synthesis, threat scoring. Forensic timeline reconstruction across all domains. |
| **Automation** | praxis | Event-driven triggers, named playbooks, PACE communications, operational state machines. The layer that turns awareness into action - not just monitoring, responding. |
| **Navigation** | chorografia | RF propagation modeling, infrastructure dependency graphs, cascade analysis. Vehicle and foot navigation with offline OSM maps. Military planning overlays. Space weather integration for HF propagation prediction. |
| **Knowledge** | pinax | Offline knowledge repository - frequency databases, protocol specs, equipment manuals, topo maps, emergency procedures, vulnerability databases. Compressed, indexed, searchable. When the internet dies, the knowledge survives. |
| **Privacy** | lethe | VPN/proxy management, anonymization, metadata scrubbing, IMSI catcher detection, continuous OPSEC scoring. The etymological complement to [Aletheia](https://github.com/forkwright/aletheia) - same root (λήθη), opposite directions. |
| **Interface** | opsis | TUI (ratatui), native app (Dioxus), web UI (Axum over Tailscale). Spectrum waterfall, mesh topology, intelligence dashboard, map display, after-action replay. |

Foundation crates: **koinon** (shared types, signal model, entity index, temporal engine), **kryphos** (encryption, key management, credential vault, identity segregation).

---

## Architecture

```
                Collection                    Processing              Action
          ┌─────────────────┐          ┌──────────────────┐    ┌─────────────┐
          │ syntonia (radio) │          │ semaino          │    │ praxis      │
          │ kerykeion (mesh) │  typed   │ (aggregation,    │    │ (playbooks, │
          │ dektis (SDR/EW)  │ signals  │  convergence,    │    │  triggers,  │
          │ engys (proximity)├────────►│  anomaly          ├───►│  PACE,      │
          │ aspis (defense)  │         │  baselines)       │    │  state      │
          │ skopos (OSINT)   │         │                   │    │  machines)  │
          │ peira (offense)  │         │ ichneutes         │    │             │
          └────────┬─────────┘         │ (correlation,     │    └──────┬──────┘
                   │                   │  focal points,    │           │
          ┌────────▼─────────┐         │  threat scoring)  │    ┌──────▼──────┐
          │ koinon           │         └──────────────────┘    │ opsis       │
          │ (signal model,   │                                  │ (TUI, app,  │
          │  entity index,   │         ┌──────────────────┐    │  web UI)    │
          │  temporal engine)│         │ chorografia      │    └─────────────┘
          │                  │         │ (geo, nav, RF    │
          │ kryphos          │         │  propagation)    │
          │ (crypto, keys,   │         │                  │
          │  credentials)    │         │ pinax            │
          │                  │         │ (offline maps,   │
          │ lethe            │         │  specs, manuals) │
          │ (privacy, VPN,   │         └──────────────────┘
          │  OPSEC)          │
          └──────────────────┘
```

Every collection crate produces typed `GeoSignal` objects into koinon. Semaino aggregates domain-agnostically. Ichneutes analyzes domain-agnostically. Praxis acts. Opsis displays. Add a domain, add a crate - signals flow automatically.

---

## Design constraints

- **Standalone.** Runs without internet, without an LLM, without anything but the hardware in front of you. Grid-down capable.
- **Sovereignty.** Every protocol owned. No cloud dependencies, no subscriptions, no external trust.
- **Security default.** Encrypted by default. Unencrypted is the opt-in.
- **Auditable.** Tamper-evident logging with hash chains. Every action recorded. Evidence packaging with chain of custody.
- **NixOS.** Reproducible builds, systemd hardening, declarative deployment from day one.

---

## Technical

| | |
|---|---|
| Language | Rust (edition 2024, MSRV in Cargo.toml) |
| Errors | snafu (context wrapping, not thiserror) |
| Async | tokio, native async traits |
| SDR runtime | FutureSDR (async block graph) |
| FFT | rustfft + realfft |
| SDR hardware | rtl-sdr-rs (RTL-SDR V4), soapysdr (multi-hardware) |
| Mesh | Clean-room Meshtastic (prost protobuf, not official crate) |
| IDS/IPS | Suricata + Zeek orchestration |
| Maps | OSM vector tiles, SRTM elevation |
| Search | tantivy (full-text indexing) |
| TUI | ratatui |
| Desktop | Dioxus |
| Web | Axum |
| License | AGPL-3.0-or-later |

---

## Documentation

- [standards/STANDARDS.md](standards/STANDARDS.md): Coding standards (universal + per-language)
- [docs/gnomon.md](docs/gnomon.md): Naming methodology
- [docs/lexicon.md](docs/lexicon.md): Project name registry

## Status

Wave 1 (kryphos, 7 PRs) and Wave 2 (syntonia, 7 PRs) are complete. Architecture finalized. Active development ongoing.

The scope is massive. The architecture makes each domain independent: a crate with clear boundaries, producing typed signals into the shared model. The pieces don't need to arrive simultaneously. They just need to speak the same language when they do.

---

## Hardware

Developed against:

- **SDR:** RTL-SDR Blog V4, HackRF One
- **Mesh:** Lilygo T-Echo, T-Deck Plus, RAK Pi HAT gateway, WisBlock
- **Radio:** Baofeng HTs (UV-5R series), Yaesu mobile (FTM-510DR), Yaesu HF (FT-891)
- **Compute:** Linux server, ruggedized field laptop, Raspberry Pi
- **Proximity:** nRF52840 (BLE), Proxmark3 (NFC/RFID), WiFi monitor mode adapters

Hardware support is additive: if it speaks serial, USB, or IP, it can be integrated.

---

## Name

ἀκρόασις - from Aristotle's Physics, "Physike Akroasis" - learning through attentive reception. Not passive hearing but the disciplined act of listening that brings understanding to what is received.

Names follow [gnomon](https://github.com/forkwright/aletheia/blob/main/docs/gnomon.md) - the naming philosophy where each name reveals its essential nature across four layers of reading.

**Lethe** (λήθη) and **Aletheia** (ἀ-λήθεια) share the same root. One unconceals truth. The other conceals the operator. Same word, opposite directions. Two systems, one for understanding and one for sovereignty, and the Greek already knew they were the same thing.

---

*See [docs/gnomon.md](docs/gnomon.md) for the complete name registry.*

---

## Disclaimer

This software is for research and educational purposes. See [DISCLAIMER.md](DISCLAIMER.md) for details on user responsibility, licensing, and legal considerations. The authors accept no responsibility for any specific use of this software.
