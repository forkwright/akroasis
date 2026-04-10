# Shell

> Additive to STANDARDS.md. Read that first. Everything here is shell-specific.
>
> Target: Bash 5.x. For scripts, CI pipelines, and automation. Use `just` for task running.
>
> **Key decisions:** shellcheck (CI-gated), just (not Make), set -euo pipefail, bats testing, flock locking, mktemp temp files, strict quoting, arrays over string-splitting, signal traps for resource cleanup.

---

## Toolchain

- **Shell:** Bash 5.x (not sh, not zsh)
- **Linter:** `shellcheck`: all scripts must pass with zero warnings
- **Task runner:** `just` for project automation (replaces Makefiles for non-build tasks)
- **Testing:** `bats` (Bash Automated Testing System) for scripts beyond wrappers
- **Shebang:** `#!/usr/bin/env bash`
- **Validation:**
  ```bash
  shellcheck script.sh
  bats tests/
 ```

### Shellcheck CI integration

Every shell script in the repo is linted on every PR. No exceptions, no per-file opt-outs. WHY: a linter that doesn't run on every change is advisory, not a gate.

```yaml
# .github/workflows/shellcheck.yml
name: shellcheck
on:
  pull_request:
    paths: ["**/*.sh", "**/*.bash"]
  push:
    branches: [main]
    paths: ["**/*.sh", "**/*.bash"]

jobs:
  shellcheck:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install shellcheck
        run: sudo apt-get install -y shellcheck
      - name: Lint all shell scripts
        run: |
          find . -name '*.sh' -o -name '*.bash' | xargs shellcheck --severity=warning --shell=bash
```

Configuration lives in `.shellcheckrc` at the repo root, not in per-file directives. WHY: scattered directives hide suppressions from review.

```bash
# .shellcheckrc
shell=bash
severity=warning
# Only disable rules with a comment explaining WHY
# disable=SC2059  # printf format strings are intentionally dynamic in log()
```

Rules for suppressions:

- **Inline `# shellcheck disable=SCXXXX`**: allowed only when the suppression is local to a single line and the WHY comment is on the same line or the line above. Never blanket-disable at the top of a file.
- **`.shellcheckrc` disable**: allowed only for project-wide rules that apply everywhere with a documented reason. Reviewers must approve additions.
- **`--exclude` in CI**: never. If a rule fires, either fix the code or document the suppression in `.shellcheckrc`.

### just for task automation

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

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Scripts | `kebab-case.sh` | `deploy-worker.sh`, `run-tests.sh` |

---

## Safety

### Strict mode

Every script starts with:

```bash
#!/usr/bin/env bash
set -euo pipefail
```

- `set -e`: exit on error
- `set -u`: error on undefined variables
- `set -o pipefail`: pipe fails if any command in the pipe fails

Add `set -E` (errtrace) when using ERR traps: makes traps inherited by functions and subshells:

```bash
set -Eeuo pipefail
trap 'echo "Error at line $LINENO" >&2' ERR
```

Without `-E`, ERR traps are invisible inside functions. Don't add `-E` without an actual `trap ... ERR`.

Note: `set -e` has subtle edge cases in compound commands and conditionals. Don't rely on it as your sole error-handling strategy: explicit `|| handle_error` on critical commands is more reliable.

### Quoting

Quote all variable expansions. Always. `"$var"` not `$var`.

```bash
# Correct
if [[ -f "$config_path" ]]; then
    cp "$source" "$dest"
fi

# Wrong: word splitting, glob expansion
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

### Option terminator

Use `--` before user-supplied arguments to prevent option injection:

```bash
rm -- "$file"
grep -- "$pattern" "$file"
```

---

## Security

### Input validation

```bash
# Allowlist-validate input
[[ "$input" =~ ^[a-zA-Z0-9_-]+$ ]] || { echo "error: invalid input" >&2; exit 1; }
```

Never `eval "$user_input"`. If unavoidable, allowlist-validate first.

### PATH hardening

Set PATH explicitly in scripts that run with elevated privileges or in CI:

```bash
PATH=/usr/local/bin:/usr/bin:/bin
```

Use absolute paths for security-critical commands: `/usr/bin/openssl` not `openssl`.

### Temp file security

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

### File locking

Use `flock` for mutual exclusion, not PID files:

```bash
exec 9>/var/lock/myapp.lock
flock -n 9 || { echo "error: already running" >&2; exit 1; }
```

### CI pipeline security (GitHub actions)

Never interpolate untrusted input directly in `run:` blocks:

```yaml
# VULNERABLE: string-interpolated before shell execution
- run: echo "${{ github.event.pull_request.title }}"

# SAFE: pass through environment variable
- run: echo "$PR_TITLE"
  env:
    PR_TITLE: ${{ github.event.pull_request.title }}
