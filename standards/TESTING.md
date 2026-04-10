# Testing

> Sole authority for testing principles across all languages. Language-specific docs (RUST.md, PYTHON.md, etc.) cover framework choices and tooling only — they reference this file for principles.

---

## When to test

| Situation | Required | Type |
|-----------|----------|------|
| Public API function | Yes | Unit |
| Error path / edge case | Yes | Unit |
| Cross-module interaction | Yes | Integration |
| User-visible workflow | Yes | Integration or E2E |
| Pure internal helper | No (unless complex) | Unit if needed |
| Rendering / UI layout | No (use snapshots sparingly) | Visual regression |
| Performance-sensitive path | Yes | Benchmark |

Don't test private functions directly. Test them through the public API. If a private function is complex enough to need its own tests, it should be a public function in a smaller module.

## When NOT to test

- getters/setters
- Direct delegation (fn that calls one other fn and returns)
- Generated code (macros, derive)
- Third-party library behavior (test your usage, not their code)

## What to focus on

Test behavior with consequences:

- Boundary conditions (empty, one, many, max, overflow)
- Error paths (invalid input, unavailable service, timeout, permission denied)
- State transitions (especially concurrent access to shared state)
- Serialization round-trips (`deserialize(serialize(x)) == x`)
- Idempotency (replaying the same operation produces the same result)
- Security boundaries (authentication, authorization, input validation)

---

## Organization

### Colocated tests (preferred)

```rust
// In the same file as the code
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn rejects_empty_input() { /* ... */ }
}
```

Tests live next to the code they test. One `#[cfg(test)]` block per file. No separate `tests/` directory for unit tests.

### Integration tests

For cross-crate behavior, use a dedicated `tests/` directory or integration test crate. Integration tests exercise the public API as an external consumer would.

### Test naming

Name tests for the behavior, not the function:

| Bad | Good |
|-----|------|
| `test_parse` | `parses_valid_toml` |
| `test_error` | `rejects_negative_timeout` |
| `test_new` | `default_config_uses_port_18789` |

Pattern: `verb_condition` or `condition_produces_result`.

---

## Test quality

### Behavior over implementation

Test what the code **does**, not how it does it. If a refactor breaks your tests but doesn't break behavior, your tests are wrong.

### One assertion per behavior

A test should verify one behavior. Multiple assertions are acceptable when they verify facets of the same behavior, not when they test unrelated things.

### No test interdependence

Tests must pass in any order, in isolation, and in parallel. No shared mutable state between tests. No test that depends on another test having run first.

### Deterministic

No sleep, no wall-clock time, no network calls, no filesystem race conditions. Use:
- `tokio::time::pause()` for async timing
- Temp directories for filesystem
- Mock servers for HTTP
- Fixed seeds for random

A flaky test is a bug. Fix or delete it.

### Real over mock

Prefer real implementations over mocks when practical. A test hitting a real SQLite database catches more bugs than a mock that returns hardcoded data. Mock only at boundaries you don't control (external APIs, hardware).

### Error paths are paths

Test error cases with the same rigor as success cases. Every `Result::Err` variant should have at least one test that triggers it.

### Idempotency

Every state-modifying operation must have a test proving idempotency: running it N times produces the same result as running it once. This enforces the "Idempotent by design" principle in STANDARDS.md.

**Required test pattern:** call the operation, capture resulting state, call it again with identical input, assert state is unchanged.

**Naturally idempotent operations** (PUT replacing a resource, DELETE removing it, GET reading it) still need tests proving the property holds. A PUT that increments a counter on each call is not idempotent despite being a PUT.

**Operations requiring explicit idempotency mechanisms** (POST creating resources, side-effecting event handlers, message consumers) must use idempotency keys, content hashes, or deduplication checks. The test must verify the mechanism: send the same request with the same idempotency key twice, assert the side effect occurred once.

**API handlers:**

```rust
#[tokio::test]
async fn create_prompt_is_idempotent() {
    let app = test_app().await;
    let request = CreatePromptRequest {
        idempotency_key: "req-abc-123".into(),
        title: "fix lint violation".into(),
        // ...
    };

    let first = app.create_prompt(request.clone()).await.unwrap();
    let second = app.create_prompt(request.clone()).await.unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(app.count_prompts().await, 1);
}
```

