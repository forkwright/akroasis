# YAML

> Standards for YAML files, with emphasis on GitHub Actions workflows. Covers formatting, structure, security, and common pitfalls.

---

## Formatting

### Indentation

2 spaces. No tabs. YAML parsers reject tabs as indentation -- they produce syntax errors, not style issues.

### Trailing whitespace

No trailing whitespace on any line. No trailing blank lines at end of file.

### Line length

120 characters soft limit. Use multiline strings or YAML folding for longer values rather than horizontal scrolling.

---

## Key ordering

Workflow files use a consistent key order for readability:

```yaml
name:
on:
permissions:
concurrency:
env:
jobs:
```

Within a job:

```yaml
jobs:
  build:
    name:
    runs-on:
    permissions:
    concurrency:
    env:
    defaults:
    if:
    timeout-minutes:
    strategy:
    steps:
```

Within a step:

```yaml
- name:
  id:
  if:
  uses:
  with:
  env:
  run:
```

---

## String quoting and type coercion

YAML 1.1 (used by most parsers including GitHub Actions) coerces unquoted values into unexpected types. This is the single largest source of YAML bugs.

### The truthy problem

These bare values all parse as booleans, not strings:

```
yes, no, y, n, true, false, on, off
YES, NO, Y, N, True, False, On, Off
```

The `on:` key in GitHub Actions workflows is actually parsed as `{true: ...}` -- GitHub Actions has special handling for this, but other YAML consumers do not.

**Rule**: Quote any string value that matches a YAML boolean keyword. For map keys that must remain unquoted for tool compatibility (like `on:` in workflows), accept the tool convention but document it.

### Numeric coercion

```yaml
version: 3.10    # Parses as float 3.1, not string "3.10"
zipcode: 01onal  # Parses as octal 1 in YAML 1.1
time: 22:22      # Parses as sexagesimal integer 1342 in YAML 1.1
```

**Rule**: Quote version numbers, zip codes, and any value where leading zeros or decimal precision matter.

### When to quote

| Context | Quote? | Example |
|---------|--------|---------|
| String that looks like bool | Yes | `"true"`, `"on"`, `"yes"` |
| Version numbers | Yes | `"3.10"`, `"1.0.0"` |
| Values with special chars | Yes | `"contains: colon"` |
| Plain strings | No | `name: CI` |
| URLs | No | `url: https://example.com` |

Prefer double quotes over single quotes for consistency. Single quotes do not support escape sequences.

---

## Multiline strings

### Literal block scalar (`|`)

Preserves newlines. Use for shell scripts and multi-line commands:

```yaml
run: |
  cargo fmt --check
  cargo clippy --all-targets
  cargo test
```

### Folded block scalar (`>`)

Folds newlines into spaces. Use for long prose values:

```yaml
description: >
  This workflow runs on every push to main
  and validates the full test suite.
```

### Strip modifier (`|-`, `>-`)

Removes trailing newline. Use when the trailing newline would cause issues:

```yaml
if: >-
  github.event_name == 'pull_request' ||
  github.ref == 'refs/heads/main'
```

### When to use which

| Content | Style |
|---------|-------|
| Shell commands | `\|` (literal) |
| Long conditions | `>-` (folded, stripped) |
| Prose descriptions | `>` (folded) |
| Single-line values | Plain or quoted |

---

## GitHub actions workflows

### Workflow name

Every workflow file must have a top-level `name:` key. The name appears in the GitHub UI and in status checks. Unnamed workflows display as the filename, which is ambiguous when multiple workflows exist.

### Permissions

Every workflow must declare an explicit `permissions:` block. Default permissions are overly broad (`contents: write`, `packages: write`, etc. for `GITHUB_TOKEN`).

**Least privilege**: declare only what the workflow needs:

```yaml
permissions:
  contents: read

jobs:
  build:
    # inherits workflow-level permissions
```

For workflows that need no token at all:

```yaml
permissions: {}
```

For mixed-permission workflows, set restrictive defaults at workflow level and grant per-job:

```yaml
permissions:
  contents: read

jobs:
  deploy:
    permissions:
      contents: write
    steps: ...
```

### Concurrency

Long-running workflows (CI, security scans, builds) must define concurrency groups to prevent redundant runs:

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

Short-lived workflows (stale issue cleanup, labeling) may omit concurrency if runs are idempotent and fast.

### Action pinning

Pin all third-party actions to full commit SHAs, not tags or branches. Tags are mutable -- a compromised action can push malicious code to an existing tag.

```yaml
# Correct -- immutable SHA reference
uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11  # v4

# Wrong -- mutable tag
uses: actions/checkout@v4
```

