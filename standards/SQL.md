# SQL

> Additive to STANDARDS.md. Read that first. Everything here is SQL-specific.
>
> Covers: PostgreSQL, SQLite, Redshift, CozoDB Datalog, Room (Android). Dialect-specific notes are marked.
>
> **Key decisions:** Lowercase keywords, CTEs over subqueries, explicit columns, nullif division, PG identity columns, SQLite STRICT tables, CozoDB Datalog.

---

## Formatting

### Keywords

Lowercase keywords everywhere. `select`, `from`, `where`, `join`, `group by`, `order by`.

### Layout

- One clause per line
- `select` columns each on their own line when more than two
- Indent joins one level. Join conditions on same line as `on`.
- `case` statements: one `when` per line, `end as column_name`
- Trailing commas in select lists (easier diffs)
- Max line length: 100 characters

```sql
select
    e.employer_id,
    e.name,
    count(c.case_id) as total_cases,
from employers e
    left join cases c on c.employer_id = e.employer_id
where
    e.created_at >= '2025-01-01'
group by
    e.employer_id,
    e.name
order by
    total_cases desc
```

### Operators

- `<>` not `!=` for inequality (SQL standard, ISO 9075)
- `is null` / `is not null`, never `= null`
- `coalesce()` nulls at the query boundary, not mid-CTE

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Tables / Views | `snake_case` | `case_appointments`, `active_members` |
| Columns | `snake_case` | `employer_id`, `created_at` |
| CTEs | `snake_case`, descriptive | `active_cases_by_employer` |
| Prefixes (tables) | `dim_`, `fact_`, `agg_`, `map_`, `v_` | `dim_employer`, `fact_billing` |
| Boolean columns | `is_` or `has_` prefix | `is_active`, `has_billing` |
| Date columns | `_date` suffix | `start_date`, `close_date` |
| Timestamp columns | `_at` suffix | `created_at`, `updated_at` |
| ID columns | `_id` suffix matching dimension | `employer_id`, `member_id` |

### CTE Naming

CTEs are self-documenting. Pattern: `{what}_{by/for}_{grain}`

The name answers: what does it contain, at what grain, what filter is applied?

| Good | Bad |
|------|-----|
| `first_appointment_noshow_by_member` | `initial_noshows` |
| `case_activity_summary` | `cte1` |
| `members_with_billable_event` | `temp_members` |
| `employer_contract_periods` | `data` |

### Table Aliases

Alias every table. Short but meaningful: `e` for `employers`, `c` for `cases`, `m` for `members`. Consistent within a query and across related queries.

---

## Structure

### CTEs Over Subqueries

Always. CTEs are readable, debuggable, and composable. Name them well and they document the query's logic.

```sql
with active_cases as (
    select case_id, employer_id, status
    from cases
    where status = 'active'
),
employer_summary as (
    select
        employer_id,
        count(*) as active_count,
    from active_cases
    group by employer_id
)
select *
from employer_summary
where active_count > 10
```

### Explicit Column Lists

Never `select *` except in ad-hoc exploration. Name every column. This documents intent and prevents breakage when schemas change.

### JOINs — Be Intentional

Don't default to `left join` without thinking about the semantics.

| Join | When |
|------|------|
| `join` (inner) | Both sides required — unmatched rows should be excluded |
| `left join` | Preserve left side — nulls acceptable for unmatched |
| `full outer join` | Need all records from both sides |

Common mistake: `left join` everywhere "to be safe" when inner join semantics are needed, then filtering out nulls later.

### Division Safety

`nullif(denominator, 0)` for all division. No exceptions.

```sql
select
    numerator * 1.0 / nullif(denominator, 0) as rate,
```

### Percentages and Formatting

Return percentages as decimals (0.42, not 42). Format at the display layer.

### Window Functions

Window functions compute across rows without collapsing them. Primary use cases: ranking, running totals, row-relative comparisons, and deduplication.

```sql
-- Rank within partition (deduplication pattern)
select *
from (
    select
        *,
        row_number() over (partition by member_id order by created_at desc) as rn,
    from appointments
)
where rn = 1

-- Previous value comparison
select
    month,
    revenue,
    lag(revenue) over (order by month) as prev_revenue,
    revenue - lag(revenue) over (order by month) as delta,
from monthly_revenue
```