**Event handlers:**

```rust
#[tokio::test]
async fn pr_merged_handler_tolerates_replay() {
    let (bus, store) = test_event_system().await;
    let event = PrMergedEvent { pr_number: 42, sha: "abc123".into() };

    bus.emit(event.clone()).await;
    bus.emit(event.clone()).await;
    bus.emit(event.clone()).await;

    assert_eq!(store.archived_count().await, 1);
}
```

**CLI state mutations (bookkeeper, triage, steward):**

```rust
#[tokio::test]
async fn bookkeeper_archive_is_idempotent() {
    let env = test_worktree().await;
    env.create_merged_pr("prompt-1800.md").await;

    bookkeeper_archive(&env.config).await.unwrap();
    let state_after_first = env.snapshot_archive().await;

    bookkeeper_archive(&env.config).await.unwrap();
    let state_after_second = env.snapshot_archive().await;

    assert_eq!(state_after_first, state_after_second);
}

#[tokio::test]
async fn triage_generate_is_idempotent() {
    let env = test_worktree().await;
    env.create_open_issue(770, "add idempotency testing").await;

    triage_generate(&env.config).await.unwrap();
    let prompts_first = env.list_prompts().await;

    triage_generate(&env.config).await.unwrap();
    let prompts_second = env.list_prompts().await;

    assert_eq!(prompts_first, prompts_second);
}
```

**Idempotency key patterns:**

| Pattern | When to use | Example |
|---------|------------|---------|
| Request ID | Client-supplied, API mutations | `Idempotency-Key` header |
| Event ID | Event bus delivery | `event.id` field on every emitted event |
| Content hash | File operations, prompt generation | SHA-256 of canonical input |
| Source reference | Archive, sync, triage | Issue number, PR number, file path |

The idempotency key must be part of the operation's input, not generated internally. Internal UUIDs cannot detect duplicates because each invocation generates a new one.

---

## Flake policy

A flaky test is a test that produces different results on the same code. It is always a bug — in the test, in the code under test, or in the test infrastructure. Flakes erode trust: developers learn to ignore failures, CI becomes a slot machine, and real regressions hide behind "just retry it." This section defines how to detect, contain, and eliminate flakes.

WHY: Without a systematic flake policy, the response to flakes is always ad hoc: someone retries, someone ignores, someone disables the test. Each response is a workaround that hides the problem. A policy makes flake handling deterministic and visible.

### Detection

A test is flaky when it fails on a commit where it previously passed, and the failure is not reproducible on demand. Detection methods, in order of reliability:

1. **CI bisection.** If a test fails on a commit that did not touch its code or dependencies, it is a flake candidate. Confirm by re-running the exact commit in isolation.
2. **Nextest retries.** A test that fails then passes on retry within the same CI run is a confirmed flake. Nextest records this in JUnit output (`flakyTests` element). Extract and report these automatically.
3. **Historical analysis.** A test that has failed on 3+ unrelated PRs within a 14-day window is a flake, even if each individual failure was dismissed.

WHY: Detection must not depend on human judgment. Humans under deadline pressure classify everything as "probably a flake" and retry. Automated detection based on retry records and cross-PR failure correlation removes that bias.

Every detected flake produces a tracking issue tagged `flake` with: test name, failure message, affected commits, and detection method. No flake is addressed without a tracking issue.

### Disable threshold

A test that fails in more than 1% of CI runs is flaky. Disable it immediately with `#[ignore]` and an issue reference:

```rust
#[test]
#[ignore = "flaky: port conflict in parallel runs — see #456"]
fn integration_connects_to_service() {
    // ...
}
```

Every `#[ignore]` attribute must include a comment or reason string referencing a tracking issue. Bare `#[ignore]` without an issue reference is a lint violation (`TESTING/ignore-no-issue`). The issue tracks root-cause investigation and has an SLA (see Resolution SLAs below).

WHY: `#[ignore]` is visible in the source, greppable, and enforced by the linter. It prevents the test from running by default while keeping it in the codebase for re-enablement. The 1% threshold is low enough to catch flakes early but high enough to avoid false positives from one-off infrastructure hiccups.

### Quarantine

For tests that need continued execution for signal gathering (frequency analysis, root-cause narrowing), quarantine via nextest overrides instead of `#[ignore]`:

