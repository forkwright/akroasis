# Hybrid PQ Content-Key Wrapping Boundary

Issue #131 is design input for future multi-device content-key distribution.
Current main does not implement content-key wrapping, ML-KEM, P-256, AES-KW, or
a versioned wrapped-key envelope. The shipped vault path derives a symmetric
key from the operator passphrase and encrypts credential entries directly.

This note records the minimum design boundary before akroasis adds new
cryptographic dependencies. It is not an implementation of #131.

## Current State

- `kryphos` encrypts vault secrets with ChaCha20-Poly1305 using an Argon2id
  passphrase-derived `VaultKey`.
- Workspace dependencies include the current classical primitives used by the
  vault and mesh stack, including `chacha20poly1305`, `ed25519-dalek`, and
  `x25519-dalek`.
- There is no `ml-kem`, `p256`, `hkdf`, AES-KW, or `WrappedContentKey` model in
  the workspace.
- The offline reference store is still a planned `pinax` instance layout, not a
  checked-in runtime store or sharing workflow.

Adding a hybrid wrapper now would force protocol choices before the shared
content model exists.

## Target Envelope

The future wrapper should be versioned and per-recipient:

```text
WrappedContentKey {
    version,
    recipient_id,
    classical_public,
    kem_ciphertext,
    nonce_or_kw_metadata,
    encrypted_content_key,
}
```

Each recipient gets a separate wrapped copy of the same content key. Revoking a
device means generating a new content key or re-wrapping the current content key
only for the remaining recipients, depending on the store's forward-secrecy
requirement.

The domain tag for version 1 should be fixed before implementation, for example
`akroasis-hybrid-ck-wrap-v1`. Later protocol changes get new versions instead
of silent reinterpretation.

## Required Decisions

1. Classical half: use P-256 as described in the issue, or use the existing
   X25519 dependency to reduce the number of primitives in the workspace.
2. PQ half: select an ML-KEM-768 crate and require upstream/FIPS test vectors in
   the implementation PR.
3. Combiner: define HKDF extract/expand inputs, salt policy, and domain
   separation tags.
4. Envelope encryption: choose AES-KW, AES-GCM, or the existing
   ChaCha20-Poly1305 stack for the wrapped content key.
5. Feature gate: decide whether the first implementation is behind a preview
   feature until cryptographic review is complete.
6. Store integration: decide whether this belongs in `kryphos` alone or in the
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
