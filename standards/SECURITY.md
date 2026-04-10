# Security

> Standards for secure code, credential handling, input validation, threat mitigation, firewall management, and browser policy. Applies to all languages and all codebases.
>
> See also: OPERATIONS.md (DNS configuration, service management), SYSTEMD.md (unit hardening), PODMAN.md (container security).

---

## Credentials

### Storage

- Never store credentials in source code, config files, or environment variable defaults
- Credential files: 0600 permissions (owner-read-only). Verify on write, not just creation.
- Use `SecretString` (or language equivalent) for in-memory credential handling. Zeroize on drop.
- No credentials in log output. Redact at the tracing layer, not at each call site.

### Rotation

- OAuth tokens: auto-refresh before expiry. Log refresh failures at WARN.
- API keys: support multiple active keys for zero-downtime rotation.
- JWT signing keys: document rotation procedure in runbook. Never embed in service files.

### Transmission

- TLS for all credential transmission. No HTTP fallback.
- Token prefix in logs (first 8 chars) for debugging. Never full token.
- Credential source (oauth, api-key, file) logged at INFO on startup.

---

## Secret type discipline

Secrets must be distinguishable from ordinary strings at the type level. A plain `String` holding an API key is a bug: it can be logged, serialized, compared in variable time, and will persist in memory after the value is no longer needed.

WHY: Every secret leak in production traces back to a secret that was indistinguishable from regular data at some point in the call chain. Typed wrappers make misuse a compile error (or at minimum a visible violation), not a silent runtime behavior.

### Requirements

| Requirement | Rule |
|-------------|------|
| Typed wrapper | Every secret has a dedicated type. Rust: `secrecy::SecretString` or a newtype around it. Other languages: equivalent opaque wrapper. Never bare `String`, `str`, `[]byte`. |
| Zeroize on drop | Secret memory is overwritten when the value is dropped. Rust: derive or implement `Zeroize` + `ZeroizeOnDrop`. This is not optional for keys, tokens, or passwords. |
| No `Display` | Secret types must not implement `Display`. Accidental interpolation in format strings, log lines, or error messages is the most common leak vector. |
| Redacted `Debug` | Implement `Debug` manually, emitting `[REDACTED]`. Derive-based `Debug` prints field contents. |
| Explicit expose | Accessing the inner value requires an explicit call (`expose_secret()`, not `Deref`). This makes every use site auditable via grep. |
| Constant-time comparison | Use `subtle::ConstantTimeEq` (Rust) or equivalent for secret comparison. Standard `==` leaks secret length and content via timing. |
| No serialization | Secret types must not implement `Serialize`. Secrets should never appear in JSON responses, state dumps, or telemetry payloads. If serialization is needed for storage, use an explicit encryption step. |

WHY: Each rule targets a specific leak vector. `Display` causes log leaks. `Serialize` causes API response leaks. `Deref` causes implicit coercion leaks. Variable-time `==` causes timing side-channels. Zeroize prevents memory forensics. The discipline is cumulative: omitting any single rule reopens the vector it guards.

### Language-specific guidance

**Rust**: Use `secrecy::SecretString` (or `koina::SecretString` if available in the workspace). `SecretString` and `Secret<T>` provide zeroize-on-drop, no Display, and `expose_secret()` gating out of the box. Combine with `subtle::ConstantTimeEq` for comparisons. See RUST.md for crate details.

**Python**: Create a `SecretStr` class that overrides `__str__` and `__repr__` to return `[REDACTED]`. Use pydantic's `SecretStr` as a reference implementation. Python cannot guarantee zeroize (GC controls lifetime), but the wrapper prevents accidental logging. Use `hmac.compare_digest()` for constant-time comparison.

**TypeScript**: Wrap secrets in an opaque class whose `toString()` and `toJSON()` return `[REDACTED]`. Use `timingSafeEqual` from `node:crypto` for comparison.

**Go**: Use a struct with a private field and no `String()` method. Use `subtle.ConstantTimeCompare` for comparison. Zero the backing byte slice explicitly when done.

