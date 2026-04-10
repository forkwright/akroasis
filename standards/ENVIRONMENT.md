# Environment Configuration

> Standards for environment variables, configuration files, and runtime settings. Defines how values vary across deploy contexts and how configuration is discovered, loaded, and validated.

---

## Guiding principles

**Configuration is data, not code.** Values that change between environments (credentials, hostnames, feature flags) live in configuration, not source. The same binary runs in dev, staging, and production with different configuration.

**Hierarchical configuration.** Configuration layers from most general (compiled defaults) to most specific (CLI flags). Each layer overrides the previous. This enables shared defaults with environment-specific overrides without duplication.

**Explicit over implicit.** Configuration paths, variable names, and default values are explicit and documented. No hidden defaults, no surprise environment variable mappings.

**Fail fast on invalid config.** Configuration is validated at startup. Missing required values, invalid paths, or type mismatches produce clear errors immediately, not at first use.

**Secrets are configuration, but special.** Secrets follow the same hierarchy but are handled with additional care: never logged, never committed, rotated automatically when possible.

---

## Configuration hierarchy

Configuration loads in five layers, from lowest to highest precedence:

```
Layer 1: Compiled defaults
    ↓
Layer 2: Global config (~/.config/kanon/config.toml)
    ↓
Layer 3: Repo-local config (workflow/kanon.toml)
    ↓
Layer 4: Environment variables (KANON_*)
    ↓
Layer 5: CLI flags
```

Higher layers override lower layers. Environment variables override file config. CLI flags override everything.

### Layer 1: Compiled defaults

Default values embedded in the binary. Every configuration field has a default. These defaults are intentionally conservative and safe for development.

```rust
fn default_model() -> String { "claude-opus-4-6".to_string() }
fn default_state_dir() -> PathBuf { dirs::home_dir().unwrap().join(".dispatch") }
```

### Layer 2: Global config

User-specific settings that persist across repository clones. Lives at:

- Linux: `~/.config/kanon/config.toml`
- macOS: `~/Library/Application Support/kanon/config.toml`

Override path via `KANON_GLOBAL_CONFIG` environment variable.

Use for: personal preferences, API keys, default models, local paths that don't vary by project.

### Layer 3: Repo-local config

Project-specific settings. Lives at `workflow/kanon.toml` relative to the kanon repository root.

Use for: project definitions, per-project paths, gate command overrides, feature flags specific to this deployment.

### Layer 4: Environment variables

Runtime overrides. Variables use `KANON_` prefix with double-underscore separator for nested keys:

| Environment variable | Maps to config field |
|---------------------|---------------------|
| `KANON_MODEL` | `model` |
| `KANON_MODEL_QA` | `model_qa` |
| `KANON_STATE_DIR` | `state_dir` |
| `KANON_DISPATCH__MAX_PARALLEL` | `dispatch.max_parallel` |
| `KANON_PROJECTS__ALEtheia__REPO_DIR` | `projects.aletheia.repo_dir` |

The `__` (double underscore) separates struct fields. Single `_` is part of the field name.

### Layer 5: CLI flags

Highest precedence. CLI arguments override all other sources. Used for one-off overrides and scripting.

---

## Required environment variables

No environment variables are strictly required for basic operation. The system uses compiled defaults and discovers paths when possible. However, production deployments typically set:

| Variable | Purpose | When required |
|----------|---------|---------------|
| `KANON_ROOT` | Path to kanon repository | When running outside the repo |
| `KANON_GLOBAL_CONFIG` | Override global config path | Testing, isolated environments |

## Optional environment variables

### Core configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `KANON_MODEL` | `claude-opus-4-6` | Default model for agent sessions |
| `KANON_MODEL_QA` | `claude-sonnet-4-6` | Model for QA evaluation |
| `KANON_EFFORT` | `high` | Agent effort level (low/medium/high) |
| `KANON_STATE_DIR` | `~/.dispatch` | Runtime state directory (SQLite, logs) |

### Dispatch configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `KANON_DISPATCH__MAX_PARALLEL` | `10` | Maximum concurrent sessions per group |
| `KANON_DISPATCH__WORKTREE_BASE` | `/data/worktrees` | Base directory for git worktrees |
| `KANON_DISPATCH__TARGET_DIR` | `/data/target` | Shared cargo target directory |
| `KANON_DISPATCH__SKIP_QA` | `false` | Skip QA gate evaluation |
| `KANON_DISPATCH__DETACH` | `true` | Run sessions in detached process group |

### Provider configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `KANON_PROVIDER__DEFAULT` | `claude-code` | Default agent provider |
| `KANON_PROVIDER__PROXY_URL` | (none) | LLM proxy URL for multi-backend routing |
| `KANON_PROVIDER__PROXY_API_KEY` | (none) | API key for LLM proxy authentication |

### Steward configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `KANON_STEWARD__CI_MODE` | `github` | CI validation mode (`github` or `local`) |
| `KANON_STEWARD__REACTIONS__AUTO_MERGE` | `true` | Enable auto-merge handler |
| `KANON_STEWARD__REACTIONS__FIX_CI` | `true` | Enable CI-fix handler |

---

## Path resolution

### Variable expansion

Configuration supports limited variable expansion in path values:

| Pattern | Expansion |
|---------|-----------|
| `~/path` | `$HOME/path` |
| `$HOME/path` | Home directory |
| `$KANON_ROOT/path` | Kanon repository root |

Expansion happens after configuration loading. The raw value is stored; expansion occurs when the path is used.

### Discovery order

Kanon root discovery (for finding `workflow/kanon.toml`):

1. Walk up from current directory looking for `projects/` and `workflow/kanon.toml`
2. `KANON_ROOT` environment variable
3. Error if neither succeeds

Project repo directories (per-project `repo_dir`):

1. Use configured path from `kanon.toml`
2. Expand variables (`~`, `$HOME`, `$KANON_ROOT`)
3. Validate directory exists on use

---

## Feature flags

Runtime feature flags enable experimental functionality without code changes. Flags live in the `[features]` section of config or via `KANON_FEATURES__*` variables.

| Flag | Variable | Default | Description |
|------|----------|---------|-------------|
| `cron_scheduler` | `KANON_FEATURES__CRON_SCHEDULER` | `false` | Cron-based scheduler for recurring jobs |
| `progress_streaming` | `KANON_FEATURES__PROGRESS_STREAMING` | `false` | Stream progress events from sessions |
| `tool_whitelisting` | `KANON_FEATURES__TOOL_WHITELISTING` | `false` | Restrict MCP tool visibility |
| `coordinator_fanout` | `KANON_FEATURES__COORDINATOR_FANOUT` | `false` | Fan out coordinator work |
| `microcompaction` | `KANON_FEATURES__MICROCOMPACTION` | `false` | Compact SQLite during idle periods |
| `dispatch_local` | `KANON_FEATURES__DISPATCH_LOCAL` | `false` | Enable local LLM provider support |

All flags default to `false` (conservative). Enable explicitly in production when feature is stable.

---

## Secrets handling

Secrets are environment variables or configuration values that grant access to external systems.

### Rules

1. **Never commit secrets.** API keys, tokens, and passwords live in environment variables or secret stores, never in git.

2. **Use secret types.** In code, use dedicated secret types that prevent accidental exposure:
   - Rust: `secrecy::SecretString`
   - Never implement `Display` for types containing secrets
   - Redact in `Debug` output: `[REDACTED]`

3. **No secret defaults.** Secret configuration fields have no defaults. Missing secrets produce clear errors at startup.

4. **Log carefully.** Never log secret values, even at trace level. Log the presence/absence of secrets, not their content.

### Required secrets

| Secret | Environment variable | Used for |
|--------|---------------------|----------|
| LLM API key | `ANTHROPIC_AUTH_TOKEN` | Claude Code sessions |
| LLM proxy key | `KANON_PROVIDER__PROXY_API_KEY` | Proxy authentication |
| GitHub token | `GITHUB_TOKEN` | PR operations, steward |

---

## Configuration validation

Configuration validation occurs at load time. Invalid configuration produces errors with specific messages.

### Validation rules

- **Required directories exist:** `state_dir`, project `repo_dir` paths are validated
- **Project prefixes unique:** No two projects share the same 2-3 character prefix
- **Prefixes well-formed:** 2-3 lowercase ASCII letters only
- **References valid:** Standards files referenced in config exist on disk

### Error messages

Error messages explain what is wrong and how to fix it:

```
project "foo": prefix "f" must be 2-3 lowercase ASCII letters
state directory is not accessible: /data/dispatch
project repo directory does not exist: /home/user/missing-project
```

---

## Environment-specific patterns

### Development

Use global config for personal preferences. Local config for project paths. Minimal environment variables.

```toml
# ~/.config/kanon/config.toml
model = "claude-sonnet-4-6"
effort = "medium"

[dispatch]
max_parallel = 4
```

### CI/CD

Use environment variables for all configuration. No persisted config files. Explicit and reproducible.

```bash
export KANON_MODEL="claude-sonnet-4-6"
export KANON_DISPATCH__MAX_PARALLEL="2"
export KANON_STEWARD__CI_MODE="local"
```

### Production

Use repo-local config for stable settings. Environment variables for secrets and host-specific overrides.

```toml
# workflow/kanon.toml (committed)
[dispatch]
max_parallel = 20
worktree_base = "/data/worktrees"

[features]
cron_scheduler = true
```

```bash
# /etc/kanon/environment (not committed)
KANON_PROVIDER__PROXY_API_KEY="sk-..."
ANTHROPIC_AUTH_TOKEN="sk-ant-..."
```

---

## Anti-patterns

1. **Hardcoded paths.** Never hardcode paths to user directories or system locations. Use configuration.

2. **Environment variables for logic.** Use feature flags for behavioral changes, not environment variable presence checks.

3. **Silent defaults for secrets.** Secrets should error if missing, not silently use a default.

4. **Complex environment variable parsing.** Don't implement custom parsing for environment variables. Use the configuration system's type coercion.

5. **Configuration in code.** If it varies by environment, it belongs in configuration, not `#[cfg(...)]` or conditional compilation.

---

## Debugging configuration

To see effective configuration, run with debug logging:

```bash
RUST_LOG=debug kanon config show
```

This outputs the merged configuration from all layers (with secrets redacted).

To test configuration without side effects:

```bash
kanon config validate
```

Returns exit code 0 if configuration is valid, non-zero with error messages otherwise.
