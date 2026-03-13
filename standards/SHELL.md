# Shell

> Additive to STANDARDS.md. Read that first. Everything here is shell-specific.
>
> Target: Bash 5.x. For scripts, CI pipelines, and automation. Use `just` for task running.
>
> **Key decisions:** shellcheck, just (not Make), set -euo pipefail, bats testing, flock locking, mktemp temp files, strict quoting.

---

## Toolchain

- **Shell:** Bash 5.x (not sh, not zsh)
- **Linter:** `shellcheck` — all scripts must pass with zero warnings
- **Task runner:** `just` for project automation (replaces Makefiles for non-build tasks)
- **Testing:** `bats` (Bash Automated Testing System) for scripts beyond trivial wrappers
- **Shebang:** `#!/usr/bin/env bash`
- **Validation:**
  ```bash
  shellcheck script.sh
  bats tests/
  ```

### just for Task Automation

`just` is a command runner, not a build system. Use it for dev commands, CI scripts, deploy recipes. Reserve Make for actual build dependency graphs (C/C++, generated files).

```just
# justfile
set dotenv-load

test *args:
    cargo test {{args}}

lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo fmt --check

deploy target:
    ssh {{target}} 'cd /app && git pull && systemctl restart app'
```

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Scripts | `kebab-case.sh` | `deploy-worker.sh`, `run-tests.sh` |
| Functions | `snake_case` | `load_config`, `check_health` |
| Variables | `snake_case` | `output_dir`, `retry_count` |
| Constants / Env vars | `UPPER_SNAKE_CASE` | `MAX_RETRIES`, `CONFIG_PATH` |

---

## Safety

### Strict Mode

Every script starts with:

```bash
#!/usr/bin/env bash
set -euo pipefail
```

- `set -e` — exit on error
- `set -u` — error on undefined variables
- `set -o pipefail` — pipe fails if any command in the pipe fails

Add `set -E` (errtrace) when using ERR traps — makes traps inherited by functions and subshells:

```bash
set -Eeuo pipefail
trap 'echo "Error at line $LINENO" >&2' ERR
```

Without `-E`, ERR traps are invisible inside functions. Don't add `-E` without an actual `trap ... ERR`.

Note: `set -e` has subtle edge cases in compound commands and conditionals. Don't rely on it as your sole error-handling strategy — explicit `|| handle_error` on critical commands is more reliable.

### Quoting

Quote all variable expansions. Always. `"$var"` not `$var`.

```bash
# Correct
if [[ -f "$config_path" ]]; then
    cp "$source" "$dest"
fi

# Wrong — word splitting, glob expansion
if [ -f $config_path ]; then
    cp $source $dest
fi
```

### Conditionals

`[[ ]]` not `[ ]`. Double brackets are safer (no word splitting, supports `&&`/`||`, regex).

### Timeouts

Timeout all external calls. Network operations, API calls, and long-running processes must have explicit timeouts.

```bash
timeout 30 curl -s "$url" || { echo "error: request timed out" >&2; exit 1; }
```

### Option Terminator

Use `--` before user-supplied arguments to prevent option injection:

```bash
rm -- "$file"
grep -- "$pattern" "$file"
```

---

## Security

### Input Validation

```bash
# Allowlist-validate input
[[ "$input" =~ ^[a-zA-Z0-9_-]+$ ]] || { echo "error: invalid input" >&2; exit 1; }
```

Never `eval "$user_input"`. If unavoidable, allowlist-validate first.

### PATH Hardening

Set PATH explicitly in scripts that run with elevated privileges or in CI:

```bash
PATH=/usr/local/bin:/usr/bin:/bin
```

Use absolute paths for security-critical commands: `/usr/bin/openssl` not `openssl`.

### Temp File Security

```bash
tmpdir=$(mktemp -d) || exit 1
trap 'rm -rf -- "$tmpdir"' EXIT
tmpfile=$(mktemp "$tmpdir/work.XXXXXX") || exit 1
umask 077  # before handling sensitive data
```