### Lint enforcement

The `RUST/plain-string-secret` basanos rule enforces secret type discipline at lint time. It flags any binding where the name contains `secret`, `password`, `token`, `key`, or `credential` and the type is bare `String`. This catches both struct fields and function parameters.

The rule is auto-fixable: it replaces `String` with `SecretString` in flagged positions. Projects should run `kanon lint` (or the language-equivalent linter) in CI to prevent plain-string secrets from merging.

### Examples

**Rust** — correct secret handling:

```rust
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq;

struct ServiceConfig {
    api_token: SecretString,     // NOT String
    password: SecretString,      // NOT String
    endpoint: String,            // non-secret, String is fine
}

fn authenticate(credential: &SecretString) {
    // Access requires explicit unwrap — every use site is auditable
    let header_value = format!("Bearer {}", credential.expose_secret());

    // Constant-time comparison
    let expected: &[u8] = b"expected_value";
    let matches = credential
        .expose_secret()
        .as_bytes()
        .ct_eq(expected)
        .into();
}

// Debug output prints [REDACTED], not the secret value
// Display is not implemented — format!("{}", token) won't compile
```

**Python** — correct secret handling:

```python
import hmac


class SecretStr:
    """Opaque wrapper that prevents accidental secret logging."""

    def __init__(self, value: str) -> None:
        self._value = value

    def get_secret_value(self) -> str:
        return self._value

    def __str__(self) -> str:
        return "[REDACTED]"

    def __repr__(self) -> str:
        return "SecretStr('[REDACTED]')"

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, SecretStr):
            return NotImplemented
        return hmac.compare_digest(self._value, other._value)


# Usage
api_token = SecretStr(os.environ["API_TOKEN"])
print(api_token)          # prints: [REDACTED]
logging.info("token=%s", api_token)  # logs: token=[REDACTED]
```

**TypeScript** — correct secret handling:

```typescript
import { timingSafeEqual } from "node:crypto";

class SecretString {
  readonly #value: string;

  constructor(value: string) {
    this.#value = value;
  }

  exposeSecret(): string {
    return this.#value;
  }

  toString(): string {
    return "[REDACTED]";
  }

  toJSON(): string {
    return "[REDACTED]";
  }

  equals(other: SecretString): boolean {
    const a = Buffer.from(this.#value);
    const b = Buffer.from(other.#value);
    if (a.length !== b.length) return false;
    return timingSafeEqual(a, b);
  }
}

// Usage
const apiToken = new SecretString(process.env.API_TOKEN!);
console.log(apiToken);              // prints: [REDACTED]
JSON.stringify({ token: apiToken }); // produces: {"token":"[REDACTED]"}
```

### Boundary rules

- Secrets enter the system as typed wrappers at the earliest possible point: config loading, environment variable parsing, or HTTP header extraction.
- Secrets leave the typed wrapper only at the point of use (e.g., setting an HTTP Authorization header). The unwrapped value must not be stored in an intermediate variable with a broader scope.
- Functions that accept secrets must accept the wrapper type, not the inner string. This pushes type safety through the entire call chain.

WHY: If a secret enters as `String` and is wrapped later, every line between entry and wrapping is an unguarded leak surface. Wrapping at the boundary means the leak surface is zero lines.

---

## Input validation

### Trust boundaries

Validate at system boundaries only. Internal function calls between trusted modules don't need re-validation.

| Boundary | Validate |
|----------|----------|
| HTTP request body | Schema, size limits, type coercion |
| CLI arguments | Format, range, existence |
| File paths from LLM/user | Canonicalize, check allowed roots, reject symlinks in sensitive paths |
| Database query parameters | Parameterized queries only. Never string concatenation. |
| Tool inputs from agents | Schema validation + path validation + size limits |

### Path validation

```
normalize -> check allowed_roots -> canonicalize -> re-check allowed_roots
```

This sequence catches symlink-based escapes. For writes to sensitive locations, use `O_NOFOLLOW` to prevent symlink following after validation.

### Size limits

Every input has a maximum size. Define it explicitly:

