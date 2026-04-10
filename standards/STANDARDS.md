# Coding Standards

> Universal principles for all code, all languages, all projects. All other standards in this directory are additive to this document. Read this first.

### Standards index

| File | Scope |
|------|-------|
| **This file** | Universal principles: philosophy, comments, naming, errors, concurrency, config, git, security, logging, observability contracts, writing, code review (testing → TESTING.md) |
| REPO-SETUP.md | New project checklist: required files, directories, CI, Cargo.toml template, deny.toml baseline, verification script |
| ENVIRONMENT.md | Environment variables, configuration files, secrets handling, feature flags, path resolution |
| PLANNING.md | Project planning: phase structure, ROADMAP/STATE/PLAN/SUMMARY formats, lifecycle, migration |
| RELEASES.md | Versioning policy, CHANGELOG format, release-please config, binary distribution, target matrix |
| ARCHITECTURE.md | Dependency direction, crate boundaries, modularity, encapsulation, API surface rules, feature flag propagation |
| API.md | HTTP API design, request/response patterns, error responses, pagination, versioning |
| DEPLOYMENT.md | **Sole authority** for deployment: gates, merge policy, release timing, rollback, health check requirements, fleet management |
| CI.md | CI tooling: which checks run, how they're configured, target matrices, test sharding, workflow generation |
| TESTING.md | **Sole authority** for testing: principles, strategy, organization, coverage, test data, fuzz, benchmarks, property tests |
| SECURITY.md | Credential handling, input validation, dependency audit, sandboxing, secret types |
| OPERATIONS.md | Service-specific: runbooks, monitoring, backup, incident response, DNS, service management, observability |
| PERFORMANCE.md | Resource budgets, benchmarks, profiling, binary size, build optimization, algorithmic complexity, regression detection |
| SYSTEMD.md | Service units, timers, security hardening, resource limits, journald logging |
| PODMAN.md | Pod architecture, container naming, volume mounts (SELinux), health checks, auto-update, systemd integration, rootless vs rootful, image pinning |
| NGINX.md | Reverse proxy configuration, SSL/TLS, rate limiting, load balancing, security headers |
| STORAGE.md | Database patterns, migrations, connection management, index rebuild, data versioning, consistency guarantees |
| RESTIC.md | Restic backup: repository setup, operations, retention, restore, automation |
| WRITING.md | Prose style, banned words, FK grade targeting, structural anti-patterns |
| RUST.md | Rust-specific: edition, lints, snafu error enums, async (tokio), dependencies, crate layout |
| PYTHON.md | Python-specific: uv, typing, async patterns |
| TYPESCRIPT.md | TypeScript-specific: strict mode, framework patterns |
| SHELL.md | Shell-specific: set -euo pipefail, quoting, portability |
| SQL.md | SQL-specific: naming, CTEs, parameterization |
| DATALOG.md | Datalog-specific: rule naming, stratification |
| NIX.md | Nix-specific: flake patterns, reproducibility |
| KOTLIN.md | Kotlin-specific: coroutines, sealed classes |
| CSHARP.md | C#-specific: async/await, nullable references |
| CPP.md | C++-specific: smart pointers, RAII |
| PROTOBUF.md | Proto3 schema design, gRPC patterns, JSON mapping, pagination, type wrappers, Rust codegen |

---

## Philosophy

**Code is the documentation.** Names, types, and structure carry meaning. If code needs a comment to explain what it does, rewrite the code. Comments explain *why*, never *what*.

**Fail fast, fail loud.** Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values and silent degradation are bugs. Surface errors at the point of origin with full context.

**Parse, don't validate.** Invalid data cannot exist past the point of construction. Newtypes, validation constructors, and type-level guarantees enforce invariants at the boundary: HTTP handlers, config loading, deserialization, CLI argument parsing. Once a value is constructed, its validity is a compile-time or construction-time guarantee. Deserialization must route through the parser: derive-based frameworks (`serde`, `System.Text.Json`, `encoding/json`) bypass constructors by default.

**Prefer immutable.** Default to immutable data. Require explicit justification for mutability. Mutable shared state is the root of most concurrency bugs and a common source of aliasing surprises.

