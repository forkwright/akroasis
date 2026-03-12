# Akroasis — Name Registry

*ἀκρόασις — the act of hearing, attentive reception*

All names follow the [gnomon](https://github.com/forkwright/aletheia/blob/main/docs/gnomon.md) naming philosophy.

---

## Project Name

**Akroasis** (ἀκρόασις) — Active listening as a discipline. Not passive hearing but
*attentive reception* — the listener who brings understanding to what they receive.
Aristotle's Physics was called "Physike Akroasis" — learning through careful listening.

| Layer | Reading |
|-------|---------|
| L1 | A platform for RF listening — radio management, mesh monitoring, SDR reception, signal analysis |
| L2 | The system that makes electromagnetic activity audible and intelligible across all modes |
| L3 | Active listening as discipline — Aristotle's akroasis is attentive reception, bringing understanding to what is received |
| L4 | The system listens at every level: SDR receives spectrum, mesh relays what was heard, intelligence listens to all of it |

**Topology:** Pairs with Aletheia (you listen to unconceal). Akroasis as perception feeds Aletheia as understanding.

---

## Crate Names

### Foundation Layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **koinon** | κοινόν | "common/shared" | The commons — shared types, signal model, geographic primitives. What belongs to everyone. |
| **kryphos** | κρύφος | "crypto/security" | The hidden — encryption makes communication invisible to all but the intended. |
| **lethe** | λήθη | "privacy" | Forgetting — the river that makes what passes through it unseen. Etymological complement to Aletheia: ἀ-λήθεια negates λήθη. Aletheia unconceals truth. Lethe conceals the operator. Same root, opposite directions. |

### Collection Layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **syntonia** | συντονία | "radio management" | The act of bringing into harmony with a frequency. Radio management is fundamentally tuning — aligning equipment to the spectrum. |
| **kerykeion** | κηρύκειον | "mesh networking" | The staff Hermes carries — the instrument of the messenger. Mesh networking is herald work: carrying messages across distances. |
| **dektis** | δέκτης | "SDR reception" | The one who receives. SDR is pure reception — the disciplined act of receiving what the spectrum offers. |
| **aspis** | ἀσπίς | "network defense" | The hoplite's shield — not a wall (passive) but a weapon of formation. The aspis protects the soldier beside you. Defense as active, communal discipline. |
| **skopos** | σκοπός | "OSINT" | The scout on the high ground — the one who sees far and reports back. Not a spy (kataskopos), but an observer with purpose. Every scan has a target. |
| **peira** | πεῖρα | "offensive security" | The trial, the attempt. You don't know your defenses until they're tested. Experiential knowledge — the kind that only comes from trying. Penetration testing IS trial. |

### Processing & Analysis Layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **semaino** | σημαίνω | "signal processing" | To make a sign — the oracle at Delphi "neither speaks nor conceals but *semainei*" — gives signs. Signal aggregation reads signs in the noise. |
| **ichneutes** | ἰχνευτής | "intelligence analysis" | The one who follows tracks. Intelligence analysis follows traces across domains — correlating signals into meaning. |

### Model & Interface Layer

| Crate | Greek | Over | L3 Essential Nature |
|-------|-------|------|---------------------|
| **chorografia** | χωρογραφία | "geographic model" | Writing the land — geographic modeling, coverage analysis, infrastructure mapping. The world made legible through description. |
| **opsis** | ὄψις | "frontend/visualization" | The faculty of seeing — making the invisible visible. The interface that renders perception into sight. |

---

## Key Topological Relationships

- **Lethe ↔ Aletheia** — Strongest pairing. Same root (λήθη), opposite directions. Unconcealment for understanding, concealment for sovereignty.
- **Akroasis → Semaino → Ichneutes** — Listen → read signs → follow tracks. The intelligence pipeline.
- **Aspis ↔ Peira** — Shield and trial. Defense tested by offense. Complementary disciplines.
- **Skopos → Semaino** — The scout reports to the signal reader. Collection feeds analysis.
- **Dektis → Semaino** — The receiver feeds the sign-reader. Hardware reception → intelligence.

---

## Rejected Names

| Name | Meaning | Why Rejected |
|------|---------|-------------|
| **Pheme** (Φήμη) | Rumor, report, reputation | System is about listening, not hearsay. Pheme unconceals gossip, not attentive reception. |
| **Phrourion** (φρούριον) | Watchtower, garrison | Too defensive/military. The system actively listens, manages, communicates — not just watches. |
