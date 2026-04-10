# Storage

> Standards for database access, migrations, connection management, data integrity, index rebuild, versioning, and consistency guarantees. Applies to any persistent storage layer.

---

## Migrations

### Versioned and checksummed

Every schema change is a numbered migration file. Applied migrations are recorded with:
- Version number (monotonic)
- SHA-384 checksum of the SQL/DDL text
- Applied timestamp
- Success flag

On startup, verify all applied migration checksums match source files. A mismatch means someone edited an applied migration. Fail loudly.

### Forward-only by default

Migrations add, never remove or modify existing columns in the same migration. Pattern for safe schema evolution:
1. Add column as nullable (old code compatible)
2. Deploy new code that writes to new column
3. Backfill existing rows
4. In separate migration, add NOT NULL constraint

### Reversible when needed

Reversible migrations have explicit UP and DOWN scripts. DOWN must be tested. Don't assume a DOWN migration that's never been run will work.

### Locking

Only one migration runs at a time. Use advisory locks (Postgres) or PRAGMA (SQLite) to prevent concurrent schema changes.

---

## Connection management

### Pooling

Every database connection goes through a pool. Never open connections directly in application code.

Pool configuration:
- **max_connections**: hard limit respecting database server limits
- **min_connections**: pre-opened for fast startup
- **acquire_timeout**: fail fast if no connection available
- **idle_timeout**: close idle connections to free resources
- **max_lifetime**: rotate connections to prevent stale TCP state

### Lifecycle callbacks

Pools support three hooks:
- **after_connect**: run once per new connection (PRAGMA setup, session variables)
- **before_acquire**: health-check before giving to caller (reject stale connections)
- **after_release**: reset state after caller returns connection

### Statement caching

Frequently-used queries get prepared statement caching. LRU eviction with bounded size (default 100 per connection). Key is the full query text.

---

## Query safety

### Parameterized only

All user-influenced values go through parameterized queries. Never string-concatenate values into SQL, Datalog, or any query language. This includes values from LLM/agent tool calls.

### Compile-time validation (when available)

Prefer compile-time query checking over runtime validation. If the query language supports it (SQL via sqlx, GraphQL via cynic), validate at build time. Cache query metadata for offline CI.

### Size limits

Every query has a timeout. Every result set has a row limit. Unbounded queries against large tables will eventually OOM.

---

## Transactions

### Explicit scope

Transactions have explicit begin/commit/rollback. No implicit transaction management hidden in ORM layers.

### Auto-rollback on drop

If a transaction goes out of scope without explicit commit, it rolls back. This prevents partial writes from leaked transactions.

### Savepoints for nesting

Nested transactions use savepoints. Track transaction depth to distinguish between outer transaction and inner savepoints.

---

## Data integrity

### Backup before migrate

Automated backup runs before every migration. If migration fails, restore is documented and tested.

### Checksum verification

Data integrity checks run periodically (not just on open). For SQLite: `PRAGMA integrity_check`. For custom stores: consistency verification between indices and source data.

### Index consistency

If an index can diverge from its source (e.g., search index vs database), the system must detect and repair divergence. A fact in the database but missing from the search index is a silent recall failure.

---

## Index rebuild

WHY: Indices are derived state. Any derived state can diverge from its source. A system that cannot rebuild its indices from source data is a system that cannot recover from corruption.

### Rebuild from source of truth

Every index must be rebuildable from the authoritative data it indexes. The rebuild path is a first-class operation, not an emergency procedure. Test it regularly, not just when things break.

Requirements:
- **Documented command or function**: a single invocation rebuilds the index from scratch. No multi-step runbooks with manual SQL.
- **Idempotent**: running rebuild twice produces the same result. Partial rebuilds from a crashed previous attempt do not corrupt the index.
- **Progress reporting**: rebuilds on large datasets emit structured progress events (records processed, estimated time remaining). A rebuild that runs silently for hours is indistinguishable from a hang.

### Incremental vs full rebuild

Prefer incremental rebuild (process only changed records) for routine maintenance. Require full rebuild capability for corruption recovery.

Incremental rebuild tracks a high-water mark (timestamp, sequence number, or change-data-capture cursor). The high-water mark is stored alongside the index, not in the source data. WHY: if the index is lost, the high-water mark should be lost too, triggering a full rebuild.

### Rebuild without downtime

Rebuild into a shadow copy, then swap atomically. Never rebuild in place on a live index.

Pattern:
1. Create new index alongside the live one
2. Populate from source data
3. Atomic swap (rename, pointer update, or config toggle)
4. Verify the new index serves correct results
5. Drop the old index

WHY: in-place rebuild means queries hit a partially-built index during the rebuild window. Users see missing results and blame the search, not the rebuild.

### Scheduled validation

Run index-vs-source consistency checks on a schedule, not just on suspicion. The check compares record counts and samples random records for presence in the index. Log divergence as an error, not a warning. WHY: silent index drift is a recall failure that no user will report because they don't know what they're not seeing.

### Rebuild vs repair

