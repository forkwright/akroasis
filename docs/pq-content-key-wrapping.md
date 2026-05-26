# PQ Content-Key Wrapping

Design decision for #131 (multi-device content-key distribution) is finalized.
Implementation lives in the standalone crate `forkwright/sphragis`.

## Adopted Design

**X-Wing hybrid KEM** (X25519 + ML-KEM-768), HKDF-SHA256 envelope, ChaCha20-Poly1305
seal, per-recipient `WrappedContentKey` (CBOR). Full rationale in
`forkwright/sphragis/DECISION.md`.

The earlier note's "PQ-only ML-KEM" direction is superseded. The hybrid construction
is the correct choice: an adversary must break both ML-KEM and X25519, matching
TLS 1.3 (`X25519MLKEM768`), Signal (PQXDH), and the CFRG general-purpose answer
(X-Wing).

## Dependency

```toml
sphragis = { git = "https://github.com/forkwright/sphragis", features = ["preview-pq"] }
```

Wire format: `WRAP_DOMAIN_V1 = "sphragis-ck-wrap-v1"`.

## Status

Unaudited preview behind `preview-pq`. The known-answer tests (X-Wing draft KAT,
RFC 5869, RFC 7748) prove the construction; cryptographic review per #131
done-criterion 6 is the remaining gate before promotion to default.

## Acceptance gate

```sh
cargo test -p sphragis --features preview-pq   # 12 tests
```
