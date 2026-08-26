# Akroasis: Lexicon

*Living registry. Updated as crates are added or renamed.*
*For the naming methodology and construction system, see `crates/basanos/standards/GNOMON.md` in the canonical kanon repo (see [`standards/README.md`](../standards/README.md) for the pointer).*

---

## Project name

**Akroasis** (ἀκρόασις) - Active listening as a discipline. Not passive hearing but *attentive reception* - the listener who brings understanding to what they receive. Aristotle's Physics was called "Physike Akroasis" - learning through careful listening.

| Layer | Reading |
|-------|---------|
| L1 | A platform for RF listening, communications sovereignty, and operational awareness |
| L2 | The system that makes electromagnetic and digital activity audible, intelligible, and actionable |
| L3 | Active listening as discipline - Aristotle's akroasis is attentive reception, bringing understanding to what is received |
| L4 | The system listens at every level: spectrum, mesh, network, proximity, open sources - then understands, then acts |

**Topology:** Pairs with Aletheia (you listen to unconceal). Akroasis as perception feeds Aletheia as understanding.

---

## Crate names

### Foundation layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **stoicheion** | στοιχεῖον | "element" | The elements - shared types, signal model, entity index, temporal engine, geographic primitives, hardware asset registry. The letters of the alphabet and the primary constituents of matter bore the same name: what everything else is composed of and stated in, carrying no argument of its own. |
| **tekmerion** | τεκμήριον | "proof" | The evidence - validated callers and authority, effect receipts, tamper-evident logging. In Attic legal usage a τεκμήριον was conclusive proof, distinguished from σημεῖον, a mere sign: not what suggests a conclusion but what establishes it. What the system can still assert about its own past. |
| **kryphos** | κρύφος | "crypto/identity" | The hidden - encryption, key management, forward secrecy, credential vault, identity/persona management, callsign compliance. That which is kryphos resists discovery by nature. |
| **lethe** | λήθη | "privacy" | Forgetting - VPN/proxy management, anonymization, metadata scrubbing, counter-surveillance, OPSEC scoring, IMSI catcher detection. The river that makes what passes through it unseen. Etymological complement to Aletheia: ἀ-λήθεια negates λήθη. Same root, opposite directions. Strongest topological pairing in the ecosystem. |

### Collection layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **syntonia** | συντονία | "radio management" | The act of bringing into harmony with a frequency. Radio management, frequency plans, serial protocols, vehicle telemetry. Syntonia is the ongoing condition of being properly tuned - not just the act but the state. |
| **kerykeion** | κηρύκειον | "mesh networking" | The staff Hermes carries - the instrument of the messenger. Mesh networking, DTN, multi-path routing, PACE communications. The herald doesn't choose the easy path - the herald finds ANY path. When the road is broken, the herald finds another way. |
| **dektis** | δέκτης | "SDR reception" | The one who receives. SDR hardware, I/Q pipeline, spectrum analysis, demodulation, EW detection (jamming, direction finding, emitter fingerprinting). The receiver doesn't choose what to hear - it hears everything. The skill is in distinguishing signal from noise, natural from intentional. |
| **engys** | ἐγγύς | "proximity protocols" | The near, at hand, close by. WiFi, BLE, Zigbee, Z-Wave, Thread, Matter, NFC, RFID monitoring. What's close enough to touch. The near field is the most intimate and most revealing. Every phone, every beacon, every smart lock broadcasting whether it intends to or not. |
| **aspis** | ἀσπίς | "network defense" | The hoplite's shield - not a wall (passive) but a weapon of formation. IDS/IPS orchestration, Suricata/Zeek, active response, CAN bus security, IoT monitoring. The aspis protects the soldier beside you. Defense as active, communal discipline. |
| **skopos** | σκοπός | "OSINT" | The scout on the high ground - the one who sees far and reports back. Feed aggregation, threat intel, asset discovery, web scraping, dark web monitoring. Not a spy (kataskopos) but an observer with purpose. Every scan has a target. |
| **peira** | πεῖρα | "offensive security" | The trial, the attempt. Penetration testing, vulnerability scanning, wireless security testing. You don't know your defenses until they're tested. Peira is experiential knowledge - the kind that only comes from trying. |

### Processing & analysis layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **semaino** | σημαίνω | "signal processing" | To make a sign - the oracle at Delphi "neither speaks nor conceals but *semainei*" - gives signs. Signal aggregation, convergence detection, anomaly baselines, alert deduplication. Reads signs in the noise. |
| **ichneutes** | ἰχνευτής | "intelligence analysis" | The one who follows tracks. Entity correlation, focal point detection, threat scoring, intelligence synthesis, forensic timeline reconstruction. In Sophocles' satyr play, the tracker follows Apollo's cattle by their hoofprints. The skill is not in seeing the print but in understanding where the trail leads. |

### Orchestration layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **praxis** | πρᾶξις | "automation" | Purposeful action. Event-driven triggers, playbooks, PACE communications, operational state machines, scheduled operations. Aristotle distinguished theoria (contemplation), poiesis (production), and praxis (purposeful action). Akroasis has all three - praxis completes the triad. Action taken because the situation demands it. |

### Model & knowledge layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **chorografia** | χωρογραφία | "geographic model" | Writing the land - geographic modeling, RF propagation, infrastructure dependency graph, cascade analysis, vehicle/foot navigation, military planning, space weather, terrain, offline map rendering. Ptolemy's term for detailed regional description - intimate knowledge of a specific place. |

The reference-library application is intentionally unnamed. **Pinax** remains
reserved for the standalone fleet relational engine at `forkwright/pinax`;
using the same identity for an Akroasis application crate would collapse engine
and domain authority. The original register/catalog resonance remains strong,
but the mesh collision is decisive. A distinct application name must pass the
Gnomon gate when the consumer is ready to exist.

### Interface layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **opsis** | ὄψις | "frontend" | The faculty of seeing - making the invisible visible. Operator surfaces: desktop-first via theatron (akroasis-desktop). Stack and order locked #118: desktop first, `akroasis-server` axum backend provides the typed API. TUI/web follow if use-cases arise. |

---

## Key topological relationships

- **Lethe ↔ Aletheia** - Same root (λήθη), opposite directions. Unconcealment for understanding, concealment for sovereignty. The strongest topological pairing.
- **Akroasis → Semaino → Ichneutes** - Listen → read signs → follow tracks. The intelligence pipeline.
- **Aspis ↔ Peira** - Shield and trial. Defense tested by offense. Complementary disciplines.
- **Engys → Lethe** - What's near reveals what you're leaking. Proximity intelligence feeds counter-surveillance.
- **Skopos → Semaino** - The scout reports to the sign reader. Collection feeds analysis.
- **Dektis → Semaino** - The receiver feeds the sign reader. Hardware reception → intelligence.
- **Ichneutes → Praxis** - Analysis produces understanding. Praxis converts understanding to action.
- **Reference library ↔ Chorografia** - The library holds the data (maps, terrain, specs). The model computes against it (propagation, routing, cascade).

---

## Rejected names

| Name | Meaning | Why Rejected |
|------|---------|-------------|
| **Pheme** (Φήμη) | Rumor, report, reputation | System is about listening, not hearsay. Pheme unconceals gossip, not attentive reception. |
| **Phrourion** (φρούριον) | Watchtower, garrison | Too defensive/military. Merged into Akroasis - the system actively listens, manages, communicates. |
| **Mouseion** (Μουσεῖον) | Seat of the Muses | Already used by another project (Aletheia); the application identity remains open. |
