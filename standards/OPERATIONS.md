# Operations

> Standards for service-specific concerns: runbooks, monitoring, backup, incident response, and operational readiness. If it runs in production, these rules apply.
>
> See also: DEPLOYMENT.md (gates, merge policy, rollback, health check requirements), SYSTEMD.md (unit files), PODMAN.md (containers), SECURITY.md (firewall, browser policy), NGINX.md (reverse proxy).

---

## Runbooks

Every deployed service has a runbook covering:

| Section | Contents |
|---------|----------|
| Architecture | What runs, what it depends on, how components connect |
| Start/stop | Exact commands for service lifecycle |
| Health check | How to verify the service is working |
| Common issues | Top 5 failure modes with resolution steps |
| Credential rotation | Step-by-step for each credential type |
| Database inspection | How to query, verify integrity, repair |
| Backup/restore | How to create, verify, and restore from backup |
| Performance debugging | How to identify and resolve latency/memory/CPU issues |
| Escalation | Who to contact, when to escalate |

A runbook is complete when an on-call engineer who has never seen the service can diagnose and resolve the top 5 issues without help.

---

## Monitoring

### Required health checks

See DEPLOYMENT.md for health check endpoint requirements (liveness, readiness, dependency). This section covers monitoring-specific metrics and alerting thresholds.

### Required metrics

| Metric | Type | Purpose |
|--------|------|---------|
| Request latency (p50, p95, p99) | Histogram | Performance regression detection |
| Error rate by type | Counter | Reliability tracking |
| Active connections/sessions | Gauge | Capacity planning |
| Dependency latency | Histogram | Bottleneck identification |
| Resource usage (CPU, memory, disk) | Gauge | Capacity planning |

### Alerting thresholds

Document alerting thresholds in the runbook. At minimum:
- Error rate > 5% sustained for 5 minutes
- p99 latency > 10x baseline for 5 minutes
- Disk usage > 80%
- Health check failure for 3 consecutive checks

---

## Backup

### Automated schedule

Every persistent data store has automated backups. No manual-only backup processes.

| Frequency | Retention | Verification |
|-----------|-----------|-------------|
| Daily | 7 days | Automated restore test weekly |
| Weekly | 4 weeks | Manual restore test monthly |
| Pre-upgrade | Until next successful upgrade | Verify before upgrading |

### Restore verification

A backup you've never restored from is not a backup. Test restores on a schedule. Document the restore procedure. Time the restore. Include the time in the runbook.

### What to back up

- Database files (SQLite, Postgres dumps)
- Knowledge stores (vector indices, graph databases)
- Configuration (encrypted credentials, TOML config)
- Agent state (workspace files, memory)

What NOT to back up: logs (ephemeral), build artifacts (reproducible), cache (rebuildable).

---

## Incident response

### Severity levels

| Level | Definition | Response time |
|-------|-----------|---------------|
| P0 | Service down, data loss risk | Immediate |
| P1 | Degraded, user impact | 1 hour |
| P2 | Degraded, no user impact | Next business day |
| P3 | Cosmetic or minor | Next sprint |

### Post-incident

Every P0/P1 incident gets a post-mortem within 48 hours:
- Timeline (what happened, when)
- Root cause (not "human error" but what made the error possible)
- Action items (what changes prevent recurrence)
- Owner and deadline for each action item

---

## Config validation

### Pre-flight checks

Before starting a service, validate not just config syntax but resource availability:

| Check | What | Why |
|-------|------|-----|
| Disk space | Data directories have sufficient free space | Prevents write failures mid-operation |
| Port availability | Listen ports are free | Prevents bind errors at startup |
| Credential validity | Auth tokens work (single probe request) | Prevents cryptic 401s after minutes of operation |
| Network reachability | External dependencies respond | Surfaces network issues before user traffic |

### Hot reload

File-based config should support hot reload:
1. File watcher (inotify/kqueue) with debounce (1 second minimum)
2. Config loaded and validated before applying
3. Diff against running config: only changed components restart
4. Rollback on validation failure (keep running config)
5. Log what changed and what restarted

### Environment variable interpolation

Config values support variable expansion: `${VAR}`, `${VAR:-default}`, `${VAR:?error}`.
Reject multiline values to prevent config injection.

## Observability patterns

### Internal events

Every significant internal event co-emits a structured log AND a metric counter. They are coupled, not separate concerns. Pattern:

```
event happens -> emit(InternalEvent) -> log at appropriate level + increment counter
```

This prevents drift between what logs say and what metrics measure.

### Circuit breaker

External service calls use a 3-state circuit breaker:
- **Closed**: normal operation
- **Open**: N consecutive failures, all calls rejected (backoff with jitter)
- **HalfOpen**: after backoff, probe single request. Success closes, failure reopens.

Prevents thundering herd on recovery.

### Adaptive concurrency

