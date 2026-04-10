# TOML

> Formatting, structure, and conventions for TOML files across forkwright projects. This document covers the TOML format itself -- what goes *in* the files, not which files must exist (see REPO-SETUP.md) or which dependencies to use (see RUST.md).

---

## Spec version

Target TOML 1.1. Cargo supports it since Rust 1.94.0. Key improvements over 1.0: multi-line inline tables with trailing commas, `\xHH` hex escapes, `\e` escape for ANSI sequences, optional seconds in datetime.

Do not use 1.1-only features in libraries that must parse on older toolchains. For application Cargo.toml files and internal config, 1.1 is the default.

---

## Cargo.toml section ordering

Sections appear in this order. Omit sections that don't apply.

```
[workspace]
[workspace.package]
[workspace.dependencies]
[workspace.lints.clippy]
[workspace.lints.rust]

[package]
[lib]
[[bin]]
[[test]]
[[bench]]
[[example]]

[features]
[dependencies]
[build-dependencies]
[dev-dependencies]
[target.*.dependencies]

[lints]

[profile.dev]
[profile.dev.package."*"]
[profile.release]

[package.metadata.*]
```

### Within `[package]`

1. `name` (first)
2. `version` (second)
3. Remaining keys in alphabetical order
4. `description` (last)

### Within `[workspace]`

1. `resolver` (if explicit)
2. `members`
3. `exclude`

All other sections: keys in alphabetical order. Dependencies are alphabetical by crate name.

---

## Dependency declaration

### Version-only (simple)

Use a bare string when only the version is needed:

```toml
serde = "1"
tokio = "1"
```

### Table form (complex)

Use an inline table when the dependency needs additional fields:

```toml
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", default-features = false, features = ["rt", "macros"] }
```

Key order within inline dependency tables: `version`, `path`, `git`, `branch`, `rev`, `default-features`, `features`, `optional`, `workspace`.

### Expanded form (long)

When the inline table exceeds 80 columns, switch to an expanded `[dependencies.X]` block:

```toml
[dependencies.tower-http]
version = "0.6"
default-features = false
features = [
    "compression-gzip",
    "cors",
    "normalize-path",
    "timeout",
    "trace",
]
```

### Workspace inheritance

In multi-crate workspaces, declare shared dependencies once in `[workspace.dependencies]` and reference with `{ workspace = true }`:

```toml
# Root Cargo.toml
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }

# Crate Cargo.toml
[dependencies]
serde = { workspace = true }
```

Override workspace features only when a crate needs a subset. Never duplicate the version -- workspace inheritance ensures consistency. See REPO-SETUP.md for workspace configuration details.

---

## Formatting

### Spacing

- Single space before and after `=`
- No indentation under table headers (keys start at column 0)
- One blank line between sections
- No blank line between a section header and its first key
- No blank lines between keys within a section

### String quoting

Use bare keys for all standard key names. Only quote keys that contain characters outside `A-Za-z0-9_-`:

```toml
# correct
name = "my-crate"
rust-version = "1.85"

# wrong -- unnecessary quoting
"name" = "my-crate"
```

For values:

| Type | Syntax | Use when |
|------|--------|----------|
| Basic | `"..."` | Default for most string values |
| Literal | `'...'` | Regex, glob patterns, Windows paths -- anything with literal backslashes |
| Multi-line basic | `"""..."""` | Long descriptions, multi-line text |
| Multi-line literal | `'''...'''` | Large raw text blocks |

### Arrays

Single-line when the full line (key + ` = ` + array) fits within 80 columns:

```toml
members = ["crates/*"]
allow = ["MIT", "Apache-2.0", "BSD-2-Clause"]
```

Multi-line when it exceeds 80 columns or has 4+ elements:

```toml
features = [
    "compression-gzip",
    "cors",
    "normalize-path",
    "timeout",
    "trace",
]
```

Multi-line arrays use 4-space indentation, one element per line, trailing comma on every element including the last, closing bracket on its own line at column 0.

### Inline tables

Inline tables stay on one line and fit within 80 columns:

```toml
serde = { version = "1", features = ["derive"] }
```

When the table exceeds 80 columns or has 4+ keys, switch to expanded form. With TOML 1.1, multi-line inline tables are valid but expanded `[section]` form remains preferred for readability.

---

## Comments

Use `#` comments to explain *why*, not *what*. Follow the same structured tag system as code comments (WHY, WARNING, NOTE).

```toml
# WHY: optimize deps for faster runtime during development
opt-level = 2

# WARNING: changing this version requires updating CI matrix
rust-version = "1.85"
```

Place comments on the line above the key they describe, not inline. Exception: short clarifications on the same line are acceptable when the context is obvious:

```toml
strip = true  # debug symbols
```

Section separators: a single blank line between sections is sufficient. Do not use comment-based separators (`# ---` or `# ====`).

---

## Config file conventions

### deny.toml

Section order: `[graph]`, `[output]`, `[advisories]`, `[licenses]`, `[licenses.private]`, `[bans]`, `[sources]`. Include only deviations from cargo-deny defaults -- a dense config with project-specific comments beats a verbose template with generic ones. See REPO-SETUP.md for the required policy settings.

### rustfmt.toml / clippy.toml

Minimal. Only include settings that deviate from defaults. An empty file is valid -- its presence signals the tool is active.

### kanon.toml

Section order: `[projects]`, `[dispatch]`, `[gate]`, `[steward]`. Each project entry uses an inline table. Dispatch config uses flat key-value pairs.

### .taplo.toml

Configure taplo for repo-wide TOML formatting. Recommended baseline:

```toml
[formatting]
column_width = 80
indent_string = "    "
trailing_newline = true
array_trailing_comma = true
reorder_keys = false

[[rule]]
include = ["**/Cargo.toml"]
keys = ["dependencies", "dev-dependencies", "build-dependencies", "workspace.dependencies"]

[rule.formatting]
reorder_keys = true
```

---

## Anti-patterns

**Wildcard versions.** `dep = "*"` is rejected by crates.io and means "any version" locally. Use semver ranges: `dep = "1"` (equivalent to `^1`).

**Over-constrained versions.** `dep = ">=2.0, <2.4"` prevents the resolver from using compatible newer versions. Use caret requirements unless pinning is deliberate and documented.

**Unnecessary key quoting.** `"name" = "value"` adds noise when `name = "value"` works. Only quote keys with special characters.

**Type confusion.** `port = "8080"` (string) vs `port = 8080` (integer). TOML is typed -- the wrong type causes runtime deserialization errors, not parse errors.

**Implicit table conflicts.** Defining `[a.b]` then later `[a]` with key `b` is a duplicate key error. Dotted keys and table headers interact -- keep one style per subtable.

**Deep inline nesting.** `config = { a = { b = { c = 1 } } }` is unreadable. Expand to `[config.a.b]` sections.

**Missing workspace inheritance.** In multi-crate workspaces, duplicating dependency versions across crate Cargo.toml files causes drift. Use `[workspace.dependencies]` for all shared deps.

**Missing trailing commas in multi-line arrays.** Valid TOML but creates noisier diffs. Always use trailing commas.

**Bare `path` without `version` for publishable crates.** Cargo requires `version` for publishing. Always include both for workspace crates that publish:

```toml
my-lib = { version = "0.1", path = "../my-lib" }
```
