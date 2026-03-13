# Rust

> Additive to STANDARDS.md. Read that first. Everything here is Rust-specific.
>
> **Key decisions:** 2024 edition, snafu errors, tokio async, tracing logging, jiff time, cancel-safe select, pub(crate) default, cargo-deny.

---

## Toolchain

- **Edition:** 2024 (Rust 1.85+)
- **MSRV:** Set explicitly in `Cargo.toml` — the MSRV-aware resolver (default since 1.84) respects it during dependency resolution
- **Async runtime:** Tokio
- **Build/test cycle:**
  ```bash
  cargo test -p <crate>                                    # targeted tests during development
  cargo clippy --workspace --all-targets -- -D warnings    # lint + type-check full workspace
  cargo test --workspace                                   # full suite as final gate before PR
  ```
- **Formatting:** `cargo fmt` — default rustfmt config, no overrides
- **Audit:** `cargo-deny` for licenses, advisories, bans, and sources (see Dependencies)

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `snake_case.rs` | `session_store.rs` |
| Types / Traits | `PascalCase` | `SessionStore`, `LlmProvider` |
| Functions / Methods | `snake_case` | `load_config`, `create_session` |
| Constants / Statics | `UPPER_SNAKE_CASE` | `MAX_TURNS`, `DEFAULT_PORT` |
| Crate names | `kebab-case` (Cargo) / `snake_case` (code) | `aletheia-mneme` / `aletheia_mneme` |
| Feature flags | `kebab-case` | `full-text-search` |

- `into_` for ownership-consuming conversions, `as_` for cheap borrows, `to_` for expensive conversions.

---

## Type System

### Newtypes for Domain Concepts

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
```

### `#[non_exhaustive]` on Public Enums

All public enums that may grow variants must use `#[non_exhaustive]`. This preserves backward compatibility — adding a variant isn't a breaking change.

### Typestate Pattern

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

### Exhaustive Matching

Use `match` with explicit variants over wildcard `_` arms when the enum is under your control. Wildcards hide new variants.

### `#[must_use]` on Results and Pure Functions

`#[must_use]` on all public functions that return `Result`, builder methods, and pure functions. Compile-time enforcement that return values aren't silently dropped.

```rust
#[must_use]
pub fn validate_config(raw: &str) -> Result<Config, ConfigError> { /* ... */ }

#[must_use]
pub fn with_timeout(self, timeout: Duration) -> Self { /* ... */ }
```

Crate-level `#![warn(unused_must_use)]` is the default since 2024 edition. Apply `#[must_use]` on the *function/type*, not just at the call site.

### `#[expect(lint)]` Over `#[allow(lint)]`

`#[expect]` warns you when the suppression is no longer needed. `#[allow]` silently persists forever.

### Standard Library Types (2024 Edition)

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

### 2024 Edition Specifics

**`unsafe_op_in_unsafe_fn`:** Warns by default. Unsafe operations inside `unsafe fn` bodies must be wrapped in explicit `unsafe {}` blocks. Narrow the scope — don't treat the entire function body as unsafe.

**RPIT lifetime capture:** Return-position `impl Trait` automatically captures all in-scope type and lifetime parameters. Use `use<..>` for precise capturing when needed:

```rust
fn process<'a>(&'a self) -> impl Iterator<Item = &str> + use<'a, Self> {
    self.items.iter().map(|i| i.as_str())
}
```

**Trait upcasting:** `&dyn SubTrait` coerces to `&dyn SuperTrait` (stable since 1.86). No more manual `as_super()` methods.

### Diagnostic Attributes

```rust
#[diagnostic::on_unimplemented(message = "cannot store {Self} — implement StorageCodec")]
pub trait StorageCodec { /* ... */ }

#[diagnostic::do_not_recommend]
impl<T: Display> StorageCodec for T { /* ... */ }
```

Use `#[diagnostic::on_unimplemented]` for domain-specific trait error messages. Use `#[diagnostic::do_not_recommend]` to suppress unhelpful blanket-impl suggestions.

---

## Error Handling

**snafu** (not thiserror) for all library crate error enums. GreptimeDB pattern.