```toml
# .config/nextest.toml
[profile.default.overrides]
# Quarantined: flaky — see #NNN
filter = "test(=crate::module::flaky_test_name)"
retries = 5
test-group = "quarantine"
```

Quarantined tests:
- **Run on every CI invocation.** Skipping them hides whether they're still failing and prevents detecting when a fix lands.
- **Do not block merge.** Their failures are reported but non-fatal.
- **Carry a tracking issue number** in the override comment. No anonymous quarantines.
- **Are reviewed weekly.** If a quarantined test has passed consistently for 14 days, remove the quarantine. If it has not been fixed within the SLA, escalate or delete.

WHY: Quarantine is containment, not resolution. It stops the bleeding (CI blockage) while preserving signal (the test still runs). The tracking issue and weekly review prevent quarantine from becoming a graveyard where tests go to be forgotten.

### Retry policy

Nextest retries exist to detect flakes, not to hide them.

| Context | Retries | Rationale |
|---------|---------|-----------|
| CI default | 3 | Enough to distinguish flakes from deterministic failures. A test that fails 4 consecutive times on the same commit is not flaky — it is broken. |
| Quarantined tests | 5 | Higher retry count gathers signal on flake frequency for root-cause analysis. |
| Local development | 0 | Developers must see failures immediately. Retries on local runs train developers to ignore the first failure. |
| Release gate | 0 | Release builds must pass without retries. Any test that cannot pass deterministically does not gate a release — it must be quarantined or fixed first. |

WHY: Retries are a diagnostic tool. Used correctly, they identify flakes for quarantine. Used incorrectly (high retry counts everywhere, no tracking), they become a system-wide workaround that makes flakes invisible. The retry count is deliberately low: three retries catch flakes without hiding persistent failures.

A test that passes on retry is not "passing." It is flaky and must be tracked. CI pipelines must report retry-passed tests distinctly from clean-passed tests.

### Flake budget

Each project has a flake budget: the maximum number of tests that may be quarantined at any time.

| Project size | Budget |
|-------------|--------|
| < 500 tests | 3 |
| 500–2000 tests | 5 |
| > 2000 tests | 2% of test count, rounded down |

When the budget is exceeded, no new features merge until quarantined tests are resolved below the budget. The budget is enforced in CI: a gate check counts quarantined tests and fails if over budget.

WHY: Without a budget, quarantine grows without bound. A project with 40 quarantined tests has not contained its flakes — it has normalized them. The budget creates back-pressure: every new quarantine consumes a scarce resource, forcing teams to fix existing flakes before adding new ones.

### Resolution SLAs

Every flake tracking issue has a resolution deadline based on severity:

| Severity | Definition | SLA |
|----------|-----------|-----|
| Critical | Flake in release-gate or security test | 3 business days |
| High | Flake in integration test or cross-crate test | 7 business days |
| Normal | Flake in unit test | 14 business days |
| Low | Flake in benchmark or non-blocking test | 30 business days |

Resolution means one of:
1. **Root-cause fix.** The underlying cause (timing, shared state, resource leak, test ordering) is identified and eliminated. The test is unquarantined.
2. **Test rewrite.** The test is rewritten to avoid the flaky condition. The new test is proven stable over 50+ consecutive green runs before unquarantining.
3. **Test deletion.** If the behavior cannot be tested deterministically and the test provides no unique coverage, delete it. A flaky test that covers the same path as a deterministic test adds negative value.

If the SLA expires without resolution, the test is escalated: it appears in the weekly review as overdue, and no new quarantines are permitted until the overdue item is resolved or explicitly accepted at a higher severity SLA.

WHY: SLAs prevent the "we'll get to it" pattern where flake issues accumulate indefinitely. The escalation mechanism — blocking new quarantines when SLAs are breached — ensures that unresolved flakes cannot be ignored by simply quarantining more tests. Resolution has a defined meaning: the flake is gone, not just managed.

### Common flake root causes

Address these during investigation. Most flakes trace to a small number of patterns:

| Root cause | Fix |
|-----------|-----|
| Wall-clock timing (`sleep`, `Instant::now`) | Use `tokio::time::pause()`, inject clocks |
| Port conflicts (hardcoded ports in parallel tests) | Use port 0 (OS-assigned) or `portpicker` |
| Shared filesystem state (temp dirs, fixture files) | Unique temp dirs per test via `tempfile` |
| Test ordering dependence (shared `static mut`, leaked state) | Reset state in setup, use `#[serial]` only as last resort |
| Resource exhaustion (file descriptors, threads, connections) | Explicit cleanup in test teardown, bounded pools |
| External service flakiness (DNS, network, third-party API) | Mock at the boundary, never depend on external services in CI |
| Async race conditions (missing `await`, unbounded channels) | Use deterministic async test harness, assert on completion signals not timing |

WHY: Most flake investigations stall at "it's intermittent, I can't reproduce it." This table provides a starting checklist that resolves the majority of flakes. When the root cause is not in this list, add it — the table is a living document.

---

## Coverage expectations

No numeric coverage target. Coverage metrics reward testing code and penalize testing complex code. Instead:

- Every public function has at least one test
- Every error variant has at least one test that produces it
- Every match arm with non-logic has a test
- Critical paths (auth, payment, data persistence) have integration tests

---

## Test data policy

All test data must be synthetic. No real personal information in test fixtures, assertions, or examples.

**Standard test identities:**
- Users: `alice`, `bob`, `charlie`
- Emails: `alice@example.com`, `bob@example.org` (RFC 2606 reserved domains only)
- Phones: `+1-555-0100` through `+1-555-0199` (ITU reserved for fiction)
- IPs: `192.0.2.x`, `198.51.100.x`, `203.0.113.x` (RFC 5737 documentation ranges)
- IPv6: `2001:db8::/32` (RFC 3849 documentation range)
- Domains: `example.com`, `example.org`, `example.net`, `*.test` (RFC 2606/6761 reserved)

**Never use:** real names, emails, usernames, internal IPs/hostnames, personal facts, credentials, or API keys. Never copy production data into test environments.

**Test data builders:** Use builder/factory patterns with sensible defaults. Each test overrides only the fields it cares about. When a field is added to the struct, only the builder default needs updating, not every test.

**Determinism:** Any randomized test data must be seeded. The seed must be logged or persisted. Proptest regression files, hypothesis databases, and equivalent fixtures are checked into version control.

---

## Fuzz testing

Parsers, serializers, and any code that handles untrusted input should have fuzz targets. Use `cargo fuzz` with `libfuzzer`. Maintain a corpus directory with seed inputs.

Run fuzz targets periodically (CI nightly or pre-release), not on every PR.

---

## Benchmarks

Performance-sensitive code gets benchmarks, not just tests. Use `criterion` or `divan`. Benchmarks live in `benches/` and run separately from tests.

Benchmark before optimizing. Benchmark after optimizing. Commit both results.

### Benchmark baselines

Benchmark baselines are committed after each release. The baseline is the reference point for detecting performance regressions in subsequent development.

**Baseline workflow:**
1. After tagging a release, run the full benchmark suite: `cargo bench -- --save-baseline release-vX.Y.Z`
2. Commit the baseline output to `benches/baselines/` in version control
3. CI compares every PR's benchmark results against the current baseline

**CI regression detection:** CI runs benchmarks against the committed baseline and flags regressions exceeding 10%:

```bash
cargo bench -- --baseline release-vX.Y.Z --save-baseline pr-$PR_NUMBER
# Compare and fail on >10% regression
critcmp release-vX.Y.Z pr-$PR_NUMBER --threshold 10
```

A regression exceeding 10% is a CI failure. The PR author must either fix the regression, justify it in the PR body with evidence, or update the baseline with reviewer approval.

WHY: Without committed baselines, benchmarks only detect regressions relative to the previous run — which may itself be regressed. Anchoring to release baselines provides a stable reference. The 10% threshold balances noise tolerance (benchmark variance is typically 2-5%) against catching meaningful regressions.

---

## Component spec validation

Define compliance specs for each component type. After every test, automatically validate that required metrics, events, and traces were produced.

```rust
pub async fn assert_component_compliance<T>(spec: &ComponentSpec, f: impl Future<Output = T>) -> T {
    init_test();
    let result = f.await;
    spec.assert();  // Validates metrics + events were emitted
    result
}
```

This catches observability regressions: if a refactor removes a metric emission, the test fails even though the functional behavior is unchanged.

