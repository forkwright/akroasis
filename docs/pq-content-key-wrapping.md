# PQ Content-Key Wrapping (sphragis)

Issue #131 — multi-device content-key distribution for the offline reference
store. The `sphragis` crate implements a versioned, per-recipient hybrid
post-quantum content-key wrap behind the `preview-pq` feature.

WARNING: `sphragis` is unaudited preview cryptography. The known-answer tests
prove the construction matches the published standards; they are not a
substitute for cryptographic review. It is never on the default binary path.

## Construction (v1)

```text
# Encapsulate to a recipient's X-Wing public key:
(ct, ss)  = XWing.Encaps(recipient_ek)           # ss is 32 bytes
wrap_key  = HKDF-SHA256(salt = 0x00 * 32,
                        ikm  = ss,
                        info = "akroasis-sphragis-ck-wrap-v1")   # 32 bytes
nonce     = random(12)
sealed    = ChaCha20-Poly1305(key   = wrap_key,
                              nonce = nonce,
                              aad   = version(1) || recipient_id(32),
                              pt    = content_key(32))            # 48 bytes
```

- KEM: **X-Wing** (`draft-connolly-cfrg-xwing-kem`, IACR 2024/039) — X25519 +
  ML-KEM-768, combined as `SHA3-256(ss_M || ss_X || ct_X || pk_X || "\.//^\")`.
  ML-KEM shared secret first (FIPS SP 800-56C ordering); binds the X25519
  ciphertext and recipient public key.
- Envelope: HKDF-SHA256 (null salt) under a versioned domain tag, then
  ChaCha20-Poly1305 (the existing akroasis AEAD).

## WrappedContentKey (CBOR)

| Field | Type | Notes |
|---|---|---|
| `version` | `u8` | 1 |
| `recipient_id` | `[u8; 32]` | BLAKE3 of the recipient X-Wing encapsulation key |
| `kem_ciphertext` | `bytes` | X-Wing ct: ML-KEM ct (1088) \|\| X25519 ct (32) |
| `aead_nonce` | `[u8; 12]` | random per wrap |
| `sealed_key` | `bytes` | ChaCha20-Poly1305(content_key) = 48 bytes |

## Multi-device + revocation

`seal_for(content_key, recipients)` produces one `WrappedContentKey` per device;
all unseal to the same content key. Revoke a device by re-running `seal_for` over
the remaining recipients — with a freshly generated content key (forward-secret)
or the same one (cheap revoke); the consuming store chooses the policy.

## Crypto-agility

`version` and the domain tag both carry `v1`. A new primitive set is a new
`version` + a new tag (`...-v2`); decoders reject unknown versions rather than
reinterpreting bytes (enforced by a negative test).

## Why hybrid, not PQ-only

ML-KEM-768 alone places all trust in a 2024-vintage primitive and its pre-1.0
Rust implementations. The hybrid forces an adversary to break both ML-KEM **and**
X25519. This matches TLS 1.3 (`X25519MLKEM768`), Signal (PQXDH), SSH
(`mlkem768x25519`), and the CFRG general-purpose answer (X-Wing). See the crate
`DECISION.md` for the full rationale and alternatives considered.

## Status / gates

- `preview-pq` feature, off by default.
- Acceptance gate: X-Wing draft KAT + RFC 5869 (HKDF) + RFC 7748 (X25519) +
  round-trip + negative tests (wrong recipient, wrong version, tampered KEM ct,
  tampered sealed key). ML-KEM-768 (FIPS-203 ACVP) and ChaCha20-Poly1305
  (RFC 8439) primitive vectors are covered upstream.
- Promotion to the default path requires cryptographic review.
