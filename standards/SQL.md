# SQL

> Additive to STANDARDS.md. Read that first. Everything here is SQL-specific.
>
> Primary dialect: **SQLite** (kanon uses rusqlite). PostgreSQL and Redshift notes where they diverge. Mneme Datalog and Room (Android) in appendix.
>
> **Key decisions:** Uppercase keywords, CTEs over subqueries, explicit columns, nullif division, STRICT tables (SQLite), identity columns (PG), JSON as TEXT with json_each() for querying, index every FK and filter column.

---

## Dialect selection

| Dialect | When | Configuration |
|---------|------|---------------|
| **SQLite** | Embedded/local databases, kanon internals, mobile (Room) | `.kanon.yml`: `dialect: sqlite` |
| **PostgreSQL** | Server databases, multi-user OLTP, PG10+ features | `.kanon.yml`: `dialect: postgresql` |
| **Redshift** | Data warehouse, columnar analytics | Follow PostgreSQL rules except where noted in § Redshift compatibility |

Universal rules (Formatting, Naming, Structure, Testing) apply to all dialects. When behavior diverges, guidance appears under dialect-specific headings within each section or under the dedicated dialect sections (§ SQLite, § PostgreSQL). Portable SQL uses `CAST()` syntax and avoids dialect-specific extensions.

---

## Formatting

### Keywords

Uppercase keywords everywhere. `SELECT`, `FROM`, `WHERE`, `JOIN`, `GROUP BY`, `ORDER BY`.

### Layout

- One clause per line
- `select` columns each on their own line when more than two
- Indent joins one level. Join conditions on same line as `on`.
- `case` statements: one `when` per line, `end as column_name`
- Trailing commas in select lists (easier diffs)
- Max line length: 100 characters

```sql
SELECT
    e.employer_id,
    e.name,
    count(c.case_id) AS total_cases,
FROM employers e
    LEFT JOIN cases c ON c.employer_id = e.employer_id
WHERE
    e.created_at >= '2025-01-01'
GROUP BY
    e.employer_id,
    e.name
ORDER BY
    total_cases DESC
```

### Operators

- `<>` not `!=` for inequality (SQL standard, ISO 9075)
- `is null` / `is not null`, never `= null`
- `coalesce()` nulls at the query boundary, not mid-CTE

---

## Naming

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Tables / Views | `snake_case` | `case_appointments`, `active_members` |
| Columns | `snake_case` | `employer_id`, `created_at` |
| CTEs | `snake_case`, descriptive | `active_cases_by_employer` |
| Prefixes (tables) | `dim_`, `fact_`, `agg_`, `map_`, `v_` | `dim_employer`, `fact_billing` |
| Date columns | `_date` suffix | `start_date`, `close_date` |
| Timestamp columns | `_at` suffix | `created_at`, `updated_at` |
| ID columns | `_id` suffix matching dimension | `employer_id`, `member_id` |

### CTE naming

CTEs are self-documenting. Pattern: `{what}_{by/for}_{grain}`

The name answers: what does it contain, at what grain, what filter is applied?

| Good | Bad |
|------|-----|
| `first_appointment_noshow_by_member` | `initial_noshows` |
| `case_activity_summary` | `cte1` |
| `members_with_billable_event` | `temp_members` |
| `employer_contract_periods` | `data` |

### Table aliases

Alias every table. Short but meaningful: `e` for `employers`, `c` for `cases`, `m` for `members`. Consistent within a query and across related queries.

---

## Structure

### CTEs over subqueries

Always. CTEs are readable, debuggable, and composable. Name them well and they document the query's logic.

```sql
WITH active_cases AS (
    SELECT case_id, employer_id, status
    FROM cases
    WHERE status = 'active'
),
employer_summary AS (
    SELECT
        employer_id,
        count(*) AS active_count,
    FROM active_cases
    GROUP BY employer_id
)
SELECT *
FROM employer_summary
WHERE active_count > 10
```

### Explicit column lists

Never `select *` except in ad-hoc exploration. Name every column. This documents intent and prevents breakage when schemas change.

### JOINs: be intentional

Don't default to `left join` without thinking about the semantics.

| Join | When |
|------|------|
| `join` (inner) | Both sides required: unmatched rows should be excluded |
| `left join` | Preserve left side: nulls acceptable for unmatched |
| `full outer join` | Need all records from both sides |

