# Rust Coding Standards

## Error Handling

- All error types use `snafu`: `#[derive(Debug, Snafu)]` with descriptive display strings.
- Return `Result<T, E>` from fallible functions; never silently swallow errors.
- Never use `.unwrap()` or `.expect()` outside `#[cfg(test)]` modules.
- Never use `panic!`, `todo!()`, or `unreachable!()` in production code paths.
- Box large error variants to keep enum sizes reasonable (see `clippy::result_large_err`).

## Workspace Lints

Every crate must include `[lints] workspace = true` in its `Cargo.toml`. This inherits:

- `unsafe_code = "forbid"` — no unsafe blocks
- `missing_docs = "warn"` — all public items need doc comments
- `unwrap_used = "deny"`, `expect_used = "deny"`, `panic = "deny"`
- `pedantic` and `nursery` at `warn` (becomes error in CI via `-D warnings`)

## Doc Comments

- All `pub` structs, enums, functions, and fields need a `///` doc comment.
- Functions returning `Result` need a `# Errors` section listing each error variant.
- Functions that can panic need a `# Panics` section (avoid panicking functions).
- Items referenced in docs should be wrapped in backticks or `[link]` syntax.

## Module Structure

```
crate/
  src/
    lib.rs       — module declarations + re-exports
    module.rs    — types + inline #[cfg(test)] mod tests { ... }
  tests/         — integration tests (if needed)
```

## Constructors and Const

- Prefer `const fn` constructors when the body allows it (e.g. simple field assignment).
- Use `Self` in `impl` blocks rather than the concrete type name (`nursery::use_self`).
- Use `mul_add` for multiply-then-add float expressions (`nursery::suboptimal_flops`).

## Tests

- Test modules: `#[cfg(test)] #[allow(clippy::unwrap_used, clippy::expect_used)] mod tests { ... }`
- Test function names are descriptive sentences: `fn valid_coordinates_accepted()`.
- Each new type needs at minimum: constructor, display/formatting, serde roundtrip tests.