| Input | Limit |
|-------|-------|
| HTTP request body | Configurable (default 1MB) |
| Tool write content | Configurable (default 10MB) |
| Tool exec command | 10KB |
| Datalog query | 10KB |
| File read | 50MB |

Enforce server-side. Client-side limits are UX, not security.

---

## Sandboxing

### Defense in depth

No single security layer. Stack them:

1. **Filesystem** (Landlock on Linux): restrict read/write/exec to declared paths
2. **Syscalls** (seccomp): block dangerous syscalls (exec, mount, ptrace)
3. **Network** (namespace or firewall): restrict egress to declared destinations
4. **Process** (cgroups or rlimit): cap CPU, memory, open files, child processes

### Fail closed

If the sandbox can't be applied (old kernel, missing capability), deny the operation. Don't fall back to unsandboxed execution without explicit operator opt-in.

Log sandbox enforcement status at startup: ENFORCING, PERMISSIVE, or UNAVAILABLE.

---

## Session and identity

### Token generation

Use cryptographically random tokens (128+ bits of entropy). ULIDs (80 bits random + 48 bits time) are insufficient for security-sensitive identifiers when auth is disabled.

Prefer `uuid::Uuid::new_v4()` (128-bit random) or `rand::OsRng` with 256-bit output for session tokens.

### CSRF

State-changing endpoints (POST, PUT, DELETE, PATCH) require CSRF protection. When auth is disabled, CSRF should also be disabled (no circular dependency where the token is only available via an authenticated endpoint).

---

## Error messages

### To users/operators

Include: what failed, how to fix it. Exclude: stack traces, internal paths, database schema.

### To lLM/agents

Return generic "access denied" for path validation failures. Don't reveal whether a path exists. Don't include the rejected path.

### To logs

Full detail: stack trace, paths, parameters, timing. This is where debugging happens.

---

## Dependency supply chain

- `cargo audit` and `cargo-deny` (or language equivalents) run on every PR — fail on critical/high, warn on medium/low
- Known CVEs tracked in allow list with justification and review date
- No pre-1.0 crates with <1000 monthly downloads in critical paths
- Lockfiles committed for all binary crates
- Verify new dependency exists before adding (AI tools hallucinate package names)

---

## Audit

Every deployed system should have:
- Automated secret scanning (gitleaks, trufflehog) in CI
- Dependency vulnerability scanning (`cargo audit`, `cargo-deny`) in CI with severity-based gating
- Manual security review for: auth flows, credential handling, sandbox boundaries, input validation
- Documented threat model: what are we protecting, from whom, at what cost

---

## Plugin/extension security

### Capability-based access

Extensions declare capabilities in their manifest. The host enforces both:
1. What the extension CLAIMS to need (manifest)
2. What the user GRANTS (settings/permissions)

No capability is implicit. Extensions can't escalate privileges at runtime.

### WASM sandboxing

WASM plugins run in sandboxed runtimes (Wasmtime, wasmer). They cannot:
- Access the filesystem beyond preopened directories
- Make network calls except through host-provided imports
- Execute subprocesses except through declared capabilities
- Access memory outside their linear memory space

### Version embedding

Plugin API version is embedded in the WASM binary as a custom section, not just the manifest. This prevents version spoofing (manifest is editable, binary section is not).

### Async isolation

Plugins run on their own task, not the host thread. Communication is via message queue. A stalled plugin cannot freeze the host. Epoch-based yielding prevents infinite loops.

---

## Firewall

### nftables

nftables is the standard Linux firewall framework (successor to iptables). All firewall rules use nftables syntax.

#### Rule structure

```nft
table inet filter {
    chain input {
        type filter hook input priority 0; policy drop;

        # Allow established connections
        ct state established,related accept

        # Allow loopback
        iif "lo" accept

        # Allow SSH
        tcp dport 22 accept

        # Allow DNS (TCP + UDP)
        tcp dport 53 accept
        udp dport 53 accept

        # Allow HTTP/HTTPS
        tcp dport { 80, 443 } accept

        # Drop everything else (implicit via policy)
    }
}
```

