# Coding Standards

> Universal principles for all code, all languages, all projects. Language-specific rules live in separate files in this directory — they are **additive** to this document. Read this first.

---

## Philosophy

**Code is the documentation.** Names, types, and structure carry meaning. If code needs a comment to explain what it does, rewrite the code. Comments explain *why*, never *what*.

**Fail fast, fail loud.** Crash on invariant violations. No defensive fallbacks for impossible states. Sentinel values and silent degradation are bugs. Surface errors at the point of origin with full context.

**Parse, don't validate.** Invalid data cannot exist past the point of construction. Newtypes, validation constructors, and type-level guarantees enforce invariants at the boundary — HTTP handlers, config loading, deserialization, CLI argument parsing. Once a value is constructed, its validity is a compile-time or construction-time guarantee. Deserialization must route through the parser: derive-based frameworks (`serde`, `System.Text.Json`, `encoding/json`) bypass constructors by default.

**Prefer immutable.** Default to immutable data. Require explicit justification for mutability. Mutable shared state is the root of most concurrency bugs and a common source of aliasing surprises.

**Minimize surface area.** Private by default. Every public item is a commitment. Expose the smallest API that serves the need. `pub(crate)` (Rust), `internal` (C#), unexported (Kotlin/TS), `_prefix` (Python).

**No dead weight.** No dead code. No commented-out blocks. No unused imports. Delete it — git has history. No hardcoded IDs, dates, or magic numbers. Parameterize or reference a constant.

**No shortcuts.** Build the right thing from the start. If the SDK is better than the CLI wrapper, build the SDK. If the architecture needs three crates, build three crates. Don't ship a "quick version" you know you'll replace — time spent on throwaway work is stolen from the real thing. MVPs are for validating markets, not for code you're certain about.

**Best tool for the job.** Every decision — language, library, architecture, data structure — is made on merit. No defaults by inertia. No "we've always done it this way." If the current tool is wrong, replace it. If a better option exists and the migration cost is justified, migrate. Comfort with a tool is not a reason to use it; fitness for the problem is.

**No compromise on quality.** Every PR should be clean, tested, and reviewed before merge. Fix issues immediately, don't defer. "Good enough" is not a standard. The goal is code you'd be confident handing to a stranger with zero context — they should be able to read it, understand it, and trust it. Cutting corners creates debt that compounds faster than the time it "saved."

**Format at the boundary.** Percentages as decimals (0.42), currency as numbers, dates as timestamps internally. Format when rendering for display, not in queries or transforms.

**Idempotent by design.** Operations that may be retried, replayed, or delivered more than once must produce the same result regardless of repetition. Use idempotency keys for API mutations. Design event handlers to tolerate duplicate delivery. Message processing, webhook handlers, and state transitions are the primary risk areas. If replaying an operation would corrupt state, the operation is broken.

---

## Comments

### Zero-Comment Default

Most code should have zero inline comments. Self-documenting names and clear structure are the standard. Inline comments exist only for genuinely non-obvious *why* explanations.

Never include:
- Creation dates, author attributions, changelog entries
- AI generation indicators
- "Upgraded from X" or migration notes
- Comments restating what the code does

### Structured Comment Tags

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
// WHY: CozoDB returns results as JSON arrays, not named columns.
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

### Banned Patterns

- Bare `// TODO` or `// FIXME` without an issue number — invisible debt
- `// HACK`, `// XXX`, `// TEMP` — use `FIXME(#NNN)` with a tracking issue
- `// NOTE:` as a substitute for clear code — rewrite the code first
- Commented-out code blocks — delete them, git has history
- End-of-line comments explaining what a line does — rename the variable instead

### Doc Comments

Doc comments (`///` in Rust, `/** */` in TS/Kotlin, `<summary>` in C#, docstrings in Python) apply to:

- Public API items that cross module boundaries
- Functions that can panic or throw unexpectedly (document when/why)
- Functions with non-obvious error conditions
- `unsafe` functions — mandatory safety contract documentation

Not required on:
- Private/internal functions with self-documenting signatures
- Test functions (the name IS the documentation)
- Trivial getters, builders, or standard trait implementations

One-line file headers (module-level doc comment) are encouraged: describe the module's purpose in a single sentence.

---

## Naming

### Code Identifiers

| Context | Convention | Example |
|---------|-----------|---------|
| Types / Traits / Classes | `PascalCase` | `SessionStore`, `MediaProvider` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_TURNS`, `DEFAULT_PORT` |
| Events | `noun:verb` | `turn:before`, `tool:called` |

Function and variable casing is language-specific — see individual language files.

**Universal naming rules:**
- Verb-first for functions: `load_config`, `create_session`, `parse_input`. Drop `get_` prefix on simple getters.
- Boolean variables/columns: `is_` or `has_` prefix.
- Self-documenting over short. `schema_db_path` not `p`. `active_cases` not `df2`.
- If you need a comment to explain what a name means, rename it.

### Gnomon System (Persistent Names)

Module directories, agent identities, subsystems, and major features follow the gnomon naming convention. Names identify **essential natures**, not implementations.

Applies to: modules, crates, agents, subsystems, features that persist across refactors.
Does not apply to: variables, functions, test fixtures, temporary branches.

Process:
1. Identify the essential nature (not the implementation detail)
2. Construct from Greek roots using the prefix-root-suffix system
3. Validate with the layer test (L1 practical → L4 reflexive)
4. Check topology against existing names in the ecosystem
5. If no Greek word fits naturally, the essential nature isn't clear yet — wait

### File & Directory Organization

| Context | Convention | Example |
|---------|-----------|---------|
| Source files | Language convention (see language files) | `session_store.rs`, `SessionStore.cs` |
| Scripts | `kebab-case` | `deploy-worker.sh` |
| Canonical docs | `UPPER_SNAKE.md` | `STANDARDS.md`, `ARCHITECTURE.md` |
| Working docs | `lower-kebab.md` | `planning-notes.md` |
| Directories | `snake_case` | `session_store/`, `test_fixtures/` |
| Timestamped files | `YYYYMMDD_description.ext` | `20260313_export.csv` |

- `snake_case` for directories. No hyphens, no camelCase, no spaces.
- Max 2–3 nesting levels inside any project. Flat > nested.
- No version numbers in filenames — version in file headers or git tags.

### Project Structure

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

---

## Error Handling

Every error must:

1. **Carry context** — what operation failed, with what inputs
2. **Be typed** — callers can match on error kind, not parse strings
3. **Propagate** — chain errors with context, never swallow the cause
4. **Surface** — log at the point of *handling*, not the point of *origin*

### Fail Fast

- Panic/crash on programmer bugs: violated invariants, impossible states, corrupted data
- Return errors for anything the caller could reasonably handle or report
- Prefer descriptive assertions over silent fallbacks: `expect("session must exist after authentication")` over bare `unwrap()`
- Never panic in library code for recoverable conditions

### No Silent Catch

- Every catch/except/match block must: log, propagate, return a meaningful value, or explain why it discards
- `/* intentional: reason */` for deliberate discard — never an empty catch body
- If you're catching to ignore, you're hiding a bug

### No Sentinel Values

- Do not return `null`/`None`/`-1`/empty string to signal failure
- Use the language's error type: `Result`, exceptions, `sealed class` error hierarchies
- Invalid data cannot exist past the point of construction

### Resource Lifecycle

Acquired resources must have a defined cleanup path. Use RAII (`Drop` in Rust), `defer` (Go), `with`/context managers (Python), `using` (C#), `use` (Kotlin). Never rely on garbage collection or finalizers for resource cleanup. Database connections, file handles, and sockets are released as soon as work completes.

---

## Concurrency

### Ownership

Every spawned task, goroutine, thread, or async operation must have a defined owner responsible for its lifecycle. Fire-and-forget is banned — if you spawn it, something must join, cancel, or supervise it.

### Shared State

- **Prefer message passing** (channels, actors) over shared memory and locks
- When shared mutable state is necessary, synchronize all access. Document which lock guards which data.
- Prefer higher-level constructs (channels, executors, actors) over raw mutexes and atomics. Use atomics only for single counters or flags, not for coordinating state.
- Never hold a lock across an await point, an I/O operation, or a callback

### Thread Safety Contracts

Public types that may be used concurrently must declare their safety guarantee: immutable (always safe), thread-safe (synchronized internally), conditionally thread-safe (caller must synchronize), or not thread-safe (single-threaded only).

### Ordering

Never rely on execution order between concurrent units unless explicitly synchronized. Code that "works because the goroutine is always fast enough" is a race condition.

### Testing Concurrent Code

Concurrency bugs live in interleavings, not in text. Static analysis and code review catch a fraction. Use the tools:
- **Sanitizers:** TSan (C++, Rust via `-Z sanitizer=thread`), `go test -race`
- **Model checkers:** `loom` (Rust), `jcstress` (JVM) for lock-free algorithms
- **Stress tests:** Run concurrent tests under high contention with randomized timing. Single-pass success proves nothing; 10,000-pass success builds confidence.
- **Deterministic replay:** Seed-based schedulers for reproducing intermittent failures

---

## Configuration

- **Config in environment, not code.** Values that vary between deploys — credentials, hostnames, feature flags — live in environment variables or external config stores, never compiled in.
- **No hardcoded secrets.** Connection strings, API keys, and passwords never in source. Not in config files committed to git. Use secret stores or environment injection.
- **Inject inward, never fetch.** Configuration values are pushed from the outermost layer (main, entry point) and injected into inner modules. Inner code receives config — it never reads environment variables or config files directly.
- **Fail on invalid config at startup.** Validate all configuration during initialization. Don't discover bad config at 3 AM when the code path first executes.

---

## Information Hierarchy

This principle governs everything: documentation, configuration, standards, code architecture, API design. Not just docs.

### Single Source of Truth

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

### Document Lifecycle

Documentation follows the code it describes. When code is deleted, moved, or substantially refactored, update or remove its documentation in the same change. Orphaned docs — documentation for code that no longer exists — are worse than no docs because they actively mislead.

### Litmus Test

Before writing anything (doc, config, code), ask:
1. Does this fact already exist somewhere? → Point to it.
2. Is this true for more than one context? → Move it up.
3. Will someone need to update this in two places? → Wrong level.

---

## Testing

### Behavior Over Implementation

Test what the code **does**, not how it does it. If a refactor breaks your tests but doesn't break behavior, your tests are wrong.

- One logical assertion per test
- Descriptive names: `returns_empty_when_session_has_no_turns`, not `test_add` or `it_works`
- Same-directory test files (colocated with source, not in a separate tree)

### Property-Based Testing

Use property tests for:
- Serialization round-trips (`deserialize(serialize(x)) == x`)
- Algebraic properties (commutativity, associativity, idempotency)
- State machine invariants
- Edge case discovery at scale

Example tests document expected behavior. Property tests catch the unexpected.

### What to Test

Focus testing effort on behavior with consequences:
- Boundary conditions (empty, one, many, max, overflow)
- Error paths (invalid input, unavailable service, timeout, permission denied)
- State transitions (especially concurrent access to shared state)
- Serialization round-trips (`deserialize(serialize(x)) == x`)
- Idempotency (replaying the same operation produces the same result)
- Security boundaries (authentication, authorization, input validation)

### No Coverage Targets

Coverage is a vanity metric. Test the behavior that matters. Untested code should be deliberately untested (trivial, generated, or unreachable), not accidentally missed.

### Test Data Policy

All test data must be synthetic. No real personal information in test fixtures, assertions, or examples.

**Standard test identities:**
- Users: `alice`, `bob`, `charlie`
- Emails: `alice@example.com`, `bob@example.org` (RFC 2606 reserved domains only)
- Phones: `+1-555-0100` through `+1-555-0199` (ITU reserved for fiction)
- IPs: `192.0.2.x`, `198.51.100.x`, `203.0.113.x` (RFC 5737 documentation ranges)
- IPv6: `2001:db8::/32` (RFC 3849 documentation range)
- Domains: `example.com`, `example.org`, `example.net`, `*.test` (RFC 2606/6761 reserved)

**Never use:** real names, emails, usernames, internal IPs/hostnames, personal facts, credentials, or API keys. Never copy production data into test environments.

**Test data builders:** Use builder/factory patterns with sensible defaults. Each test overrides only the fields it cares about. When a field is added to the struct, only the builder default needs updating — not every test.

**Determinism:** Any randomized test data must be seeded. The seed must be logged or persisted. Proptest regression files, hypothesis databases, and equivalent fixtures are checked into version control.

---

## Git & Workflow

### Conventional Commits

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

### Worktrees for Parallel Work

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

### PR Discipline

- PR title matches the conventional commit format
- PR description states what changed and why — not how (the code shows how)
- Every PR targets `main`
- Lint and type checks pass before pushing (don't rely solely on CI)

### CI Validation Gate

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

## Module Boundaries & API Design

### Dependency Direction

Imports flow from higher layers to lower layers only. No dependency cycles. Adding a cross-module import requires verifying the dependency graph.

### Explicit Public Surface

Each module declares its public surface explicitly. Consumers import from the public API, not internal files.

### API Principles

- **Return empty collections, not null.** Callers should not need null checks for collection returns.
- **Return values over output parameters.** Data flows through return values, not side-effect mutation of passed-in references.
- **Validate parameters at public boundaries.** Public functions validate their arguments. Private functions may rely on invariants established by callers.
- **Defensive copy at API boundaries.** Copy mutable data received from and returned to callers. Never let callers alias internal mutable state.

### Deprecation

Mark deprecated code with the language's mechanism (`#[deprecated]`, `@Deprecated`, `@warnings.deprecated`). Document the replacement. Set a removal version or date. Remove it when the time comes. Dead deprecation warnings that persist indefinitely are noise.

---

## Security

### Credentials and Secrets

- No secrets in code. Not in constants, not in comments, not in test fixtures, not in config files checked into version control.
- Environment variables or secret managers for all credentials.
- `.gitignore` sensitive paths. Pre-commit hooks (gitleaks) catch accidental commits.
- Rotate credentials immediately if they ever touch version control, even on a branch that was never pushed.

### Input Boundaries

- All external input is hostile until parsed. Validate on the trusted side of the boundary. Allowlists over denylists. Validate type, range, and length.
- Parameterized queries for all SQL. No string interpolation. No exceptions.
- Size limits on all user-provided input (file uploads, text fields, API payloads). Fail before allocating.
- Canonicalize paths and encodings before validating.

### Output Encoding

Encode data for its output context. Context-appropriate escaping for HTML, shell commands, LDAP, log messages. The encoding belongs at the point of interpolation, not at the point of data entry.

### Deny by Default

Access control fails closed. If authorization state is unknown or ambiguous, deny.

### Dependencies

- `cargo-deny`, `npm audit`, `dotnet list package --vulnerable`, `pip-audit` on every CI run.
- Evaluate transitive dependencies, not just direct ones.
- No dependencies with known CVEs in production builds.

### Principle of Least Privilege

- Services run with minimum required permissions.
- API tokens scoped to the narrowest access needed.
- File permissions explicit, not inherited defaults.

---

## Logging and Observability

### Universal Logging Rules

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
- **Handle errors once.** Each error is either logged **or** propagated — never both. Logging at the origin and propagating with context produces duplicate noise. Log at the point where the error is finally handled.
- **Guard expensive construction.** Don't compute values for log messages that won't be emitted at the current level. Check the level before building the message, or use lazy evaluation.
- **Include correlation IDs.** Every request, session, or operation chain carries an ID that appears in all related log entries.

### What to Log

- Service startup and shutdown with configuration summary
- External service connections (established, lost, reconnected)
- Authentication events (success, failure, token refresh)
- Error handling decisions (what was caught, what was done about it)
- Configuration changes at runtime

### What Not to Log

- Routine success on hot paths (every request succeeded, every query returned)
- Full request/response bodies at info level (use debug)
- Redundant messages (logging both the throw and the catch of the same error)

---

## Writing

All prose — documentation, READMEs, specs, PR descriptions, commit bodies, comments — follows the same standards. Full treatment in `WRITING.md`. Summary:

- **Direct and concrete.** State the thing. No throat-clearing, no preamble, no "it's worth noting that."
- **Active voice.** "The server rejects malformed requests" not "Malformed requests are rejected by the server."
- **Short sentences.** If a sentence has three commas, it's two sentences.
- **No em dashes.** Use commas, parentheses, or separate sentences.
- **No AI tropes.** See `WRITING.md` for the full banned-words list.
- **Answer first.** Lead with the conclusion or decision. Context follows.
- **Structure over paragraphs.** Tables, headers, and lists when the content has structure. Prose when it doesn't.

---

## Code Review

### What Reviewers Check

1. **Does it do what the PR says?** Read the description, read the diff. Do they match?
2. **Error handling.** Are errors propagated with context? Any silent catches? Any unwraps in library code?
3. **Naming.** Do names describe what things are? Would a reader unfamiliar with the PR understand the code?
4. **Tests.** Does the change have tests? Do the tests test behavior, not implementation?
5. **Scope.** Does the PR do one thing? Unrelated changes get their own PR.
6. **Information hierarchy.** Is new code in the right place? Shared logic in the right module? No duplication?

### How to Give Feedback

- **Be specific.** "This name is unclear" is useless. "Rename `proc` to `process_session` since it handles session lifecycle" is actionable.
- **Distinguish blocking from suggestion.** "Nit:" for style preferences. No prefix for things that must change.
- **Explain why.** "Add `.context()` here because bare `?` loses the file path" teaches. "Add context" doesn't.
- **Don't bikeshed.** If the formatter doesn't catch it, it's probably not worth a comment.

---

## AI Agent Guidance

Patterns that AI agents (Claude Code, Copilot, Cursor) consistently get wrong, validated against 2025 empirical research:

1. **Over-engineering** — wrapper types with no value, trait abstractions with one implementation, premature generalization
2. **Outdated patterns** — using deprecated libraries, old language features, patterns from 3 years ago
3. **Hallucinated APIs** — method signatures that don't exist. Always `cargo check` / compile / type-check.
4. **Clone/copy to satisfy type system** — restructure ownership instead of papering over it
5. **Comments restating code** — the code is the documentation. Delete the comment.
6. **Inconsistent error handling** — mixing error strategies within a codebase
7. **Test names like `test_add` or `it_works`** — names must describe behavior
8. **Suppressing warnings** — fix the warning, don't suppress it. `#[allow]` / `@SuppressWarnings` require justification.
9. **Adding dependencies for trivial functionality** — if it's 10 lines, write it
10. **Performing social commentary in code comments** — no "this is a great pattern" or "elegant solution". Just the code.
11. **Silent failure** — removing safety checks, swallowing errors, or generating plausible-looking but incorrect output to avoid crashing. AI produces code that *runs* but silently does the wrong thing. Worse than a crash.
12. **Hallucinated dependencies** — referencing packages that don't exist. 20% of AI code samples reference nonexistent packages. Attackers register these names (slopsquatting). Verify every dependency.
13. **Code duplication over refactoring** — generating new code blocks rather than reusing existing functions. AI doesn't propose "use the existing function at line 340." Extract and reuse.
14. **Context drift in multi-file changes** — patterns applied consistently to early files but drifting in later files as context fills. Renaming a type in 30 of 50 files. Validate consistency post-refactor.
15. **Tautological tests** — mocking the system under test, asserting on values constructed inside the test, achieving 100% coverage with near-zero defect detection. If the test can't fail when the code is wrong, it's not a test.
16. **Concurrency errors** — naive locking, missing synchronization, holding locks across await points. AI can describe race conditions but cannot diagnose them because bugs live in interleavings, not in text.
17. **Stripping existing safety checks** — removing input validation, authentication checks, rate limiting, or error boundaries during refactoring because it doesn't understand *why* they were there. Preserve every guard unless you can explain why it's unnecessary.
18. **Adding unrequested features** — padding implementations with config options, extra error variants, helper functions, and generalization nobody asked for. Implement exactly what was specified. Extra code is extra maintenance, extra surface area, and extra merge conflicts.
19. **Refactoring adjacent code** — renaming variables in untouched files, reorganizing imports in modules that aren't part of the task, adding docstrings to functions that weren't changed. Diff noise kills parallel work and obscures the actual change. Touch only what the task requires.
20. **Happy-path-only tests** — writing tests for the success case and ignoring error paths, boundary conditions, and edge cases. If every test passes a valid input and asserts on the expected output, the test suite is decorative.