**Minimize surface area.** Private by default. Every public item is a commitment. Expose the smallest API that serves the need. `pub(crate)` (Rust), `internal` (C#), unexported (Kotlin/TS), `_prefix` (Python).

**Everything must earn its place.** Every file, every directory, every dependency, every document, every config, every script must justify its existence against the current state, a planned future state, or its archival value. If it serves none of these, delete it. Git has history. This applies to code (no dead code, no commented-out blocks, no unused imports), to infrastructure (no stale scripts, no orphaned configs, no empty placeholder directories), and to documentation (no docs describing deleted features, no plans for abandoned work, no redundant references). The cost of keeping something "just in case" is that every future reader must evaluate whether it matters. Multiply that by every agent and every session.

**Define once, reference everywhere.** Every value, pattern, and behavior has exactly one authoritative definition. Constants, not literals. Functions, not copy-paste. Macros or generics when the same structure repeats across types. Config files for values that vary by environment. If you're typing the same string, number, or pattern a second time, extract it. Three copies of a mock struct is three bugs waiting to diverge. The cost of extraction is always less than the cost of inconsistency.

This applies at every level:
- **Literals**: model names, header values, schema names, timeouts, limits. Named constants in the lowest common module.
- **Patterns**: error handling boilerplate, validation logic, serialization. Shared functions or macros.
- **Test fixtures**: mock providers, setup helpers, sample data. Shared test utilities module.
- **Config defaults**: define in one place (config struct Default impl), reference from there. Never hardcode the same default in two files.

**Derive, don't maintain.** If a value can be computed from a source of truth, compute it — don't store a copy that drifts. Rule exclusion lists derive from training data success rates. Blast radius derives from cargo metadata. Routing decisions derive from per-provider success rates. Planning docs derive from git history and issue trackers. Command documentation derives from `--help`, not static markdown. Every piece of manually maintained state that duplicates a computable fact is a bug waiting to diverge. The test: if the source of truth changes, does this value update automatically? If not, it's maintained, not derived, and it will go stale.

This extends "define once" from code to state: configuration, documentation, dispatch routing, quality thresholds, and operational metrics should all trace back to a single authoritative source. When you find yourself writing the same information in two places, one of them should be a derivation of the other. If both are manual, build the derivation first, then delete the manual copy.

**No workarounds.** If something is broken, fix it properly or log it as blocked and move on. Never build a workaround that becomes the permanent path. Each workaround hides the broken thing from attention, becomes load-bearing as other things depend on it, compounds as new workarounds stack on old ones, and makes the eventual fix harder. Silent failures are workarounds in disguise. If you can't fix it now, file an issue AND log a loud warning (log.error, not log.debug). The standard is: it gets built to the best standard we can, or it waits until we can. No stepping stones that become permanent.

**No shortcuts.** Build the right thing from the start. If the SDK is better than the CLI wrapper, build the SDK. If the architecture needs three crates, build three crates. Don't ship a "quick version" you know you'll replace: time spent on throwaway work is stolen from the real thing. MVPs are for validating markets, not for code you're certain about.

**Best tool for the job.** Every decision: language, library, architecture, data structure: is made on merit. No defaults by inertia. No "we've always done it this way." If the current tool is wrong, replace it. If a better option exists and the migration cost is justified, migrate. Comfort with a tool is not a reason to use it; fitness for the problem is.

**No compromise on quality.** Every PR should be clean, tested, and reviewed before merge. Fix issues immediately, don't defer. "Good enough" is not a standard. The goal is code you'd be confident handing to a stranger with zero context: they should be able to read it, understand it, and trust it. Cutting corners creates debt that compounds faster than the time it "saved."

**Format at the boundary.** Percentages as decimals (0.42), currency as numbers, dates as timestamps internally. Format when rendering for display, not in queries or transforms.

**Idempotent by design.** Operations that may be retried, replayed, or delivered more than once must produce the same result regardless of repetition. Use idempotency keys for API mutations. Design event handlers to tolerate duplicate delivery. Message processing, webhook handlers, and state transitions are the primary risk areas. If replaying an operation would corrupt state, the operation is broken.

**Observability as contract.** If you can't see what a system is doing, you can't trust it to run without you. Every module must emit structured signals (logs, metrics, events) sufficient to diagnose failures without attaching a debugger. These emissions are not optional instrumentation: they are part of the module's public contract, with the same standing as its type signatures and error variants. Removing or changing an emitted event is a breaking change. Failing to emit a promised signal is a bug.

This means:
- **Async operations carry tracing spans.** Every `async fn` that performs I/O, crosses a process boundary, or takes more than trivial time is instrumented with a span that records its inputs, duration, and outcome. Correlation IDs propagate across span boundaries.
- **Structured logging, not println.** All log output uses the structured logging framework (`tracing` in Rust, `loguru` in Python). No `println!`, no `eprintln!`, no `dbg!` in production paths. Structured fields enable machine parsing; interpolated strings do not.
- **Long-running services expose health endpoints.** Any process that runs continuously (servers, workers, schedulers, stewards) exposes a health check that reports readiness and dependency status. A service that cannot report its own health is not production-ready.
- **Modules document their observability contract.** Each module's doc comment includes the events it emits, the metrics it records, and the conditions under which each fires. Downstream consumers (dashboards, Vector pipelines, alert rules) depend on this contract. See § Logging and observability for the contract format.

---

## Comments

### Zero-Comment default

Most code should have zero inline comments. Self-documenting names and clear structure are the standard. Inline comments exist only for genuinely non-obvious *why* explanations.

Never include:
- Creation dates, author attributions, changelog entries
- AI generation indicators
- "Upgraded from X" or migration notes
- Comments restating what the code does

### Structured comment tags

When a comment is warranted, use exactly one of these prefixes. No freeform comments outside this system.

| Tag | Purpose | Issue required |
|-----|---------|:--------------:|
| `WHY:` | Non-obvious design decision. Explains rationale, not mechanism. | No |
| `WARNING:` | Fragile coupling, dangerous assumption, will-break-if. | No |
| `NOTE:` | Non-obvious context that doesn't fit other categories. | No |
| `PERF:` | Performance-critical path, deliberate optimization, or known bottleneck. | No |
| `SAFETY:` | Precedes unsafe or dangerous operations. Explains why invariants hold. | No |
| `INVARIANT:` | Documents a maintained invariant at a call site or type definition. | No |
| `TODO(#NNN):` | Planned work. Must reference a tracking issue. | **Yes** |
| `FIXME(#NNN):` | Known defect or temporary workaround. Must reference a tracking issue. | **Yes** |

Usage:
```
// WHY: Datalog engine returns results as JSON arrays, not named columns.
// Positional indexing is intentional and matches their wire format.

// WARNING: This timeout must exceed the LLM provider's own timeout,
// or we'll cancel requests that are still in-flight upstream.

// PERF: Pre-allocated buffer avoids per-turn heap allocation.
// Measured 3x throughput improvement in session replay benchmarks.

// SAFETY: The pointer is valid because we hold the arena lock and
// the allocation lifetime is tied to the arena's drop.

// INVARIANT: session.turns is always sorted by timestamp ascending.
// Callers depend on this for binary search in recall.

// TODO(#342): Replace linear scan with bloom filter after mneme v2.

// FIXME(#118): Workaround for upstream bug in serde_yml. Remove
// when we migrate to serde_yaml 0.9+.
```

### Banned patterns

- Bare `// TODO` or `// FIXME` without an issue number: invisible debt
- `// HACK`, `// XXX`, `// TEMP`: use `FIXME(#NNN)` with a tracking issue
- `// NOTE:` as a substitute for clear code: rewrite the code first
- Commented-out code blocks: delete them, git has history
- End-of-line comments explaining what a line does: rename the variable instead

### Doc comments

Doc comments (`///` in Rust, `/** */` in TS/Kotlin, `<summary>` in C#, docstrings in Python) apply to:

- Public API items that cross module boundaries
- Functions that can panic or throw unexpectedly (document when/why)
- Functions with non-obvious error conditions
- `unsafe` functions: mandatory safety contract documentation

Not required on:
- Private/internal functions with self-documenting signatures
- Test functions (the name IS the documentation)
- getters, builders, or standard trait implementations

One-line file headers (module-level doc comment) are encouraged: describe the module's purpose in a single sentence.

---

## Naming

### Code identifiers

| Context | Convention | Example |
|---------|-----------|---------|
| Types / Traits / Classes | `PascalCase` | `SessionStore`, `MediaProvider` |
| Functions / Methods | `snake_case` or `camelCase` (language-specific) | `load_config`, `createSession` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_TURNS`, `DEFAULT_PORT` |
| Booleans | `is_` or `has_` prefix | `is_active`, `has_billing` |
| Events | `noun:verb` | `turn:before`, `tool:called` |

**Universal naming rules:**
- Verb-first for functions: `load_config`, `create_session`, `parse_input`. Drop `get_` prefix on getters.
- Self-documenting over short. `schema_db_path` not `p`. `active_cases` not `df2`.
- If you need a comment to explain what a name means, rename it.

### Gnomon system (Persistent names)

Module directories, agent identities, subsystems, and major features follow the gnomon naming convention. Names identify **essential natures**, not implementations.

Applies to: modules, crates, agents, subsystems, features that persist across refactors.
Does not apply to: variables, functions, test fixtures, temporary branches.

Process:
1. Identify the essential nature (not the implementation detail)
2. Construct from Greek roots using the prefix-root-suffix system
3. Validate with the layer test (L1 practical → L4 reflexive)
4. Check topology against existing names in the ecosystem
5. If no Greek word fits naturally, the essential nature isn't clear yet: wait

### Vertical structure

Code reads top-to-bottom. Spatial grouping communicates logical relationships.

**Blank lines separate logical units:**
- One blank line between functions/methods
- One blank line between logical sections within a function (setup, action, cleanup)
- Two blank lines between major sections in a file (imports, constants, types, impl blocks, tests)
- No blank lines inside a single logical step (declaration + immediate use)

**Group related code together:**
- Imports: stdlib first, then external crates, then internal crates, then local modules. One blank line between each group.
- Struct fields: group by purpose (identity fields, config fields, state fields), not alphabetically. A blank line between groups if the struct is large.
- Impl blocks: constructors first, then public methods, then private methods. One blank line between each method.
- Match arms: no blank lines between arms unless arms are multi-line.

**Function internal structure:**
```
// 1. Validate/extract inputs (guard clauses, early returns)
// 2. Set up resources
//
// 3. Perform the operation
//
// 4. Transform/format results
// 5. Return
```

Blank line between "set up" and "act" and between "act" and "return." No blank line within a single step. If a function has no natural sections, no internal blank lines needed.

**No trailing blank lines** at end of file. No multiple consecutive blank lines anywhere.

**File ordering:**
1. Module-level doc comment
2. Imports
3. Constants
4. Type definitions (structs, enums)
5. Trait definitions
6. Impl blocks (in order: core type, then trait impls)
7. Free functions
8. Tests (`#[cfg(test)] mod tests`)

### File & directory organization

| Context | Convention | Example |
|---------|-----------|---------|
| Source files | Language convention (see language files) | `session_store.rs`, `SessionStore.cs` |
| Scripts | `kebab-case` | `deploy-worker.sh` |
| Canonical docs | `UPPER_SNAKE.md` | `STANDARDS.md`, `ARCHITECTURE.md` |
| Working docs | `lower-kebab.md` | `planning-notes.md` |
| Directories | `snake_case` | `session_store/`, `test_fixtures/` |
| Timestamped files | `YYYYMMDD_description.ext` | `20260313_export.csv` |

- `snake_case` for directories. No hyphens, no camelCase, no spaces.
- Max 2-3 nesting levels inside any project. Flat > nested.
- No version numbers in filenames: version in file headers or git tags.

### Project structure

**Group by feature, not by type.** Code that changes together lives together. A feature directory contains its own models, services, routes, and tests. Fall back to layers within a feature when it grows large enough to need internal organization.

| Pattern | When | Example |
|---------|------|---------|
| Feature-first | Default for all projects | `playback/`, `library/`, `auth/` |
| Layers within feature | Feature exceeds ~10 files | `playback/models/`, `playback/services/` |
| Pure layer-based | Small projects (<10 source files) | `models/`, `services/`, `routes/` |

**Predictable top-level directories:**

| Directory | Contents |
|-----------|----------|
| `src/` | All source code. No code at root level. |
| `tests/` | Integration tests (unit tests colocated with source) |
| `scripts/` | Build, deploy, and maintenance scripts |
| `docs/` | Documentation beyond README |
| `config/` | Configuration templates and defaults (not secrets) |

Language-specific layouts (crate structure, package hierarchy) live in the language files.

**Rules:**
- Build artifacts and generated code are gitignored, never committed
- Vendored or third-party code lives in an explicit directory (`vendor/`, `third_party/`), never mixed with project source
- Entry points live in `src/`, not at repository root
- CI configuration in `.github/`, `.gitlab-ci.yml`, or equivalent standard location

### Repository hygiene

**Every root file must justify its presence.** A file at the repo root must be required there by the tool that reads it (Cargo.toml by cargo, flake.nix by nix, CLAUDE.md by Claude Code, etc.). Files that can live deeper should live deeper. Config belongs with the system that reads it. Data belongs with the system that produces or consumes it. If a file doesn't need to be at root, it's misplaced.

**Absorb tooling into the native binary.** Shell scripts are bootstrapping tools -- acceptable when the native binary isn't available (provisioning a bare server). Once the binary can handle the task, the script should be replaced with a subcommand. One binary is one thing to deploy, one thing to version, one thing to discover via `--help`. Five scripts are five things to find, five shebangs to worry about, five sets of error handling to maintain.

**Separate operational artifacts from source code.** Source code (crates, modules, libraries) is what the compiler processes. Operational artifacts (prompts, templates, roles, training data, research) are what the workflow system processes. Mixing them in one directory creates confusion about what's code and what's data. Group by consumer: the compiler's input in one tree, the workflow engine's input in another.

**Standards live with the tool that enforces them.** If a linting engine reads standards documents to check code against them, those documents are the engine's data -- they belong in the engine's directory, not in a separate top-level directory that happens to be conceptually related. The principle: data lives with the system that uses it.

**Dead weight applies to everything, not just code.** Dead scripts, stale configs, empty placeholder directories, orphaned documentation, unused templates -- all are dead weight. Git has history. Delete it. The cost of keeping something "just in case" is that every future reader must evaluate whether it matters. Multiply that by every agent and every session.

**Audit structure periodically.** Code structure drifts. Directories that made sense six months ago accumulate cruft. Refactors leave orphaned files. New tools make old scripts redundant. A periodic structural audit (during QA cycles) catches drift before it compounds. The question at each directory, each file, each root-level artifact: does this still earn its place? See QA.md for the audit process.

---

## Error handling

Every error must:

1. **Carry context**: what operation failed, with what inputs
2. **Be typed**: callers can match on error kind, not parse strings
3. **Propagate**: chain errors with context, never swallow the cause
4. **Surface**: log at the point of *handling*, not the point of *origin*

### Fail fast

- Panic/crash on programmer bugs: violated invariants, impossible states, corrupted data
- Return errors for anything the caller could reasonably handle or report
- Prefer descriptive assertions over silent fallbacks: `expect("session must exist after authentication")` over bare `unwrap()`
- Never panic in library code for recoverable conditions

### No silent catch

- Every catch/except/match block must: log, propagate, return a meaningful value, or explain why it discards
- `/* intentional: reason */` for deliberate discard: never an empty catch body
- If you're catching to ignore, you're hiding a bug

### No sentinel values

- Do not return `null`/`None`/`-1`/empty string to signal failure
- Use the language's error type: `Result`, exceptions, `sealed class` error hierarchies
- Invalid data cannot exist past the point of construction

### Exhaustive error types

No catch-all error variants. `Other(String)`, `Unknown`, or `Unexpected` are escape hatches that erode type safety. Every error variant must represent a specific, matchable failure mode. If a new failure is possible, add a variant. If you have 30+ variants, consider whether a structured diagnostic (single struct with severity + message + trace) scales better than an enum.

### Error boundaries

Errors are converted at module and crate boundaries, propagated within them. When an error crosses a boundary, wrap it with context: what the callee was trying to do, not just what went wrong. The boundary layer translates implementation errors into domain errors. Internal errors never leak through public API surfaces.

### Resource lifecycle

Acquired resources must have a defined cleanup path. Use RAII (`Drop` in Rust), `defer` (Go), `with`/context managers (Python), `using` (C#), `use` (Kotlin). Never rely on garbage collection or finalizers for resource cleanup. Database connections, file handles, and sockets are released as soon as work completes.

---

## Concurrency

### Ownership

Every spawned task, goroutine, thread, or async operation must have a defined owner responsible for its lifecycle. Fire-and-forget is banned: if you spawn it, something must join, cancel, or supervise it.

### Shared state

- **Prefer message passing** (channels, actors) over shared memory and locks
- When shared mutable state is necessary, synchronize all access. Document which lock guards which data.
- Prefer higher-level constructs (channels, executors, actors) over raw mutexes and atomics. Use atomics only for single counters or flags, not for coordinating state.
- Never hold a lock across an await point, an I/O operation, or a callback

### Thread safety contracts

Public types that may be used concurrently must declare their safety guarantee: immutable (always safe), thread-safe (synchronized internally), conditionally thread-safe (caller must synchronize), or not thread-safe (single-threaded only).

### Ordering

Never rely on execution order between concurrent units unless explicitly synchronized. Code that "works because the goroutine is always fast enough" is a race condition.

### Testing concurrent code

Concurrency bugs live in interleavings, not in text. Static analysis and code review catch a fraction. Use the tools:
- **Sanitizers:** TSan (C++, Rust via `-Z sanitizer=thread`), `go test -race`
- **Model checkers:** `loom` (Rust), `jcstress` (JVM) for lock-free algorithms
- **Stress tests:** Run concurrent tests under high contention with randomized timing. Single-pass success proves nothing; 10,000-pass success builds confidence.
- **Deterministic replay:** Seed-based schedulers for reproducing intermittent failures

---

## Configuration & operability

This project is maintained by a single developer with AI agents. Every design decision must account for that: one person deploys, monitors, debugs, and evolves the system. Complexity that requires a team is a bug.

### Configuration

- **Config in environment, not code.** Values that vary between deploys: credentials, hostnames, feature flags: live in environment variables or external config stores, never compiled in.
- **No hardcoded secrets.** Connection strings, API keys, and passwords never in source. Not in config files committed to git. Use secret stores or environment injection.
- **Inject inward, never fetch.** Configuration values are pushed from the outermost layer (main, entry point) and injected into inner modules. Inner code receives config. It never reads environment variables or config files directly.
- **Fail on invalid config at startup.** Validate all configuration during initialization with clear error messages. Don't discover bad config at 3 AM when the code path first executes.
- **Sensible defaults for everything.** A fresh deployment with zero config should start, serve health, and explain what's missing. Not crash with a stack trace.

### Policy and mechanism separation

Behavioral parameters (thresholds, weights, timing, capacity limits, scoring coefficients) are **policy**. They belong in configuration. Code implements **mechanism** — how things work, not when/how-much/how-fast they trigger. If an agent or operator might reasonably want to tune a value, it is not a compile-time constant.

- **Convention over configuration.** Every parameter has a default matching current behavior. Config only contains deviations. Zero-config deployments work identically to today.
- **Single source of truth for defaults.** Every default value is defined exactly once: in a config struct `Default` impl or in `koina/defaults.rs`. Never duplicated across crates. CLI `default_value` attributes and scaffold templates reference the central definition, not a string literal.
- **Three-tier classification.** (1) `const` — mathematical/algorithmic invariants that never change (nonce length, hash digest size). (2) Deployment-tunable — `aletheia.toml` values the operator sets per installation. (3) Per-agent-tunable — values agents adjust within operator-set bounds.
- **Hot vs cold.** Every deployment-tunable parameter is classified as hot-reloadable (SIGHUP applies immediately) or cold (requires restart). See `taxis::reload::RESTART_PREFIXES`. Cold parameters: port bindings, TLS certs, auth mode, storage paths. Everything else is hot.
- **Cross-parameter validation.** `taxis::validate` enforces that parameter combinations are valid at config load, not discovered at runtime.

### Deployment

- **One command to deploy.** Build, stop, copy, refresh credentials, start, health check. If it takes more than one command, wrap it in a script.
- **One command to roll back.** Previous binary preserved. Swap and restart.
- **Self-diagnosing errors.** When something fails, the error message must say what's wrong AND how to fix it. "embed-candle feature not enabled: build with --features embed-candle" is good. "provider unavailable" is bad.
- **No manual token management.** Credential lifecycle (refresh, rotation, expiry) is the system's job, not the operator's. If a token expires, the system refreshes it. If refresh fails, the error says why and how to re-authenticate.
- **Health check that checks everything.** Not just "process is running" but "can reach LLM, can write to store, can read config, credentials valid." One endpoint, complete picture.

### Maintenance

- **Automated monitoring.** Health checks on a timer. Token expiry warnings before they expire. Cost tracking. Log rotation. If it needs watching, automate the watching.
- **No tribal knowledge.** Every operational procedure is documented or scripted. A fresh agent session with access to the repo should be able to deploy, diagnose, and fix without prior context.
- **Upgrades are boring.** Pull, build, deploy script. No migration scripts to run manually. No config format changes without backward compatibility. Schema migrations run automatically on startup.
- **Logs tell the full story.** Structured JSON with correlation IDs. When something breaks, the logs contain everything needed to diagnose without reproducing. No "turn on debug logging and try again."

### Code for one developer + AI

- **Minimize moving parts.** Single binary over microservices. Embedded databases over external services. Fewer processes = fewer failure modes.
- **Automate the automation.** Standards are enforced by linters, not code review. Deployments are scripts, not runbooks. Issue triage generates prompts. Prompts dispatch agents. Agents create PRs. Steward merges them. The human architects; the system executes.
- **Make agents effective.** Clear CLAUDE.md, accurate docs, self-documenting code. An agent reading the repo for the first time should understand the architecture, find the right file, and make the right change without asking.
- **No context required.** Every error, every log, every doc assumes the reader has zero prior context. The system explains itself. If understanding requires "you had to be there," something is missing.

---

## Information hierarchy

This principle governs everything: documentation, configuration, standards, code architecture, API design. Not just docs.

### Single source of truth

Every fact, rule, or definition lives in exactly one place. Everything else points to it.

When information exists at multiple levels (universal standards, language addenda, repo docs, module docs), it belongs at the **lowest common ancestor**: the most general file where it's universally true. Children inherit; they never restate.

This standards package itself follows this rule:
- `STANDARDS.md` holds universal principles (you're reading it)
- Language files (`RUST.md`, `PYTHON.md`, etc.) hold only what's language-specific
- Language files don't repeat anything from this file
- If a principle applies to two or more languages, it moves here

The same applies to code:
- Shared types live in the lowest common crate/module
- Config defaults live at the most general level; overrides at the specific level
- Error types are defined per crate boundary, not duplicated across crates
- A helper function used in two places gets extracted, not copied

### Rules

- **Don't duplicate down.** If a rule applies everywhere, it goes in the shared file. Children inherit silently.
- **Don't duplicate up.** If a rule is specific to one context, it stays there. The parent doesn't mention it.
- **Pointers, not copies.** When a child needs to reference a parent rule: `See STANDARDS.md`. Don't paste content.
- **One update, one file.** If changing a fact requires editing multiple files, the hierarchy is wrong. Fix the hierarchy.
- **Delete redirects.** If a file exists only to say "moved to X", delete it. Git has history.

### Repository boundaries

Project repos contain only product artifacts: source code, reference docs about the code, config examples, CI definitions. Everything else lives in the workspace repo (kanon).

**Lives in the project repo:**
- Source code and tests
- Reference docs: architecture, configuration, deployment, runbooks, troubleshooting, changelogs
- Naming conventions and lexicons (part of the codebase contract)
- Config examples and templates
- CI/CD workflows

**Lives in the workspace repo:**
- Roadmaps, requirements, backlogs
- Research docs and spike notes
- Architecture decision records (ADRs)
- Planning artifacts (prompts, triage output, sprint records)
- Ground truth validation checkpoints
- Anything time-bound or process-bound

The test: if a new contributor clones the project repo, should they see this file? Reference docs yes. Planning artifacts no.

### Document lifecycle

Documentation follows the code it describes. When code is deleted, moved, or substantially refactored, update or remove its documentation in the same change. Orphaned docs (documentation for code that no longer exists) are worse than no docs because they actively mislead.

### Litmus test

Before writing anything (doc, config, code), ask:
1. Does this fact already exist somewhere? → Point to it.
2. Is this true for more than one context? → Move it up.
3. Will someone need to update this in two places? → Wrong level.

---

## Testing

See TESTING.md — sole authority for testing principles, strategy, organization, coverage, test data policy, and quality expectations.

---

## Git & workflow

### Conventional commits

All commits use conventional commit format: `type(scope): description`

| Type | When |
|------|------|
| `feat` | New capability |
| `fix` | Bug fix |
| `refactor` | Code change that neither fixes nor adds |
| `test` | Adding or fixing tests |
| `chore` | Build, CI, docs, tooling |
| `perf` | Performance improvement |
| `ci` | CI/CD changes |

- Present tense, imperative mood: "add X" not "added X"
- First line ≤ 72 characters
- Body wraps at 80 characters
- One logical change per commit
- Scope is the module/crate/component name: `feat(mneme): add graph score aggregation`

### Branching

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/<description>` | `feat/audiobook-chapters` |
| Bug fix | `fix/<description>` | `fix/gapless-gap` |
| Chore | `chore/<description>` | `chore/update-deps` |

- Always branch from `main`
- Always rebase before pushing (linear history)
- Never commit directly to `main`
- Squash merge is the default for PRs

### Worktrees for parallel work

When multiple agents or sessions work in parallel, use git worktrees for full filesystem isolation:

```bash
git worktree add ../repo-feat-name -b feat/name main
cd ../repo-feat-name
# work, commit, push, PR
# after merge:
git worktree remove ../repo-feat-name
git branch -d feat/name
```

One task, one worktree. Don't reuse worktrees. Build and test in the worktree, not in main.

### PR discipline

- PR title matches the conventional commit format
- PR description states what changed and why: not how (the code shows how)
- Every PR targets `main`
- Lint and type checks pass before pushing (don't rely solely on CI)

### CI validation gate

Every merge requires four passing checks: lint, type-check, test, and dependency audit. No exceptions, no manual overrides. Each language file specifies the exact commands under "Build/validate."

### Authorship

All commits use the operator's identity. Agents are tooling, not contributors.

---

## Dependencies

- **Justify every addition.** Each new dependency must earn its place. Prefer the standard library when adequate.
- **Pin unstable versions.** Pre-1.0 crates/packages pin to exact versions. Wrap external APIs in traits for replaceability.
- **Audit regularly.** Know what you depend on. `cargo-deny`, `npm audit`, `dotnet list package --vulnerable`.
- **No banned dependencies.** Each language file lists specific banned packages with reasons.
- **Verify packages exist.** AI tools hallucinate package names at a 20% rate. Confirm every new dependency exists and is the intended package before adding it.
- **Semantic versioning for libraries.** Follow SemVer. Breaking changes bump major. Pre-1.0 means the API can change without notice. Pin pre-1.0 dependencies to exact versions.

---

## Module boundaries & API design

### Dependency direction

Imports flow from higher layers to lower layers only. No dependency cycles. Adding a cross-module import requires verifying the dependency graph.

### Explicit public surface

Each module declares its public surface explicitly. Consumers import from the public API, not internal files.

### API principles

- **Return empty collections, not null.** Callers should not need null checks for collection returns.
- **Return values over output parameters.** Data flows through return values, not side-effect mutation of passed-in references.
- **Validate parameters at public boundaries.** Public functions validate their arguments. Private functions may rely on invariants established by callers.
- **Defensive copy at API boundaries.** Copy mutable data received from and returned to callers. Never let callers alias internal mutable state.

### Deprecation

Mark deprecated code with the language's mechanism (`#[deprecated]`, `@Deprecated`, `@warnings.deprecated`). Document the replacement. Set a removal version or date. Remove it when the time comes. Dead deprecation warnings that persist indefinitely are noise.

---

## Security

### Credentials and secrets

- No secrets in code. Not in constants, not in comments, not in test fixtures, not in config files checked into version control.
- Environment variables or secret managers for all credentials.
- `.gitignore` sensitive paths. Pre-commit hooks (gitleaks) catch accidental commits.
- Rotate credentials immediately if they ever touch version control, even on a branch that was never pushed.
- Use dedicated secret types (Rust: `secrecy::SecretString`, Python: avoid plain `str` for tokens). Secret types are zeroized on drop and excluded from Debug/Display output.
- Constant-time comparison for secrets (prevents timing attacks). Never use `==` on tokens or passwords.
- Implement Debug manually on types holding secrets. Redact the value: `[REDACTED]`.

### Crash diagnostics

- Structured panic/crash handlers that log to the persistent log file (not just stderr). A crash that only prints to a terminal nobody is watching is invisible.
- Enable backtraces in deployed binaries (Rust: `RUST_BACKTRACE=1` in systemd, Python: default behavior, Go: `GOTRACEBACK=all`).
- Assert messages are mandatory. Every assertion must describe the invariant it guards. Bare `assert(x)` is a debugging dead end.

### Input boundaries

- All external input is hostile until parsed. Validate on the trusted side of the boundary. Allowlists over denylists. Validate type, range, and length.
- Parameterized queries for all SQL. No string interpolation. No exceptions.
- Size limits on all user-provided input (file uploads, text fields, API payloads). Fail before allocating.
- Canonicalize paths and encodings before validating.

### Output encoding

Encode data for its output context. Context-appropriate escaping for HTML, shell commands, LDAP, log messages. The encoding belongs at the point of interpolation, not at the point of data entry.

### Deny by default

Access control fails closed. If authorization state is unknown or ambiguous, deny.

### Dependencies

- `cargo-deny`, `npm audit`, `dotnet list package --vulnerable`, `pip-audit` on every CI run.
- Evaluate transitive dependencies, not just direct ones.
- No dependencies with known CVEs in production builds.

### Principle of least privilege

- Services run with minimum required permissions.
- API tokens scoped to the narrowest access needed.
- File permissions explicit, not inherited defaults.

---

## Logging and observability

### Universal logging rules

- **Structured logging.** Key-value pairs, not interpolated strings. `session_id=abc123 action=load_config status=ok` not `"Loaded config for session abc123 successfully."`
- **Log at the handling site.** Not at the throw site. The handler has context about what to do with the error.
- **Log levels mean something:**

| Level | When |
|-------|------|
| `error` | Something failed that requires attention. Data loss, service degradation, unrecoverable state. |
| `warn` | Something unexpected happened but was handled. Approaching limits, deprecated usage. |
| `info` | Normal operations worth recording. Service start/stop, config loaded, connection established. |
| `debug` | Detailed operational data. Request/response details, state transitions, intermediate calculations. |
| `trace` | High-volume diagnostic data. Per-iteration values, wire-level protocol details. |

- **Never log secrets.** Credentials, tokens, API keys, passwords. Redact or omit.
- **Never log PII at info level or above.** User emails, names, IPs are debug-level at most, and only when necessary for diagnosis.
- **Handle errors once.** Each error is either logged **or** propagated: never both. Logging at the origin and propagating with context produces duplicate noise. Log at the point where the error is finally handled.
- **Guard expensive construction.** Don't compute values for log messages that won't be emitted at the current level. Check the level before building the message, or use lazy evaluation.
- **Include correlation IDs.** Every request, session, or operation chain carries an ID that appears in all related log entries.

### What to log

- Service startup and shutdown with configuration summary
- External service connections (established, lost, reconnected)
- Authentication events (success, failure, token refresh)
- Error handling decisions (what was caught, what was done about it)
- Configuration changes at runtime

### What not to log

- Routine success on hot paths (every request succeeded, every query returned)
- Full request/response bodies at info level (use debug)
- Redundant messages (logging both the throw and the catch of the same error)

### Observability contracts

Each module documents its observability emissions in its module-level doc comment. This is a contract: downstream consumers (Vector pipelines, GreptimeDB dashboards, alert rules, test assertions) depend on these signals existing with these names and fields.

**Contract format:**

```rust
//! # Observability
//!
//! ## Events
//! | Event | Level | Fields | Condition |
//! |-------|-------|--------|-----------|
//! | `prompt.dispatched` | info | `prompt_id`, `provider`, `project` | Prompt sent to worker |
//! | `prompt.completed` | info | `prompt_id`, `duration_ms`, `outcome` | Worker returns result |
//! | `prompt.failed` | error | `prompt_id`, `error`, `retries` | All retries exhausted |
//!
//! ## Metrics
//! | Metric | Type | Labels | Condition |
//! |--------|------|--------|-----------|
//! | `dispatch.queue_depth` | gauge | `project` | Per scheduling cycle |
//! | `dispatch.latency_ms` | histogram | `provider`, `outcome` | Per prompt completion |
//!
//! ## Spans
//! | Span | Fields | Wraps |
//! |------|--------|-------|
//! | `dispatch_prompt` | `prompt_id`, `provider` | Full dispatch lifecycle |
//! | `run_gate` | `prompt_id`, `gate_name` | Single gate check |
```

**Rules:**

- Events use `noun.verb` naming: `prompt.dispatched`, `gate.passed`, `worktree.created`.
- Metrics use `namespace.snake_case` naming: `dispatch.queue_depth`, `gate.pass_rate`.
- Every event listed in the contract must have a corresponding `tracing` call in the module's code. A contract entry without an emission is a failing lint check.
- Every `tracing::info!` or `tracing::error!` call in a module should appear in that module's contract. An emission without a contract entry is undocumented behavior.
- Changing event names, removing fields, or altering emission conditions is a breaking change. Add new events freely; modify or remove through deprecation.
- Span names are stable identifiers. Renaming a span breaks any downstream query that references it.

### Tracing instrumentation

**Spans on async operations:**

```rust
#[tracing::instrument(skip(self), fields(prompt_id = %prompt.id, provider = %provider))]
async fn dispatch_prompt(&self, prompt: &Prompt, provider: &str) -> Result<Outcome> {
    // span auto-records duration and outcome
}
```

**Structured event emission:**

```rust
// WHY: Structured fields enable Vector/GreptimeDB pipeline parsing.
// Interpolated strings require regex extraction downstream.
tracing::info!(
    prompt_id = %prompt.id,
    provider = %provider,
    duration_ms = elapsed.as_millis(),
    outcome = %outcome,
    "prompt.completed"
);
```

**Health endpoints for long-running services:**

```rust
async fn health_check(&self) -> HealthStatus {
    HealthStatus {
        ready: self.is_ready(),
        dependencies: vec![
            ("database", self.db.ping().await.is_ok()),
            ("queue", self.queue.is_connected()),
        ],
    }
}
```

---

## Writing

All prose: documentation, READMEs, specs, PR descriptions, commit bodies, comments: follows the same standards. Full treatment in `WRITING.md`. Summary:

- **Direct and concrete.** State the thing. No throat-clearing, no preamble, no "it's that."
- **Active voice.** "The server rejects malformed requests" not "Malformed requests are rejected by the server."
- **Short sentences.** If a sentence has three commas, it's two sentences.
- **No em dashes.** Use commas, parentheses, or separate sentences.
- **No AI tropes.** See `WRITING.md` for the full banned-words list.
- **Answer first.** Lead with the conclusion or decision. Context follows.
- **Structure over paragraphs.** Tables, headers, and lists when the content has structure. Prose when it doesn't.

---

## Code review

### What reviewers check

1. **Does it do what the PR says?** Read the description, read the diff. Do they match?
2. **Error handling.** Are errors propagated with context? Any silent catches? Any unwraps in library code?
3. **Naming.** Do names describe what things are? Would a reader unfamiliar with the PR understand the code?
4. **Tests.** Does the change have tests? Do the tests test behavior, not implementation?
5. **Scope.** Does the PR do one thing? Unrelated changes get their own PR.
6. **Information hierarchy.** Is new code in the right place? Shared logic in the right module? No duplication?

### How to give feedback

- **Be specific.** "This name is unclear" is useless. "Rename `proc` to `process_session` since it handles session lifecycle" is actionable.
- **Distinguish blocking from suggestion.** "Nit:" for style preferences. No prefix for things that must change.
- **Explain why.** "Add `.context()` here because bare `?` loses the file path" teaches. "Add context" doesn't.
- **Don't bikeshed.** If the formatter doesn't catch it, it's probably not worth a comment.

---

## AI agent guidance

Patterns that AI agents (Claude Code, Copilot, Cursor) consistently get wrong, validated through 2025-2026 testing:

1. **Over-engineering**: wrapper types with no value, trait abstractions with one implementation, premature generalization
2. **Outdated patterns**: using deprecated libraries, old language features, patterns from 3 years ago
3. **Hallucinated APIs**: method signatures that don't exist. Always `cargo check` / compile / type-check.
4. **Clone/copy to satisfy type system**: restructure ownership instead of papering over it
5. **Comments restating code**: the code is the documentation. Delete the comment.
6. **Inconsistent error handling**: mixing error strategies within a codebase
7. **Test names like `test_add` or `it_works`**: names must describe behavior
8. **Suppressing warnings**: fix the warning, don't suppress it. `#[allow]` / `@SuppressWarnings` require justification.
9. **Adding dependencies for functionality**: if it's 10 lines, write it
10. **Performing social commentary in code comments**: no "this is a great pattern" or "elegant solution". Just the code.
11. **Silent failure**: removing safety checks, swallowing errors, or generating plausible-looking but incorrect output to avoid crashing. AI produces code that *runs* but silently does the wrong thing. Worse than a crash.
12. **Hallucinated dependencies**: referencing packages that don't exist. 20% of AI code samples reference nonexistent packages. Attackers register these names (slopsquatting). Verify every dependency.
13. **Code duplication over refactoring**: generating new code blocks rather than reusing existing functions. AI doesn't propose "use the existing function at line 340." Extract and reuse.
14. **Context drift in multi-file changes**: patterns applied consistently to early files but drifting in later files as context fills. Renaming a type in 30 of 50 files. Validate consistency post-refactor.
15. **Tautological tests**: mocking the system under test, asserting on values constructed inside the test, achieving 100% coverage with near-zero defect detection. If the test can't fail when the code is wrong, it's not a test.
16. **Concurrency errors**: naive locking, missing synchronization, holding locks across await points. AI can describe race conditions but cannot diagnose them because bugs live in interleavings, not in text.
17. **Stripping existing safety checks**: removing input validation, authentication checks, rate limiting, or error boundaries during refactoring because it doesn't understand *why* they were there. Preserve every guard unless you can explain why it's unnecessary.
18. **Adding unrequested features**: padding implementations with config options, extra error variants, helper functions, and generalization nobody asked for. Implement exactly what was specified. Extra code is extra maintenance, extra surface area, and extra merge conflicts.
19. **Refactoring adjacent code**: renaming variables in untouched files, reorganizing imports in modules that aren't part of the task, adding docstrings to functions that weren't changed. Diff noise kills parallel work and obscures the actual change. Touch only what the task requires.
20. **Happy-path-only tests**: writing tests for the success case and ignoring error paths, boundary conditions, and edge cases. If every test passes a valid input and asserts on the expected output, the test suite is decorative.