#### Principles

| Principle | Rule |
|-----------|------|
| Default deny | `policy drop` on input chain. Allow only what is needed. |
| Stateful | Allow `established,related` connections. Don't re-evaluate every packet. |
| Specific | Match on port AND protocol. Don't allow TCP when only UDP is needed. |
| Documented | Comment every allow rule with the service name it enables. |
| Persistent | Save rules to `/etc/nftables.conf` or load via systemd. Unsaved rules vanish on reboot. |

#### Verification

```bash
# List active rules
nft list ruleset

# Test connectivity after rule changes
ss -tlnp                       # Verify listening ports
curl -sf http://localhost:80    # Verify allowed traffic
```

### CrowdSec integration

CrowdSec is a collaborative IDS that bans malicious IPs based on log analysis and community threat intelligence.

#### Architecture

```
logs → CrowdSec engine → decisions → nftables bouncer → firewall rules
```

| Component | Role |
|-----------|------|
| CrowdSec engine | Parses logs, matches scenarios, issues ban decisions |
| Community blocklists | Shared threat intelligence (~15K+ IPs) |
| nftables bouncer | Translates CrowdSec decisions into nftables rules |
| Local API | Connects bouncers to the engine |

#### Bouncer configuration

The nftables bouncer creates and manages its own nftables sets. Do not manually edit bouncer-managed sets.

```yaml
# /etc/crowdsec/bouncers/crowdsec-firewall-bouncer.yaml
mode: nftables
nftables:
  ipv4:
    table: crowdsec
    chain: crowdsec-chain
  ipv6:
    table: crowdsec6
    chain: crowdsec6-chain
```

NOTE: On Fedora, the CrowdSec nftables bouncer RPM may ship with iptables-era configuration paths. Verify the config references nftables explicitly after installation.

#### Operations

```bash
# Check CrowdSec decisions (active bans)
cscli decisions list

# Check bouncer status
cscli bouncers list

# Manually ban an IP
cscli decisions add --ip 1.2.3.4 --reason "manual ban"

# Remove a ban
cscli decisions delete --ip 1.2.3.4

# Update community blocklists
cscli hub update && cscli hub upgrade
```

---

## Browser policy

### Firefox policies.json schema

Firefox enterprise policies control browser behavior through a JSON configuration file. The schema follows Mozilla's enterprise policy specification.

#### File format validation

| Requirement | Rule | Validation |
|-------------|------|------------|
| Valid JSON | Must parse without errors | `python3 -m json.tool policies.json` |
| Root object | Must contain `policies` key | `grep '"policies"' policies.json` |
| Policy keys | Must be valid policy names | See Mozilla policy documentation |
| Value types | Must match policy schema | Strings for URLs, booleans for flags |

Schema example with all required fields:

```json
{
  "policies": {
    "DisableTelemetry": true,
    "DisableFirefoxStudies": true,
    "DisablePocket": true,
    "DNSOverHTTPS": {
      "Enabled": false,
      "Locked": true
    },
    "ExtensionSettings": {
      "*": {
        "installation_mode": "blocked"
      }
    }
  }
}
```

#### Required security policies

These policies must be set for controlled environments:

| Policy | Value | Rationale |
|--------|-------|-----------|
| `DisableTelemetry` | `true` | Prevents data exfiltration to Mozilla |
| `DisableFirefoxStudies` | `true` | Blocks automatic feature experiments |
| `DisablePocket` | `true` | Disables third-party integration |
| `DNSOverHTTPS.Enabled` | `false` | Forces system DNS resolver |
| `DNSOverHTTPS.Locked` | `true` | Prevents user override |
| `ExtensionSettings.*.installation_mode` | `"blocked"` | Default-deny for extensions |

Additional recommended policies:

| Policy | Value | Rationale |
|--------|-------|-----------|
| `PasswordManagerEnabled` | `false` | Enforce external password manager |
| `FirefoxHome.Search` | `false` | Disable search on new tab |
| `FirefoxHome.TopSites` | `false` | Disable top sites on new tab |
| `FirefoxHome.Highlights` | `false` | Disable highlights on new tab |
| `UserMessaging.UrlbarInterventions` | `false` | Disable interventions |
| `UserMessaging.FeatureRecommendations` | `false` | Disable recommendations |

