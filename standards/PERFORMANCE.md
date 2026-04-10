# Performance

> Standards for performance-aware development. Not premature optimization: measured, intentional performance decisions.

---

## Philosophy

Don't optimize without measuring. Don't measure without a reason. The default is "fast enough." Optimize only when evidence shows a bottleneck.

---

## Measurement

### Before optimizing

1. Define the metric (latency, throughput, memory, binary size)
2. Measure the baseline (with the current code, in realistic conditions)
3. Set the target (what "fast enough" means for this use case)
4. Profile to find the bottleneck (don't guess)

### After optimizing

1. Measure again (same conditions as baseline)
2. Verify the target is met
3. Commit the benchmark alongside the optimization
4. Document why in the commit message

---

## Resource budgets

Define explicit resource budgets for deployed services:

| Resource | Budget | Enforcement |
|----------|--------|-------------|
| Memory (RSS) | Documented per service | Monitor, alert at 80% |
| Startup time | < 5 seconds (excluding model loading) | CI benchmark |
| Request latency (p99) | Documented per endpoint | Prometheus alert |
| Binary size | Documented, tracked per release | CI size check |
| Database size | Documented growth rate | Maintenance task alerts |

Budgets are documented in the runbook and enforced by monitoring.

---

## Benchmarks

### Infrastructure

Use `criterion` (Rust), `pytest-benchmark` (Python), or equivalent. Benchmarks live in `benches/` and run separately from tests.

### What to benchmark

- Hot paths (request handling, parsing, serialization)
- Startup time
- Memory allocation patterns in loops
- Database query patterns under load

### What NOT to benchmark

- Cold paths (config loading, one-time initialization)
- I/O-bound operations (network, disk) unless testing batching/buffering
- operations where the overhead of benchmarking exceeds the operation

---

## Common patterns

### Avoid allocation in hot loops

Pre-allocate buffers. Reuse `Vec`, `String`, `HashMap` across iterations. Use `with_capacity()` when the size is known.

### Prefer streaming over buffering

Process data as it arrives. Don't accumulate entire responses in memory when streaming is possible. This applies to: LLM responses, file I/O, HTTP responses, database result sets.

### Lazy initialization

Expensive resources (embedding models, database connections, large indices) initialize on first use, not at startup. Use `LazyLock` or equivalent.

### Feature-gate heavy dependencies

ML models, GUI frameworks, and optional integrations behind feature flags. A minimal build should be fast to compile and small to deploy.

---

## Algorithmic complexity

### Documentation requirements

Every public function that operates on a collection, processes input of variable size, or performs search/sort/traversal must document its time and space complexity. Document complexity on the function, not on the module — readers need it at the call site, not in a separate file they won't open.

WHY: Undocumented complexity hides quadratic behavior behind innocent-looking APIs. A function that works fine on 10 items and destroys latency at 10,000 is a production incident waiting for enough data. Making complexity visible at the declaration forces the author to reason about scaling behavior at design time, not after the alert fires.

| Requirement | Rule |
|-------------|------|
| Public collection operations | Document time and space complexity in the doc comment. No exceptions. |
| Trait implementations | Document complexity if it differs from the trait's expected contract (e.g., an `Index` impl that is O(n) instead of O(1)). |
| Internal hot-path functions | Document complexity. These are not public, but they appear in profiles and need the annotation for optimization work. |
| Trivial O(1) accessors | No annotation required. Getters, field access, and constant-time lookups are assumed O(1). |

### Big-O annotation conventions

Use the following format in doc comments:

```rust
/// Finds the nearest neighbor in the spatial index.
///
/// Time: O(log n) average, O(n) worst case
/// Space: O(1)
///
/// Where `n` is the number of indexed points.
pub fn nearest(&self, point: &Point) -> Option<&Point> { /* ... */ }
```

Rules:

1. **State average and worst case separately** when they differ. An `O(1) amortized` without noting `O(n) worst case` hides resize spikes that cause latency outliers.
2. **Define variables.** `O(n)` means nothing without specifying what `n` is. State it: "where `n` is the number of items in the queue."
3. **Include space complexity** when the function allocates. Omit only for truly zero-allocation functions.
4. **Use standard notation.** `O(n)`, `O(n log n)`, `O(n * m)`. Do not use informal descriptions like "linear" or "quadratic" as substitutes — use them alongside, not instead of, the notation.
5. **Document amortized bounds explicitly.** Write `O(1) amortized` not just `O(1)` when the bound depends on amortization. Callers in latency-sensitive loops need to know about occasional expensive operations.

WHY: Consistent notation enables mechanical grep. `rg "Time: O\(n\^2\)"` finds every quadratic function in the codebase. Informal descriptions defeat this. Defining variables prevents the ambiguity of `O(n)` in a function that takes two collections of different sizes.

### Complexity budgets

Assign complexity budgets to hot paths the same way resource budgets are assigned to services:

| Path type | Maximum time complexity | Escalation |
|-----------|------------------------|------------|
| Per-request handler | O(n log n) | O(n^2) requires written justification in the doc comment and a benchmark proving acceptable wall time at projected max n |
| Inner loop / tight path | O(n) | Anything super-linear requires profiling evidence that it is not the bottleneck |
| Startup / initialization | No budget | Document complexity, but startup paths are not latency-sensitive |
| Batch / offline processing | No budget | Document complexity for capacity planning |

WHY: Budgets turn complexity from an observation into a constraint. Without a budget, O(n^2) drifts in through convenience (nested loops, repeated lookups) and is only caught when production data grows. A budget forces the conversation at review time, not incident time.

### Complexity testing

For performance-critical paths, assert that complexity holds at scale. A doc comment claiming O(n log n) is a promise — tests verify the promise holds as data grows.

#### Growth ratio pattern

Run the operation at multiple input sizes and assert that the runtime growth ratio matches the expected complexity class:

```rust
use std::time::Instant;

/// Assert that `f` scales as O(n log n) by measuring at three input sizes.
///
/// Time: O(n log n) where n is the largest test size (10,000)
/// Space: O(n)
fn assert_nlogn_scaling<F: Fn(usize)>(f: F) {
    let sizes = [100, 1_000, 10_000];
    let mut timings = Vec::with_capacity(sizes.len());

    for &n in &sizes {
        let start = Instant::now();
        f(n);
        timings.push((n, start.elapsed().as_nanos() as f64));
    }

    // Compare growth ratio between consecutive sizes.
    // For O(n log n): ratio ≈ (n2 * log(n2)) / (n1 * log(n1))
    for window in timings.windows(2) {
        let (n1, t1) = window[0];
        let (n2, t2) = window[1];
        let expected_ratio =
            (n2 as f64 * (n2 as f64).ln()) / (n1 as f64 * (n1 as f64).ln());
        let actual_ratio = t2 / t1;

        // Allow 3x tolerance for measurement noise.
        assert!(
            actual_ratio < expected_ratio * 3.0,
            "scaling exceeded O(n log n): n1={n1}, n2={n2}, \
             expected_ratio={expected_ratio:.1}, actual_ratio={actual_ratio:.1}"
        );
    }
}
```

#### Rules

| Requirement | Rule |
|-------------|------|
| Test sizes | N=100, N=1,000, N=10,000 minimum. Choose sizes large enough to dominate constant factors. |
| Growth ratio | Compare `t(N2) / t(N1)` against the expected ratio for the complexity class. Allow 2-3x tolerance for measurement noise. |
| Criterion benchmarks | Performance-critical paths use `criterion` benchmarks at multiple sizes. Complexity tests use `#[test]` with `Instant` for simplicity. |
| Complexity classes | O(n): ratio ≈ 10x per 10x input. O(n log n): ratio ≈ 13x per 10x input. O(n²): ratio ≈ 100x per 10x input. |
| Flaky test prevention | Use large enough N that constant overhead is negligible. Warm up before measuring. Assert ratio bounds, not absolute times. |

WHY: A doc comment claiming O(n) is unfalsifiable without a test. Growth ratio tests turn complexity claims into regression-catchable assertions. They also catch accidental quadratic behavior — O(n²) disguised as "a bit slow" at N=100 becomes obviously wrong at N=10,000.

### Documentation examples

#### Collection operation

```rust
/// Resolves all pending tasks in the queue, executing them in priority order.
///
/// Time: O(n log n) — priority sort dominates
/// Space: O(n) — allocates a sorted copy of the task list
///
/// Where `n` is the number of pending tasks in `self.queue`.
pub fn resolve_pending(&mut self) -> Vec<TaskResult> { /* ... */ }
```

#### Recursive traversal

```rust
/// Walks the directory tree rooted at `path`, collecting all file metadata.
///
/// Time: O(n) where `n` is the total number of filesystem entries
/// Space: O(d) where `d` is the maximum directory depth (recursion stack)
pub fn walk_tree(&self, path: &Path) -> Result<Vec<FileInfo>> { /* ... */ }
```

#### Amortized operation

```rust
/// Inserts a key-value pair into the map.
///
/// Time: O(1) amortized, O(n) worst case (resize)
/// Space: O(1) amortized
///
/// Where `n` is the current number of entries. Resize occurs when
/// load factor exceeds 0.75.
pub fn insert(&mut self, key: K, value: V) -> Option<V> { /* ... */ }
```

#### Multi-variable complexity

```rust
/// Joins two sorted iterators, yielding matched pairs.
///
/// Time: O(n + m) where `n = left.len()`, `m = right.len()`
/// Space: O(1) — streaming, no intermediate allocation
pub fn merge_join<'a, L, R>(left: L, right: R) -> MergeJoin<L, R>
where
    L: Iterator<Item = &'a Entry>,
    R: Iterator<Item = &'a Entry>,
{ /* ... */ }
```

---

## Benchmark-driven optimization

### Workflow

Never optimize based on reading the code. Optimize based on profiling data. The workflow is:

1. **Reproduce the slow case.** Write a benchmark that exercises the hot path with realistic data sizes. Artificial micro-benchmarks that don't reflect production load mislead.
2. **Profile.** Use `cargo flamegraph`, `perf`, `samply`, or equivalent. Identify the actual bottleneck — it is rarely where intuition says.
3. **Hypothesize and measure.** Change one thing. Run the benchmark. Record before/after numbers with statistical significance (criterion provides this automatically).
4. **Commit the benchmark.** The benchmark is part of the deliverable. An optimization without a benchmark is an unverifiable claim.

WHY: Code review cannot evaluate performance claims. "This is faster" is an assertion. A benchmark with before/after numbers and statistical confidence is evidence. Without the benchmark committed alongside the change, the next developer cannot verify the claim, reproduce the measurement, or detect when a later change regresses it.

### Benchmark requirements

| Requirement | Rule |
|-------------|------|
| Location | `benches/` directory, separate from tests |
| Framework | `criterion` (Rust), `pytest-benchmark` (Python), or equivalent statistical benchmarking tool |
| Data size | Benchmark at minimum two data sizes (small and projected-max). Single-size benchmarks hide complexity class. |
| Baseline | Record baseline numbers in the PR description. CI stores historical data for trend detection. |
| Naming | `bench_{operation}_{data_size}` — e.g., `bench_parse_10k_events`, `bench_index_lookup_1m_entries` |

### When to benchmark

Benchmark when:

- Optimizing an existing path (before and after — mandatory)
- Adding a new hot path (establish baseline — mandatory)
- Changing data structures in a hot path (verify no regression — mandatory)
- Reviewing a PR that claims performance improvement (reproducing the claim — mandatory)

Do not benchmark when:

- The operation is I/O-bound and the benchmark would measure I/O, not computation
- The path runs once at startup and is not user-facing
- The function is trivial and the benchmark overhead exceeds the function time

---

## Performance regression detection

### CI integration

Performance-sensitive paths must have benchmarks that run in CI. Regressions are caught by comparing against a stored baseline, not by eyeballing numbers.

| Component | Tool | Threshold |
|-----------|------|-----------|
| Rust benchmarks | criterion + `critcmp` or `cargo-bench-cmp` | >5% regression flags warning, >15% blocks merge |
| Python benchmarks | pytest-benchmark `--benchmark-compare` | Same thresholds |
| Binary size | CI size check (see resource budgets) | >10% increase blocks merge |
| Startup time | CI benchmark | Exceeding budget blocks merge |

WHY: Human review cannot detect a 7% regression in a function that runs 50,000 times per second. Automated comparison against a baseline catches it. The 5% warning threshold accounts for measurement noise; the 15% merge-blocking threshold catches real regressions. Without automation, performance degrades monotonically — each small regression is individually tolerable and collectively fatal.

### Baseline management

1. **Store baselines in CI artifacts**, not in the repository. Baselines are environment-specific and pollute version history.
2. **Update baselines explicitly.** A baseline update is a deliberate act with a justification in the commit message, not an automatic side effect of a green build.
3. **Pin CI runner hardware** (or use consistent instance types) for benchmark jobs. Cross-machine comparison is meaningless.
4. **Track trends over time.** A sequence of 3% regressions across five PRs is a 15% regression that no individual PR flagged. Store historical data (GreptimeDB or equivalent) and alert on sustained trends.

WHY: Baseline drift is the silent killer of performance regression detection. If baselines auto-update on every merge, every regression becomes the new normal within one commit. Explicit updates with justification create an audit trail: someone decided this slower number is acceptable, and they wrote down why.

### Responding to regressions

1. **If CI flags a regression:** investigate before merging. Do not bump the baseline to make CI green.
2. **If the regression is real and intentional** (e.g., added validation that costs CPU): document the tradeoff in the PR description, update the baseline with justification.
3. **If the regression is real and unintentional:** fix it. If the fix is non-trivial, file an issue and revert the regressing change.
4. **If the regression is noise:** re-run the benchmark. If it persists across three runs, it is not noise.
