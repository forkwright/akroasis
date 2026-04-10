# Architecture

> Structural standards for multi-crate workspaces, module organization, and system design. Language-agnostic principles with Rust-specific guidance where noted.

---

## Dependency direction

Dependencies flow one way. Lower layers never import higher layers.

```
Leaf (types, errors, utilities)
  ↓
Low (storage, providers, config)
  ↓
Mid (domain logic, orchestration)
  ↓
High (API surface, handlers)
  ↓
Top (binary entrypoint)
```

Every import must go downward. If a lower crate needs behavior from a higher crate, define a trait in the lower crate and implement it in the higher crate. Dependency inversion, not dependency violation.

No circular dependencies. `cargo tree` must show a DAG. If crate A depends on B and B depends on A, one of them is wrong.

---

## Crate boundaries

Each crate owns one concern. The name states the concern. If you can't name the concern in one word, the crate does too much.

### When to split a crate

- Two modules have no shared types and no mutual imports
- A module could be used by a different project without modification
- Compile times are dominated by one crate (split reduces incremental rebuild scope)

### When NOT to split

- The modules share internal types extensively
- Splitting would require duplicating types or adding a "types" crate
- The split would create a crate with <200 lines

---

## API surface

### Minimize public surface

Default to private. Promote to `pub(crate)` when another module needs it. Promote to `pub` only when another crate needs it.

Every `pub` item is a commitment. Downstream code can depend on it, blocking refactors. A function that's `pub` but only used within the crate is a maintenance liability.

### Thin binaries

The binary crate (top layer) is a thin shell. It parses arguments, wires dependencies together, and delegates to library crates. Business logic never lives in the binary.

Target: binary entrypoint under 100 lines. Each subcommand in its own file.

### Re-exports

Explicit re-exports over wildcards. `pub use types::Fact;` not `pub use types::*;`. Wildcard re-exports leak internal module structure and make it impossible to know what's public without reading the submodule.

---

## Error boundaries

See STANDARDS.md § Error Handling for universal principles (exhaustive types, boundary conversion, context propagation).

### One error type per crate

Each library crate defines one `Error` enum (or struct). The binary crate uses `anyhow` for top-level aggregation. Library crates never use `anyhow`.

### Context at every boundary

```rust
store.open(&path).context(OpenStoreSnafu { path })?;
```

---

## Configuration

### Inject, don't global

Configuration flows through function parameters or trait implementations, not global state or environment reads deep in library code. The binary reads config; libraries accept config as arguments.

### Feature flags for optional capabilities

Heavy dependencies (ML models, GUI frameworks, optional integrations) go behind feature flags. A minimal `cargo build` compiles only the core. Default features include what most users need. Optional features require explicit opt-in.

---

## Feature flag propagation policy

Feature flags are architectural decisions. A poorly propagated flag silently compiles out functionality, creates untested code paths, and blocks refactors. This section covers compile-time Cargo features. For runtime feature flags (config/env var toggling), see ENVIRONMENT.md § Feature flags.

### Compile-time vs runtime feature flags

Use compile-time flags (`#[cfg(feature = "...")]`) when the flag controls which code is compiled. Use runtime flags (config values) when the flag controls which code path executes.

**Compile-time flags are correct when:**

- The feature pulls in heavy dependencies that most users don't need. WHY: dependencies that aren't compiled don't affect build time or binary size. A runtime check still links the dependency.
- The feature is platform-specific or requires system libraries not universally available. WHY: the code won't compile on platforms without the library. A runtime check would fail at link time.
- The feature gates test infrastructure (`test-support`). WHY: production binaries must not ship mock providers or test fixtures.

**Runtime flags are correct when:**

- The feature is experimental behavior that needs gradual rollout or quick rollback. WHY: recompiling and redeploying to toggle a flag is too slow for operational response.
- The feature gates a code path where both sides compile against the same dependencies. WHY: compile-time gating adds conditional compilation complexity for no binary size or build time benefit.
- The flag must change per-environment without rebuilding. WHY: the same binary must run in dev, staging, and production with different behavior.

Do not mix the two. A feature that requires a compile-time dependency but is toggled at runtime needs both: a compile-time flag for the dependency and a runtime flag for activation. The compile-time flag gates availability; the runtime flag gates use.

### Propagation across crate boundaries