## Trace assertion patterns

Observability contracts (see STANDARDS.md § Logging and observability) are testable. Use `tracing-test` to capture spans and events, then assert that the module emits what its contract promises.

### Capturing events in tests

```rust
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn dispatch_emits_completion_event() {
    let dispatcher = Dispatcher::new(mock_config());

    dispatcher.dispatch_prompt(&prompt, "test-provider").await.unwrap();

    // Assert the contracted event was emitted with required fields
    assert!(logs_contain("prompt.completed"));
    assert!(logs_contain("prompt_id"));
    assert!(logs_contain("duration_ms"));
}
```

### Asserting span structure

```rust
#[tokio::test]
#[traced_test]
async fn gate_check_creates_span() {
    let gate = Gate::new(mock_config());

    gate.run(&prompt).await.unwrap();

    // Assert the contracted span exists
    assert!(logs_contain("run_gate"));
    assert!(logs_contain("gate_name"));
}
```

### Testing contract completeness

For modules with observability contracts, write a test that exercises every documented event and verifies emission. This test is the enforcement mechanism: if a refactor removes an event, this test fails.

```rust
#[tokio::test]
#[traced_test]
async fn observability_contract_complete() {
    // Exercise success path
    let result = module.process(&valid_input).await;
    assert!(result.is_ok());
    assert!(logs_contain("input.processed"));

    // Exercise error path
    let result = module.process(&invalid_input).await;
    assert!(result.is_err());
    assert!(logs_contain("input.rejected"));
}
```

### What to assert

| Contract element | Assertion |
|-----------------|-----------|
| Event name | `logs_contain("event.name")` |
| Required field present | `logs_contain("field_name")` |
| Correct log level | `logs_contain("ERROR")` for error events |
| Span creation | `logs_contain("span_name")` |
| Span fields | `logs_contain("field=value")` within span context |

Do not assert on exact log message text or field values that vary per run (timestamps, UUIDs). Assert on event names, field presence, and log levels.

## Mock components as real implementations

Mocks implement the same traits as production code. They compose into real topologies for integration testing. A mock that returns hardcoded data through a different interface than production code tests the mock, not the system.

Pattern: `MockProvider` implements `LlmProvider`. `MockStore` implements `SessionStore`. Both plug into real pipelines.

## Property-based testing

For stateful systems, use property-based testing with action sequence generation:

1. Define possible actions (create, read, update, delete, etc.)
2. Generate random action sequences
3. Sanitize sequences to only valid combinations
4. Execute against system under test AND in-memory model
5. Assert system state matches model state

Persist regression corpus (minimal failing cases) in git via `proptest-regressions/`.

### Proptest regression management

`proptest-regressions/` directories must be committed to version control. These files contain minimal failing inputs that reproduce past bugs — losing them means losing regression coverage.

**CI verification:** CI must verify that `proptest-regressions/` is not listed in `.gitignore`. A pipeline step checks this explicitly:

```bash
# Fail if any gitignore rule excludes proptest regression files
if git check-ignore -q proptest-regressions/ 2>/dev/null; then
  echo "ERROR: proptest-regressions/ is gitignored — regression corpus must be tracked"
  exit 1
fi
```

WHY: Proptest generates regression files automatically when a test fails, but developers sometimes gitignore them as noise. Losing the regression corpus silently removes coverage for previously-discovered edge cases. The CI check makes this a hard failure rather than an invisible drift.

## Test runner configuration

Use nextest for Rust projects:

```toml
# .config/nextest.toml
[profile.default]
retries = 3
slow-timeout = { period = "30s", terminate-after = 4 }
failure-output = "immediate-final"
junit.path = "junit.xml"
```

Benefits over `cargo test`: retries, timeouts, JUnit output, per-test parallelism, better failure reporting.

## Async test utilities

Build reusable async wait helpers instead of `sleep()`:

```rust
pub async fn wait_for<F, Fut>(f: F)
where F: Fn() -> Fut, Fut: Future<Output = bool> {
    let mut delay = 5; // ms
    loop {
        if f().await { return; }
        tokio::time::sleep(Duration::from_millis(delay)).await;
        delay = (delay * 2).min(500);
    }
}
```

Exponential backoff from 5ms to 500ms. No arbitrary `sleep(1s)`.
