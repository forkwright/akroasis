# Datalog (CozoDB / Krites)

> Additive to STANDARDS.md. Read that first. Everything here is specific to the CozoDB-derived Datalog dialect used in the krites engine.

> **Status: pre-production.** No code in any repo uses Datalog today. Krites (Phase 05b) has not started implementation. This document defines the target standard so that design decisions and dispatched work converge on one dialect from the start. Treat every section as a constraint on future implementation, not a description of current behavior.

---

## Overview

Krites (the query engine inside mneme) runs a Datalog dialect based on CozoDB. It's used for the knowledge store: facts, entities, relationships, graph algorithms, and recall scoring. Agents interact with it via the `datalog_query` tool and the `KnowledgeStore` API.

This is not SQL. Datalog is declarative and rule-based. Queries define what you want, not how to get it. The engine resolves rules recursively.

---

## Syntax

### Relations

Relations are stored (persistent) or derived (computed from rules).

```datalog
# Stored relation: insert data
?[entity, relation, target] <- [["agent-01", "knows", "user"], ["agent-01", "uses", "aletheia"]]

# Query a stored relation
?[entity, target] := *facts[entity, _, target]
```

### Rules

Rules define derived relations. The head (left of `:=`) is what you produce. The body (right) is the conditions.

```datalog
# Single rule
?[name] := *entities[id, name, _], *facts[id, "type", "person"]

# Multiple conditions (AND)
?[a, b] := *edges[a, b], *nodes[a, "active", true], *nodes[b, "active", true]

# Negation
?[name] := *entities[id, name, _], not *forgotten[id]

# Aggregation
?[entity, count(fact_id)] := *facts[fact_id, entity, _, _]
```

### Variables

- Named: `entity`, `fact_id`, `name`
- Anonymous: `_` (matches anything, not bound)
- Constants: strings in quotes, numbers bare, booleans `true`/`false`

### Operators

```datalog
# Comparison
?[name, score] := *facts[_, name, _, score], score > 0.5

# String matching
?[name] := *entities[_, name, _], starts_with(name, "agent")

# Arithmetic
?[entity, total] := *scores[entity, s1], *bonuses[entity, s2], total = s1 + s2
```

---

## Conventions

### Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Stored relations | `snake_case` | `facts`, `entities`, `relationships` |
| Rule names | `snake_case` | `active_facts`, `related_entities` |
| Variables | `snake_case` | `entity_id`, `fact_content` |
| Constants | As stored (case-sensitive) | `"Verified"`, `"agent-01"` |

### Query structure

1. Put the output columns first: `?[entity, name, score]`
2. Start with the primary relation, then add filters
3. Use meaningful variable names (not `a`, `b`, `x`)
4. One condition per line for readability

```datalog
# Good: readable, structured
?[entity_name, fact_content, confidence] :=
    *facts[fact_id, entity_id, fact_content, confidence],
    *entities[entity_id, entity_name, _],
    confidence > 0.3,
    not *forgotten_facts[fact_id]

# Bad: dense, unclear
?[n,c,s] := *facts[f,e,c,s], *entities[e,n,_], s>0.3
```

### Read-Only by default

The `datalog_query` tool and `KnowledgeStore::query()` execute read-only queries. Mutations go through the `KnowledgeStore` API methods (store_fact, forget_fact, etc.), not raw Datalog.

Never expose mutable Datalog queries to agents or external callers.

---

## Knowledge store schema

The krites engine manages these stored relations:

| Relation | Columns | Purpose |
|----------|---------|---------|
| `facts` | fact_id, entity_id, content, confidence, tier, fact_type, created_at, updated_at | Core fact storage |
| `entities` | entity_id, name, entity_type | Named entities |
| `relationships` | source_id, target_id, relation_type, weight | Entity graph edges |
| `embeddings` | fact_id, vector | Embedding vectors for semantic search |
| `access_log` | fact_id, accessed_at, context | Recall tracking |
| `succession` | fact_id, superseded_by, reason | Fact versioning chain |

### Epistemic tiers

Facts have a tier indicating confidence level:

| Tier | Meaning | Decay Rate |
|------|---------|-----------|
| `Verified` | Confirmed by user or external source | Slowest |
| `Inferred` | Derived from patterns or extraction | Medium |
| `Assumed` | Default or heuristic-based | Fastest |

### Fact types

| Type | Example |
|------|---------|
| `Identity` | "Cody is a data scientist" |
| `Preference` | "User prefers tables over prose" |
| `Skill` | "Agent can query Redshift" |
| `Relationship` | "Agent-01 reports to User" |
| `Event` | "Deployed v0.11.0 on 2026-03-14" |
| `Task` | "Investigate Landlock exec failure" |
| `Observation` | "Haiku pricing not configured" |

---

## Mneme integration

Mneme is the memory subsystem. Krites is the query engine inside mneme. All Datalog access flows through the `KnowledgeStore` API — never through raw engine handles.

### Query composition

Use `KnowledgeStore::query()` for read-only Datalog. Pass the query string and receive typed results. WHY: a single entry point lets mneme enforce access control, audit logging, and timeout budgets uniformly.

```rust
// Correct: query through the KnowledgeStore API
let results = store.query(r#"
    ?[entity_name, confidence] :=
        *facts[_, entity_id, _, confidence, "Verified", _, _, _],
        *entities[entity_id, entity_name, _],
        confidence > 0.7
"#).await?;

// Wrong: obtaining a raw engine handle bypasses validation and audit
let results = engine.run_query(...); // never do this
```