Cargo features are additive and workspace-wide. A feature enabled by any crate in the dependency tree is enabled for all crates that depend on that package. This means feature flags propagate upward — and mistakes propagate silently.

**Rule: features propagate through explicit forwarding, never through default features of dependencies. Transitive propagation is the default — if crate C depends on B which depends on A, and B forwards A's feature `foo` as `b/foo`, then C must forward it as `c/foo = ["b/foo"]` to make it available to its own consumers.**

The crate that owns the functionality defines the feature. Every crate above it in the dependency graph that needs to expose the feature re-exports it by forwarding:

```toml
# crates/phronesis/Cargo.toml (owns the feature)
[features]
cron_scheduler = ["dep:uuid", "dep:fd-lock"]

# crates/kanon-cli/Cargo.toml (forwards the feature)
[features]
cron_scheduler = ["phronesis/cron_scheduler"]
```

WHY: forwarding makes the propagation chain visible in `Cargo.toml`. If `kanon-cli` doesn't forward `cron_scheduler`, the feature is invisible from the binary crate and cannot be enabled by the end user. If it's hidden inside a `default` feature of a mid-layer crate, the end user can't disable it.

**Rules for propagation:**

1. **Define features in the lowest crate that implements the gated code.** Higher crates forward; they don't redefine. WHY: feature definitions in high-level crates that conditionally import low-level code invert the dependency direction.

2. **Never add features to `default` that pull in optional dependencies.** Default features are for the common case. Heavy or experimental dependencies require explicit opt-in at the top of the chain. WHY: `default-features = false` in one dependency doesn't cascade — any other dependency that enables the same crate with default features will re-enable them.

3. **Name features after the capability, not the dependency.** `cron_scheduler`, not `uuid`. WHY: the dependency is an implementation detail. If you swap `uuid` for `ulid`, every `Cargo.toml` in the chain must change its feature name if the feature was named after the dependency.

4. **Document the forwarding chain.** Each crate's `CLAUDE.md` lists its features and which downstream features they activate. WHY: `cargo tree -e features` shows the resolved graph but not the intent.

**Exception: internal-only features.** Features that exist solely for the crate's own test or build infrastructure — `test-support`, `test-utils`, `_internal` — do not need forwarding. These features are never meaningful to downstream consumers and exposing them pollutes the public feature surface. If a feature is internal-only, name it with a `test-` prefix or `_internal` suffix to signal intent. WHY: forwarding a test fixture feature to a binary crate serves no purpose and creates a maintenance burden with no consumer benefit.

### Auditing feature propagation

Run `cargo tree -f '{p} {f}'` to display each package with its resolved features. This shows what is actually enabled, not what is declared. Compare against `Cargo.toml` forwarding entries to find:

- **Missing propagation:** a dependency's feature is enabled via `features = ["foo"]` in `[dependencies]` but the consuming crate has no corresponding `foo = ["dep/foo"]` in its own `[features]` section.
- **Dead features:** a feature is declared in `[features]` but never activated by any path in the workspace.
- **Implicit activation:** a feature is enabled via a transitive dependency's `default` features rather than explicit forwarding.

```bash
# Show all packages and their resolved features
cargo tree -f '{p} {f}'

# Show the feature graph specifically
cargo tree -e features

# Check a specific feature's activation path
cargo tree -f '{p} {f}' -i crate_name
```

### Feature flag testing

Untested feature combinations are untested code. Compile-time flags create a combinatorial matrix of possible builds — each combination is a distinct program.

**Minimum test matrix:**

- **All features off** (minimal build). Confirms the core compiles and runs without optional functionality.
- **All features on** (maximal build). Confirms no feature combinations conflict.
- **Each feature individually on** (isolation). Confirms each feature is self-contained and doesn't implicitly depend on another feature being enabled.

```yaml
# CI matrix example
strategy:
  matrix:
    features: ["", "--all-features", "--features cron_scheduler"]
```

WHY: the most common feature flag bug is code inside `#[cfg(feature = "A")]` that calls a function only available under `#[cfg(feature = "B")]`. This compiles when both are enabled (the "all features" case every developer uses) and fails only when A is enabled without B — a configuration no one tests until a user hits it.

**Guard every `#[cfg(feature)]` block with a corresponding test under the same gate:**

