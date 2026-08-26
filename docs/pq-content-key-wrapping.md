# PQ Content-Key Wrapping

The construction selected through #131 (multi-device content-key distribution)
remains the historical design basis. Implementation and cryptographic review
live in the standalone [`forkwright/sphragis`](https://github.com/forkwright/sphragis)
crate. Akroasis has no Sphragis dependency and has not adopted a release for a
durable consumer.

## Construction boundary

**X-Wing hybrid KEM** (X25519 + ML-KEM-768), HKDF-SHA256 envelope, ChaCha20-Poly1305
seal, per-recipient `WrappedContentKey` (CBOR). Full rationale in
`forkwright/sphragis/DECISION.md`.

The hybrid design supersedes the earlier note's "PQ-only ML-KEM" direction.
The selected future profile uses a hybrid construction so its security goal
does not depend on only ML-KEM or only X25519. Akroasis may consume only
Sphragis's versioned profile API. Akroasis must not call `hazmat` or raw-KEM
operations.

## Consumer adoption

Sphragis exposes its PQ profile as an unaudited, default-inert preview. No
published Sphragis tag is a reviewed consumer handoff. If #395 introduces a
preview integration before a reviewed release exists, it must remain behind a
default-off Akroasis feature and pin Sphragis to an exact immutable revision.
A moving Git branch or repository HEAD is not an acceptable dependency.

The intended wire domain is `WRAP_DOMAIN_V1 = "sphragis-ck-wrap-v1"`.

## Evidence and promotion ownership

Known-answer tests for X-Wing, HKDF, and X25519 establish conformance to their
claimed vectors. They do not establish implementation security or substitute
for qualified independent cryptographic review.

[`forkwright/sphragis#43`](https://github.com/forkwright/sphragis/issues/43)
owns qualified review and promotion to a reviewed Sphragis release. Akroasis
#395 owns the exact immutable revision or release tag accepted by the first
consumer. Akroasis must keep preview use default-off before that exact handoff.

## Preview verification

Verification belongs to Sphragis, not the Akroasis workspace. Run it in a
Sphragis checkout at the exact immutable revision recorded by the consumer, or
use Sphragis CI for that same revision:

```sh
cargo test --features preview-pq
```