```

Untrusted contexts: `body`, `title`, `head_ref`, `label`, `message`, `name`, `email`. Pin actions to commit SHA, not tags.

---

## Error handling

- Error messages to stderr: `echo "error: description" >&2`
- Exit with non-zero status on failure: `exit 1`
- Use `trap` for cleanup:
  ```bash
  cleanup() { rm -f "$tmpfile"; }
  trap cleanup EXIT
 ```
- Never `|| true` to suppress errors without explanation

---

## Signal handling

Scripts that create resources (temp files, lock files, child processes, state files) must handle signals. WHY: `set -e` does not run cleanup on SIGTERM or SIGINT — only `trap` does. A script killed by `systemctl stop`, Ctrl-C, or a CI timeout that doesn't trap signals leaks resources.

### Cleanup on exit and signals

```bash
cleanup() {
    rm -rf -- "$tmpdir"
    # Kill child processes if any were spawned
    kill -- -$$ 2>/dev/null || true
}
trap cleanup EXIT INT TERM
```

- **EXIT**: runs on normal exit, `exit N`, and `set -e` failures. This is your primary cleanup hook.
- **INT**: Ctrl-C (SIGINT). Without this, interactive kills skip EXIT on some shells.
- **TERM**: `kill`, `systemctl stop`, CI timeout. The default signal. Without this, graceful shutdown is impossible.
- **Do not trap HUP** unless the script runs as a daemon. HUP means "reload config" for daemons and "terminal closed" for interactive scripts — conflating the two causes bugs.

### Idempotent cleanup

Cleanup functions must be idempotent. WHY: signals can arrive during cleanup itself, and EXIT runs after signal traps.

```bash
cleanup() {
    # Guard: only run once
    [[ "${_cleanup_done:-}" == "1" ]] && return
    _cleanup_done=1

    rm -rf -- "$tmpdir"
}
```

### Propagating exit status through traps

A trapped signal should re-raise to preserve the correct exit status for the parent. WHY: without re-raise, the parent sees exit 0 and believes the script succeeded.

```bash
trap 'cleanup; trap - INT; kill -INT $$' INT
trap 'cleanup; trap - TERM; kill -TERM $$' TERM
```

This pattern: run cleanup, reset the trap to default, re-send the signal. The parent then sees the correct signal-killed exit status (128 + signal number).

### Long-running scripts with child processes

Scripts that spawn background work must forward signals to children. WHY: killing the parent does not automatically kill children — they become orphans.

```bash
child_pid=
cleanup() {
    if [[ -n "${child_pid:-}" ]]; then
        kill -- "$child_pid" 2>/dev/null
        wait "$child_pid" 2>/dev/null
    fi
    rm -rf -- "$tmpdir"
}
trap cleanup EXIT INT TERM

long_running_command &
child_pid=$!
wait "$child_pid"
child_pid=
```

Capture the PID, kill it in cleanup, `wait` to reap. Clear the PID after `wait` returns to make cleanup idempotent.

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

### Bash 5.x features

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

### Arrays

Use arrays for lists of items. Never pack multiple values into a single string and split later. WHY: string-splitting breaks on whitespace, globs, and special characters. Arrays preserve element boundaries.

**Indexed arrays** for ordered lists:

```bash
local -a files=()
while IFS= read -r -d '' f; do
    files+=("$f")
done < <(find . -name '*.sh' -print0)

# Iterate safely: quoted expansion preserves elements with spaces
for f in "${files[@]}"; do
    shellcheck "$f"
done
```

- `"${array[@]}"` (quoted, `@`) expands each element as a separate word. This is the correct form for iteration and command arguments.
- `"${array[*]}"` (quoted, `*`) joins all elements into a single string with the first character of IFS. Use only for display/logging, never for passing arguments.
- `${array[@]}` (unquoted) subjects each element to word splitting and globbing. Never use this form.

**Associative arrays** for key-value maps (Bash 4.0+):

```bash
declare -A service_ports=(
    [adguard]=3000
    [plex]=32400
    [grafana]=3001
)

for name in "${!service_ports[@]}"; do
    echo "${name} -> ${service_ports[$name]}"
done
```

WHY associative arrays over parallel indexed arrays: parallel arrays (`names[0]`, `ports[0]`) drift when items are added or removed. Associative arrays bind key to value atomically.

**Building command arguments:**

```bash
local -a curl_args=(
    --silent
    --fail
    --max-time 30
    --header "Authorization: Bearer ${token}"
)

if [[ "${verbose:-}" == "1" ]]; then
    curl_args+=(--verbose)
fi

curl "${curl_args[@]}" "$url"
```

WHY: building commands with string concatenation breaks on arguments containing spaces, quotes, or glob characters. Arrays preserve argument boundaries exactly.

**Array length and emptiness:**

```bash
# Length
echo "${#files[@]} files found"

# Emptiness check
if (( ${#files[@]} == 0 )); then
    echo "error: no files found" >&2
    exit 1
fi
```

### No dead weight

- No commented-out code
- No unused variables (`set -u` catches these)
- No `echo` for debugging in committed scripts: use a `debug()` function gated on a flag

---

## Testing with bats

For projects with shell scripts beyond wrappers:

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

1. **Missing `set -euo pipefail`**: every script, no exceptions
2. **Unquoted variables**: always `"$var"`
3. **`[ ]` instead of `[[ ]]`**: double brackets are safer
4. **`echo` for error messages**: errors go to stderr: `>&2`
5. **No `trap` cleanup**: temp files leak
6. **`|| true` without comment**: hiding failures
7. **Parsing `ls` output**: use globs or `find`
8. **`cat file | grep`**: `grep pattern file` directly
9. **Hardcoded paths**: use variables or `$0`-relative paths
10. **Missing `local` in functions**: variables leak to global scope
11. **Manual temp file paths**: use `mktemp`, never `/tmp/myapp.$$`
12. **`${{ }}` interpolation in GitHub Actions `run:`**: pass through `env:` instead
13. **No signal traps**: scripts that create resources must `trap cleanup EXIT INT TERM`
14. **String-packed lists instead of arrays**: `files="a b c"` breaks on spaces; use `files=(a b c)`
15. **Unquoted `${array[@]}`**: always `"${array[@]}"` to preserve element boundaries
16. **Per-file shellcheck disables**: suppressions go in `.shellcheckrc` or inline with WHY comments, never blanket-disabled at file top
17. **`--exclude` in CI shellcheck invocation**: fix the code or document the suppression properly
