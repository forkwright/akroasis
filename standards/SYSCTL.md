# Sysctl

> Standards for Linux kernel sysctl configuration files (`/etc/sysctl.d/*.conf`, `/etc/sysctl.conf`).
> Covers naming, security baselines, validation, and documentation.

---

## File locations

- `/etc/sysctl.d/*.conf` - preferred location for drop-in configurations
- `/etc/sysctl.conf` - legacy single file (avoid for new settings)

## Naming

Files in `/etc/sysctl.d/` must use the `NN-name.conf` convention:

```
/etc/sysctl.d/
├── 10-console-messages.conf
├── 50-security-hardening.conf
└── 99-custom.conf
```

The numeric prefix controls load order. Lower numbers load first. Later files can override earlier ones.

---

## Format

One parameter per line:

```
# WHY: Prevent SYN flood attacks by enabling TCP syncookies.
net.ipv4.tcp_syncookies = 1
```

- Parameter path on the left, value on the right
- Dots separate namespaces (`net.ipv4.tcp_syncookies`)
- Values are numeric (integers or booleans as `0`/`1`)
- Empty lines are ignored
- Comments start with `#`

---

## Security baselines

These parameters should be explicitly set in every security-hardening profile:

| Parameter | Safe value | Rationale |
|-----------|-----------|-----------|
| `net.ipv4.tcp_syncookies` | `1` | Mitigate SYN flood DoS |
| `fs.protected_symlinks` | `1` | Prevent symlink TOCTOU attacks |
| `fs.protected_hardlinks` | `1` | Prevent hardlink privilege escalation |
| `kernel.yama.ptrace_scope` | `1` or `2` | Restrict cross-process ptrace |
| `kernel.kptr_restrict` | `1` or `2` | Hide kernel pointer addresses |
| `net.ipv4.conf.all.send_redirects` | `0` | Disable ICMP redirect sending |
| `net.ipv4.conf.default.send_redirects` | `0` | Disable ICMP redirect sending |
| `net.ipv4.conf.all.accept_redirects` | `0` | Ignore ICMP redirect messages |
| `net.ipv4.conf.default.accept_redirects` | `0` | Ignore ICMP redirect messages |
| `net.ipv4.conf.all.log_martians` | `1` | Log spoofed, source-routed, or redirect packets |

### Network coherence

Redirect settings must be consistent across `all` and `default` interfaces:

```
# WHY: Ensure new and existing interfaces share the same redirect policy.
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
```

Setting one without the other is likely a mistake.

### Filesystem protection

`fs.protected_*` parameters must not be `0` on production systems:

```
fs.protected_symlinks = 1
fs.protected_hardlinks = 1
fs.protected_regular = 2
```

---

## Validation

### Boolean parameters

Parameters that are semantically boolean must use exactly `0` or `1`:

```
# Correct
net.ipv4.ip_forward = 0

# Wrong
net.ipv4.ip_forward = no
net.ipv4.ip_forward = off
```

### Value ranges

Parameters with documented kernel ranges must stay within them. Common ranges:

| Parameter | Valid range |
|-----------|-------------|
| `kernel.yama.ptrace_scope` | `0` - `2` |
| `kernel.kptr_restrict` | `0` - `2` |
| `fs.protected_regular` | `0` - `2` |

### Duplicate parameters

The same parameter must not appear more than once in a single file. The kernel applies the last value, but duplicates usually indicate a copy-paste error.

### Deprecated parameters

Avoid parameters removed in modern kernels. Examples:
- `kernel.exec-shield` (removed in Linux 5.7+)

---

## Comments

Non-obvious parameters must have a structured `WHY` comment on the preceding line:

```
# WHY: SYN cookies trade cryptographic strength for DoS resilience.
net.ipv4.tcp_syncookies = 1
```

Allowed tags follow the universal convention: `WHY`, `WARNING`, `NOTE`, `PERF`, `SAFETY`, `INVARIANT`, `TODO(#NNN)`, `FIXME(#NNN)`.

---

## Anti-patterns

| Anti-pattern | Problem | Fix |
|-------------|---------|-----|
| `sysctl.d` file without numeric prefix | Load order is undefined | Rename to `NN-name.conf` |
| Missing `tcp_syncookies` | Vulnerable to SYN floods | Add `net.ipv4.tcp_syncookies = 1` |
| `send_redirects = 1` | Enables routing redirect abuse | Set to `0` |
| `accept_redirects = 1` | Accepts routing redirect abuse | Set to `0` |
| `log_martians = 0` | Silent packet anomalies | Set to `1` |
| Duplicate parameter in same file | Later value silently wins | Remove duplicate |
| Boolean as `yes`/`no` | Kernel expects `0`/`1` | Use numeric booleans |
| Security parameter without comment | Rationale is lost | Add `WHY:` comment |
| Inconsistent `all`/`default` values | New interfaces behave differently | Set both |

---

## Cross-references

| Topic | Standard | Rules |
|-------|----------|-------|
| General comment conventions | STANDARDS.md | -- |
| Security principles | SECURITY.md | -- |
| Operations deployment | OPERATIONS.md | -- |