```rust
#[cfg(feature = "cron_scheduler")]
mod cron;

#[cfg(test)]
#[cfg(feature = "cron_scheduler")]
mod cron_tests {
    // Tests that exercise the cron module
}
```

WHY: tests that are not behind the same feature gate run (and pass vacuously) even when the feature is off, giving false confidence.

**CI must run the minimal feature set as its first check.** If the minimal build fails, the feature boundary is broken. Catching this early prevents the common failure mode where every developer builds with `--all-features` and breakage only surfaces when a user uses the default feature set.

### Feature flag deprecation lifecycle

Feature flags are not permanent. Every flag has a lifecycle: experimental, stable, deprecated, removed. Flags that linger become load-bearing and impossible to remove.

**Lifecycle stages:**

1. **Experimental.** Feature is behind a non-default flag. Documentation marks it as unstable. API may change without a major version bump. WHY: the flag boundary isolates instability from the stable surface.

2. **Stable.** Feature has shipped for at least one release cycle without breaking changes. Move it to default features if it serves the common case. If it's niche, leave it as opt-in but remove the "experimental" marker. WHY: features that stay non-default forever accumulate. Each non-default feature is a CI matrix entry and a documentation burden.

3. **Deprecated.** The feature is scheduled for removal. Add a `#[deprecated]` attribute to the feature's public API surface. Emit a compile-time warning using `#[cfg_attr]`:

    ```rust
    #[cfg(feature = "old_provider")]
    #[deprecated(since = "0.8.0", note = "use `new_provider` feature instead")]
    pub mod old_provider;
    ```

    Document the removal timeline in CHANGELOG. Keep the feature functional for one release cycle minimum. WHY: downstream users need time to migrate.

4. **Removed.** Delete the feature from `Cargo.toml`, remove all `#[cfg(feature)]` blocks, and remove the forwarding entries from every crate in the chain. Do not leave dead `cfg` blocks. WHY: a feature entry in `Cargo.toml` with no gated code is misleading. A `cfg` block with no corresponding feature silently compiles out.

**Every feature flag added must have an owner and a target lifecycle stage.** Add a comment in the defining crate's `Cargo.toml`:

```toml
[features]
# Experimental — owner: @ck — target: default or remove by 0.9.0
cron_scheduler = ["dep:uuid", "dep:fd-lock"]
```

WHY: features without owners become permanent. Features without timelines never graduate or get removed.

---

## Module organization

**Principle:** Cognitive load determines unit size. A reader should grasp a file's purpose without scrolling, and a function's logic without losing context. The numbers below are guidelines derived from this principle, not rigid limits.

### File size

~800 lines per file guideline. If a file exceeds this, split by logical concern into submodules. The parent module re-exports the public API.

**Explicit exceptions:**
- Data tables, flag definitions, and constant registries where splitting would scatter related data
- Generated code (derive output, schema dumps) that is not human-maintained
- Test fixture files with many small test cases that share setup

### Function size

~50 lines per function guideline. Functions over 50 lines should be split unless:
- **Hot loops** where splitting would hurt cache locality or add function call overhead
- **Data-driven initialization** where the structure is the logic (e.g., a long match/switch mapping inputs to outputs)
- **Sequential pipelines** where each step is one line and extracting would add indirection without clarity

### One module per concern

Each `.rs` file handles one concern. If a file has `struct Foo`, `impl Foo`, and tests for `Foo`, that's one concern. If it also has `struct Bar` with its own impl and tests, split into `foo.rs` and `bar.rs`.

---

## Versioning

### Single workspace version

Multi-crate workspaces use one version in the root `Cargo.toml`. All crates inherit it. One version to bump, one changelog, no per-crate drift.

### Semantic versioning

Pre-1.0: any change can break the API. Post-1.0: breaking changes bump major. Adding variants to a `#[non_exhaustive]` enum is not breaking.

---

## Documentation

### Architecture doc required

Every multi-crate project has an `ARCHITECTURE.md` documenting:
- Crate dependency graph
- Layer boundaries
- Extension points (where to add new functionality)
- Key architectural decisions and their rationale

### Per-crate navigation

Each library crate has a `CLAUDE.md` (or equivalent) with:
- One-line purpose
- Key types and where to find them
- Common tasks ("to add X, modify Y")
- Dependencies (what this crate uses and what uses it)

---

## Compile-time architecture enforcement