Document the version each SHA corresponds to in a trailing comment.

Maintain a pinned-action inventory in CI documentation (see `workflow/templates/ci/README.md` for the pattern).

Cross-reference: **SHELL.md#github-actions** defines the `SHELL/unpinned-action` lint rule for this.

### Checkout credentials

Always set `persist-credentials: false` on checkout actions unless the workflow explicitly needs to push commits. Persisted credentials remain in the runner's git config and can be exploited by subsequent steps.

```yaml
- uses: actions/checkout@<sha>
  with:
    persist-credentials: false
```

### Script injection

Never interpolate untrusted GitHub context values directly in `run:` blocks. PR titles, branch names, and commit messages are attacker-controlled.

```yaml
# Wrong -- injectable
run: echo "${{ github.event.pull_request.title }}"

# Correct -- bound to env var
env:
  PR_TITLE: ${{ github.event.pull_request.title }}
run: echo "$PR_TITLE"
```

Safe contexts that do not need env-var binding: `github.sha`, `github.ref`, `github.run_id`, `runner.os`, `matrix.*`, `secrets.*`, `env.*`, `needs.*`, `inputs.*`, `steps.*`.

Cross-reference: **SHELL.md#github-actions** defines the `SHELL/gha-injection` lint rule for this.

---

## Environment variables

### Naming

`UPPER_SNAKE_CASE` for all environment variables. Match the conventional name when wrapping well-known variables (`CARGO_TERM_COLOR`, `RUSTDOCFLAGS`, `GH_TOKEN`).

### Scoping

Define variables at the narrowest scope possible:

| Scope | Use when |
|-------|----------|
| Workflow-level `env:` | Every job needs it |
| Job-level `env:` | Multiple steps in one job need it |
| Step-level `env:` | Only one step needs it |

### Secrets

- Never `echo` a secret in a `run:` block, even with masking
- Use `${{ secrets.NAME }}` exclusively in `env:` or `with:` blocks
- Never pass secrets as command-line arguments (visible in process listings)
- For composite actions, pass secrets via inputs, never via environment inheritance

Cross-reference: **SECURITY.md#credentials** for general credential handling standards.

---

## Anchors and aliases

YAML anchors (`&name`) and aliases (`*name`) reduce duplication. Use them for repeated configuration blocks:

```yaml
defaults: &rust-defaults
  runs-on: ubuntu-latest
  timeout-minutes: 15

jobs:
  check:
    <<: *rust-defaults
    steps: ...
  test:
    <<: *rust-defaults
    steps: ...
```

### Limits

- Maximum anchor depth: 3 levels (anchor referencing anchor referencing anchor)
- Never use recursive anchors -- they enable denial-of-service via exponential expansion (the "billion laughs" attack)
- Prefer anchors for configuration blocks, not for complex nested structures

---

## Comments

Follow the structured comment tag convention from STANDARDS.md:

```yaml
# WHY: Separate concurrency group prevents security scans from canceling CI.
concurrency:
  group: security-${{ github.ref }}
```

Allowed tags: `WHY`, `WARNING`, `NOTE`, `PERF`, `SAFETY`, `INVARIANT`, `TODO(#NNN)`, `FIXME(#NNN)`.

For inline version documentation on pinned actions:

```yaml
uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11  # v4
```

---

## Anti-patterns

| Anti-pattern | Problem | Fix |
|-------------|---------|-----|
| `uses: actions/checkout@v4` | Mutable tag, supply chain risk | Pin to full SHA |
| No `permissions:` block | Overly broad default token | Add explicit permissions |
| `on: push` without filters | Every branch triggers CI | Add `branches:` or `paths:` filter |
| `${{ }}` in `run:` block | Script injection | Bind to env var |
| `persist-credentials: true` | Credential leakage risk | Set `false` explicitly |
| Bare `yes`/`no`/`on`/`off` values | Boolean coercion | Quote: `"yes"`, `"no"` |
| Tabs for indentation | YAML syntax error | 2-space indent |
| Missing `name:` on workflow | Ambiguous status checks | Add descriptive name |
| Deep anchor chains | Hard to trace, expansion risk | Limit to 3 levels |

---

## Cross-references

| Topic | Standard | Rules |
|-------|----------|-------|
| Action pinning | SHELL.md#github-actions | `SHELL/unpinned-action` |
| Script injection | SHELL.md#github-actions | `SHELL/gha-injection` |
| Credential handling | SECURITY.md#credentials | -- |
| Supply chain | SECURITY.md#dependency-supply-chain | -- |
| Required CI checks | CI.md#required-checks | `CI/no-format-check`, `CI/no-clippy-check`, `CI/no-test-step` |