#### Platform compatibility

Policy file locations vary by platform. Using the wrong location silently fails.

| Platform | Path | Notes |
|----------|------|-------|
| Linux (system-wide) | `/etc/firefox/policies/policies.json` | Applies to all users |
| Linux (user) | `~/.mozilla/firefox/<profile>/` | Not recommended; use system-wide |
| macOS (system) | `/Library/Preferences/org.mozilla.firefox.plist` | Convert JSON to plist |
| macOS (app bundle) | `/Applications/Firefox.app/Contents/Resources/distribution/policies.json` | Per-installation |
| Windows | `C:\Program Files\Mozilla Firefox\distribution\policies.json` | Or Group Policy ADMX |

Platform-specific warnings:

- **Linux**: Directory must be created: `sudo mkdir -p /etc/firefox/policies/`
- **macOS**: JSON format not natively supported at system level; use `plutil` to convert: `plutil -convert xml1 policies.json -o org.mozilla.firefox.plist`
- **Windows**: Group Policy overrides local file; check `gpresult /r` for conflicts

#### Extension management

Control which extensions can be installed. Extension IDs are UUIDs or email-style identifiers.

```json
{
  "policies": {
    "ExtensionSettings": {
      "*": {
        "installation_mode": "blocked"
      },
      "uBlock0@raymondhill.net": {
        "installation_mode": "force_installed",
        "install_url": "https://addons.mozilla.org/firefox/downloads/latest/ublock-origin/latest.xpi"
      },
      "{446900e4-71c2-419f-a6a7-df60251e0f8a}": {
        "installation_mode": "allowed"
      }
    }
  }
}
```

Extension ID formats:

| Format | Example | Source |
|--------|---------|--------|
| Email-style | `uBlock0@raymondhill.net` | `manifest.json` → `browser_specific_settings.gecko.id` |
| UUID | `{446900e4-71c2-419f-a6a7-df60251e0f8a}` | AMO-assigned for unbranded extensions |

Finding extension IDs:

```bash
# From installed extension (Linux)
cat ~/.mozilla/firefox/*.default/extensions/*.xpi | unzip -p - manifest.json | grep '"id"'

# From AMO page
# View source, search for "guid" in JSON-LD metadata
```

Installation modes:

| Mode | Behavior |
|------|----------|
| `blocked` | Cannot be installed |
| `allowed` | User can install manually |
| `force_installed` | Auto-install, user cannot remove |
| `normal_installed` | Auto-install, user can remove |

### Firefox DNS-over-HTTPS (DoH) bypass prevention

Firefox enables DNS-over-HTTPS by default, which bypasses the network's DNS resolver (AdGuard). This undermines DNS-based ad blocking, security filtering, and internal DNS rewrites.

#### Enterprise policy

Disable Firefox DoH via enterprise policy so the browser uses the system resolver:

```json
{
  "policies": {
    "DNSOverHTTPS": {
      "Enabled": false,
      "Locked": true
    }
  }
}
```

| Platform | Policy file location |
|----------|---------------------|
| Linux | `/etc/firefox/policies/policies.json` |
| macOS | `/Library/Preferences/org.mozilla.firefox.plist` or `/Applications/Firefox.app/Contents/Resources/distribution/policies.json` |
| Windows | Group Policy or `distribution/policies.json` in Firefox install dir |

`Locked: true` prevents users from re-enabling DoH in `about:preferences`. Without this, individual users can override the policy.

#### Verification

```bash
# Verify policy is loaded
# In Firefox: about:policies → should show DNSOverHTTPS Enabled=false

# Verify DNS queries go through system resolver
dig @<resolver-ip> example.com  # Should show AdGuard as resolver
```

#### Other browsers

Chromium-based browsers also support DoH. Disable via enterprise policy or managed preferences if running a controlled DNS environment. The key principle: no client should bypass the network resolver.