Common mistake: `left join` everywhere "to be safe" when inner join semantics are needed, then filtering out nulls later.

### Division safety

`nullif(denominator, 0)` for all division. No exceptions.

```sql
SELECT
    numerator * 1.0 / nullif(denominator, 0) AS rate,
```

### Percentages and formatting

Return percentages as decimals (0.42, not 42). Format at the display layer.

### Window functions

Window functions compute across rows without collapsing them. Primary use cases: ranking, running totals, row-relative comparisons, and deduplication.

```sql
-- Rank within partition (deduplication pattern)
SELECT *
FROM (
    SELECT
        *,
        row_number() OVER (PARTITION BY member_id ORDER BY created_at DESC) AS rn,
    FROM appointments
)
WHERE rn = 1

-- Previous value comparison
SELECT
    month,
    revenue,
    lag(revenue) OVER (ORDER BY month) AS prev_revenue,
    revenue - lag(revenue) OVER (ORDER BY month) AS delta,
FROM monthly_revenue
```

Key rules:
- `row_number()` for dedup, `rank()` for ties, `dense_rank()` for gapless ties
- `lag()`/`lead()` for row-relative comparisons: avoid self-joins for previous/next row access
- Default frame is `range between unbounded preceding and current row`: this is unintuitive for running averages. Use explicit `rows between` when computing running aggregates:
  ```sql
  avg(revenue) OVER (ORDER BY month ROWS BETWEEN 2 PRECEDING AND CURRENT ROW) AS rolling_3mo_avg
 ```
- Window functions execute after `where`/`group by`/`having`: you cannot filter on a window function result in the same query level. Wrap in a CTE.

### Transaction isolation

Choose isolation level deliberately. The default (`read committed` in PostgreSQL, `serializable` in SQLite) is not always correct.

| Level | Guarantees | Risk | Use |
|-------|-----------|------|-----|
| `read committed` | No dirty reads | Non-repeatable reads, phantom rows | Default OLTP, independent statements |
| `repeatable read` | Snapshot at transaction start | Serialization failures (retry required) | Read-heavy transactions needing consistency |
| `serializable` | Full serializability | Higher serialization failure rate | Financial calculations, inventory, anything with read-then-write dependencies |

Rules:
- Read-then-write patterns (check balance, then debit) require at minimum `repeatable read`
- Always handle serialization failures with retry logic: they are expected, not exceptional
- SQLite in WAL mode is effectively `serializable` for writes (single writer). Configure `busy_timeout` instead of retrying manually.
- Never weaken isolation "for performance" without proving the weaker level is correct for the workload

### Concurrency patterns

Transaction isolation levels prevent data anomalies at the database level. These patterns handle application-level concurrency where isolation alone is insufficient.

#### Write skew

Write skew occurs when two concurrent transactions read the same rows, make decisions based on those reads, and write non-conflicting rows -- but the combined result violates a business invariant.

Example: two on-call doctors both read "2 doctors on-call", both decide to remove themselves, both commit successfully, leaving 0 doctors on-call. Neither transaction saw a conflict because they wrote different rows.

Prevention by dialect:
- **PostgreSQL**: Use `SERIALIZABLE` isolation. The database detects write skew and aborts one transaction. Always retry on serialization failure (`SQLSTATE 40001`).
- **SQLite**: Single-writer architecture in WAL mode prevents write skew by default (writes are serialized).
- **Application-level**: `SELECT ... FOR UPDATE` on the rows that inform the decision. This locks the read set, preventing concurrent modifications.

```sql
-- PostgreSQL: prevent write skew with explicit locking
BEGIN;
SELECT count(*) FROM oncall_doctors WHERE on_duty = true FOR UPDATE;
-- decision logic in application
UPDATE oncall_doctors SET on_duty = false WHERE doctor_id = ?;
COMMIT;
```

#### Advisory locks (PostgreSQL)

Use advisory locks for application-level mutual exclusion that does not map to row-level locking.

```sql
-- Session-level lock (held until session ends or explicit unlock)
SELECT pg_advisory_lock(hashtext('migration-runner'));
-- ... do work ...
SELECT pg_advisory_unlock(hashtext('migration-runner'));

-- Transaction-level lock (released at COMMIT/ROLLBACK)
SELECT pg_advisory_xact_lock(hashtext('batch-processor'));

-- Try without blocking (returns false if already held)
SELECT pg_try_advisory_lock(hashtext('singleton-job'));
```