Use `clippy.toml` with `disallowed-methods` and `disallowed-types` to enforce architectural boundaries at compile time.

Each crate can have its own `clippy.toml` banning patterns specific to its layer:

```toml
# crates/handlers/clippy.toml
disallowed-methods = [
  { path = "std::fs::read_to_string", reason = "use FileSystem trait" },
  { path = "reqwest::Client::new", reason = "use HttpClient wrapper" },
  { path = "std::process::exit", reason = "use graceful shutdown" },
]
```

This catches architecture violations at compile time. Stronger than code review.

## System abstraction traits

All system operations (filesystem, time, environment, networking) go through trait abstractions, not direct `std::` calls. This enables:
- Cross-platform testing without real filesystem
- Deterministic time in tests (no `sleep`, no wall clock)
- Mockable network for integration tests

Define traits in the lowest common crate. Implement concretely in the binary. Mock in tests.

## Cleanup registration

Register cleanup callbacks at setup time, not drop time. Drop order depends on field declaration order (fragile). Async cleanup in Drop is impossible.

Pattern: explicit callback list registered during initialization, executed in declared order during graceful shutdown.

---

## Trait boundaries

Traits are the primary mechanism for enforcing module boundaries, enabling extension, and controlling coupling. Use them deliberately. A trait with one implementation is overhead; a trait at the wrong layer is a dependency magnet.

### When to use a trait

Define a trait when there are (or will be) multiple implementations behind a single call site. Concrete patterns that justify traits:

- **Plugin registries.** A registry stores `Box<dyn T>` and iterates over heterogeneous implementations. Gate checks, reaction handlers, behavioral rules, and agent providers all follow this pattern. WHY: the registry must accept types it doesn't know about at compile time.
- **Dependency inversion across layers.** A lower crate defines the trait; a higher crate implements it. The lower crate never imports the higher crate. WHY: this is the only way to depend on behavior from a higher layer without creating a circular dependency.
- **System abstractions.** Filesystem, time, network, and process operations go through traits so tests can substitute deterministic implementations. WHY: real I/O in unit tests is slow, flaky, and non-reproducible.
- **Replaceability boundaries.** External SDKs and APIs get wrapped in traits so the codebase is not structurally coupled to a vendor. WHY: vendors change APIs, get deprecated, or need swapping under deadline pressure.

### When NOT to use a trait

- **Single implementation with no planned second.** Use a concrete struct. If a second implementation materializes later, extract the trait then. Premature abstraction costs more than the refactor.
- **Internal module boundaries.** Modules within a crate communicate through concrete types, not traits. WHY: intra-crate coupling is expected. Traits add indirection without isolation benefit when both sides compile together.
- **Configuration differences.** Two behaviors that differ only in config values are one struct with different field values, not two trait implementations.

### Trait object (`dyn Trait`) vs generics (`T: Trait`)

Choose based on whether the concrete type is known at the call site.

**Use trait objects (`dyn Trait`) when:**

- A collection holds heterogeneous implementations. `Vec<Box<dyn GateCheck>>`, `HashMap<String, Arc<dyn AgentProvider>>`. WHY: generics require homogeneous collections; trait objects allow mixed types.
- The caller does not and should not know the concrete type. Plugin registries, handler dispatch loops, and provider resolution all operate on trait objects. WHY: the whole point is runtime polymorphism.
- The trait crosses an async boundary and the future is not `Send`. Return `Pin<Box<dyn Future<Output = T> + 'a>>` explicitly. WHY: native `async fn` in traits is dyn-incompatible when the returned future must be stored or dispatched.

**Use generics (`T: Trait`) when:**

- The concrete type is known at the call site and there's one type per instantiation. `fn print_progress<W: Write>(w: &mut W, ...)`. WHY: monomorphization eliminates vtable overhead and enables inlining.
- The function is called in a hot path. WHY: dynamic dispatch has a per-call cost (vtable load + indirect branch) that matters at scale.
- The caller benefits from knowing the concrete type (e.g., for method chaining or associated types). WHY: trait objects erase associated types and const generics.

**Use `impl Trait` in argument position for single-use generics.** `fn new(name: impl Into<String>)` instead of `fn new<S: Into<String>>(name: S)` when the generic parameter appears only once and is not needed in the return type. WHY: less visual noise, same monomorphization.