### Certificate management

For internal services using TLS with self-signed or internal CA certificates:

| Task | Method |
|------|--------|
| System trust store | Add CA cert to `/etc/pki/ca-trust/source/anchors/` then `update-ca-trust` (Fedora/RHEL) |
| Browser trust | Firefox uses its own trust store — import via `certutil` or enterprise policy `Certificates.Install` |
| Container trust | Mount CA cert into container and set `SSL_CERT_FILE` or add to container's trust store |

Never disable TLS verification (`--insecure`, `verify=False`) as a workaround for certificate issues. Fix the trust chain instead.

---

## DNS security

### Threat model

DNS is a high-value target. Compromise gives attackers the ability to redirect all traffic silently.

| Threat | Mitigation |
|--------|-----------|
| DNS spoofing | Use DoH/DoT for upstream queries (encrypted + authenticated) |
| Cache poisoning | DNSSEC validation on upstream resolvers |
| DNS exfiltration | Monitor query logs for anomalous patterns (long subdomains, high entropy) |
| Resolver hijacking | Firewall blocks outbound port 53 from all hosts except the resolver |
| Internal rewrite tampering | Restrict AdGuard admin access (auth required, not exposed externally) |

### Outbound DNS lockdown

Only the designated resolver should make outbound DNS queries. All other hosts on the network must use the resolver.

#### nftables rules

```nft
table inet filter {
    chain output {
        type filter hook output priority 0; policy accept;
        
        # Allow resolver host to reach external DNS
        ip saddr <resolver-ip> tcp dport 53 accept
        ip saddr <resolver-ip> udp dport 53 accept
        
        # Allow resolver host to reach DoH/DoT ports
        ip saddr <resolver-ip> tcp dport 443 accept
        ip saddr <resolver-ip> tcp dport 853 accept
        
        # Block all other hosts from outbound DNS
        ip saddr != <resolver-ip> tcp dport 53 drop
        ip saddr != <resolver-ip> udp dport 53 drop
    }
}
```

Replace `<resolver-ip>` with the actual IP address of your AdGuard/Pi-hole instance (e.g., `192.168.1.10`).

#### Validation

Verify the lockdown after applying rules:

```bash
# From resolver host: should work
dig @8.8.8.8 example.com

# From other host: should timeout/fail
dig @8.8.8.8 example.com
# Expected: connection timed out

# Check nftables counters
nft list chain inet filter output
# Look for packet counts on DNS drop rules
```

This prevents clients from bypassing the resolver by hardcoding external DNS servers (e.g., `8.8.8.8`).

### Query logging

Log DNS queries for security analysis and debugging. Retain query logs for at minimum 7 days. Query logs feed into the observability pipeline (see OPERATIONS.md § Observability patterns) for anomaly detection.

---

## Software Bill of Materials (SBOM)

Every released binary and container image must have a machine-readable SBOM attached. The SBOM lists every dependency (direct and transitive), its version, and its license. Without an SBOM, you cannot answer "are we affected?" when a CVE drops — you are reduced to grepping lockfiles across dozens of repos under time pressure.

WHY: An SBOM turns CVE triage from an emergency investigation into a database query. Regulators (NIST EO 14028, EU CRA) increasingly mandate SBOMs for software sold or deployed in critical infrastructure. Even without regulatory pressure, the operational value is immediate: you know what you ship.

### Format

| Requirement | Rule |
|-------------|------|
| Primary format | CycloneDX 1.5+ (JSON). CycloneDX is the default because it models vulnerabilities, services, and build metadata natively — not just package lists. |
| Secondary format | SPDX 2.3+ (JSON) when required by a downstream consumer or compliance framework. Generate both if needed; never skip CycloneDX to produce only SPDX. |
| Scope | Every direct and transitive dependency. Runtime, build-time, and dev dependencies tracked separately via CycloneDX component scopes. |
| Content | Component name, version, purl (package URL), license (SPDX expression), hash (SHA-256 minimum). Missing fields are a lint failure. |