Key rules:
- `row_number()` for dedup, `rank()` for ties, `dense_rank()` for gapless ties
- `lag()`/`lead()` for row-relative comparisons — avoid self-joins for previous/next row access
- Default frame is `range between unbounded preceding and current row` — this is unintuitive for running averages. Use explicit `rows between` when computing running aggregates:
  ```sql
  avg(revenue) over (order by month rows between 2 preceding and current row) as rolling_3mo_avg
  ```
- Window functions execute after `where`/`group by`/`having` — you cannot filter on a window function result in the same query level. Wrap in a CTE.

### Transaction Isolation

Choose isolation level deliberately. The default (`read committed` in PostgreSQL, `serializable` in SQLite) is not always correct.

| Level | Guarantees | Risk | Use |
|-------|-----------|------|-----|
| `read committed` | No dirty reads | Non-repeatable reads, phantom rows | Default OLTP, independent statements |
| `repeatable read` | Snapshot at transaction start | Serialization failures (retry required) | Read-heavy transactions needing consistency |
| `serializable` | Full serializability | Higher serialization failure rate | Financial calculations, inventory, anything with read-then-write dependencies |

Rules:
- Read-then-write patterns (check balance, then debit) require at minimum `repeatable read`
- Always handle serialization failures with retry logic — they are expected, not exceptional
- SQLite in WAL mode is effectively `serializable` for writes (single writer). Configure `busy_timeout` instead of retrying manually.
- Never weaken isolation "for performance" without proving the weaker level is correct for the workload

---

## Query Plan Analysis

Use `EXPLAIN ANALYZE` before optimizing. Intuition about query performance is unreliable.

### What to Look For (Priority Order)

1. **Estimated vs actual rows** — the most important signal. If the planner estimates 10 rows but gets 100,000, every downstream decision is wrong. Fix: run `ANALYZE` to update statistics.
2. **Seq Scan on large tables** — not always bad (small tables, high selectivity), but on >10K rows with a selective WHERE, investigate missing indexes.
3. **Nested Loop with high loop counts** — `loops=50000` with 100 rows per loop = 5M row touches. Consider hash join or adding an index.
4. **Sort spilling to disk** — `Sort Method: external merge` means `work_mem` is too low or an index could provide pre-sorted data.
5. **Buffer counts** (with `BUFFERS` option) — `shared hit` = cache, `shared read` = disk. `temp read/written` = spill.

### Conventions

```sql
-- Human reading (development)
explain (analyze, buffers, format text) select ...;

-- Production slow query logging
-- Use auto_explain module with log_min_duration
```

Run the query 2-3 times to warm the cache before capturing the plan.

---

## Testing

### Validation Pattern

- Test with `limit` first, expand after validation
- Check row counts at each CTE stage
- Validate edge cases: nulls, empty sets, division by zero, boundary dates
- Compare against known-good results for at least one deterministic case

### Migration Safety

- Never destructive migrations without backup verification
- Test migrations on a copy first
- `Room` (Android): explicit migration code for every schema change — never `fallbackToDestructiveMigration`

---

## Dialect-Specific Notes

### PostgreSQL

**Identity columns over SERIAL** — use `generated always as identity` for all new tables:

```sql
create table sessions (
    id integer generated always as identity primary key,
    name text not null,
    created_at timestamptz not null default now()
);
```

- SQL-standard (portable to DB2, Oracle)
- Sequence tied to column in metadata (clean dumps, no orphaned sequences)
- `generated always` prevents accidental manual inserts; `generated by default` allows them
- SERIAL is legacy for PG >= 10

**JSON_TABLE (PG17)** — converts JSON to relational rows in a `FROM` clause:

```sql
select j.*
from api_responses r,
    json_table(
        r.body, '$.items[*]'
        columns (
            id integer path '$.id',
            name text path '$.name',
            status text path '$.status'
        )
    ) as j
where j.status = 'active'
```

Prefer `json_table` over chains of `json_extract_path` / `->>` for structured JSON.

**MERGE with RETURNING (PG17):**

```sql
merge into inventory t
using incoming s on t.sku = s.sku
when matched then
    update set quantity = t.quantity + s.quantity
when not matched then
    insert (sku, quantity) values (s.sku, s.quantity)
returning t.*;
```

