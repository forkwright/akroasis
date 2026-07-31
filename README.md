# Akroasis

*ἀκρόασις - attentive reception*

---

Every tool for radio, mesh networking, spectrum monitoring, network security, or communications is a separate thing. Separate interfaces, separate data models, separate mental contexts. A mesh node goes offline while frequency activity spikes nearby and a network IDS fires an alert. Three tools. Three windows. No one connecting the dots.

Akroasis is the attempt to fix that.

One system. One signal model. The shared pipeline is designed for every domain to produce typed signals; today the live production `GeoSignal` producer is kerykeion mesh, while the remaining domains are planned or covered with synthetic pipeline tests. Radio anomalies correlate with network threats correlate with proximity intelligence correlate with OSINT. The convergence is where the intelligence lives - not in any single domain but in the relationships between them.

Capability domains span radio, mesh, SDR, proximity, network defense, OSINT, offensive security, signal intelligence, and geospatial modeling. Rust from the ground up. See the domain table below for shipped crates (✓) vs planned crates (◻).

---

## What it does

| Domain | Crate | Crate Shipped | Hardware Backend | What |
|--------|-------|:-------------:|:----------------:|------|
| **Application shell** | akroasis, akroasis-server | ✓ | ◻ | CLI binary + typed axum HTTP backend. Radio uses `StubHardware` by default; opt-in `hardware-serial` enables live detect. Mesh CLI is static/no-live-connection until daemon mode is implemented. |
| **Foundation** | koinon | ✓ |  -  | Shared IDs, coordinates, frequency and power types, 7-domain `GeoSignal` model, hardware asset registry, temporal baselines, and tamper-evident logging. |
| **Foundation** | kryphos | ✓ |  -  | Credential vault and installation identity: fjall-backed encrypted storage, Argon2id derivation, ChaCha20-Poly1305 encryption, Ed25519 signing keys, rotation/revocation metadata, and mutation audit logging at `tamper.log` beside the vault store. |
| **Radio Management** | syntonia | ✓ | ◻ | Frequency plans, CHIRP CSV/IMG import, CHIRP CSV export, validation, USB detection metadata, and Baofeng UV-5R-family codec. With `akroasis/hardware-serial`, `radio detect` uses live serial probing; read/program/export still require the future protocol session backend. |
| **Mesh Networking** | kerykeion | ✓ | ✓ | Clean-room Meshtastic stack: protobuf framing, serial/TCP transports, handshake, encryption, node database, topology, discovery, routing, delivery tracking, store-and-forward, gateway bridge, and signal conversion. |
| **Signal Processing** | semaino | ✓ |  -  | Signal aggregation, per-kind anomaly baselines, convergence detection, and deduplicated severity-classified alert pipeline. |
| **SDR / Reception** | dektis | ◻ | ◻ | Future spectrum monitoring, FM/AM/SSB demodulation, protocol decoding (APRS, ADS-B, P25), jamming detection, direction finding, and emitter fingerprinting. |
| **Proximity Intelligence** | engys | ◻ | ◻ | Future WiFi, BLE, Zigbee, Z-Wave, NFC, and RFID collection with presence analytics, rogue device detection, and counter-surveillance input. |
| **Network Defense** | aspis | ◻ | ◻ | Future IDS/IPS orchestration, CAN bus security, IoT monitoring, and active response. |
| **OSINT** | skopos | ◻ | ◻ | Future feed aggregation, threat intelligence, asset discovery, and anonymized collection paths. |
| **Offensive Security** | peira | ◻ | ◻ | Future penetration testing, vulnerability scanning, wireless security testing, scope locks, and audit trails. |
| **Signal Intelligence** | ichneutes | ◻ |  -  | Future entity correlation, focal point synthesis, threat scoring, and forensic timeline reconstruction across all domains. |
| **Automation** | praxis | ◻ |  -  | Future event-driven triggers, named playbooks, PACE communications, and operational state machines. |
| **Navigation** | chorografia | ◻ | ◻ | Future RF propagation modeling, infrastructure graphs, offline OSM navigation, and space weather HF prediction. |
| **Knowledge** | pinax | ◻ |  -  | Future offline repository for frequency databases, protocol specs, equipment manuals, topo maps, and indexed references. Target instance layout is documented in [docs/reference-store.md](docs/reference-store.md). |
| **Privacy** | lethe | ◻ | ◻ | Future VPN/proxy management, anonymization, IMSI catcher detection, and OPSEC scoring. The etymological complement to [Aletheia](https://github.com/forkwright/aletheia). |
| **Interface** | opsis | ◻ |  -  | Operator surfaces: desktop-first via theatron (akroasis-desktop). `akroasis-server` (shipped) provides the typed HTTP API (`/api/v1/*`) that the desktop and agent clients call. #118 resolved. |

**Legend:** ✓ = shipped in `crates/`, ◻ = planned/not shipped,  -  = not applicable.

> **Snapshot 2026-05-04.** Workspace ships 6 crates under `crates/`: `akroasis`,
> `kerykeion`, `koinon`, `kryphos`, `semaino`, and `syntonia`. The long-term
> domains below are architectural targets, not claims of current crate or hardware
> availability.

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
          │ (signal model,   │                                  │ (operator   │
          │  entity index,   │         ┌──────────────────┐    │  surfaces)  │
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

Every collection crate is expected to produce typed `GeoSignal` objects into koinon; the current live producer is kerykeion mesh. Semaino aggregates domain-agnostically, and its tests cover the full seven-domain signal model synthetically. Ichneutes analyzes domain-agnostically. Praxis acts. Opsis displays. Add a domain, add a crate - signals flow automatically once the collector exists.

---

## Design constraints

- **Standalone.** Runs without internet, without an LLM, without anything but the hardware in front of you. Grid-down capable.
- **Sovereignty.** Every protocol owned. No cloud dependencies, no subscriptions, no external trust.
- **Security default.** Encrypted by default. Unencrypted is the opt-in.
- **Auditable.** Credential vault mutations are recorded in a tamper-evident BLAKE3 hash-chain log beside the vault store. Broader action logging and evidence packaging are planned follow-ons.
- **Reproducible deployment (planned).** NixOS flake + systemd unit hardening + declarative deployment is the intended target shape; no deployment artifacts ship today. Tracked in #125.

---

## Technical

| Area | Current / Planned |
|------|-------------------|
| Language | Rust edition 2024, MSRV 1.85 |
| Version | 0.1.11 workspace package version |
| Errors | snafu context wrapping |
| Async | tokio |
| Storage | fjall for vault state; CBOR + BLAKE3 hash chains for tamper logs |
| Mesh | Shipped: clean-room Meshtastic stack with prost protobuf, serial/TCP transports, AES-CTR channel crypto, routing, topology, and store-and-forward |
| Radio | Shipped: frequency-plan model, validation, import/export, Baofeng UV-5R-family codec, and opt-in live serial detection through `akroasis/hardware-serial`; planned: live read/program/export protocol sessions beyond stub dispatch |
| SDR | Planned: FutureSDR, FFT, RTL-SDR, and SoapySDR work will land with `dektis` |
| IDS/IPS | Planned: Suricata and Zeek orchestration will land with `aspis` |
| Maps | Planned: OSM vector tiles and SRTM elevation will land with `chorografia` |
| Search | Planned: full-text indexing will land with `pinax` |
| Interfaces | Schema-versioned JSON is the canonical programmatic contract. CLI: `akroasis radio import --json`, `radio detect --json`, `radio export --json`, `mesh {status,nodes,topology} --json`, `vault list --json`, `vault identity --json`. HTTP: `akroasis-server` exposes `/api/v1/radio/detect`, `/api/v1/mesh/{status,nodes,topology}` with the same JSON schemas for durable clients. Interactive secret vault commands and planned placeholder domains remain TTY-only until their service surfaces ship. Desktop: desktop-first via theatron + akroasis-server (chalkeion Phase 6). |
| License | AGPL-3.0-only |

---

## Environment Variables

Akroasis reads these environment variables at runtime; unset variables fall back to the defaults below.

| Variable | Purpose | Default |
|----------|---------|---------|
| `AKROASIS_VAULT_PATH` | Overrides the credential vault's storage directory. | `~/.local/share/akroasis/vault` |

---

## Documentation

- [standards/](standards/): Pointer to canonical kanon standards (STANDARDS.md, GNOMON.md, etc.)
- [docs/lexicon.md](docs/lexicon.md): Project name registry
- [docs/reference-store.md](docs/reference-store.md): Target `/instance/reference/` layout for the planned pinax knowledge store

## Status

**Phase 02 complete** (project state 2026-04-22). Kerykeion mesh networking fully landed and is the current live `GeoSignal` collector.
For current planning and phase status, see
[CLAUDE.md](CLAUDE.md) and the kanon planning substrate at `kanon:projects/akroasis/STATE.md`.

The scope is massive. Each domain is independent: a crate with clear boundaries, producing typed signals into the shared model. Pieces don't need to arrive simultaneously. They just need to speak the same language when they do.

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

Names follow the project naming philosophy, where each name reveals its essential nature across four layers of reading.

**Lethe** (λήθη) and **Aletheia** (ἀ-λήθεια) share the same root. One unconceals truth. The other conceals the operator. Same word, opposite directions. Two systems, one for understanding and one for sovereignty, and the Greek already knew they were the same thing.

---

*See [docs/lexicon.md](docs/lexicon.md) for the complete name registry and naming methodology.*

---

## Disclaimer

This software is for research and educational purposes. See [DISCLAIMER.md](DISCLAIMER.md) for details on user responsibility, licensing, and legal considerations. The authors accept no responsibility for any specific use of this software.

<!-- kanon:auto-start -->
## Repository Metadata

- Registry name: `akroasis`
- Description: Kanon-managed forkwright repository `akroasis`.
- Forge repo: `forkwright/akroasis`
- Kanon prefix: `ak`
- Config source: `workflow/kanon.toml [projects.akroasis]`
- Planning state: `projects/akroasis/STATE.md`
- Last state update: `not recorded`

Run `kanon docs sync --check --repo akroasis` to verify this generated
section and `kanon docs sync --apply --repo akroasis` to refresh it.

## Blast zone

- Paths explicitly named by the rendered prompt, role, or template input.

## Acceptance verifier

```bash
kanon gate
```
<!-- kanon:auto-end -->