- Per-crate error enums with `.context()` propagation and `Location` tracking
- No `unwrap()` in library code. `anyhow` only in CLI entry points (`main.rs`).
- Convention: `source` field = internal error (walk the chain), `error` field = external (stop walking)
- `expect("invariant description")` over bare `unwrap()` — the message documents the invariant

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
        source: serde_yaml::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

fn load_config(path: &Path) -> Result<Config, ConfigError> {
    let contents = std::fs::read_to_string(path)
        .context(ReadConfigSnafu { path: path.display().to_string() })?;
    let config: Config = serde_yaml::from_str(&contents)
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

## Async & Concurrency

### Cancellation Safety

Document cancellation safety for every public async method. In `select!`:

| Cancel-safe | Cancel-unsafe |
|-------------|---------------|
| `sleep()`, `Receiver::recv()` | `Sender::send(msg)` — message lost |
| `Sender::reserve()` | `write_all()` — partial write |
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

### Biased Select

Use `biased;` in `select!` when polling order matters — cancellation/shutdown branches first, then work channels:

```rust
loop {
    tokio::select! {
        biased;
        _ = shutdown.cancelled() => break,
        Some(job) = rx.recv() => process(job).await,
    }
}
```

Without `biased`, branch order is randomized. A high-volume stream placed first in biased mode will starve later branches — put low-frequency/high-priority branches first.

### JoinSet for Dynamic Task Management

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

### Graceful Shutdown

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

### Locks Across Await

Never hold `std::sync::Mutex` guards across `.await` points. Either scope the lock and drop before the await, or use `tokio::sync::Mutex`.

```rust
// Correct: scope the lock
let data = {
    let guard = state.lock().unwrap();
    guard.clone()
};
let result = process(data).await;
```

### Mutex Selection

- `std::sync::Mutex` for short, non-async critical sections (faster, no overhead)
- `tokio::sync::Mutex` only when holding the lock across `.await` points

### Spawned Tasks

Spawned tasks are `'static` — they outlive any reference. Move owned data in. Clone `Arc`s before spawn. Always propagate tracing spans.

```rust
let this = Arc::clone(&self);
let span = tracing::Span::current();
tokio::spawn(async move {
    this.handle_request().await
}.instrument(span));
```

Never:
- `tokio::spawn(async { self.handle().await })` — `&self` is not `'static`
- Bare `tokio::spawn` without `.instrument()` — loses trace context

### No Nested Runtimes

Never call `Runtime::block_on()` from within async context. Use `spawn_blocking` for sync-in-async.

---

## Lifetime & Borrowing

### No Clone Spam

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

// Async context: Arc required — futures are Send
let shared = Arc::clone(&state);
tokio::spawn(async move { shared.process().await });
```

If a type is stored in a struct that implements `Send`, its `Rc` fields won't compile. Don't "fix" this by removing `Send` — switch to `Arc`.

### Own by Default

Start with owned types. Only add lifetimes when profiling shows the allocation matters. Config structs own their strings. This is not permission to `.clone()` everywhere — if you're cloning to satisfy the borrow checker, restructure ownership (see No Clone Spam above).

```rust
// Long-lived: own the data
struct Config {
    name: String,
    host: String,
}

// Short-lived view: borrow justified by hot path
struct RequestView<'a> {
    path: &'a str,
    method: Method,
}
```

### `Cow` for Mixed Owned/Borrowed

```rust
fn normalize_path(path: &str) -> Cow<'_, str> {
    if path.starts_with('/') {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(format!("/{path}"))
    }
}
```

### Arena Over Self-Referential Structs

Never fight the borrow checker with `RefCell` or `unsafe` for graph structures. Use arena allocation with index-based references.

```rust
struct Arena {
    nodes: Vec<Node>,
}
struct Node {
    children: Vec<usize>,  // indices into Arena.nodes
    parent: Option<usize>,
}
```

---

## Testing

- `#[cfg(test)] mod tests` in the same file — colocated, not in a separate tree
- `#[test]` names describe behavior: `returns_empty_when_no_turns`, not `test_recall`
- Mock at trait boundaries. Don't mock internal functions.
- Every `Serialize + Deserialize` type gets a roundtrip property test
- `proptest` / `bolero` for property-based testing
- Targeted tests during development (`cargo test -p <crate>`), full suite as final gate