Use cases:
- Singleton job execution (cron-like tasks that must not overlap)
- Migration locking (prevent concurrent schema changes)
- Rate limiting at the database level

Key rules:
- Use `hashtext()` or a consistent hashing scheme for lock IDs -- raw integer literals are collision-prone across unrelated code paths
- Session-level locks survive transaction boundaries -- always explicitly unlock or use transaction-level variants
- Advisory locks do not appear in `pg_locks` with meaningful names -- document the mapping between lock IDs and their purposes in application code

#### Optimistic concurrency

For low-contention workloads where locking would add unnecessary overhead:

```sql
-- Version column pattern
UPDATE accounts
SET balance = balance - 100, version = version + 1
WHERE account_id = ? AND version = ?;
-- If affected_rows = 0, another transaction modified the row. Retry.
```

Prefer optimistic concurrency when:
- Conflicts are rare (<5% of transactions)
- The retry cost is low (read + recompute + retry)
- Row-level locking would create contention hotspots

Prefer pessimistic locking (`FOR UPDATE`, advisory locks) when:
- Conflicts are frequent
- The computation between read and write is expensive
- Correctness is more important than throughput

---

## JSON columns

### Storage strategy

Store JSON as `TEXT` in SQLite. There is no native JSON type; the json functions operate on TEXT values. Validate structure at the application boundary before insertion.

**When to use JSON columns:**
- Variable-length lists of simple values (e.g., crate names, file paths, tags)
- Semi-structured metadata that varies between rows
- Blob-like payloads consumed whole by the application (e.g., checkpoint snapshots, health snapshots)

**When NOT to use JSON columns:**
- Data you regularly filter or join on. Extract to a proper column or junction table instead.
- Relational data with foreign key semantics. JSON defeats referential integrity.

### SQLite JSON functions

SQLite provides two function families: `json_*` (returns TEXT) and `jsonb_*` (returns binary, 3.45+). Use the `json_*` family unless profiling shows the binary variants improve performance on large documents.

Core functions:

| Function | Purpose | Example |
|----------|---------|---------|
| `json(value)` | Validate and minify | `json('{"a": 1}')` |
| `json_extract(json, path)` | Extract scalar | `json_extract(config, '$.timeout')` |
| `->` / `->>` | Extract (JSON / scalar) | `config ->> '$.timeout'` |
| `json_each(json)` | Unnest array to rows | `FROM table, json_each(table.tags)` |
| `json_group_array(value)` | Aggregate rows to array | `json_group_array(name)` |
| `json_valid(value)` | Check well-formedness | `CHECK (json_valid(payload))` |

### Querying JSON arrays with json_each

The primary pattern for querying JSON array columns in kanon. Use `json_each()` as a table-valued function in a cross join:

```sql
-- Unnest a JSON array column to filter on individual elements
SELECT sm.session_id, je.value AS crate_name
FROM session_metadata sm, json_each(sm.crates) je
WHERE je.value = 'phronesis'

-- Aggregate across JSON array elements
SELECT
    je.value AS crate_name,
    count(*) AS session_count,
    sum(CASE WHEN sm.qa_verdict = 'PASS' THEN 1 ELSE 0 END) AS pass_count,
FROM session_metadata sm, json_each(sm.crates) je
WHERE sm.completed_at >= ?1
GROUP BY je.value
ORDER BY session_count DESC
```

Key rules:
- The comma-join syntax (`FROM t, json_each(t.col)`) is an implicit cross join. Every row produces one output row per array element. Be aware of row multiplication in aggregations.
- `je.value` gives the element value; `je.key` gives the array index (0-based).
- For nested JSON objects, chain paths: `json_each(col, '$.items')`.
- Parameterize filter values, not the JSON path.

### Dynamic IN clauses with json_each

When building an IN clause from application-provided lists, prefer `json_each()` over string-concatenated placeholders when the list is already JSON:

```sql
-- Application passes a JSON array as a single parameter
SELECT * FROM sessions
WHERE prompt_number IN (SELECT value FROM json_each(?1))
```

When the list comes from individual values (not already JSON), positional placeholders are acceptable:

```sql
-- Individual parameters: ?3, ?4, ?5 etc.
WHERE je.value IN (?3, ?4, ?5)
```

### Enforcing JSON validity

Use CHECK constraints on JSON columns for defense in depth:

```sql
CREATE TABLE events (
    id      INTEGER PRIMARY KEY,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    tags    TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(tags))
) STRICT;
```

The application boundary is the primary validator; the CHECK constraint catches bugs that bypass validation.

### PostgreSQL JSON

PostgreSQL has native `json` and `jsonb` types. Use `jsonb` exclusively -- it is binary, indexable, and supports containment operators.

| Operator | Purpose | Example |
|----------|---------|---------|
| `->` | Extract JSON element | `config -> 'timeout'` |
| `->>` | Extract as text | `config ->> 'timeout'` |
| `@>` | Contains | `tags @> '["urgent"]'` |
| `?` | Key exists | `config ? 'timeout'` |
| `jsonb_array_elements()` | Unnest array | `FROM jsonb_array_elements(tags)` |

Index with GIN for containment/existence queries:

```sql
CREATE INDEX idx_events_tags ON events USING GIN (tags);
-- Enables: WHERE tags @> '["urgent"]'
```

Use `JSON_TABLE` (PG17+) to convert JSON to relational rows in a `FROM` clause:

```sql
SELECT j.*
FROM api_responses r,
    json_table(
        r.body, '$.items[*]'
        COLUMNS (
            id integer PATH '$.id',
            name text PATH '$.name',
            status text PATH '$.status'
        )
    ) AS j
WHERE j.status = 'active'
```

Prefer `json_table` over chains of `json_extract_path` / `->>` for structured JSON.

**Path queries** (`jsonb_path_query`, PG12+) for complex JSON traversal:

```sql
-- Find all items where price > 100
SELECT jsonb_path_query(data, '$.items[*] ? (@.price > 100)') AS expensive_items
FROM orders;

-- Check existence (returns boolean)
SELECT jsonb_path_exists(config, '$.features[*] ? (@ == "dark_mode")');

-- Extract with default
SELECT jsonb_path_query_first(config, '$.timeout') AS timeout
FROM settings;
```

Prefer `jsonb_path_query` over chains of `->` / `->>` for:
- Filtered array traversal (the `?` filter is more readable than subqueries)
- Existence checks on nested structures
- Queries that would otherwise require `jsonb_array_elements` + `WHERE`

**Casting traps:**

```sql
-- PostgreSQL: idiomatic cast
SELECT '{"a": 1}'::jsonb;

-- Portable (but verbose)
SELECT CAST('{"a": 1}' AS jsonb);

-- TRAP: ->> returns text, not the original JSON type
SELECT config ->> 'timeout';            -- returns TEXT '30', not INTEGER 30
SELECT (config ->> 'timeout')::integer;  -- explicit cast needed for arithmetic

-- TRAP: -> returns jsonb, ->> returns text
SELECT config -> 'nested' -> 'key';     -- returns jsonb (for further traversal)
SELECT config ->> 'key';                -- returns text (for comparison/output)
```

Rule: `->` for traversal (returns jsonb), `->>` for terminal extraction (returns text). Always cast `->>` results explicitly when using them in arithmetic or typed comparisons. Use `CAST()` instead of `::` when portability matters.

---

## Index strategy

### When to index

Index every column that appears in:
- `WHERE` clauses (filter columns)
- `JOIN ... ON` conditions (foreign keys)
- `ORDER BY` clauses on large tables (>1K rows)
- `GROUP BY` when used with selective `WHERE` filters

Do NOT index:
- Columns with very low cardinality (boolean flags on small tables) -- the planner will prefer a table scan
- Tables under ~100 rows -- full scan is faster than index lookup
- Write-heavy columns on write-dominated workloads without measuring the write penalty

### Index naming

Convention: `idx_{table}_{columns}` for regular indexes, `idx_{table}_{purpose}` for composite or special-purpose indexes.

```sql
CREATE INDEX idx_sessions_dispatch ON sessions(dispatch_id);
CREATE INDEX idx_corrective_outcomes_lookup
    ON corrective_outcomes(project, crate_name, failure_type);
CREATE INDEX idx_pr_rebase_attempts_stale
    ON pr_rebase_attempts(last_attempt_at);
```

### Composite indexes

Column order matters. The index is a sorted tree; the leftmost column is the primary sort. Put the most selective (highest cardinality) column first, unless the access pattern always filters on a specific column.