Long-running services adjust concurrent request limits based on response latency:
- Start at 1
- Increase gradually while latency stays within bounds
- Decrease on latency spikes (EWMA smoothing)
- Hard cap at configured maximum

### Backpressure

Every buffered channel has explicit capacity. When full:
- **Block** (default): upstream pauses, backpressure propagates
- **Drop**: excess events dropped, counter incremented

No unbounded channels in production.

---

## DNS

> Implementation details (AdGuard config, validation commands, bootstrap setup): `docs/deployment/DNS-SETUP.md`

### Principles

DNS is a single point of failure for the entire network. Treat it as critical infrastructure.

| Principle | Requirement | WHY |
|-----------|-------------|-----|
| Redundancy | Minimum two upstream resolvers | Single upstream failure takes down all resolution |
| Encryption | All upstream queries use DoH or DoT | Plaintext DNS leaks browsing data to ISP and intermediaries |
| DNSSEC | Enabled on all resolvers that support it | Prevents cache poisoning and spoofed responses |
| Internal resolution | All service hostnames resolve via DNS, not `/etc/hosts` | Hosts files don't propagate; DNS is the single source of truth |
| VPN accessibility | Internal rewrites point to Tailscale IPs, not LAN IPs | Services must be reachable from any Tailscale-connected device |
| Blocking mode | NXDOMAIN, not `0.0.0.0` or `127.0.0.1` | Prevents connection attempts and false positives from loopback services |

### Architecture

| Component | Role |
|-----------|------|
| DNS resolver (e.g., AdGuard Home) | Primary resolver for LAN and VPN clients |
| Upstream resolvers | DoH to two independent providers for redundancy and privacy |
| DHCP server | Points all LAN clients to the resolver |
| VPN DNS | Global nameserver override for VPN clients |

### Failure modes

| Symptom | Likely cause | Resolution |
|---------|-------------|------------|
| All resolution fails | Resolver process down or port 53 conflict | Check service status and `ss -tlnp \| grep ':53'` |
| Internal names fail, external works | DNS rewrites misconfigured or missing | Check resolver rewrite rules |
| External fails, internal works | Upstream resolvers unreachable or DoH cert issue | Test upstream DoH endpoints directly |
| Intermittent failures | systemd-resolved stub listener conflict | Disable `DNSStubListener` in `/etc/systemd/resolved.conf` |
| Blocked domains still resolve | Stale or failed filter lists | Verify filter list fetch status; lists fail silently |
| VPN clients can't resolve internal names | VPN DNS override not pointing to resolver | Check VPN nameserver configuration |

### systemd-resolved conflict

WHY: On Fedora (systemd 256+), `systemd-resolved` binds `127.0.0.54:53`, conflicting with the DNS resolver's wildcard bind. Set `DNSStubListener=no` in `/etc/systemd/resolved.conf`. This is a host-level concern -- containerized resolvers bind to the host network namespace via pod port mapping.

### Validation

After any DNS change, verify:
1. Internal resolution: `dig @<resolver-ip> <internal-hostname>`
2. External resolution: `dig @<resolver-ip> example.com`
3. NXDOMAIN blocking: blocked domain returns NXDOMAIN
4. No port conflict: single process on port 53

---

## Service management

### Container lifecycle

Containerized services managed by systemd follow this lifecycle:

```
create pod → create containers → start pod → health check → serve traffic
```

#### Pod architecture

Group containers into pods when they need to share network namespace (e.g., reverse proxy + DNS in same pod share `127.0.0.1`). Standalone containers for services with no inter-container localhost dependencies.

| Grouping | When |
|----------|------|
| Pod | Containers communicate over localhost (same network namespace) |
| Standalone | No localhost dependency on other containers |

#### Systemd unit naming

Container services use the pattern `<name>-container.service`. Pod services create the pod first, then add containers.

```bash
# Check all container services
sudo podman ps --format "table {{.Names}} {{.Status}}"

# Restart a service
sudo systemctl restart <name>-container

# View logs
sudo podman logs --tail 50 <container-name>
```

#### Auto-updates

Use `podman-auto-update.timer` for image-based auto-updates on a schedule. Pin the schedule to low-traffic windows (e.g., Sunday 4 AM). All containers using `io.containers.autoupdate=registry` label receive updates automatically.

#### Environment files

Service credentials live in environment files, not in systemd units or container command lines.

| Location | Permissions | Purpose |
|----------|------------|---------|
| `/etc/menos-*.env` | 0600 root:root | Service credentials read by systemd at start |
| `~/menos-ops/secrets/` | 0600 user | Backup passwords, API keys for scripts |

systemd reads `EnvironmentFile=` at service start. Credentials never appear in `podman inspect` or process listings.

### Health checks and upgrades

See DEPLOYMENT.md for container health check requirements and upgrade procedures (binary and container).