WHY: purl enables cross-ecosystem vulnerability matching (a CVE references a purl, your SBOM contains purls — join on purl). Without purl, matching is string heuristics that miss or false-positive.

### Generation

Generate SBOMs in CI, not locally. Manual generation drifts from what was actually built.

| Language | Tool | Command |
|----------|------|---------|
| Rust | `cargo-cyclonedx` | `cargo cyclonedx --format json --all` |
| Python | `cyclonedx-bom` | `cyclonedx-py environment --output-format json -o sbom.cdx.json` |
| TypeScript | `@cyclonedx/cdxgen` | `cdxgen -o sbom.cdx.json` |
| Go | `cyclonedx-gomod` | `cyclonedx-gomod mod -json -output sbom.cdx.json` |
| Container images | `syft` | `syft <image> -o cyclonedx-json > sbom.cdx.json` |

For multi-language monorepos, generate one SBOM per deliverable artifact (binary, container image, published package), not one per workspace.

### Storage and distribution

- SBOMs are CI artifacts: attach to the release (GitHub release asset, container registry annotation, or artifact store).
- SBOMs are versioned with the artifact they describe. An SBOM without a matching artifact version is useless.
- Do not commit SBOMs to the source repo. They are build outputs, not source.

### Validation

Validate SBOM completeness in CI before publishing:

```bash
# Validate CycloneDX schema conformance
cyclonedx validate --input-format json --input-file sbom.cdx.json --fail-on-errors

# Check that every component has a purl and license
# (project-specific lint rule — add to kanon lint)
```

Treat validation failures as release blockers. An invalid SBOM is worse than no SBOM — it provides false confidence.

---

## CVE response SLAs

When a CVE affects a dependency in any shipped artifact, the response timeline is determined by severity. These SLAs measure time from awareness (CVE published or privately disclosed) to remediation deployed.

WHY: Without defined SLAs, CVE response is ad-hoc and severity-blind. A critical RCE and a low-severity information disclosure get the same treatment: "we'll get to it." SLAs force triage discipline and make response time auditable.

### Severity tiers

| Severity | CVSS range | Response SLA | Remediation SLA | Examples |
|----------|------------|--------------|-----------------|----------|
| Critical | 9.0 – 10.0 | 4 hours | 24 hours | RCE, auth bypass, sandbox escape |
| High | 7.0 – 8.9 | 24 hours | 7 days | Privilege escalation, data exposure |
| Medium | 4.0 – 6.9 | 7 days | 30 days | DoS, limited information disclosure |
| Low | 0.1 – 3.9 | 30 days | 90 days | Theoretical attacks, unlikely conditions |

- **Response** means: CVE triaged, affected artifacts identified (via SBOM query), impact assessed, and a tracking issue filed with owner assigned.
- **Remediation** means: patched dependency merged, CI green, and deployed to all affected environments. If a patch is not available upstream, remediation means applying a mitigation (disabling the affected feature, adding a workaround, or pinning to a non-vulnerable version) and documenting the residual risk.

WHY: "Response" and "remediation" are separate because they require different resources. Response is a triage function (one person, one hour). Remediation may require upstream patches, testing, and coordinated deployment. Conflating them makes the SLA either unachievable or meaningless.

### Process

1. **Detection**: `cargo audit` / `cargo-deny` / `osv-scanner` / GitHub Dependabot alerts / manual disclosure. At least one automated scanner must run on every PR and on a daily schedule against the default branch. CI must fail on critical/high advisories and warn on medium/low (see CI.md § Vulnerability scanning for severity thresholds).
2. **Triage**: Query SBOM to identify all affected artifacts. Determine actual exploitability (not all CVEs in a dependency are reachable). Record the determination.
3. **Track**: File a tracking issue with severity, affected artifacts, SLA deadline, and assigned owner. Link to the CVE and the SBOM query result.
4. **Remediate**: Update dependency, apply workaround, or accept risk (with documented justification and a review date no later than 90 days). If accepting risk, the acceptance must be reviewed by a second person.
5. **Verify**: Confirm the fix by regenerating the SBOM and re-running the vulnerability scanner. The CVE must not appear in the new scan.
6. **Communicate**: Notify affected downstream consumers if the vulnerability is in a published library or API.