```sql
-- Good: queries always filter on project, then narrow by crate_name and failure_type
CREATE INDEX idx_corrective_outcomes_lookup
    ON corrective_outcomes(project, crate_name, failure_type);

-- The index above satisfies:
--   WHERE project = ?                               (uses prefix)
--   WHERE project = ? AND crate_name = ?            (uses prefix)
--   WHERE project = ? AND crate_name = ? AND failure_type = ?  (full match)
-- But NOT:
--   WHERE crate_name = ?                            (skips prefix, no index use)
```

### Partial indexes (SQLite 3.8+, PostgreSQL)

Index only the rows that matter. Reduces index size and write overhead.

```sql
-- Only index non-finished dispatches (the ones queries actually filter on)
CREATE INDEX idx_dispatches_active
    ON dispatches(status) WHERE status <> 'finished';

-- Only index sessions with errors
CREATE INDEX idx_sessions_errors
    ON sessions(error_message) WHERE error_message IS NOT NULL;
```

Use partial indexes when:
- A status column has one frequently queried value among many
- Nullable columns are queried only for non-null rows
- A small subset of rows dominates the query pattern

### Covering indexes (SQLite 3.22+, PostgreSQL)

A covering index includes all columns a query needs, avoiding the table lookup entirely. SQLite reads the index B-tree and never touches the main table.

```sql
-- If the common query is: SELECT project, completed_at FROM session_metadata WHERE project = ?
CREATE INDEX idx_session_metadata_project_completed
    ON session_metadata(project, completed_at);
```

The planner will use an index-only scan when all selected columns are in the index. Verify with `EXPLAIN QUERY PLAN` -- look for "USING COVERING INDEX".

### SQLite-specific index behavior

- `INTEGER PRIMARY KEY` is the rowid alias and is already the clustered index. Do not create a separate index on it.
- `WITHOUT ROWID` tables use the PRIMARY KEY as the clustered index. Beneficial for composite-PK lookup tables.
- SQLite has no `INCLUDE` clause for covering indexes (unlike PostgreSQL). To cover extra columns, add them to the index key.
- `UNIQUE` constraints implicitly create indexes. Do not create redundant indexes on columns with UNIQUE constraints.
- `REINDEX` rebuilds indexes after bulk inserts. Run it after initial data loads, not routinely.
- `ANALYZE` populates the `sqlite_stat1` table that the query planner reads. Run after significant data changes so the planner has accurate statistics.

### PostgreSQL-specific index features

**INCLUDE clause** for covering indexes without affecting sort order:

```sql
CREATE INDEX idx_sessions_dispatch ON sessions(dispatch_id) INCLUDE (status, started_at);
```

**Expression indexes** for computed values:

```sql
CREATE INDEX idx_users_lower_email ON users (lower(email));
```

**GIN indexes** for jsonb, arrays, full-text search:

```sql
CREATE INDEX idx_events_tags ON events USING GIN (tags);
```

**BRIN indexes** for time-series and naturally ordered data:

```sql
CREATE INDEX idx_events_created_at ON events USING BRIN (created_at);
```

BRIN (Block Range INdex) stores min/max summaries per block of table pages. Extremely small (100-1000x smaller than B-tree) but only effective when physical row order correlates with index column order.

| Index type | Best for | Size | Write overhead |
|-----------|----------|------|----------------|
| B-tree | Random access, range queries, any row ordering | Large | Moderate |
| BRIN | Time-series, append-only, naturally ordered data | Tiny | Low |
| GIN | jsonb containment, arrays, full-text search | Large | High |

Use BRIN when:
- Data is inserted roughly in order (timestamps, sequential IDs)
- Table is large (>1M rows) and B-tree size is a concern
- Queries filter on ranges, not exact values
- Slight false-positive overhead is acceptable (BRIN eliminates blocks, not individual rows)