When an index is suspected corrupt, choose rebuild over repair. Repair assumes the existing structure is mostly correct and patches specific entries. Rebuild assumes nothing and reconstructs from source.

Use repair only when the corruption is localized and the repair tool can prove completeness (e.g., SQLite `REINDEX` on a single table with a known constraint violation). For custom indices (search, vector, graph), always rebuild from source.

WHY: repair tools operate on the index's own data structures. If those structures are corrupt in a way the repair tool does not detect, the "repaired" index silently returns wrong results. Rebuild uses the source of truth, bypassing the corrupted structures entirely.

### Validation after rebuild

After every rebuild, verify the result before swapping it into service:
- **Record count**: total records in the new index matches the source
- **Sample verification**: random sample of records present and correct in the new index
- **Known-answer queries**: a set of queries with known expected results returns those results against the new index
- **Performance bounds**: query latency and memory footprint are within expected ranges for the dataset size

Automated validation runs as part of the rebuild pipeline, not as a manual step. If validation fails, the rebuild is discarded and the previous index remains in service.

WHY: a rebuild that completes without error is not the same as a rebuild that produced correct results. Embedding model bugs, truncated source data, and configuration drift can all produce a "successful" rebuild of a wrong index.

### Rollback strategy

The previous index version is retained until the new index is validated and serving correctly.

Requirements:
- **Retain the previous index** for at least one full validation cycle after cutover
- **Store index metadata** alongside the index files: source data version, embedding model version, build timestamp, record count
- **Rollback is an atomic swap** back to the previous index, using the same mechanism as the cutover (see § Rebuild without downtime)

Never delete the previous index as part of the cutover step. Deletion is a separate operation that runs after the new index has been validated in production.

WHY: if the new index passes automated validation but fails on real query patterns (distribution shift, edge cases not in the validation set), the only recovery is to swap back. Deleting the previous index during cutover turns a recoverable situation into an outage.

---

## Data versioning

WHY: Data changes over time. Without versioning, you cannot answer "what did this record look like yesterday?" and you cannot safely roll back a bad data migration.

### Schema versioning

Schema versions are monotonic integers, tracked in the migration table (see § Migrations). The current schema version is queryable at runtime. Health checks and startup logs include the schema version. WHY: when debugging a production issue, the first question is "what schema is this instance running?" If you have to check migration history to answer that, you've already lost time.

### Record-level versioning

For data that supports concurrent or historical access, each record carries:
- **version**: monotonic integer, incremented on every write
- **updated_at**: timestamp of last modification

Optimistic concurrency uses the version field: UPDATE ... WHERE version = $expected_version. If no rows are affected, the record was modified since the caller last read it. Fail explicitly; do not silently overwrite.

WHY: last-write-wins is acceptable only when every writer has full context. In systems with multiple agents, API callers, or background jobs, last-write-wins silently destroys data.

### Append-only history (when warranted)

For audit-critical data (configuration changes, access grants, scoring decisions), use an append-only history table alongside the current-state table. The history table records every version of the record with the actor, timestamp, and change reason.

Do not use this pattern for high-volume operational data (metrics, logs, vector embeddings). WHY: history tables on hot-path data become the largest table in the database and the slowest to query. Use time-series storage for that.

### Migration versioning

Data migrations (backfills, transformations) are versioned alongside schema migrations. A data migration that runs outside the migration framework is invisible to the next operator. Record:
- What changed (table, column, transformation)
- How many records were affected
- Whether it is re-runnable

### Index versioning

When the embedding model, tokenizer, or similarity metric changes, the existing index is incompatible with new vectors. The index must be rebuilt, but the old index must continue serving until the new one is ready.

WHY: embedding model changes produce vectors in a different space. Querying a v1 index with v2 vectors returns meaningless results. This is not gradual degradation — it is a complete failure that looks like "search is broken."

Version strategy:
- **Version prefix**: each index is namespaced by its model version (e.g., `v1/`, `v2/`). The version identifier includes the model name, model version, and any parameters that affect the embedding space (dimensionality, distance metric).
- **Parallel build**: the new index is built alongside the live one. Source data is re-embedded with the new model and indexed into the new version namespace. The old index continues serving all queries during this process.
- **Atomic cutover**: once the new index is fully built and validated (see § Validation after rebuild), switch the query path to the new version. The old index remains available for rollback.
- **Stale version cleanup**: remove old index versions only after the new version has been validated in production for a defined retention period. Never remove the immediately previous version.

Record the active index version in a metadata store. Health checks and startup logs include the index version. WHY: when recall degrades, the first diagnostic question is "which embedding model is this index built from?" If the answer requires inspecting index files, you have already lost time.

---

## Consistency guarantees

WHY: Every storage system makes tradeoffs between consistency, availability, and performance. Document what your system actually guarantees so that callers don't assume stronger semantics than they get.

### Document the guarantee