### Exceptions

- If an upstream patch does not exist and no workaround is viable, document the gap, set a review cadence (minimum weekly for Critical/High), and escalate if the upstream patch SLA exceeds your remediation SLA by more than 2x.
- SLA clock pauses only for externally-blocked dependencies (waiting on upstream). Internal delays (backlog, prioritization) do not pause the clock.
- `cargo-deny` allow-list entries for known CVEs must include the justification, the date added, and a review-by date. Stale allow-list entries (past review-by date) are CI failures.

---

## Vulnerability disclosure

This section defines how to handle vulnerability reports received from external researchers and how to disclose vulnerabilities found internally.

WHY: A defined disclosure process protects researchers who report in good faith, gives maintainers a structured timeline to fix before public knowledge, and prevents the chaos of ad-hoc responses where legal, engineering, and communications improvise under pressure.

### Receiving reports

| Requirement | Rule |
|-------------|------|
| Contact method | Publish a `SECURITY.md` in the repo root (GitHub renders this on the Security tab) with an email address or a link to a private reporting mechanism (GitHub Private Vulnerability Reporting preferred). |
| Acknowledgment | Acknowledge receipt within 2 business days. Confirm the report is being investigated. Do not disclose details of the investigation. |
| Communication cadence | Update the reporter at least every 7 days until resolution. Silence erodes trust and incentivizes public disclosure. |
| Safe harbor | State explicitly that good-faith security research will not result in legal action. Researchers who follow the disclosure policy are acting in the interest of the project. |
| Scope | Define what is in scope (production services, published libraries, CLI tools) and what is out of scope (third-party dependencies with their own disclosure processes, test/staging environments). |

### Disclosure timeline

| Phase | Timeline | Action |
|-------|----------|--------|
| Report received | Day 0 | Acknowledge, assign handler, begin triage |
| Triage complete | Day 0 + 5 | Confirm or deny vulnerability, assess severity, estimate fix timeline |
| Fix developed | Day 0 + 45 | Patch ready, tested, reviewed |
| Coordinated disclosure | Day 0 + 90 | Public advisory published, CVE requested if applicable, fix released |
| Grace period | +14 days | If fix is not ready at day 90, negotiate extension with reporter. Maximum one extension of 14 days. After 104 days, the reporter may disclose publicly regardless. |

WHY: 90 days is the industry-standard coordinated disclosure window (Google Project Zero, CERT/CC). It balances giving maintainers time to fix with not leaving users exposed indefinitely. The 14-day grace period acknowledges that complex fixes sometimes need more time, but caps the extension to prevent indefinite delay.

### Publishing advisories

- Use GitHub Security Advisories (GHSA) for public projects. This automatically requests a CVE ID and notifies Dependabot users.
- Advisory must include: affected versions, fixed version, severity (CVSS), description of the vulnerability, description of the fix, and credit to the reporter (with their consent).
- For libraries: publish the advisory before or simultaneously with the patched release. Never publish a patched release silently — downstream consumers need the advisory to know they must update.
- For services: publish the advisory after the fix is deployed to all production environments. Disclosing before deployment creates a window of known-vulnerable exposure.

### Internal discovery

When a vulnerability is found internally (code review, automated scanning, testing):

1. File a private tracking issue immediately. Do not discuss in public channels.
2. Follow the same severity-based SLA as external CVEs (see CVE response SLAs above).
3. If the vulnerability affects downstream consumers (published crate, API), follow the coordinated disclosure timeline above, treating the internal discovery date as Day 0.
4. Request a CVE ID for any vulnerability in a published component, even if found internally. CVE IDs enable downstream consumers to track the issue in their own vulnerability management systems.

WHY: Internal discoveries that skip the formal process tend to get quiet fixes without advisories. This leaves downstream consumers vulnerable and unaware. The process is the same regardless of who finds the bug.