Do NOT use BRIN when:
- Row order does not correlate with column values (e.g., frequently updated status columns)
- Point queries dominate (BRIN's block-level granularity wastes I/O)
- Table is small (<100K rows) -- B-tree overhead is negligible

**CONCURRENTLY** for zero-downtime index creation:

```sql
CREATE INDEX CONCURRENTLY idx_large_table_col ON large_table(col);
```

### Validating index effectiveness

Always verify with the query planner before and after adding indexes.

**SQLite:**

```sql
EXPLAIN QUERY PLAN SELECT ...;
-- Look for: SEARCH ... USING INDEX (good)
-- Watch for: SCAN TABLE (potential problem on large tables)
```

**PostgreSQL:**

```sql
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) SELECT ...;
```

Run 2-3 times to warm the cache before capturing the plan. See "Query plan analysis" section for interpretation.

---

## Query plan analysis

Use `EXPLAIN ANALYZE` (PostgreSQL) or `EXPLAIN QUERY PLAN` (SQLite) before optimizing. Intuition about query performance is unreliable.

### SQLite query plans

`EXPLAIN QUERY PLAN` returns a tree of operations. Key terms:

| Term | Meaning | Action |
|------|---------|--------|
| `SCAN TABLE` | Full table scan | Add index if table is large and query is selective |
| `SEARCH ... USING INDEX` | Index lookup | Good -- verify it is the expected index |
| `USING COVERING INDEX` | Index-only scan | Best case -- no table lookup needed |
| `USE TEMP B-TREE` | Temporary sort | Consider adding an index to provide pre-sorted data |
| `AUTOMATIC COVERING INDEX` | SQLite auto-created temp index | The planner thinks an index would help -- create a permanent one |

SQLite does not have `EXPLAIN ANALYZE` with timing. Use `.timer on` in the CLI or measure from application code.

### PostgreSQL query plans

#### What to look for (Priority order)

1. **Estimated vs actual rows**: the most important signal. If the planner estimates 10 rows but gets 100,000, every downstream decision is wrong. Fix: run `ANALYZE` to update statistics.
2. **Seq Scan on large tables**: not always bad (small tables, high selectivity), but on >10K rows with a selective WHERE, investigate missing indexes.
3. **Nested Loop with high loop counts**: `loops=50000` with 100 rows per loop = 5M row touches. Consider hash join or adding an index.
4. **Sort spilling to disk**: `Sort Method: external merge` means `work_mem` is too low or an index could provide pre-sorted data.
5. **Buffer counts** (with `BUFFERS` option): `shared hit` = cache, `shared read` = disk. `temp read/written` = spill.

### Conventions

```sql
-- PostgreSQL: human reading (development)
EXPLAIN (ANALYZE, BUFFERS, FORMAT TEXT) SELECT ...;

-- PostgreSQL: production slow query logging
-- Use auto_explain module with log_min_duration

-- SQLite: query plan inspection
EXPLAIN QUERY PLAN SELECT ...;
```

---

## Implicit casting gotchas

SQL dialects handle type coercion differently. These differences cause silent data corruption or unexpected results when porting queries.

### Integer division

All three dialects truncate integer division (`SELECT 5 / 2` returns `2`). To get decimal results:

```sql
-- Portable: multiply by 1.0 to force float
SELECT 5 * 1.0 / 2;                   -- 2.5 in all dialects

-- PostgreSQL-only: cast syntax
SELECT 5::numeric / 2;                -- 2.5

-- Portable: explicit CAST
SELECT CAST(5 AS REAL) / 2;           -- 2.5 (SQLite uses REAL, PostgreSQL uses numeric/float)
```

Use the multiply-by-1.0 pattern for portable code. The `::` cast operator is PostgreSQL-only.

### Boolean handling

| Dialect | Native boolean | Storage |
|---------|---------------|---------|
| PostgreSQL | Yes (`true`/`false`) | Native type |
| SQLite | No | `INTEGER` (0/1) |
| Redshift | No | `INTEGER` (0/1), use `CASE WHEN` |

Portable pattern: store as `INTEGER` (0/1), compare with `= 1` / `= 0`, never rely on truthiness.

### NULL in concatenation

| Dialect | `'hello' \|\| NULL` | Behavior |
|---------|---------------------|----------|
| PostgreSQL | `NULL` | NULL propagates (SQL standard) |
| SQLite | `'hello'` | NULL treated as empty string |

Use `coalesce()` to make behavior explicit and portable:

```sql
SELECT 'hello' || coalesce(suffix, '') FROM ...;
```

### Type affinity (SQLite)

SQLite uses type affinity, not strict types (unless STRICT mode). A `TEXT` column accepts integers, a `REAL` column accepts text. Comparisons between mismatched types use affinity rules that differ from PostgreSQL's strict type system.

Defense: use STRICT tables (see § SQLite > STRICT tables). With STRICT, SQLite rejects type mismatches at insertion.

### CAST portability

| Syntax | PostgreSQL | SQLite | Redshift |
|--------|-----------|--------|----------|
| `CAST(x AS type)` | Yes | Yes | Yes |
| `x::type` | Yes | No | No |
| `TRY_CAST(x AS type)` | No | No | Yes (returns NULL on failure) |

Rule: use `CAST()` for portable code. Use `::` only in PostgreSQL-specific contexts.

For JSON casting specifically:
- PostgreSQL: `value::jsonb` or `CAST(value AS jsonb)` -- both work, `::` is idiomatic
- SQLite: no JSON cast -- use `json()` to validate and minify, `json_valid()` to check

---

## Testing

### Validation pattern

- Test with `limit` first, expand after validation
- Check row counts at each CTE stage
- Validate edge cases: nulls, empty sets, division by zero, boundary dates
- Compare against known-good results for at least one deterministic case

### Migration safety

- Never destructive migrations without backup verification
- Test migrations on a copy first
- `Room` (Android): explicit migration code for every schema change: never `fallbackToDestructiveMigration`

---

## SQLite

Primary dialect for kanon. All tables use rusqlite via the phronesis crate.

### STRICT tables

All new tables must use STRICT mode. Enforces type checking and prevents silent data corruption from type mismatches.

```sql
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 1
) STRICT;
```

Allowed types in STRICT mode: `INTEGER`, `REAL`, `TEXT`, `BLOB`, `ANY`. No `VARCHAR`, `BOOLEAN`, `DATETIME`.

Combine with `WITHOUT ROWID` for composite primary key tables where the PK is the natural lookup key.

### Connection pragmas

Configure on every new connection, in this order:

```sql
PRAGMA busy_timeout = 5000;    -- prevent SQLITE_BUSY on concurrent access
PRAGMA journal_mode = WAL;     -- concurrent read/write
PRAGMA synchronous = NORMAL;   -- safe with WAL, better throughput than FULL
PRAGMA foreign_keys = ON;      -- enforce referential integrity (off by default)
```

`journal_mode=WAL` persists per-database (set once), but the others reset per-connection.

### ID columns

`INTEGER PRIMARY KEY` is the rowid alias. Use it for all single-column auto-incrementing keys. Do NOT use `AUTOINCREMENT` -- it adds overhead (tracking in `sqlite_sequence`) without benefit since `INTEGER PRIMARY KEY` already guarantees monotonically increasing IDs for inserts without explicit values.

### UPSERT patterns

```sql
-- Basic upsert: excluded.col refers to the would-have-been-inserted value
INSERT INTO kv (key, value) VALUES (?, ?)
ON CONFLICT(key) DO UPDATE SET value = excluded.value;

-- Conditional update (only if newer)
INSERT INTO cache (key, data, updated_at) VALUES (?, ?, ?)
ON CONFLICT(key) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at
WHERE excluded.updated_at > cache.updated_at;

-- Counter increment
INSERT INTO counters (key, count) VALUES (?, 1)
ON CONFLICT(key) DO UPDATE SET count = counters.count + 1;
```

Prefer `DO UPDATE` over `DO NOTHING`: `DO NOTHING` silently swallows data.

### RETURNING clause (3.35+)

Works on INSERT, UPDATE, DELETE:

```sql
INSERT INTO sessions (name) VALUES (?) RETURNING id, created_at;
DELETE FROM sessions WHERE expired_at < ? RETURNING id;
```

Caveat: with UPSERT `ON CONFLICT DO NOTHING`, no row is returned when the conflict fires.

### Date/time handling

SQLite has no native date type. Store as ISO 8601 TEXT (`2025-01-15T10:30:00Z`). This sorts correctly, is human-readable, and works with SQLite's built-in date functions (`date()`, `datetime()`, `strftime()`).

For kanon: timestamps use RFC 3339 via jiff's `Timestamp::to_string()`.

### Other

- Parameterized queries always: never string interpolation
- WAL2 does not exist in mainline SQLite -- do not reference it

---

## PostgreSQL

### Identity columns over SERIAL

Use `GENERATED ALWAYS AS IDENTITY` for all new tables:

```sql
CREATE TABLE sessions (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
```

- SQL-standard (portable to DB2, Oracle)
- Sequence tied to column in metadata (clean dumps, no orphaned sequences)
- `GENERATED ALWAYS` prevents accidental manual inserts; `GENERATED BY DEFAULT` allows them
- SERIAL is legacy for PG >= 10

### MERGE with RETURNING (PG17)

```sql
MERGE INTO inventory t
USING incoming s ON t.sku = s.sku
WHEN MATCHED THEN
    UPDATE SET quantity = t.quantity + s.quantity
WHEN NOT MATCHED THEN
    INSERT (sku, quantity) VALUES (s.sku, s.quantity)
RETURNING t.*;
```

### Redshift compatibility

- `dateadd()` not `interval` for date math
- `convert_timezone('UTC', 'America/New_York', ts)` not `AT TIME ZONE`
- No native boolean: `CASE WHEN ... THEN 1 ELSE 0 END`
- Keep scripts under 30,000 characters (Redshift limit)
- `DISTKEY` and `SORTKEY` on every table definition

---

## Appendix: Mneme Datalog

- Relations are the unit of storage. Rules are the unit of computation.
- Atoms in rule bodies are positional: order maps to schema column order
- `?variable` for logic variables, `:param` for input parameters
- Aggregation via `<-` (stored relation query) and `?[] <~ Algo(...)` for graph algorithms
- No implicit joins: every shared variable name across atoms is an explicit join condition
- Decompose complex queries into named rules freely: no efficiency penalty
- Recursive rules are Datalog's primary advantage over SQL: use for transitive closure, reachability, path-finding
- Built-in graph algorithms (PageRank, community detection, shortest path) as special rules: prefer over hand-rolled Datalog for performance
- HNSW vector indices integrate directly into Datalog queries with ad-hoc joins
- Test rules with small fixture relations before running against production data

---

## Appendix: Room (Android)

- DAOs return `Flow<T>` for observable queries
- Entity classes are data classes
- Explicit migrations for every schema change
- Type converters for complex types (dates, enums, JSON)

---

## Anti-Patterns

1. **`SELECT *` in production queries**: name every column
2. **Bare `LEFT JOIN` without thinking**: use inner join when both sides are required
3. **Division without `nullif`**: division by zero is always possible
4. **Formatting in queries**: format at display layer, store raw values
5. **Magic strings/dates**: parameterize or reference constants
6. **CTE names like `cte1`, `temp`, `data`**: name must describe content
7. **String interpolation in queries**: SQL injection. Parameterize always.
8. **`!= null` instead of `IS NOT NULL`**: SQL null semantics
9. **Correlated subqueries where joins work**: performance trap
10. **Missing indexes on join/filter columns**: check query plans
11. **SERIAL in new PostgreSQL tables**: use `GENERATED ALWAYS AS IDENTITY`
12. **Non-STRICT SQLite tables**: use `STRICT` for type enforcement in new tables
13. **Self-join for previous/next row**: use `lag()`/`lead()` window functions
14. **Implicit window frame**: use explicit `ROWS BETWEEN` for running aggregates. The default `RANGE` frame produces unexpected results with duplicates.
15. **Filtering on window function in same query**: window functions execute after `WHERE`. Wrap in a CTE first.
16. **Default isolation for read-then-write**: check-then-act patterns need `REPEATABLE READ` or higher, not `READ COMMITTED`
17. **JSON column without CHECK constraint**: use `CHECK (json_valid(col))` for defense in depth on JSON TEXT columns
18. **Querying JSON for relational data**: if you regularly filter/join on a value inside JSON, extract it to a proper column
19. **AUTOINCREMENT in SQLite**: use `INTEGER PRIMARY KEY` alone -- AUTOINCREMENT adds tracking overhead without benefit
20. **Missing ANALYZE after bulk operations**: the query planner relies on `sqlite_stat1` statistics. Stale stats produce bad plans.
21. **Redundant index on UNIQUE column**: UNIQUE constraints already create an implicit index
22. **`::` cast in portable SQL**: use `CAST(x AS type)` for cross-dialect code; `::` is PostgreSQL-only
23. **`->>` result used in arithmetic without cast**: `->>` returns text; cast explicitly before math (`(col ->> 'key')::integer`)
24. **B-tree index on append-only timestamp column in large tables**: consider BRIN for 100-1000x smaller index with minimal query overhead
25. **Advisory lock without documented ID mapping**: raw integer lock IDs become mystery constants; use `hashtext()` and document the mapping
26. **Optimistic retry without version column**: `UPDATE ... WHERE id = ?` with no version check silently overwrites concurrent changes
