# Decision: Fleet-wide post-quantum hybrid sealing (`sphragis`)

Status: proposed (reference scaffold landed behind `preview-pq`)
Trigger: akroasis #131 (multi-device content-key wrapping for the offline reference store)
Scope: fleet-wide capability; first consumer akroasis (`pinax` reference store + `kryphos` vault)

## TL;DR

- KEM: **X-Wing** (X25519 + ML-KEM-768), per `draft-connolly-cfrg-xwing-kem` / IACR 2024/039.
  Not a bespoke HKDF combiner, not PQ-only.
- Combiner: **SHA3-256(ss_M || ss_X || ct_X || pk_X || `\.//^\`)** — the X-Wing
  construction. ML-KEM secret first (FIPS SP 800-56C ordering), binds the X25519
  ciphertext + recipient public key.
- Envelope: **HKDF-SHA256 (null salt)** expands the X-Wing shared secret with a
  versioned domain tag, then **ChaCha20-Poly1305** seals the 32-byte content key.
- Wire format: versioned, per-recipient `WrappedContentKey` (CBOR).
- Home: **new `sphragis` crate in the akroasis workspace**, domain-decoupled so it
  can be promoted to a standalone fleet repo when a second consumer appears.
- Gate: X-Wing draft KAT + RFC 5869 + RFC 8439 + RFC 7748 + FIPS-203 ACVP, behind
  `preview-pq`. Unaudited until cryptographic review.

## 1. Why hybrid, not PQ-only (reversing the 2026-05-26 doc)

`docs/pq-content-key-wrapping.md` (commit dbd91a9) records a "PQ-only ML-KEM"
direction and forbids the classical half. That is cryptographically regressive
and is **not** adopted here. Reasons:

1. ML-KEM was standardized in 2024 (FIPS-203). It is young. A future
   cryptanalytic break of ML-KEM alone — or, more likely, an *implementation*
   break in a pre-1.0 Rust ML-KEM crate — fully compromises a PQ-only system.
   Hybrid means an adversary must break **both** ML-KEM **and** X25519.
2. Every serious deployment is hybrid for exactly this reason: TLS 1.3
   (`X25519MLKEM768`, the de-facto browser/server default), Signal (PQXDH keeps
   classical X3DH), SSH (`sntrup761x25519`, `mlkem768x25519`), and the CFRG
   general-purpose answer, X-Wing. None ship PQ-only.
3. The "smaller audit surface" rationale is inverted: the classical half
   (X25519) is the *most* reviewed asymmetric primitive in existence; dropping
   it removes the trusted half and keeps the unproven one.
4. "No Web Crypto compatibility needed" is true and irrelevant — it argued
   against P-256, not against hybrid. Dropping P-256 in favour of the existing
   X25519 dependency is correct; dropping the classical half entirely is not.

The operator directive ("X25519 + ML-KEM-768 hybrid ... best-in-class,
no-compromise") supersedes the stale doc. This decision recommends replacing
`docs/pq-content-key-wrapping.md` with a hybrid spec.

## 2. Why X-Wing, not the baseline HKDF combiner

The directive's baseline was a PQXDH-style `HKDF-Extract(salt=null, ikm=DH||SS)`
combiner. That is sound, but X-Wing is *genuinely better-justified* for the
specific X25519+ML-KEM-768 pairing:

| Property | Baseline HKDF combiner | Thunderbolt port (#131) | **X-Wing (chosen)** |
|---|---|---|---|
| Standard | generic (PQXDH-shaped) | none (unaudited TS) | CFRG draft, IACR 2024/039 |
| Secret ordering | unspecified | X25519 first (wrong) | ML-KEM first (FIPS SP 800-56C) |
| Binds KEM ct / pk | no | no | binds ct_X + pk_X |
| Combiner primitive | HKDF-SHA256 | HKDF | SHA3-256 (matches ML-KEM's QROM) |
| IND-CCA proof | informal | none | formal (paper §6) |
| Published KAT | no | no | yes (draft Appendix C) |

X-Wing's security theorem: classically IND-CCA if the strong DH assumption holds
in the X25519 group, and post-quantum IND-CCA if ML-KEM-768 is IND-CCA and
SHA3-256 is a secure PRF. It deliberately omits `ct_M` from the combiner — proven
safe via ML-KEM's Fujisaki-Okamoto transform under QROM (paper §6); this is a
deliberate, justified optimization, not an oversight.

X-Wing replaces the *KEM*. HKDF-SHA256 + ChaCha20-Poly1305 are retained for the
*envelope* layer (key-wrapping of the content key) — that is where the directive's
HKDF/ChaCha baseline lands, and it keeps the wrapping AEAD identical to the
existing kryphos stack.

## 3. ML-KEM-768 vs -1024

**ML-KEM-768.** It is NIST Category 3 (≈AES-192). X-Wing is *defined only* over
ML-KEM-768 — choosing -1024 means abandoning the proven hybrid construction for a
bespoke one, a strictly worse trade. Cat-3 is the universal default (TLS, Signal,
X-Wing) precisely because the marginal security of Cat-5 buys little against any
realistic adversary while inflating ciphertext (1568 vs 1088 bytes) and key sizes.
For an offline reference-store wrapping a 32-byte content key, the size delta is
irrelevant, but the loss of the X-Wing proof and KATs is decisive. If a future
Cat-5 requirement appears, it is a new versioned construction (`v2`), not a tweak.

## 4. Envelope, key-wrapping, format

The content key (the symmetric key that actually encrypts reference-store
payloads / vault entries) is wrapped once per recipient device:

```
ss        = XWing.Encaps(recipient_xwing_pubkey)            # 32-byte hybrid secret + ct
wrap_key  = HKDF-SHA256(salt=<32 zero bytes>, ikm=ss,
                        info="akroasis-sphragis-ck-wrap-v1") # 32 bytes
sealed    = ChaCha20-Poly1305(key=wrap_key, nonce=random12,
                              aad=<canonical recipient-id || version>,
                              pt=content_key[32])
```

`WrappedContentKey` (CBOR, ciborium — matches akroasis serialization):

| Field | Type | Notes |
|---|---|---|
| `version` | `u8` | 1; future protocol changes increment, never silent |
| `recipient_id` | `[u8; 32]` | BLAKE3 of the recipient X-Wing encapsulation key |
| `kem_ciphertext` | `[u8; 1120]` | X-Wing ciphertext (ML-KEM ct 1088 \|\| X25519 ct 32) |
| `aead_nonce` | `[u8; 12]` | random per wrap |
| `sealed_key` | `Vec<u8>` | ChaCha20-Poly1305(content_key) = 32 + 16 tag |

Key-wrapping choice — **ChaCha20-Poly1305, not AES-KW**:
- AES-KW (RFC 3394) has no nonce and no AAD; it cannot bind the recipient-id /
  version into the wrap, and it adds an AES dependency the fleet does not have.
- The released `aes-kw` crate (0.3.0) had a failed release build in the current
  ecosystem churn — a fragile dependency for a no-compromise stack.
- ChaCha20-Poly1305 is already the akroasis AEAD; reuse keeps the trusted-compute
  base minimal and gives nonce + AAD domain-binding for free. The directive's
  "AES-GCM/AES-KW" were offered as options, not mandates; this is the
  better-justified envelope for *this* stack.

Multi-device + revocation:
- `seal_for(content_key, recipients) -> Vec<WrappedContentKey>` — one wrap per
  device, all decapsulating to the same content key.
- Revoke a device = re-run `seal_for` over the remaining recipients with a freshly
  generated content key (forward-secret rotation) or the same content key
  (cheap revoke) — the consuming store picks the policy; `sphragis` exposes both.

Crypto-agility / versioning:
- `version: u8` in the wire struct + the domain tag string both carry `v1`.
- The KEM identifier is implied by `version` (v1 = X-Wing/X25519+ML-KEM-768).
- A new primitive set = new `version` + new domain tag (`...-v2`); decoders reject
  unknown versions rather than guessing. Negative test enforces this.

## 5. Where it lives

**New `sphragis` crate inside the akroasis workspace** (`crates/sphragis`).

- Not folded into `kryphos`: kryphos is "credential vault + installation
  identity" (passphrase-derived symmetric vault key). Multi-recipient hybrid
  key-wrapping is a different concern; mixing them muddies both.
- Not a premature standalone fleet repo: today there is exactly one consumer
  (akroasis), and the entire fleet's only crypto lives in akroasis. A separate
  repo with one consumer is the speculative-extraction anti-pattern.
- Designed for promotion: `sphragis` takes only byte arrays and its own key
  types — zero akroasis-domain coupling (no `koinon`, no `GeoSignal`). When a
  second consumer appears (e.g. `theke` sync, `arche` secrets), it lifts to its
  own repo unchanged. This is the "shared fleet capability" the directive wants,
  staged correctly.

Name: σφραγίς (sphragis) — "seal / signet": the act of sealing a secret so only
the holder of the matching key can open it. Distinct from κρυφός (hidden/vault).

## 6. Dependencies (released, no release-candidates)

| Crate | Version | Role |
|---|---|---|
| `ml-kem` | 0.3.2 | FIPS-203 ML-KEM-768 (RustCrypto) |
| `x25519-dalek` | 2.0.1 | X25519 (already a workspace dep) |
| `sha3` | 0.10 | SHA3-256 combiner + SHAKE-256 seed expansion |
| `sha2` | 0.10 | HKDF-SHA256 hash |
| `hkdf` | 0.12 | RFC 5869 extract/expand (digest 0.10 generation — coherent with `sha2`/`sha3` 0.10; hkdf 0.13 requires sha2 0.11 and is incompatible) |
| `chacha20poly1305` | 0.10 | envelope AEAD (already a workspace dep) |
| `zeroize`, `subtle`, `blake3`, `ciborium`, `snafu` | workspace | hygiene/serde/errors |

Deliberately NOT the `x-wing` crate (0.1.0-rc.0): it pins a *release-candidate*
stack (`ml-kem 0.3.0-rc.0`, `x25519-dalek 3.0.0-pre.6`, `sha3 0.11.0-rc.7`) and
would pull a second, duplicate major of `x25519-dalek` alongside the workspace's
stable 2.0.1. We transcribe the ~15-line X-Wing combiner over the *released*
primitives and gate it on X-Wing's own published KAT — correctness is proven by
the vector, and the trusted-compute base stays on shipped crates. `x-wing` is the
migration target once it reaches a stable release and an audit.

`rand_core` coexistence: `ml-kem` 0.3.2's high-level API is `getrandom`-backed
(no rng handle), so it pulls `rand_core 0.10` purely transitively; `x25519-dalek`
2.0.1 uses `rand_core 0.6` at our call sites. The two majors coexist with no
call-site clash.

## 7. Acceptance gate (KATs)

Behind `preview-pq`, the test suite is the acceptance gate:
- X-Wing draft KAT vector (full hybrid encaps→decaps→shared-secret), from the
  RustCrypto x-wing `test-vectors.json` (draft-06).
- FIPS-203 / NIST ACVP ML-KEM-768 known-answer (primitive correctness).
- RFC 7748 §5.2 X25519 KAT.
- RFC 5869 HKDF-SHA256 KAT.
- RFC 8439 §2.8.2 ChaCha20-Poly1305 KAT.
- Negative tests: wrong recipient, wrong domain tag, corrupted KEM ciphertext,
  corrupted sealed key, unsupported version.

## 8. Unverified / preview status

Per #131 done-criterion 6, this lands explicitly **unaudited / Preview**:
- `preview-pq` feature, off by default; never in the default binary path.
- Crate-level `//! WARNING` and a `#[deprecated]`-style notice in docs until
  cryptographic review.
- The KATs prove the construction matches the published standard; they do **not**
  substitute for an audit of the implementation.