Every storage layer's module-level documentation states its consistency model explicitly:
- **Strict serializable**: all operations appear to execute in a single total order consistent with real time. State this only if you enforce it.
- **Serializable**: all operations appear to execute in some total order (but not necessarily real-time order).
- **Read-committed**: reads see only committed data, but two reads in the same transaction may see different snapshots.
- **Eventual**: writes propagate asynchronously. Readers may see stale data for a bounded (documented) window.

Do not claim stronger guarantees than you enforce. A system documented as "eventual" that actually provides serializable is fine. The reverse is a latent data corruption bug.

### Cross-store consistency

When data spans multiple stores (e.g., metadata in SQLite, vectors in a vector index, files on disk), document which store is the source of truth and what happens when they disagree.

Rules:
- **One source of truth per datum.** If a fact exists in two stores, one is authoritative and the other is derived. Document which is which.
- **Detect divergence.** Periodic consistency checks compare derived stores against the source of truth. Log divergence as an error.
- **Resolve toward the source of truth.** When divergence is detected, the derived store is rebuilt from the source, never the reverse. WHY: resolving toward the derived store means a corrupted index can corrupt the source data.

### Write ordering

Operations that must be visible together are written in a single transaction. If the stores do not share a transaction boundary (cross-database, database-plus-filesystem), use a two-phase approach:
1. Write to the source of truth first
2. Write to derived stores
3. If step 2 fails, the record is in the source but missing from derived stores -- a state the rebuild mechanism (§ Index rebuild) can repair

Never write to derived stores first. WHY: a record in the index but not in the database is worse than a record in the database but not in the index. The first is a dangling reference that causes errors; the second is a recall gap that the next rebuild fixes.

### Crash consistency

If the process crashes mid-operation, the storage system must recover to a consistent state on restart. This means:
- **SQLite**: WAL mode with `PRAGMA journal_mode=wal`. Recovery is automatic on next open.
- **File-based stores**: write-to-temp, fsync, rename. Never overwrite in place (see § Atomic persistence).
- **Multi-file state**: use a manifest or write-ahead log to track which files constitute a consistent snapshot. On startup, verify the manifest and discard incomplete writes.

### Replica consistency

When indices are replicated across nodes or processes, document the replication model and its consistency bounds.

WHY: a replicated index that claims to be "consistent" but actually has a 30-second propagation window will produce different results depending on which replica serves the query. Callers that assume strong consistency will build logic that intermittently fails.

Rules:
- **Document the consistency window.** State the maximum time between a write to the source and its visibility in all replicas. If the window is unbounded, state that explicitly — "eventually consistent with no upper bound" is a valid model, but callers must know.
- **Read-after-write guarantee.** If a caller writes a record and immediately queries for it, define whether the query is guaranteed to see the write. If not, document the pattern callers should use (e.g., read from the primary, not from replicas, for read-after-write scenarios). Never silently serve stale reads after a write the caller just performed.
- **Conflict resolution.** When replicas can accept writes independently (multi-primary, partitioned writes), define the conflict resolution strategy:
  - **Last-write-wins (LWW)**: acceptable only when writes carry full state, not deltas. Document the clock source (wall clock, logical clock, hybrid).
  - **Source-of-truth wins**: one replica is authoritative. Conflicts resolve by overwriting non-authoritative replicas from the source. This is the default for derived indices (see § Cross-store consistency).
  - **Application-level merge**: conflicts are surfaced to the application for resolution. Use this only when automatic resolution would lose data.
- **Replica lag monitoring.** Emit a metric for replication lag (time or sequence distance between primary and each replica). Alert when lag exceeds the documented consistency window. WHY: a consistency guarantee without monitoring is a hope, not a guarantee.

---

## Error handling

See STANDARDS.md § Error Handling for universal principles.

### is_transient()

Database errors classify as transient (retry-safe) or permanent (don't retry). Connection drops, lock timeouts, and temporary unavailability are transient. Constraint violations, type mismatches, and syntax errors are permanent.

---

## Vector index patterns (from qdrant)

### HNSW construction

Single-threaded bootstrap for the first N points (prevents disconnected graph components), then parallel insertion. Track node readiness via atomic bitfield. Skip unindexed nodes during traversal.

### Visited-list pool

Pre-allocate per-thread visited lists for HNSW search. Counter-based marking (increment counter per search, compare to mark) avoids O(n) reset between searches. Pool bounded by thread count.

### Memory-mapped vectors

For datasets exceeding RAM, use mmap with access pattern hints:
- Random access mmap for individual vector lookups
- Sequential mmap for batch scanning
- Separate mmap'd deletion bitfield

Rely on OS page cache, not application-level LRU. Call `madvise(DONTNEED)` after batch operations.

### Adaptive search strategy

Select search algorithm at query time based on:
- Filter selectivity (cardinality estimation)
- Result set size vs full_scan_threshold
- Whether exact search is requested

Small result sets: plain scan. Large unfiltered: HNSW. Large filtered: HNSW with in-graph filtering.

### Atomic persistence

Write graph state to temp file, fsync, rename. Never overwrite the active file in place. Track operation versions for point-level error recovery.