### PostgreSQL / Redshift

- `dateadd()` not `interval` for date math (Redshift compatibility)
- `convert_timezone('UTC', 'America/New_York', ts)` not `at time zone`
- No native boolean in Redshift: `case when ... then 1 else 0 end`
- Keep scripts under 30,000 characters (Redshift limit)
- Redshift: `distkey` and `sortkey` on every table definition

### SQLite

**STRICT tables** for all new tables — enforces type checking:

```sql
create table sessions (
    id integer primary key,
    name text not null,
    created_at text not null,
    is_active integer not null default 1
) strict;
```

Combine with `without rowid` for composite primary key tables.

**UPSERT patterns:**

```sql
-- Basic upsert: excluded.col refers to the would-have-been-inserted value
insert into kv (key, value) values (?, ?)
on conflict(key) do update set value = excluded.value;

-- Conditional update (only if newer)
insert into cache (key, data, updated_at) values (?, ?, ?)
on conflict(key) do update set data = excluded.data, updated_at = excluded.updated_at
where excluded.updated_at > cache.updated_at;

-- Counter increment
insert into counters (key, count) values (?, 1)
on conflict(key) do update set count = counters.count + 1;
```

Prefer `do update` over `do nothing` — `do nothing` silently swallows data.

**RETURNING clause** (3.35+) — works on INSERT, UPDATE, DELETE:

```sql
insert into sessions (name) values (?) returning id, created_at;
delete from sessions where expired_at < ? returning id;
```

Caveat: with UPSERT `on conflict do nothing`, no row is returned when the conflict fires.

**Other:**
- `integer primary key` is the rowid alias — use it
- `journal_mode=wal` for concurrent read/write (WAL2 does not exist in mainline SQLite)
- Parameterized queries always — never string interpolation
- JSON functions available: `json()`, `json_extract()`, `->` / `->>`, `json_each()`, `json_group_array()`. Binary `jsonb_*` variants (3.45+) for better performance on large documents.

### CozoDB Datalog

- Relations are the unit of storage. Rules are the unit of computation.
- Atoms in rule bodies are positional — order maps to schema column order
- `?variable` for logic variables, `:param` for input parameters
- Aggregation via `<-` (stored relation query) and `?[] <~ Algo(...)` for graph algorithms
- No implicit joins — every shared variable name across atoms is an explicit join condition
- Decompose complex queries into named rules freely — no efficiency penalty
- Recursive rules are CozoDB's primary advantage over SQL — use for transitive closure, reachability, path-finding
- Built-in graph algorithms (PageRank, community detection, shortest path) as special rules — prefer over hand-rolled Datalog for performance
- HNSW vector indices integrate directly into Datalog queries with ad-hoc joins
- Test rules with small fixture relations before running against production data

### Room (Android)

- DAOs return `Flow<T>` for observable queries
- Entity classes are data classes
- Explicit migrations for every schema change
- Type converters for complex types (dates, enums, JSON)

---

## Anti-Patterns

1. **`select *` in production queries** — name every column
2. **Bare `left join` without thinking** — use inner join when both sides are required
3. **Division without `nullif`** — division by zero is always possible
4. **Formatting in queries** — format at display layer, store raw values
5. **Magic strings/dates** — parameterize or reference constants
6. **CTE names like `cte1`, `temp`, `data`** — name must describe content
7. **String interpolation in queries** — SQL injection. Parameterize always.
8. **`!= null` instead of `is not null`** — SQL null semantics
9. **Correlated subqueries where joins work** — performance trap
10. **Missing indexes on join/filter columns** — check query plans
11. **SERIAL in new PostgreSQL tables** — use `generated always as identity`
12. **Non-STRICT SQLite tables** — use `strict` for type enforcement in new tables
13. **Self-join for previous/next row** — use `lag()`/`lead()` window functions
14. **Implicit window frame** — use explicit `rows between` for running aggregates. The default `range` frame produces unexpected results with duplicates.
15. **Filtering on window function in same query** — window functions execute after `where`. Wrap in a CTE first.
16. **Default isolation for read-then-write** — check-then-act patterns need `repeatable read` or higher, not `read committed`
