# sphragis

*σφραγίς — seal / signet*

Post-quantum hybrid sealing for multi-device content-key distribution. Seals a
32-byte content key for one or more recipient devices so only a holder of the
matching secret key can recover it, with security resting on **both** a classical
(X25519) and a post-quantum (ML-KEM-768) assumption.

> **UNAUDITED PREVIEW.** All cryptography is behind the `preview-pq` feature and
> is never on the default binary path. The known-answer tests prove the
> construction matches the published standards; they are not a substitute for a
> cryptographic review. See [`DECISION.md`](DECISION.md) and akroasis#131.

## Construction (v1)

- **KEM**: X-Wing (`draft-connolly-cfrg-xwing-kem`, IACR 2024/039) — X25519 +
  ML-KEM-768, combined via `SHA3-256(ss_M || ss_X || ct_X || pk_X || "\.//^\")`.
- **Envelope**: HKDF-SHA256 (null salt, versioned domain tag) → ChaCha20-Poly1305
  seals the content key; version + recipient id bound as AEAD associated data.
- **Wire**: versioned, per-recipient `WrappedContentKey` (CBOR).

## API

```rust,ignore
use sphragis::{HybridKem, seal_for, unseal};

// Each device holds an X-Wing keypair; publish the encapsulation (public) key.
let (dk, ek) = HybridKem::generate();

// Seal a content key for a set of devices (one wrap each, same content key).
let content_key = [0u8; 32];
let wrapped = seal_for(&content_key, &[ek])?;

// A device unseals its wrap with its decapsulation (secret) key.
let recovered = unseal(&dk, &wrapped[0])?;
assert_eq!(recovered.as_slice(), &content_key);
```

Revoke a device by re-running `seal_for` over the remaining recipients (with a
fresh content key for forward secrecy, or the same one for a cheap revoke).

## Features

- `preview-pq` — enables the hybrid KEM + envelope. **Off by default.**

## Why hybrid, not PQ-only

ML-KEM-768 alone places all trust in a 2024-vintage primitive and its pre-1.0
implementations. The hybrid forces an adversary to break both ML-KEM **and**
X25519 — matching TLS 1.3 (`X25519MLKEM768`), Signal (PQXDH), SSH
(`mlkem768x25519`), and the CFRG general-purpose answer (X-Wing).

## Acceptance gate

X-Wing draft KAT + RFC 5869 (HKDF-SHA256) + RFC 7748 (X25519) + round-trip,
multi-recipient, revocation, and negative tests. Run with:

```sh
cargo test -p sphragis --features preview-pq
```
