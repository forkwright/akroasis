# Rust

> Additive to STANDARDS.md. Read that first. Everything here is Rust-specific.
>
> **Key decisions:** 2024 edition, snafu errors, tokio async, tracing logging, jiff time, cancel-safe select, pub(crate) default, cargo-deny, zero tolerance for silent failures.

---

## Toolchain

- **Edition:** 2024 (Rust 1.85+)
- **MSRV:** Set explicitly in `Cargo.toml`. The MSRV-aware resolver (default since 1.84) respects it during dependency resolution.
- **Async runtime:** Tokio
- **Build/test cycle:**
  ```bash
  cargo test -p <crate>                                    # targeted tests during development
  cargo clippy --workspace --all-targets -- -D warnings    # lint + type-check full workspace
  cargo test --workspace                                   # full suite as final gate before PR
 ```
- **Formatting:** `cargo fmt` with default rustfmt config, no overrides
- **Audit:** `cargo-deny` for licenses, advisories, bans, and sources (see Dependencies)

### Build profiles

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 2

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

Dev profile: optimize dependencies (level 2) but keep local code fast to compile (level 1). Release profile: thin LTO for link-time optimization without full-LTO compile cost. Single codegen unit for maximum optimization. Strip symbols for smaller binary.

### CI tools

Required in CI pipelines:

| Tool | Purpose |
|------|---------|
| `cargo-deny` | License, advisory, ban, source checks |
| `cargo-udeps` | Detect unused declared dependencies |
| `cargo-semver-checks` | Detect accidental breaking changes to public API |
| `cargo-fuzz` | Fuzz testing for parser and input-handling code |

Track binary size per release. A 10%+ increase without a feature justification is a regression.

---

## File structure

Rust files follow a consistent vertical layout. `cargo fmt` handles horizontal formatting. Vertical structure is manual.

### Import ordering

```rust
// 1. std
use std::collections::HashMap;
use std::sync::Arc;

// 2. External crates
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tokio::sync::RwLock;

// 3. Workspace crates
use aletheia_koina::id::NousId;
use aletheia_taxis::config::AppConfig;

// 4. Local modules
use crate::error::{Error, Result};
use crate::pipeline::PipelineMessage;
```

One blank line between each group. Alphabetical within groups. `cargo fmt` handles the rest.

### File section order

See STANDARDS.md § Vertical Structure for the universal ordering. Rust-specific: inherent impl blocks before trait impls, `#[cfg(test)] mod tests` last. Two blank lines between sections, one blank line between items within a section.

### Impl block order

```rust
impl SessionStore {
    // Constructors
    pub fn new(...) -> Self { ... }
    pub fn open(...) -> Result<Self> { ... }

    // Public methods (in order of typical call flow)
    pub fn create_session(...) -> Result<Session> { ... }
    pub fn get_session(...) -> Result<Option<Session>> { ... }
    pub fn list_sessions(...) -> Result<Vec<Session>> { ... }

    // Private helpers
    fn validate_key(&self, key: &str) -> Result<()> { ... }
}
```

Constructors, then public API in call-flow order, then private helpers. One blank line between each method.

---

## Naming

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `snake_case.rs` | `session_store.rs` |
| Crate names | `kebab-case` (Cargo) / `snake_case` (code) | `aletheia-mneme` / `aletheia_mneme` |
| Feature flags | `kebab-case` | `full-text-search` |

- `into_` for ownership-consuming conversions, `as_` for cheap borrows, `to_` for expensive conversions.
- No magic numbers. Named constants for every numeric literal except 0, 1, and array/tuple indices.

---

## Type system

### Newtypes for domain concepts

Domain IDs are newtype wrappers, not bare `String` or `u64`. Zero-cost, compile-time parameter swap safety.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(compact_str::CompactString);

