# Fjall Column Encryption Boundary

Issue #132 tracks a future declarative encryption layer for fjall-backed
stores. Current main does not have a generic table/column store abstraction:
the only fjall-backed runtime store is `kryphos::Vault`, and it already
encrypts every field of a credential record — secret, name, type, metadata,
status, and history — through two typed fields before serializing the row.

This note defines the boundary to use when akroasis adds its first mixed
plaintext/ciphertext fjall schema for signals, references, or other indexed
runtime data. It is not an implementation of #132.

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
- There is no fjall-backed signal store in current main. Mesh signals are
  produced in memory and forwarded through the collector/processor path.

Because of that shape, wrapping the vault in a generic column codec now would
add indirection around an already-specific and working encryption path.

## Target Shape

The first store that needs mixed encrypted and plaintext fields should own a
small codec boundary with these parts:

1. A stable field identity type for the store, such as `(StoreId, FieldId)` or a
   store-local enum. Do not use ad hoc string literals at call sites.
2. A single canonical encrypted-field registry in the owning crate, for example
   `ENCRYPTED_FIELDS`.
3. A `ColumnCodec` trait or equivalent helper that receives plaintext bytes,
   field identity, and domain context, then returns authenticated ciphertext.
4. A read path that decrypts mapped fields before returning typed domain values.
5. A migration rule for legacy plaintext rows. The preferred first rule is
   re-encrypt-on-write; a one-shot migration tool is only needed after a durable
   store with existing plaintext rows ships.

The registry should be declarative, but the owning store still decides which
fields may remain plaintext for indexing, filtering, or redacted display.

## Non-Goals

- Do not retrofit `kryphos::Vault` onto the generic `ColumnCodec`/
  `ENCRYPTED_FIELDS` shape only for consistency with future stores. Its
  existing typed-field model (now two fields: `encrypted_secret` and
  `encrypted_metadata`) is clearer than a generic map until another store
  proves the abstraction.
- Do not encrypt fields that are required for safe listing or lifecycle checks
  unless the caller has an explicit decrypted view.
- Do not add new cryptographic primitives for this issue. Reuse the existing
  vault AEAD unless a later key-management design selects a different content
  key envelope.

## Open Decisions

- Whether the first codec lives in `kryphos` as a shared storage utility or in
  the first crate that owns a mixed fjall schema.
- Whether encrypted fields should use the vault passphrase key, a per-store
  content key, or a future wrapped content-key design.
- Which fields in future signal/reference stores are safe to leave plaintext for
  search and indexing.

Implementation of #132 should wait for the first durable multi-field store that
needs this boundary.
