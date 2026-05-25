# Reference Store Layout

Akroasis owns the long-term offline reference store through `pinax`, the planned
knowledge layer. The repository does not ship an `instance/` tree today; this
document defines the target layout and migration policy so the existing
`theke/_reference` staging area can move only after the source inventory is
visible and checksummed.

## Canonical path

The canonical runtime path is:

```text
${AKROASIS_INSTANCE:-~/akroasis/instance}/reference/
```

The store is instance data, not source code. It should not be committed to this
repository. Repo docs and manifests may describe captures, checksums, and index
state, but the payloads live under the operator instance root.

Expected top-level layout:

```text
reference/
  captures/
    frozen/
    refreshable/
  indexes/
    rg/
    tantivy/
    embeddings/
  manifests/
  adapters/
  staging/
```

`captures/frozen/` holds material captured for grid-down use where upstream
availability is the risk. `captures/refreshable/` holds mirrors that can be
re-pulled from upstream while the network is available. `indexes/` is generated
and disposable; it can be rebuilt from captures and manifests. `staging/` is for
imports before checksums, content class, license notes, and refresh policy are
assigned.

## Content classes

Initial supported content classes:

| Class | Destination | Notes |
|-------|-------------|-------|
| Dash docsets | `captures/frozen/docsets/` or `captures/refreshable/docsets/` | Preserve upstream docset structure and record feed URL when available. |
| Curated PDFs | `captures/frozen/pdfs/` | Keep original filename plus manifest title, source URL, capture date, and checksum. |
| Preserved wikis | `captures/frozen/wikis/` | Store static export plus source revision or dump metadata. |
| Stack Exchange snapshots | `captures/frozen/stack-exchange/` | Store dump metadata and license/provenance in manifest. |
| Model-agnostic corpora | `captures/frozen/corpora/` | Plain text, markdown, or other durable formats preferred. |
| Frequency/protocol references | `captures/refreshable/radio/` or `captures/frozen/radio/` | Use refreshable for public frequency databases; frozen for field manuals and protocol captures. |

Per-crate docs stay in the crate or `docs/`. Operational reference payloads,
large manuals, docsets, and preserved external corpora live in the instance
reference store. Theke may link to this store, but it should not own akroasis
reference data after migration.

## Manifests

Each capture set has a manifest under `manifests/` with:

- stable capture id
- content class
- relative payload path
- title and source
- upstream URL or local provenance
- capture timestamp
- refresh policy: `frozen` or `refreshable`
- checksum set
- license or redistribution notes
- index adapters enabled for the payload

Manifests are the contract between raw filesystem access, index builders, and
future query APIs.

## Agent access tiers

Access starts with the filesystem and grows toward services:

1. Raw tree: agents and humans can use `rg`, `fd`, and direct file reads under
   `reference/captures/`.
2. Text index: generated full-text indexes under `reference/indexes/` for fast
   local search.
3. Embedded index: optional vector indexes for semantic lookup. These are
   generated artifacts and must be rebuildable from captures.
4. Query API: future MCP or HTTP tools read manifests and indexes instead of
   scraping arbitrary paths. This is the programmatic surface; it should not
   become the only way to access the corpus.

## Drive and symlink policy

Docsets alone were reported as 6.1 GB in the staging issue, and the expected
store is 50-100 GB. The instance root may therefore live on a larger mount such
as `/storage` or another operator-selected drive. The canonical path remains
`~/akroasis/instance/reference`; if payloads live elsewhere, use a single
symlink at `~/akroasis/instance/reference` and keep all internal paths relative
to that canonical root.

Do not scatter per-content symlinks across `/`, `/data`, `/storage`, and
removable menos drives. Mount or link the store root once, then let manifests
describe the content inside it.

## Migration gates

Before moving data out of the current staging area:

1. Verify the source path exists on the migration host.
2. Generate checksums for every payload.
3. Classify each content set as frozen or refreshable.
4. Write manifests for each content set.
5. Create the target `reference/` tree on the chosen drive.
6. Copy payloads into `staging/`, verify checksums, then promote them into
   `captures/`.
7. Build the raw/text indexes.
8. Update fleet pointers that currently name the staging path.
9. Remove the staging copy only after checksum verification and operator signoff.

## Current repo state

As of this design note, akroasis only documents the planned `pinax` knowledge
layer. There is no checked-in `instance/` directory, no `crates/pinax`, and no
verified local copy of the source `theke/_reference` tree in this worktree.
