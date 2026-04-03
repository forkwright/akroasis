# Akroasis Project Standards

## Error Handling

- Use [`snafu`](https://docs.rs/snafu) for all error types.
- Define per-module error enums with `#[derive(Debug, Snafu)]`.
- Use context selectors (`SomeContext { field: value }.fail()` or `.context(SomeSnafu)`).
- Never use `.unwrap()` or `.expect()` outside `#[cfg(test)]` modules.
- Error variants that wrap large foreign errors (e.g. `figment::Error`) must box the source to keep enum size reasonable.

## Commit Conventions

Format: `category(scope): what`

| Category | Use for |
|----------|---------|
| `feat` | New capability |
| `fix` | Bug fix |
| `docs` | Documentation only |
| `refactor` | No behaviour change |
| `test` | Test additions/fixes |
| `chore` | Build, deps, config |
| `style` | Formatting, lint |

Scope is optional but encouraged for workspace members (e.g. `feat(koinon): ...`).

## Lint Rules

Enforced at workspace level in `Cargo.toml`:

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "warn"
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
```

CI runs `cargo clippy --workspace --all-targets -- -D warnings`, so all warn-level lints become errors in CI.

Add `#[allow(clippy::expect_used, clippy::unwrap_used)]` to `#[cfg(test)]` modules where panicking assertions are appropriate.

## Testing

- Unit tests live in the same file, inside `#[cfg(test)] mod tests { ... }`.
- Integration tests live under `tests/` in the crate root.
- Test names describe what is being verified: `fn valid_coordinates_accepted()`.
- Use `assert!`, `assert_eq!`, `assert_ne!`  -  do not write custom assertion logic unless necessary.
- Doc-tests (`cargo test --workspace --doc`) must also pass.