### Mutations

Mutate facts exclusively through `KnowledgeStore` methods (`store_fact`, `forget_fact`, `update_confidence`, `supersede`). WHY: these methods enforce schema validation, conflict detection, succession chains, and emit events for downstream consumers. Raw Datalog inserts bypass all of these.

### Vector search into Datalog

Semantic similarity searches return candidate fact IDs. Feed those IDs into a Datalog rule for logical filtering. WHY: HNSW produces approximate neighbors — Datalog applies exact constraints the vector index cannot express.

```datalog
# Step 1 (Rust): similarity search returns candidate IDs
# let candidates = store.semantic_search(query_embedding, top_k=50).await?;

# Step 2 (Datalog): filter candidates with logical constraints
?[fact_id, content, confidence] :=
    *facts[fact_id, entity_id, content, confidence, tier, _, _, _],
    *entities[entity_id, _, "person"],
    fact_id in $candidates,
    confidence > 0.3,
    tier != "Assumed"
```

### TTL and decay

Facts decay based on epistemic tier and access frequency. The `KnowledgeStore` runs decay as a background sweep, not as inline query logic. WHY: inline decay in every query adds latency and creates inconsistent snapshots when concurrent queries observe different decay states.

| Tier | Half-life | Boosted by access |
|------|-----------|-------------------|
| `Verified` | 180 days | Yes — each access resets the clock |
| `Inferred` | 60 days | Yes — diminishing returns after 5 accesses |
| `Assumed` | 14 days | No — assumed facts decay on schedule regardless |

Facts below a confidence floor (default 0.05) are candidates for garbage collection. `forget_fact` marks a fact as forgotten immediately; decay lets facts fade naturally.

### Conflict resolution

When two sources assert contradictory facts about the same entity and relation, mneme applies source-priority merge:

1. **Higher-tier wins.** A `Verified` fact supersedes an `Inferred` fact on the same claim. WHY: epistemic tier encodes how much evidence backs the claim.
2. **Same tier: later timestamp wins.** WHY: more recent observation is more likely current.
3. **Explicit supersession.** `KnowledgeStore::supersede(old_id, new_id, reason)` records the chain. WHY: the succession relation preserves provenance so downstream consumers can trace why a fact changed.

Never silently overwrite. Every conflict resolution produces a succession record.

### Recursive rule complexity

Recursive rules must terminate. Krites stratifies negation automatically, but recursion depth is unbounded by default.

- Set `:limit N` on recursive queries during development. WHY: unbounded recursion over a growing knowledge graph produces unpredictable latency.
- Recursive rules that traverse `relationships` must include a depth counter or cycle guard. WHY: entity graphs contain cycles (A knows B, B knows A). Without a guard, the engine explores the cycle until the stratification checker intervenes with a cryptic error.

```datalog
# Correct: bounded transitive closure
?[src, dst, depth] :=
    *relationships[src, dst, _, _],
    depth = 1

?[src, dst, depth] :=
    *relationships[src, mid, _, _],
    ?[mid, dst, prev_depth],
    depth = prev_depth + 1,
    depth <= 10
```

---

## Graph algorithms

Krites includes fixed rules (built-in algorithms) accessible from Datalog:

```datalog
# Shortest path
?[path] <~ ShortestPathBFS(*relationships[], src: "agent-01", dst: "user")

# Community detection
?[node, community] <~ CommunityDetectionLouvain(*relationships[])

# PageRank
?[node, rank] <~ PageRank(*relationships[])
```

Available algorithms: BFS, DFS, Dijkstra, A*, Yen's K-shortest, TopSort, Strongly Connected Components, Louvain, Label Propagation, Random Walk, PageRank (via custom rules).

### Algorithm invocation

```datalog
# Fixed rule syntax: ?[outputs] <~ AlgorithmName(*relation[], param: value)
?[node, distance] <~ ShortestPathDijkstra(
    *relationships[src, dst, weight],
    src: "entity_123",
    dst: "entity_456"
)
```

---

## Performance

- **Stored relations are indexed.** The engine auto-indexes by key columns.
- **Recursive rules are stratified.** The engine detects and handles recursion automatically.
- **Aggregation is single-pass.** Use `count`, `sum`, `min`, `max`, `mean`, `collect` in rule heads.
- **Avoid Cartesian products.** Every rule body variable should appear in at least one relation. Unbound variables cause full cross-joins.
- **Limit result sets.** Use `:limit N` for exploratory queries.
- **HNSW for vector search.** Semantic search uses the embedded HNSW index, not Datalog scan.

---

## Anti-Patterns

1. **Cartesian joins**: forgetting to bind a variable produces a cross-product. Always check that every variable in the head appears in the body.
2. **Mutation via raw queries**: use the KnowledgeStore API. Raw Datalog mutations bypass validation, conflict detection, and audit logging.
3. **Unbounded recursion**: recursive rules without a base case or convergence condition. The engine detects this but the error is cryptic.
4. **String comparison for IDs**: use the ID directly, not string equality. IDs are ULIDs, case-sensitive.
5. **Ignoring negation stratification**: `not` in rules requires the negated relation to be computed in a lower stratum. Circular negation is unsound and rejected.