### Dyn-compatible trait design

Traits intended for use as trait objects must be dyn-compatible (formerly "object safe"). Rules:

- No `Self` in return position. Use associated types or boxed returns instead.
- No generic methods. Each method must have a fixed signature. WHY: vtables cannot represent an unbounded set of monomorphized methods.
- Async methods need manual boxing. Return `Pin<Box<dyn Future<Output = T> + 'a>>` instead of `async fn`. Define a type alias: `type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;`. WHY: the compiler cannot determine the future's size for dyn dispatch.
- Require `Send + Sync` supertrait bounds when trait objects will be stored in `Arc` or passed across thread boundaries. Declare on the trait itself: `pub trait AgentProvider: Send + Sync`. WHY: adding bounds later is a breaking change for all implementations.

### Blanket implementations

Blanket impls (`impl<T: X> Y for T`) provide automatic capability based on existing trait bounds. Use them sparingly.

**Appropriate uses:**

- Bridge traits. `impl<T: Display> Log for T` where `Log` is a project trait that wraps formatting for the logging system. WHY: any displayable type automatically gains logging capability without per-type boilerplate.
- Newtype delegation. `impl<T: AsRef<str>> PartialEq<T> for SessionId` to allow comparison with any string-like type. WHY: ergonomic API without manual impls for every string type.
- Marker-based capability. `impl<T: Send + Sync + 'static> Registerable for T` for plugin registration systems. WHY: any thread-safe type can be registered; the constraint is structural, not behavioral.

**Avoid blanket impls when:**

- The impl adds behavior beyond what the source trait guarantees. A blanket `impl<T: Read> Parse for T` that assumes specific data formats is a semantic mismatch.
- The impl would conflict with future specialization needs. Blanket impls block downstream code from providing more specific implementations. WHY: Rust's coherence rules prevent overlapping impls, so a blanket impl permanently occupies the design space.
- The impl would surprise users. If `T: Display` automatically gets a blanket impl for `Serialize`, code that derives `Display` unexpectedly becomes serializable. WHY: implicit capability is invisible in the type signature.

Use `#[diagnostic::do_not_recommend]` on blanket impls that produce confusing compiler suggestions. Use `#[diagnostic::on_unimplemented]` on the trait itself to guide users when no impl exists.

### Sealed traits

Seal a trait when external code should not implement it but should use it. Pattern:

```rust
mod sealed { pub trait Sealed {} }

pub trait InternalPhase: sealed::Sealed {
    fn advance(&self);
}
```

Implement `Sealed` only for types within the crate. External code can call methods on `dyn InternalPhase` but cannot add new implementations. WHY: this preserves the crate's freedom to add required methods, change signatures, or add supertraits without breaking downstream.

### Trait placement: consumer defines, provider implements

The crate that **calls through** a trait defines it. The crate that **provides** the concrete behavior implements it. This is the Dependency Inversion Principle applied to crate boundaries: high-level modules define abstractions, low-level modules conform to them.

**Core rule:** define the trait in the lowest crate that needs to call through it. Implement it in the highest crate that has the concrete type.

```
phronesis (defines GateCheck trait — the gate orchestrator calls through it)
  ↑ implements
kanon-cli (implements ScanCheck, LintCheck, AuditCheck — domain-specific checks)
  ↑ wires
main.rs (composition root — registers impls with the orchestrator)
```

The provider crate (`kanon-cli`) depends on the consumer crate (`phronesis`) to import the trait definition. The consumer never imports the provider. This inverts the naive dependency direction where the implementation would define the interface.

**Correct — consumer defines the contract:**

```rust
// phronesis/src/gate/mod.rs — the orchestrator defines what a gate check must do
pub trait GateCheck: Send + Sync {
    fn name(&self) -> &str;
    fn run(&self, ctx: &GateContext) -> Result<CheckResult>;
}

// kanon-cli/src/commands/gate/checks.rs — the application provides implementations
use phronesis::gate::GateCheck;

pub struct LintCheck { /* ... */ }
impl GateCheck for LintCheck {
    fn name(&self) -> &str { "lint" }
    fn run(&self, ctx: &GateContext) -> Result<CheckResult> { /* ... */ }
}
```

**Incorrect — provider defines the contract:**