- Always `mktemp` (0600 perms, atomic uniqueness check)
- Never construct temp paths manually (`/tmp/myapp.$$` is predictable and exploitable via symlink attacks)
- `mktemp -d` for multi-file operations, clean up the directory
- Always `trap EXIT` for cleanup

### File Locking

Use `flock` for mutual exclusion, not PID files:

```bash
exec 9>/var/lock/myapp.lock
flock -n 9 || { echo "error: already running" >&2; exit 1; }
```

### CI Pipeline Security (GitHub Actions)

Never interpolate untrusted input directly in `run:` blocks:

```yaml
# VULNERABLE — string-interpolated before shell execution
- run: echo "${{ github.event.pull_request.title }}"

# SAFE — pass through environment variable
- run: echo "$PR_TITLE"
  env:
    PR_TITLE: ${{ github.event.pull_request.title }}
```

Untrusted contexts: `body`, `title`, `head_ref`, `label`, `message`, `name`, `email`. Pin actions to commit SHA, not tags.

---

## Error Handling

- Error messages to stderr: `echo "error: description" >&2`
- Exit with non-zero status on failure: `exit 1`
- Use `trap` for cleanup:
  ```bash
  cleanup() { rm -f "$tmpfile"; }
  trap cleanup EXIT
  ```
- Never `|| true` to suppress errors without explanation

---

## Style

### Functions

```bash
check_health() {
    local host="$1"
    local port="${2:-8080}"

    if ! curl -sf "http://${host}:${port}/health" >/dev/null; then
        echo "error: ${host}:${port} unhealthy" >&2
        return 1
    fi
}
```

- `local` for all function variables
- Default parameters with `${var:-default}`
- Return non-zero on failure, don't `exit` from functions (caller decides)

### Bash 5.x Features

Use when targeting modern Linux (5.1+ is safe for 2020+ distros):

```bash
# Timestamps without forking date
echo "started at $EPOCHSECONDS"

# Cryptographic-quality random (not the RANDOM LCG)
token=$(printf '%08x' "$SRANDOM")

# Case transformation without tr/awk
local upper="${name@U}"
local lower="${name@L}"

# Namerefs for dynamic variable references (no eval)
declare -n ref="$varname"
echo "${ref}"
```

Note: macOS ships Bash 3.2 (GPLv2 licensing). If macOS compatibility is needed, either mandate `brew install bash` or avoid 5.x features.

### No Dead Weight

- No commented-out code
- No unused variables (`set -u` catches these)
- No `echo` for debugging in committed scripts — use a `debug()` function gated on a flag

---

## Testing with bats

For projects with shell scripts beyond trivial wrappers:

```bash
#!/usr/bin/env bats

@test "deploy script creates config" {
    run ./deploy.sh --dry-run
    [ "$status" -eq 0 ]
    [[ "$output" == *"config written"* ]]
}

@test "fails on missing argument" {
    run ./deploy.sh
    [ "$status" -ne 0 ]
    [[ "$output" == *"error:"* ]]
}
```

TAP-compliant output works with CI runners. Use `bats-assert` and `bats-file` helper libraries for richer assertions.

---

## Anti-Patterns

1. **Missing `set -euo pipefail`** — every script, no exceptions
2. **Unquoted variables** — always `"$var"`
3. **`[ ]` instead of `[[ ]]`** — double brackets are safer
4. **`echo` for error messages** — errors go to stderr: `>&2`
5. **No `trap` cleanup** — temp files leak
6. **`|| true` without comment** — hiding failures
7. **Parsing `ls` output** — use globs or `find`
8. **`cat file | grep`** — `grep pattern file` directly
9. **Hardcoded paths** — use variables or `$0`-relative paths
10. **Missing `local` in functions** — variables leak to global scope
11. **Manual temp file paths** — use `mktemp`, never `/tmp/myapp.$$`
12. **`${{ }}` interpolation in GitHub Actions `run:`** — pass through `env:` instead