impl SessionId {
    pub fn new(id: impl Into<compact_str::CompactString>) -> Self {
        Self(id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str { &self.0 }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self { Self::new(s) }
}
```

Every newtype must implement: `Display`, `AsRef<str>` (for string types), `From` conversions for natural input types.

### `#[non_exhaustive]` on public types

All public enums and public structs with named fields that may grow must use `#[non_exhaustive]`. This preserves backward compatibility: adding a variant or field is not a breaking change.

### `#[must_use]` everywhere it matters

`#[must_use]` on:
- All public functions that return `Result`
- All builder methods returning `Self`
- All pure functions (no side effects, return value is the point)
- All `Iterator` adapters and combinators

Silently dropped results are bugs. The compiler should catch them.

### `Default` on config types

All structs ending in `Config`, `Settings`, or `Options` must derive or implement `Default`. The default documents what a zero-configuration value looks like.

### `#[expect(lint)]` over `#[allow(lint)]`

`#[expect]` warns you when the suppression is no longer needed. `#[allow]` silently persists forever. Every suppression must include a `reason`:

```rust
#[expect(clippy::too_many_lines, reason = "pipeline stages are sequential, splitting adds indirection")]
```

### Typestate pattern

Use typestate for multi-step builders and connection lifecycles. Compile-time state validation over runtime checks.

```rust
struct Connection<S: State> { /* ... */ _state: PhantomData<S> }
struct Disconnected;
struct Connected;

impl Connection<Disconnected> {
    fn connect(self) -> Result<Connection<Connected>, Error> { /* ... */ }
}
impl Connection<Connected> {
    fn query(&self, sql: &str) -> Result<Rows, Error> { /* ... */ }
}
// Connection<Disconnected>::query() won't compile
```

### Exhaustive matching

Use `match` with explicit variants over wildcard `_` arms when the enum is under your control. Wildcards hide new variants.

### Standard library types (2024 edition)

```rust
use std::sync::LazyLock;
static CONFIG: LazyLock<Config> = LazyLock::new(|| load_config());
// NOT: lazy_static, once_cell
```

Native `async fn` in traits (stable since 1.75). No `async-trait` crate.

Async closures (`async || { ... }`) with `AsyncFn`/`AsyncFnMut`/`AsyncFnOnce` traits (stable since 1.85). Unlike `|| async {}`, async closures allow the returned future to borrow from captures.

Let chains in `if let` expressions (2024 edition, stable since 1.88):

```rust
if let Some(session) = sessions.get(id)
    && let Some(turn) = session.last_turn()
    && turn.is_complete()
{
    process(turn);
}
```

### 2024 edition specifics

**`unsafe_op_in_unsafe_fn`:** Warns by default. Unsafe operations inside `unsafe fn` bodies must be wrapped in explicit `unsafe {}` blocks. Narrow the scope instead of treating the entire function body as unsafe.

**RPIT lifetime capture:** Return-position `impl Trait` automatically captures all in-scope type and lifetime parameters. Use `use<..>` for precise capturing when needed:

```rust
fn process<'a>(&'a self) -> impl Iterator<Item = &str> + use<'a, Self> {
    self.items.iter().map(|i| i.as_str())
}
```

**Trait upcasting:** `&dyn SubTrait` coerces to `&dyn SuperTrait` (stable since 1.86). No more manual `as_super()` methods.

### Diagnostic attributes

```rust
#[diagnostic::on_unimplemented(message = "cannot store {Self}: implement StorageCodec")]
pub trait StorageCodec { /* ... */ }

#[diagnostic::do_not_recommend]
impl<T: Display> StorageCodec for T { /* ... */ }
```

Use `#[diagnostic::on_unimplemented]` for domain-specific trait error messages. Use `#[diagnostic::do_not_recommend]` to suppress unhelpful blanket-impl suggestions.

---

## Validation constructors

Implements STANDARDS.md § Parse, Don't Validate for Rust. Invalid data must not survive construction. Every domain type enforces its invariants at the boundary — deserialization, config loading, CLI parsing — so downstream code never checks validity again.

### Newtype wrappers with `TryFrom`

Newtypes that accept arbitrary input must validate through `TryFrom`, not `new`. A public `new` that accepts any `&str` defeats the point of the wrapper — callers can construct invalid instances.

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ProjectSlug(compact_str::CompactString);
// WHY: TryFrom is the validation boundary. Once constructed, the slug is
// known-valid. No runtime checks needed downstream.

impl TryFrom<&str> for ProjectSlug {
    type Error = ValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        ensure!(
            !value.is_empty(),
            EmptyFieldSnafu { field: "project_slug" }
        );
        ensure!(
            value.len() <= 64,
            FieldTooLongSnafu { field: "project_slug", max: 64, actual: value.len() }
        );
        ensure!(
            value.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            InvalidFormatSnafu {
                field: "project_slug",
                expected: "lowercase ASCII with hyphens",
                actual: value.to_string(),
            }
        );
        Ok(Self(value.into()))
    }
}

impl ProjectSlug {
    /// Access the validated slug as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

Do not implement `From<&str>` on types that require validation. `From` is infallible — it tells callers the conversion cannot fail, which is a lie when invariants exist. Use `TryFrom` exclusively.

### Serde validation

All public types that derive `Deserialize` and have invariants must validate on deserialize, not after. Derive-based `Deserialize` bypasses constructors — serde writes directly to struct fields, skipping any `TryFrom` or `new` logic. Every type with invariants must route deserialization through the validation path. A `#[derive(Deserialize)]` on a type with a `new()` or `try_new()` constructor is a bug: those constructors exist because the type has invariants, and serde bypasses them.

```rust
// WHY: #[serde(try_from)] delegates deserialization to TryFrom, so the
// validation constructor is the single entry point for both code and data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(try_from = "&str")]
pub struct EmailAddress(compact_str::CompactString);

impl TryFrom<&str> for EmailAddress {
    type Error = ValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        ensure!(
            value.contains('@') && value.len() <= 254,
            InvalidFormatSnafu {
                field: "email",
                expected: "valid email address",
                actual: value.to_string(),
            }
        );
        Ok(Self(value.into()))
    }
}

impl<'de> Deserialize<'de> for EmailAddress {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        Self::try_from(s).map_err(serde::de::Error::custom)
    }
}
```

For struct fields that need validation but aren't newtypes, use `deserialize_with`:

```rust
#[derive(Debug, Deserialize)]
pub struct RateLimitConfig {
    #[serde(deserialize_with = "deserialize_positive_nonzero")]
    pub requests_per_second: u32,

    #[serde(deserialize_with = "deserialize_duration_secs")]
    pub window: Duration,
}

// WHY: Serde happily deserializes 0 into u32. A rate limit of 0 is
// nonsensical and would cause division-by-zero downstream. Catch it here.
fn deserialize_positive_nonzero<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u32::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom(
            "value must be greater than zero",
        ));
    }
    Ok(value)
}
```

Three patterns, in order of preference:
1. **`#[serde(try_from = "T")]`** on the type — best for newtypes where `TryFrom` is already the constructor
2. **Manual `Deserialize` impl** — when the intermediate representation differs from what `try_from` accepts
3. **`#[serde(deserialize_with = "fn")]`** on the field — for validating fields within a larger struct

Never rely on bare `#[derive(Deserialize)]` for types with invariants. If a struct has a "must be positive" or "must match pattern" field, the derive is wrong by default.

#### `deny_unknown_fields` for config types

Config types loaded from files, environment, or user input must reject unrecognized fields. A typo in a config key (`timout` instead of `timeout`) silently becomes a default value. `deny_unknown_fields` turns that into a hard error at load time.

```rust
// WHY: Config files are the #1 source of "it worked on my machine" bugs.
// A misspelled key silently uses the default, which may be zero, empty, or
// wrong. deny_unknown_fields catches typos at parse time.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub jitter: bool,
}
```

Apply `deny_unknown_fields` to all types that represent configuration, settings, or options loaded from external sources. Do not apply it to API request/response types or wire-format types where forward compatibility matters.

#### Complex struct validation via `TryFrom`

When a struct has cross-field invariants, deserialize into a raw intermediate type and validate during conversion. This keeps the validation logic in one place and prevents invalid state from ever existing.

```rust
// WHY: ScheduleConfig has a cross-field invariant: if retry is enabled,
// retry_delay must be set. Bare derive would allow { retry: true, retry_delay: None }.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleConfig {
    pub cron: String,
    pub retry: bool,
    pub retry_delay: Duration,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScheduleConfig {
    cron: String,
    retry: Option<bool>,
    retry_delay_secs: Option<u64>,
}

impl TryFrom<RawScheduleConfig> for ScheduleConfig {
    type Error = ValidationError;

    fn try_from(raw: RawScheduleConfig) -> Result<Self, Self::Error> {
        let retry = raw.retry.unwrap_or(false);
        let retry_delay = match (retry, raw.retry_delay_secs) {
            (true, None) => return Err(CrossFieldSnafu {
                rule: "retry = true requires retry_delay_secs",
            }.build()),
            (true, Some(secs)) => Duration::from_secs(secs),
            (false, _) => Duration::ZERO,
        };
        Ok(Self { cron: raw.cron, retry, retry_delay })
    }
}

impl<'de> Deserialize<'de> for ScheduleConfig {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = RawScheduleConfig::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}
```

#### Exception: pure data transfer types

Types that are pure data containers with no invariants — every combination of field values is valid — may use plain `#[derive(Deserialize)]`. These are typically:

- Wire-format types mapping 1:1 to an external API response
- Event payloads where all fields are informational
- Intermediate representations used only as input to a validation step

Mark these types explicitly so the intent is clear and the lint rule does not flag them:

```rust
// WHY: Pure data transfer type — no invariants. All field combinations are
// valid. This struct maps directly to the GitHub API response shape.
#[derive(Debug, Deserialize)]
pub struct GitHubPullRequest {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub draft: bool,
}
```

The key test: if the type has a `new()`, `try_new()`, or `build()` constructor, it has invariants and must not use bare `#[derive(Deserialize)]`. If every field combination is valid and there is no constructor, plain derive is correct.

### Builder pattern for complex configs

Use builders when construction requires multiple validated fields with cross-field constraints. A flat constructor with 5+ parameters is unreadable, and field order becomes a bug vector.

```rust
/// WHY: DispatchConfig has cross-field invariants (max_retries requires
/// retry_delay, budget_limit must exceed single-request cost). A builder
/// validates these at build() time, not scattered across call sites.
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    provider: ProviderKind,
    model: ModelId,
    max_retries: u32,
    retry_delay: Duration,
    budget_limit: BudgetLimit,
}

#[derive(Debug, Default)]
pub struct DispatchConfigBuilder {
    provider: Option<ProviderKind>,
    model: Option<ModelId>,
    max_retries: Option<u32>,
    retry_delay: Option<Duration>,
    budget_limit: Option<BudgetLimit>,
}

impl DispatchConfigBuilder {
    #[must_use]
    pub fn provider(mut self, provider: ProviderKind) -> Self {
        self.provider = Some(provider);
        self
    }

    #[must_use]
    pub fn model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    #[must_use]
    pub fn max_retries(mut self, n: u32, delay: Duration) -> Self {
        // WHY: retries without a delay is a tight loop that burns budget.
        // Requiring both together makes the invalid state unrepresentable.
        self.max_retries = Some(n);
        self.retry_delay = Some(delay);
        self
    }

    #[must_use]
    pub fn budget_limit(mut self, limit: BudgetLimit) -> Self {
        self.budget_limit = Some(limit);
        self
    }

    /// Consume the builder and produce a validated config.
    ///
    /// # Errors
    ///
    /// Returns `ValidationError` if required fields are missing or
    /// cross-field invariants are violated.
    pub fn build(self) -> Result<DispatchConfig, ValidationError> {
        let provider = self.provider.context(MissingFieldSnafu { field: "provider" })?;
        let model = self.model.context(MissingFieldSnafu { field: "model" })?;
        let max_retries = self.max_retries.unwrap_or(0);
        let retry_delay = self.retry_delay.unwrap_or(Duration::ZERO);
        let budget_limit = self.budget_limit.context(MissingFieldSnafu { field: "budget_limit" })?;

        // Cross-field validation
        ensure!(
            max_retries == 0 || retry_delay > Duration::ZERO,
            CrossFieldSnafu {
                rule: "max_retries > 0 requires retry_delay > 0",
            }
        );

        Ok(DispatchConfig { provider, model, max_retries, retry_delay, budget_limit })
    }
}

impl DispatchConfig {
    #[must_use]
    pub fn builder() -> DispatchConfigBuilder {
        DispatchConfigBuilder::default()
    }
}
```

Rules for builders:
- `#[must_use]` on every setter method and on `builder()`. A dropped builder is always a bug.
- `build()` returns `Result`, never panics. Validation failures are expected, not exceptional.
- Required fields are `Option` in the builder, validated at `build()` time. Not `Default::default()` with silent wrong values.
- Cross-field invariants live in `build()`, not in individual setters. Setters validate single fields; `build()` validates relationships.
- Implement `Deserialize` on the config type via the builder, so serde routes through the same validation.

For simple configs (2-3 fields, no cross-field constraints), `TryFrom` on a raw deserialization struct is sufficient. Use builders when the construction logic justifies the ceremony.

### Error types for validation failures

Validation errors must be structured, not stringly-typed. Callers need to match on failure kinds programmatically — for user-facing messages, retry logic, or aggregating multiple failures.

```rust
// WHY: A single validation pass may produce multiple errors (e.g., a config
// file with 3 bad fields). Collecting all of them lets the user fix
// everything in one pass instead of playing whack-a-mole.
#[derive(Debug, Snafu)]
pub enum ValidationError {
    #[snafu(display("field '{field}' is required"))]
    MissingField {
        field: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("field '{field}' is empty"))]
    EmptyField {
        field: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("field '{field}' exceeds maximum length {max} (got {actual})"))]
    FieldTooLong {
        field: &'static str,
        max: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("field '{field}': expected {expected}, got '{actual}'"))]
    InvalidFormat {
        field: &'static str,
        expected: &'static str,
        actual: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("cross-field constraint violated: {rule}"))]
    CrossField {
        rule: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("{count} validation errors"))]
    Multiple {
        count: usize,
        errors: Vec<ValidationError>,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
```

Rules for validation errors:
- One `ValidationError` enum per crate (or per domain boundary). Not per-type — validation errors share structure.
- Every variant carries the field name. "Validation failed" with no context is useless.
- Include expected vs. actual values in format errors. The user shouldn't have to guess what's wrong.
- Support a `Multiple` variant for batch validation. Fail-fast is the default; batch collection is opt-in for user-facing boundaries (config loading, HTTP request parsing).
- Implement `Display` with full context. The error message alone, without a stack trace, must be sufficient to diagnose the problem.
- Follow the crate's snafu conventions: `#[snafu(implicit)] location` on every variant, `source` field only when wrapping another error.

### What not to do

- `String` as error type from validation: callers cannot match, test, or recover programmatically
- `new()` that silently clamps or truncates invalid input: the caller doesn't know their input was altered
- `Default` values substituted for missing required fields: masks configuration errors until production
- `#[derive(Deserialize)]` on a type with invariants without `try_from` or `deserialize_with`: serde bypasses your constructor
- `unwrap()` on `TryFrom` in deserialization: converts a recoverable validation failure into a panic
- `validate()` method called after construction: the invalid state exists between `new()` and `validate()` — any code path that forgets to call `validate()` has a bug

---

## Safety and correctness

### No silent truncation

Never use `as` for numeric conversions. `as` silently truncates, wraps, or rounds. Use `try_from`/`try_into` with error handling, or `From`/`Into` when the conversion is infallible.

```rust
// Wrong: silently truncates on overflow
let small: u16 = big_number as u16;

// Right: explicit fallibility
let small: u16 = u16::try_from(big_number).context(OverflowSnafu)?;
```

### No indexing in library code

Array and string indexing panics on out-of-bounds or non-UTF8 boundaries. Use `.get()` and handle the `None` case.

```rust
// Wrong: panics if empty
let first = items[0];
let prefix = name[..3];

// Right: returns None
let first = items.first();
let prefix = name.get(..3);
```

Exception: tuple and fixed-size array access where the index is a compile-time constant and the size is known.

### Assert messages

Every `assert!`, `assert_eq!`, `assert_ne!` must include a message describing the invariant. Bare assertions produce unhelpful panic messages.

```rust
// Wrong
assert!(count > 0);

// Right
assert!(count > 0, "turn count must be positive after initialization");
```

### Debug assertions for expensive invariants

Use `debug_assert!` for invariant checks that are too expensive for production. These run in debug/test builds but compile to nothing in release.

```rust
debug_assert!(
    items.windows(2).all(|w| w[0].timestamp <= w[1].timestamp),
    "history must be sorted by timestamp"
);
```

### Secrets and sensitive data

Fields containing tokens, keys, passwords, or secrets:
- Use `secrecy::SecretString` (not plain `String`). Zeroized on drop, no accidental Display.
- Implement `Debug` manually with redaction: `[REDACTED]`
- Never log secret values, even at trace level.
- Use `subtle::ConstantTimeEq` for secret comparison (prevents timing attacks).

### Structured panic handler

The binary should install a custom panic handler that:
1. Logs the panic to the structured log file (not just stderr)
2. Includes backtrace
3. Then aborts (don't unwind past the handler)

```rust
std::panic::set_hook(Box::new(|info| {
    tracing::error!(panic = %info, "process panicked");
}));
```

Set `RUST_BACKTRACE=1` in systemd units for crash diagnostics.

---

## Error handling

See STANDARDS.md § Error Handling for universal principles.

**snafu** (not thiserror) for all library crate error enums. GreptimeDB pattern.

- Per-crate error enums with `.context()` propagation and `Location` tracking
- No `unwrap()` in library code. `anyhow` only in CLI entry points (`main.rs`).
- Convention: `source` field = internal error (walk the chain), `error` field = external (stop walking)
- `expect("invariant description")` over bare `unwrap()`. The message documents the invariant.

```rust
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(display("failed to read config from {path}"))]
    ReadConfig {
        path: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    #[snafu(display("failed to parse config"))]
    ParseConfig {
        source: toml::de::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path)
        .context(ReadConfigSnafu { path: path.display().to_string() })?;
    let config: Config = toml::from_str(&contents)
        .context(ParseConfigSnafu)?;
    Ok(config)
}
```

What not to do:
- `unwrap()` in library code
- `anyhow` in library crates (callers can't match variants)
- Bare `?` without `.context()` (loses information)
- `Box<dyn Error>` (erases type info)

---

## Documentation

### Enforce at compile time

Library crates must deny missing docs:

```rust
// lib.rs
#![deny(missing_docs)]
```

This forces every public item (struct, enum, trait, function, module) to have a doc comment. Binary crates and internal modules are exempt. The compiler catches gaps that reviews miss.

### Required doc sections

All public fallible functions must document failure conditions:

```rust
/// Load configuration from a TOML file.
///
/// # Errors
///
/// Returns `ReadConfig` if the file cannot be read.
/// Returns `ParseConfig` if the TOML is malformed.
pub fn load_config(path: &Path) -> Result<Config, ConfigError> { /* ... */ }
```

All functions that can panic (even theoretically) must document it:

```rust
/// # Panics
///
/// Panics if `capacity` is zero.
pub fn with_capacity(capacity: usize) -> Self { /* ... */ }
```

### Doc examples

Public API items crossing crate boundaries should have compilable `# Examples` sections. These are tested by `cargo test --doc`.

### Intra-Doc links

Use intra-doc links for cross-references. They're verified by rustdoc and clickable.

```rust
/// See [`SessionStore::create_session`] for session creation.
/// Uses the [`RecallEngine`] for memory retrieval.
```

### Compile-Fail tests

For type-safety guarantees, add `compile_fail` doc tests:

```rust
/// ```compile_fail
/// // SessionId and NousId are distinct types
/// let session: SessionId = NousId::new("test");
/// ```
```

---

## Async & concurrency

### Cancellation safety

Document cancellation safety for every public async method. In `select!`:

| Cancel-safe | Cancel-unsafe |
|-------------|---------------|
| `sleep()`, `Receiver::recv()` | `Sender::send(msg)` (message lost) |
| `Sender::reserve()` | `write_all()` (partial write) |
| Reads into owned buffers | Mutex guard held across `.await` |

All `select!` branches must be cancel-safe. Use the reserve-then-send pattern:

```rust
// Cancel-safe: reserve first, then send
let permit = tx.reserve().await?;
permit.send(message);

// Process outside select so cancellation doesn't lose work
let job = select! {
    Some(job) = rx.recv() => job,
    _ = cancel.cancelled() => break,
};
process(job).await;
```

### Biased select

Use `biased;` in `select!` when polling order matters. Cancellation/shutdown branches first, then work channels:

```rust
loop {
    tokio::select! {
        biased;
        _ = shutdown.cancelled() => break,
        Some(job) = rx.recv() => process(job).await,
    }
}
```

Without `biased`, branch order is randomized. A high-volume stream placed first in biased mode will starve later branches. Put low-frequency/high-priority branches first.

### JoinSet for dynamic task management

`JoinSet` for variable numbers of spawned tasks. Tasks return in completion order. All aborted on drop.

```rust
let mut set = JoinSet::new();
for item in items {
    let ctx = ctx.clone();
    set.spawn(async move { ctx.process(item).await });
}
while let Some(result) = set.join_next().await {
    handle(result??);
}
```

Use `tokio::join!` only for a fixed, known-at-compile-time number of futures.

### Graceful shutdown

Use `CancellationToken` from `tokio_util` (not ad-hoc channels):

```rust
let token = CancellationToken::new();

// In spawned tasks
let child = token.child_token();
tokio::spawn(async move {
    loop {
        tokio::select! {
            biased;
            _ = child.cancelled() => break,
            msg = rx.recv() => { /* ... */ }
        }
    }
});

// On shutdown signal
token.cancel();
set.shutdown().await;
```

### Locks across await

Never hold `std::sync::Mutex` guards across `.await` points. Either scope the lock and drop before the await, or use `tokio::sync::Mutex`.

```rust
// Correct: scope the lock
let data = {
    let guard = state.lock().unwrap();
    guard.clone()
};
let result = process(data).await;
```

### Mutex selection

- `std::sync::Mutex` for short, non-async critical sections (faster, no overhead). Add a comment: `// WHY: lock held only during HashMap lookup, no await`
- `tokio::sync::Mutex` only when holding the lock across `.await` points

### Spawned tasks

Spawned tasks are `'static`. They outlive any reference. Move owned data in. Clone `Arc`s before spawn. Always propagate tracing spans.

```rust
let this = Arc::clone(&self);
let span = tracing::Span::current();
tokio::spawn(async move {
    this.handle_request().await
}.instrument(span));
```

Never:
- `tokio::spawn(async { self.handle().await })`: `&self` is not `'static`
- Bare `tokio::spawn` without `.instrument()`: loses trace context

### No nested runtimes

Never call `Runtime::block_on()` from within async context. Use `spawn_blocking` for sync-in-async.

### Deterministic time in tests

Use `tokio::time::pause()` for tests involving timeouts, delays, or scheduling. Never use `sleep` for synchronization in tests.

```rust
#[tokio::test]
async fn timeout_triggers_after_deadline() {
    tokio::time::pause();
    // time::advance() is instant, no actual waiting
    tokio::time::advance(Duration::from_secs(300)).await;
    assert!(budget.total_exceeded());
}
```

---

## Lifetime & borrowing

### No clone spam

The borrow checker is telling you the data flow is wrong. `.clone()` silences it without fixing the architecture. Restructure ownership.

```rust
// Wrong: clone to appease borrow checker
fn process(data: &mut Vec<String>) {
    let snapshot = data.clone();
    for item in &snapshot {
        data.push(item.to_uppercase());
    }
}

// Right: restructure to avoid overlapping borrows
fn process(data: &mut Vec<String>) {
    let uppercased: Vec<String> = data.iter().map(|s| s.to_uppercase()).collect();
    data.extend(uppercased);
}
```

### `Arc` vs `Rc`

`Rc` for single-threaded graphs and tree structures. `Arc` for anything that crosses a thread or `.await` boundary. Async contexts always need `Arc` because the executor may move futures between threads.

```rust
// Single-threaded tree: Rc is correct and cheaper
let node = Rc::new(TreeNode::new());

// Async context: Arc required (futures are Send)
let shared = Arc::clone(&state);
tokio::spawn(async move { shared.process().await });
```

If a type is stored in a struct that implements `Send`, its `Rc` fields won't compile. Don't "fix" this by removing `Send`. Switch to `Arc`.

### Own by default

Start with owned types. Only add lifetimes when profiling shows the allocation matters. Config structs own their strings. This is not permission to `.clone()` everywhere. If you're cloning to satisfy the borrow checker, restructure ownership (see No Clone Spam above).

### `Cow` for mixed owned/Borrowed

```rust
fn normalize_path(path: &str) -> Cow<'_, str> {
    if path.starts_with('/') {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(format!("/{path}"))
    }
}
```

### Arena over self-Referential structs

Never fight the borrow checker with `RefCell` or `unsafe` for graph structures. Use arena allocation with index-based references.

---

## Testing

See TESTING.md for all testing principles (naming, isolation, coverage, test data, property testing).

Rust-specific framework choices and conventions:

- `#[cfg(test)] mod tests` in the same file, `use super::*` at the top
- `#[should_panic(expected = "message")]` for panic-testing (not bare `#[should_panic]`)
- `#[tokio::test]` for async tests, `tokio::time::pause()` for deterministic time
- `proptest` / `bolero` for property-based testing
- `insta` for snapshot testing of serialization formats, error messages, and CLI output
- `tracing-test` for asserting that errors are actually logged
- Targeted tests during development (`cargo test -p <crate>`), full suite as final gate

---

## Dependencies

**Preferred:**
- `snafu` (errors), `tokio` (async), `tracing` (logging), `serde` (serialization)
- `jiff` (time), `ulid` (IDs), `compact_str` (small strings)
- `figment` (config), `rusqlite` (SQLite)
- `secrecy` (secret values), `subtle` (constant-time comparison)
- `std::sync::LazyLock` (lazy statics)
- `tokio_util::sync::CancellationToken` (shutdown coordination)

**Ban principles:** Reject crates that duplicate stdlib functionality (post-stabilization), have known soundness issues, or are abandoned. When evaluating an unlisted crate, apply these same criteria: if std now covers it, use std; if the crate has open soundness advisories, avoid it; if it's unmaintained with alternatives, switch.

**Banned (applications of above):**
- `thiserror`: replaced by `snafu` (richer context, location tracking)
- `async-trait`: native `async fn` in trait since Rust 1.75 (stdlib covers it)
- `lazy_static`, `once_cell`: `std::sync::LazyLock` stabilized in 1.80 (stdlib covers it)
- `serde_yml`: unsound `unsafe` (soundness issue). Use `serde_yaml` if YAML is needed.
- `failure`: abandoned since 2019 (unmaintained). Use `snafu`.

**Exceptions:**
- `chrono`: only when required by external APIs (e.g., `cron` crate). Prefer `jiff` for all direct time handling.

**Policy:**
- Use `0.x` ranges for stable pre-1.0 crates (e.g., `snafu = "0.8"`). Pin exact versions for experimental or rapidly changing crates, documented in comments.
- Lockfiles (`Cargo.lock`) always committed for binary crates.
- Wrap external APIs in traits for replaceability.
- Each new dependency must justify itself. If it's 10 lines, write it.
- Prefer std over external. `std::sync::Mutex` over `parking_lot::Mutex` unless benchmarks prove otherwise. `std::collections::HashMap` over `hashbrown` unless the hasher matters.
- Gate heavy dependencies behind features. ML (candle), GUI (dioxus), optional integrations (pcre2) should not compile unless requested. A minimal `cargo build` pulls only what the core needs.
- Count transitive dependencies before adding. Run `cargo tree -d` and report the transitive depth. A crate that adds 3 direct deps may pull 80 transitive. Flag if a new dependency increases the workspace's total transitive count by more than 20%. WHY: a single "convenient" crate can silently double compile times and attack surface.
- When alternatives exist, prefer the crate with fewer transitive dependencies. A crate that does the same thing with 5 transitive deps is better than one with 50, even if the API is slightly less ergonomic.
- Audit new deps: maintenance status, download count, last publish date, unsafe usage. Pre-1.0 crates with <1000 downloads/month are a supply chain risk.

### Feature flags

- Feature names use `kebab-case`
- Each feature has a comment explaining what it enables
- Default features include only what a standard deployment needs
- Optional heavy dependencies (ML inference, migration tools) behind feature gates
- CI tests the default feature set plus each optional feature independently
- CI smoke test: the default binary must start successfully (`binary --version` or `binary check-config`)

### cargo-deny

Every workspace must have a `deny.toml`. Minimum configuration:

```toml
[graph]
targets = []  # check all targets
all-features = true

[advisories]
vulnerability = "deny"
unmaintained = "warn"
yanked = "deny"

[licenses]
unlicensed = "deny"
allow = ["MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0"]

[bans]
multiple-versions = "warn"
deny = [
    { crate = "openssl-sys", wrappers = [] },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

### Dependency footprint audit

Supply chain weight compounds silently. A workspace that compiles 400 crates today compiles 600 next quarter unless someone actively resists. Auditing is not a one-time gate; it is a recurring discipline.

#### Pre-addition checklist

Before adding any new crate dependency:

1. **Measure baseline:** `cargo tree --workspace --depth 999 --prefix none | sort -u | wc -l`
2. **Add the dependency** and re-measure. If the transitive count increased by more than 20%, stop and evaluate alternatives.
3. **Run `cargo tree -d`** to check for new duplicate crate versions introduced by the addition.
4. **Compare alternatives:** If multiple crates solve the problem, prefer the one with fewer transitive dependencies. Run `cargo tree -i <crate>` for each candidate to see what it pulls.
5. **Document the choice:** If the selected crate has a high transitive count, add a comment above the dependency line explaining why it was chosen over lighter alternatives.

```bash
# Full pre-addition workflow
BEFORE=$(cargo tree --workspace --depth 999 --prefix none | sort -u | wc -l)
# ... add the dependency ...
AFTER=$(cargo tree --workspace --depth 999 --prefix none | sort -u | wc -l)
INCREASE=$(( (AFTER - BEFORE) * 100 / BEFORE ))
echo "Transitive count: $BEFORE -> $AFTER (+${INCREASE}%)"
cargo tree -d  # check for new duplicates
```

#### Tools

Run both `cargo-deny` and `cargo-audit` on every PR. They overlap but are not redundant.

| Tool | Primary role | WHY both |
|------|-------------|----------|
| `cargo-deny` | Licenses, bans, duplicate versions, source restrictions | Policy enforcement: catches banned crates, license violations, and registry drift |
| `cargo-audit` | RustSec advisory database lookup | Vulnerability focus: faster advisory DB updates, `cargo audit fix` can auto-patch lockfiles |

```bash
cargo deny check                 # full policy check (deny.toml)
cargo audit                      # RustSec advisories against Cargo.lock
cargo audit --deny warnings      # CI: treat warnings (unmaintained, unsound) as failures
```

`cargo-deny` catches what `cargo-audit` misses (licenses, bans, sources). `cargo-audit` catches what `cargo-deny` sometimes lags on (RustSec DB freshness, auto-fix suggestions). Running only one leaves a gap.

#### Dependency size budgets

Track two numbers per release: **crate count** and **binary size**.

```bash
# Total unique crates compiled (including transitive)
cargo tree --workspace --depth 999 --prefix none | sort -u | wc -l

# Duplicate crates (same crate, different versions)
cargo tree -d

# Binary size (release profile)
ls -lh target/release/<binary>
```

**Budget rules:**

- Set a crate count ceiling in CI. Start with the current count + 5% margin. WHY: without a ceiling, dependency creep is invisible until compile times double.
- A PR that increases crate count by more than 5 must include justification in the PR description. WHY: small additions feel harmless individually; the budget forces a conversation.
- Binary size increases above 10% without a corresponding feature addition are regressions (already in CI tools table above). Track per release in the changelog.
- Duplicate crate versions (`cargo tree -d`) should trend toward zero. Each duplicate means two copies compiled and linked. Resolve by aligning version ranges or patching upstream.

**Transitive depth thresholds per workspace member:**

Check per-crate transitive counts with `cargo tree -p <crate> --prefix none | sort -u | wc -l`.

| Crate type | Threshold | Action |
|------------|-----------|--------|
| Leaf library (no downstream dependents in workspace) | <50 transitive | Warning above threshold; investigate and justify or trim |
| Internal library (depended on by other workspace crates) | <100 transitive | Each transitive dep is inherited by all downstream consumers; weight multiplies |
| Application / binary crate | <200 transitive | Hard ceiling; increases above this require architectural review |

WHY: A leaf crate with 80 transitive deps is a smell — it likely pulls a framework when it needs a utility. An application crate above 200 is accumulating the entire ecosystem. These thresholds are enforced by the `MANIFEST/dep-count` basanos rule on `Cargo.lock` (workspace-level total) and by manual `cargo tree -p` review per member at release time.

#### Feature flag minimization

Every dependency should be added with `default-features = false` and only the features actually used enabled explicitly. WHY: default feature sets pull transitive dependencies you never call. A single `features = ["full"]` can double the dependency tree.

```toml
# Wrong: pulls every optional feature tokio offers
tokio = "1"

# Right: only what this crate actually uses
tokio = { version = "1", default-features = false, features = ["rt-multi-thread", "macros", "signal"] }
```

Audit feature flags during dependency review:

```bash
# Show features enabled for a specific crate and why
cargo tree -e features -i tokio

# Show all features enabled across the workspace
cargo tree -e features --workspace
```

When a dependency's default features include something heavy (TLS backends, compression, serialization formats), disable defaults and opt in. Document the choice in a comment above the dependency line. WHY: the next developer adding a feature flag needs to know what was deliberately excluded.

#### Audit frequency

| Trigger | Action |
|---------|--------|
| Every PR | `cargo deny check` + `cargo audit --deny warnings` in CI |
| Weekly (automated) | `cargo audit` with updated advisory DB. WHY: new advisories land between PRs; a crate safe on Monday may have a CVE by Friday. |
| Each new dependency | Full review: maintenance status, download count, last publish date, unsafe usage, transitive tree impact (already in Dependencies policy above) |
| Quarterly (manual) | Full footprint review: crate count trend, binary size trend, duplicate versions, stale `[patch]` entries, feature flag audit. WHY: drift is only visible at longer timescales. |

For the weekly check, use a scheduled CI job or cron:

```bash
cargo install cargo-audit --locked
cargo audit fetch                # update advisory DB
cargo audit --deny warnings      # fail on any known issue
```

The quarterly review is a manual pass. Check the crate count and binary size against the previous quarter. If either grew more than 10% without a proportional feature addition, investigate and trim. Remove unused dependencies (`cargo-udeps`), consolidate duplicates, and tighten feature flags.

---

## Lints

### Workspace-Level clippy configuration

```toml
[workspace.lints.clippy]
# Style
pedantic = { level = "warn", priority = -1 }

# Safety: zero tolerance
unsafe_code = "deny"
unwrap_used = "deny"
expect_used = "deny"
indexing_slicing = "warn"
as_conversions = "warn"
arithmetic_side_effects = "warn"
string_slice = "warn"

# Quality: zero tolerance
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
await_holding_lock = "deny"
missing_assert_message = "deny"
tests_outside_test_module = "deny"

# Quality: warnings (sometimes justified with #[expect])
explicit_into_iter_loop = "warn"
fallible_impl_from = "warn"
fn_params_excessive_bools = "warn"
implicit_clone = "warn"
large_enum_variant = "warn"
large_types_passed_by_value = "warn"
map_err_ignore = "warn"
match_wildcard_for_single_variants = "warn"
needless_for_each = "warn"
rc_mutex = "warn"
redundant_clone = "warn"
string_add = "warn"
trait_duplication_in_bounds = "warn"
trivially_copy_passable_by_ref = "warn"
unused_self = "warn"
inefficient_to_string = "warn"
```

All crates inherit via `[lints] workspace = true`.

---

## Logging

`tracing` with structured spans. `#[instrument]` on public functions.

- Spawned tasks **must** propagate spans (`.instrument(span)`)
- Never hold `span.enter()` guards across `.await` points
- Log at the handling site, not the origin site
- Structured fields over string interpolation: `tracing::info!(session_id = %id, "loaded")`
- Install a panic handler that logs to the structured log file before aborting

---

## Performance

Known patterns. Apply when relevant:

- **Prepared statements:** `rusqlite::CachedStatement` for repeated queries
- **Lazy deserialization:** `serde_json::value::RawValue` for fields not always accessed
- **Regex caching:** `LazyLock<RegexSet>`. Never compile regex in loops.
- **Arena allocation:** `bumpalo` for per-turn transient data, freed in bulk
- **Batched writes:** Group mutations into single transactions, don't commit per-operation
- **File watching:** `notify` crate for config/bootstrap files, cache and recompute on change
- **SSE broadcast:** Serialize once, write bytes to all clients. Don't serialize per-connection.
- **Large enum variants:** Box the large variant to keep the enum size small.

---

## Visibility

- `pub(crate)` by default
- `pub` only for cross-crate API surface
- Every `pub` item is a commitment. It's part of your contract with downstream crates.
- Re-exports in `lib.rs` define the crate's public API explicitly
- Seal traits that external code should not implement

---

## API design

- Accept `impl Into<String>` (flexible input), return concrete types (predictable output)
- All types used in async contexts must be `Send + Sync`
- Builder pattern for complex construction: `TypeBuilder::new().field(val).build()`
- Use `impl Trait` in argument position for single-use generics
- `Display` on every public type (not just errors). Useful for logging and debugging.
- `From`/`Into` on newtypes for natural conversions
- `AsRef<str>` on string newtypes

---

## Anti-Patterns

AI agents consistently produce these in Rust:

1. **Over-engineering**: wrapper types with no value, trait abstractions with one impl, premature generalization
2. **Outdated crate choices**: `lazy_static`, `once_cell`, `async-trait`, `failure`, `chrono`
3. **Hallucinated APIs**: method signatures that don't exist. Always `cargo check`.
4. **Incomplete trait impls**: missing `size_hint`, `source()`, `Display` edge cases
5. **Clone to satisfy borrow checker**: restructure ownership instead
6. **`unwrap()` in library code**: use `?` with `.context()` or `expect("reason")`
7. **`std::sync::Mutex` in async**: use `tokio::sync::Mutex` when holding across `.await`
8. **Ignoring `Send + Sync`**: types not `Send` used across thread boundaries
9. **Bare `tokio::spawn` without `.instrument()`**: loses trace context
10. **`pub` on everything**: start `pub(crate)`, promote only when needed
11. **Ignoring `unsafe_op_in_unsafe_fn`**: 2024 edition warns. Wrap unsafe ops in explicit `unsafe {}` blocks inside unsafe functions.
12. **Ad-hoc shutdown channels**: use `CancellationToken` from `tokio_util`
13. **Missing `#[must_use]`**: Result-returning functions, builders, and pure functions must be annotated. Silently dropped results are bugs.
14. **`Rc` in async contexts**: use `Arc`. Futures are `Send`; `Rc` is not.
15. **`as` casts for numeric conversions**: use `try_from`/`try_into`. `as` silently truncates.
16. **Array indexing without bounds check**: use `.get()` in library code. Indexing panics.
17. **String slicing**: `name[..3]` panics on non-UTF8 boundaries. Use `.get(..3)`.
18. **Bare `assert!`**: always include a message describing the invariant.
19. **Plain `String` for secrets**: use `secrecy::SecretString`. Zeroized on drop, no accidental Display.
20. **`sleep` in tests**: use `tokio::time::pause()` for deterministic time.

### Workspace versioning

Multi-crate workspaces use a single version in the root `Cargo.toml`:

```toml
[workspace.package]
version = "0.13.0"
```

Each crate inherits:

```toml
[package]
version = { workspace = true }
```

One version to bump. One changelog. No per-crate drift.

### Domain invariants via clippy.toml

Use `clippy.toml` to enforce domain-specific invariants beyond standard lints. Examples:
- Banned methods (non-deterministic float operations for rendering)
- Banned types (raw pointers when safe wrappers exist)
- Maximum function cognitive complexity

The standard clippy denials cover correctness. `clippy.toml` covers domain rules.

### Visibility discipline

Default to private. Promote to `pub(crate)` when another module in the same crate needs it. Promote to `pub` only when another crate needs it.

Over-exposure is harder to fix than under-exposure. Start restrictive, widen on demand. A function that's `pub` but only used within the crate is a maintenance liability: downstream code can depend on it, blocking internal refactors.

### Cancellation safety

Document whether each public async function is cancellation-safe. A function is cancellation-safe if dropping the returned future mid-`.await` and calling the function again produces correct behavior.

**Safe to cancel**: `recv()`, `accept()`, `read()`, `next()`, stream operations.
**NOT safe to cancel**: `read_exact()`, `write_all()`, `lock()`, `acquire()` (lose queue position or partial data).

In `select!` blocks, every branch must be cancellation-safe or explicitly documented as not.

### Structured concurrency

Use `JoinSet` for managing groups of spawned tasks. Drop = abort all. Never use `Vec<JoinHandle>` for task groups.

```rust
let mut set = JoinSet::new();
set.spawn(task_a());
set.spawn(task_b());
while let Some(result) = set.join_next().await { /* handle */ }
// Drop aborts remaining tasks
```

### Cooperative yielding

Long-running async tasks should yield periodically. Tokio's cooperative budget is 128 units per poll. CPU-bound work blocks other tasks.

Options:
- `tokio::task::yield_now().await` at natural checkpoints
- `tokio::task::spawn_blocking()` for truly CPU-bound work
- `tokio::task::unconstrained()` to exempt from budget (use sparingly)