```rust
// storage/src/lib.rs — the storage layer defines its own interface
pub trait StorageBackend {
    fn read(&self, key: &str) -> Result<Vec<u8>>;
    fn write(&self, key: &str, data: &[u8]) -> Result<()>;
}

pub struct SqliteBackend { /* ... */ }
impl StorageBackend for SqliteBackend { /* ... */ }

// domain/src/lib.rs — business logic must now depend on the storage crate
use storage::StorageBackend;  // wrong direction: domain depends on infrastructure
```

The fix: move `StorageBackend` into the domain crate (or a shared leaf crate). The storage crate depends on domain for the trait definition and provides the `SqliteBackend` implementation.

**Never define a trait in the same crate as its only implementation** unless the trait is part of the public API contract (for downstream consumers or test mocking). WHY: a trait with one impl in the same crate adds indirection with no isolation benefit.

### Exception: widely-shared traits

Traits consumed by many crates across the dependency graph belong in a shared leaf crate rather than in any single consumer. This avoids forcing unrelated crates to depend on each other just to share a trait definition.

Examples:
- Standard library traits (`std::fmt::Display`, `std::error::Error`) — universally shared
- Project-wide identity traits (e.g., `koina::Id`) — used across all domain crates
- Cross-cutting behavioral traits (e.g., `kanon::BehavioralRule`) — consumed by multiple crates that each provide rules

The shared crate sits at the leaf layer. It contains trait definitions, associated types, and error types. It contains no implementations beyond trivial defaults. WHY: implementations pull in dependencies; trait crates must remain lightweight.

```
koina (shared leaf — trait definitions, identity types)
  ↑ depends on
phronesis (uses traits from koina, defines its own consumer traits)
  ↑ depends on
kanon-cli (implements traits from both koina and phronesis)
```

### Composition root

The composition root is where trait definitions meet their implementations. It lives in the binary crate — the top of the dependency graph — because only the binary has visibility into all crates.

**Responsibilities of the composition root:**
- Construct concrete types that implement traits
- Register implementations with registries, routers, or orchestrators
- Wire configuration into concrete types
- Own the lifetime of shared resources (`Arc`, connection pools)

**Pattern:**

```rust
// main.rs or a dedicated wiring module in the binary crate
fn build_gate(config: &Config) -> Gate {
    let mut gate = Gate::new();
    gate.register(Box::new(FmtCheck::new()));
    gate.register(Box::new(ClippyCheck::new()));
    gate.register(Box::new(LintCheck::new(config.lint_config())));
    gate.register(Box::new(TestCheck::new(config.test_config())));
    gate
}
```

**Rules:**
- Library crates never construct their own trait implementations for dependency injection. They accept `Box<dyn Trait>` or `impl Trait` from the caller.
- The composition root is the only place where concrete types and trait definitions are both in scope. This is by design — it forces the binary to be the integration point.
- If a library crate needs a default implementation for convenience, provide a `::default()` constructor that the composition root can call. Do not auto-wire inside the library.

---

## Scaling patterns (100K+ lOC)

### Flat module layout

Prefer `src/{a,b,c}.rs` over `src/{a/{x,y}}`. Flat structure:
- search (grep finds everything at one depth)
- Clear ownership (one file = one concern)
- to move modules between crates later
- Prevents "mega-modules" that hide complexity

When a module exceeds 800 lines, split into sibling files, not nested directories.

### Glossary

Every multi-crate project maintains a glossary defining project-specific terms. Greek names, domain concepts, runtime abstractions, pipeline stages: all defined in one document. Prevents contributors from using terms inconsistently.

### Test-support feature gates

Mock providers, test fixtures, and helper functions go behind `feature = "test-support"`. Production binary doesn't compile test infrastructure. Test features cascade: `editor/test-support` depends on `text/test-support`.

### Smart CI filtering

Map changed files to changed crates. Run tests for changed crates plus their reverse dependencies. Full suite runs on main/release branches. PR context runs only affected tests.

```
cargo nextest --filter-expr 'rdeps(changed_crates)'
```

### Compile time budget

At 100K+ LOC, compile time matters. Budget strategies:
- `codegen-units = 16` for dev (parallel compilation)
- `codegen-units = 1` for release (better output)
- Proc-macro crates: `opt-level = 3` (they run at compile time)
- Incremental compilation for dev builds
- sccache or similar for CI caching across platforms