---

## Dependencies

**Preferred:**
- `snafu` (errors), `tokio` (async), `tracing` (logging), `serde` (serialization)
- `jiff` (time), `ulid` (IDs), `compact_str` (small strings)
- `figment` (config), `redb` (embedded KV), `rusqlite` (SQLite)
- `std::sync::LazyLock` (lazy statics)
- `tokio_util::sync::CancellationToken` (shutdown coordination)

**Banned:**
- `thiserror` — replaced by `snafu` for library crates
- `async-trait` — native async fn in trait since Rust 1.75
- `lazy_static`, `once_cell` — use `std::sync::LazyLock`
- `serde_yml` — unsound unsafe. Use `serde_yaml` if YAML is needed.
- `failure` — abandoned, use `snafu`
- `chrono` — use `jiff`

**Policy:**
- Pin pre-1.0 crates to exact versions
- Wrap external APIs in traits for replaceability
- Each new dependency must justify itself — if it's 10 lines, write it

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

---

## Logging

`tracing` with structured spans. `#[instrument]` on public functions.

- Spawned tasks **must** propagate spans (`.instrument(span)`)
- Never hold `span.enter()` guards across `.await` points
- Log at the handling site, not the origin site
- Structured fields over string interpolation: `tracing::info!(session_id = %id, "loaded")`

---

## Performance

Known patterns — apply when relevant:

- **Prepared statements:** `rusqlite::CachedStatement` for repeated queries
- **Lazy deserialization:** `serde_json::value::RawValue` for fields not always accessed
- **Regex caching:** `LazyLock<RegexSet>` — never compile regex in loops
- **Arena allocation:** `bumpalo` for per-turn transient data, freed in bulk
- **Batched writes:** Group mutations into single transactions, don't commit per-operation
- **File watching:** `notify` crate for config/bootstrap files, cache and recompute on change
- **SSE broadcast:** Serialize once, write bytes to all clients. Don't serialize per-connection.

---

## Visibility

- `pub(crate)` by default
- `pub` only for cross-crate API surface
- Every `pub` item is a commitment — it's part of your contract with downstream crates
- Re-exports in `lib.rs` define the crate's public API explicitly

---

## API Design

- Accept `impl Into<String>` (flexible input), return concrete types (predictable output)
- All types used in async contexts must be `Send + Sync`
- Builder pattern for complex construction — `TypeBuilder::new().field(val).build()`
- Use `impl Trait` in argument position for single-use generics

---

## Anti-Patterns

AI agents consistently produce these in Rust:

1. **Over-engineering** — wrapper types with no value, trait abstractions with one impl, premature generalization
2. **Outdated crate choices** — `lazy_static`, `once_cell`, `async-trait`, `failure`, `chrono`
3. **Hallucinated APIs** — method signatures that don't exist. Always `cargo check`.
4. **Incomplete trait impls** — missing `size_hint`, `source()`, `Display` edge cases
5. **Clone to satisfy borrow checker** — restructure ownership instead
6. **`unwrap()` in library code** — use `?` with `.context()` or `expect("reason")`
7. **`std::sync::Mutex` in async** — use `tokio::sync::Mutex` when holding across `.await`
8. **Ignoring `Send + Sync`** — types not `Send` used across thread boundaries
9. **Bare `tokio::spawn` without `.instrument()`** — loses trace context
10. **`pub` on everything** — start `pub(crate)`, promote only when needed
11. **Ignoring `unsafe_op_in_unsafe_fn`** — 2024 edition warns. Wrap unsafe ops in explicit `unsafe {}` blocks inside unsafe functions.
12. **Ad-hoc shutdown channels** — use `CancellationToken` from `tokio_util`
13. **Missing `#[must_use]`** — Result-returning functions, builders, and pure functions must be annotated. Silently dropped results are bugs.
14. **`Rc` in async contexts** — use `Arc`. Futures are `Send`; `Rc` is not.
