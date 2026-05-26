# PQ Content-Key Wrapping Boundary

Issue #131 is design input for future multi-device content-key distribution.
Current main does not implement content-key wrapping, ML-KEM, or a versioned
wrapped-key envelope. The shipped vault path derives a symmetric key from the
operator passphrase and encrypts credential entries directly.

This note records the minimum design boundary before akroasis adds new
cryptographic dependencies. It is not an implementation of #131.

## Operator Decision

akroasis crypto direction is **PQ-only ML-KEM**. The hybrid P-256/ECDH + ML-KEM
approach described in the original issue is **not** the target. Do not add
P-256, ECDH classical half, or a hybrid HKDF combiner to the workspace.

Rationale: the project does not need Web Crypto compatibility for this boundary,
and keeping the primitive set small reduces audit surface. ML-KEM-768 alone
provides the required post-quantum protection for offline content-key
distribution.

## Current State

- `kryphos` encrypts vault secrets with ChaCha20-Poly1305 using an Argon2id
  passphrase-derived `VaultKey`.
- Workspace dependencies include `chacha20poly1305`, `ed25519-dalek`, and
  `x25519-dalek`.
- There is no `ml-kem`, `hkdf`, or `WrappedContentKey` model in the workspace.
- The offline reference store is still a planned `pinax` instance layout, not a
  checked-in runtime store or sharing workflow.

Adding a content-key wrapper now would force protocol choices before the shared
content model exists.

## Target Envelope

The future wrapper should be versioned and per-recipient:

```text
WrappedContentKey {
    version,
    recipient_id,
    kem_ciphertext,
    nonce_or_aead_metadata,
    encrypted_content_key,
}
```

Each recipient gets a separate wrapped copy of the same content key. Revoking a
device means generating a new content key or re-wrapping the current content key
only for the remaining recipients, depending on the store's forward-secrecy
requirement.

The domain tag for version 1 should be fixed before implementation, for example
`akroasis-pq-ck-wrap-v1`. Later protocol changes get new versions instead of
silent reinterpretation.

## Required Decisions

1. PQ KEM: select an ML-KEM-768 crate and require upstream/FIPS test vectors in
   the implementation PR.
2. Key derivation: define HKDF extract/expand inputs, salt policy, and domain
   separation tags for the ML-KEM shared secret.
3. Envelope encryption: reuse the existing ChaCha20-Poly1305 stack or select a
   different AEAD for the wrapped content key.
4. Feature gate: decide whether the first implementation is behind a preview
   feature until cryptographic review is complete.
5. Store integration: decide whether this belongs in `kryphos` alone or in the
   first multi-device `pinax`/reference-store workflow.

## Implementation Gates

- No new cryptographic dependencies without an explicit design review.
- No unaudited preview code in the default binary path.
- Include ML-KEM test vectors and envelope round-trip vectors.
- Include negative tests for wrong recipient, wrong domain tag, corrupted KEM
  ciphertext, corrupted encrypted content key, and unsupported version.
- Document revocation semantics in the store that consumes the wrapper.

Until those decisions are made, #131 should remain design-input rather than a
code task.
