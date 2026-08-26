# Reference-Library Encryption Authority Boundary

Issue #132 was filed on the premise that Akroasis would own a generic
declarative encryption codec over a future fjall-backed reference store. That
premise is superseded. Standalone
[`forkwright/pinax`](https://github.com/forkwright/pinax) exclusively owns the
relational engine and its page-at-rest encryption. Akroasis owns only the
reference-library domain-envelope policy above that engine.

The historical filename remains so existing issue and review links still
resolve. This living note maintains the corrected authority boundary. It is not
an implementation of #132.

## Current State

- `crates/kryphos/src/storage.rs` owns one fjall keyspace named `entries`.
- `StoredEntry` holds two independently-nonced ChaCha20-Poly1305 ciphertexts:
  `encrypted_secret` and `encrypted_metadata`. `encrypted_metadata` decrypts to
  an `EntryMetadataRecord` carrying name, credential type, metadata, status,
  and history (akroasis#215) — nothing about a credential is plaintext at
  rest. The fjall record KEY is a keyed-BLAKE3 hash of the name, not the name
  itself, so no credential name appears verbatim in the fjall data directory.
- `Vault::list` decrypts only `encrypted_metadata`, never `encrypted_secret` —
  callers already hold the vault key (an unlocked `Vault`), so this is the
  "explicit decrypted view" the Non-Goals below require, not a bypass of it.
- `Vault::add`, `Vault::get`, `Vault::rotate`, `Vault::revoke`, and
  `Vault::history` call the existing ChaCha20-Poly1305 helpers directly for
  both fields via `Vault::encrypt_metadata`/`Vault::decrypt_metadata`.
- There is no fjall-backed signal or reference store in current main. Mesh
  signals are produced in memory and forwarded through the
  collector/processor path.

Because of that shape, wrapping the vault in a generic column codec now would
add indirection around an already-specific and working encryption path.

## Authority split

Pinax owns encryption of its database pages and all storage-engine artifacts.
Akroasis consumes that contract; it does not reproduce it with a local fjall
wrapper, `ColumnCodec`, or table/column registry.

Akroasis owns a canonical, typed domain policy that decides which reference
payloads require an authenticated content envelope before they cross the
Pinax API. Every write must consult that policy, and reads must authenticate
and decrypt an envelope before returning a typed domain value. The policy uses
domain identities, not Pinax page, table, or column identifiers.

Sphragis supplies recipient distribution for domain content keys. Its profile
API wraps keys for recipients and epochs; it does not encrypt Pinax pages.
Pinax page encryption protects engine-managed data at rest, but it does not
replace an Akroasis envelope whose recipient and revocation semantics must
survive export or replication.

Any legacy plaintext reference data must have an explicit migration rule
before default promotion. A preview fixture may be discarded and recreated
only while the preview contract explicitly permits that behavior.

## Non-Goals

- Do not add a local `crates/pinax`, an Akroasis-owned relational engine, or a
  direct fjall reference store.
- Do not retrofit `kryphos::Vault`; its typed encrypted fields already fit its
  separate credential-vault domain.
- Do not copy Pinax page-encryption policy into Akroasis or treat page
  encryption as recipient distribution.
- Do not call Sphragis `hazmat` or raw KEM operations. Its versioned profile API
  is the only permitted recipient-wrapping boundary. Sphragis exposes its PQ
  profile as an unaudited, default-inert preview.
  [`forkwright/sphragis#43`](https://github.com/forkwright/sphragis/issues/43)
  owns qualified review and release promotion. Akroasis #395 owns the exact
  consumer handoff.

## Review gates

Issue #132 remains open until a real reference-library consumer demonstrates
all of the following:

1. Pinax page encryption is enabled and its producer-owned contract covers the
   engine artifacts used by the integration.
2. One typed Akroasis policy is the sole authority for domain envelopes; no
   call site can silently bypass it.
3. An on-disk adversarial test cannot find protected reference plaintext, while
   the authenticated read path returns the original typed value.
4. Migration behavior for any prior plaintext fixture or durable data is
   executable and tested.
5. The recipient and epoch lifecycle tracked by #395 remains distinct from,
   and composes with, Pinax page encryption.
