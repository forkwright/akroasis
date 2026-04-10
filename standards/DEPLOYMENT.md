# Deployment

> Single authority for deployment gates, merge policy, release timing, rollback procedures, and health check requirements. All deployment decisions reference this document.
>
> See also: CI.md (tooling configuration), OPERATIONS.md (service-specific runbooks), WORKFLOW.md (dispatch orchestration), RELEASES.md (versioning and binary distribution).

---

## Deployment gates

A deployment gate is a condition that must be satisfied before code moves from one stage to the next. Gates are automated and enforced by the pipeline, not by convention.

### PR merge gate

Every PR must pass ALL checks before merge. No exceptions, no manual overrides.

| Gate | Source | Blocks merge |
|------|--------|:------------:|
| Format | CI.md (tooling) | Yes |
| Lint | CI.md (tooling) | Yes |
| Type check | CI.md (tooling) | Yes |
| Unit tests | CI.md (tooling) | Yes |
| Integration tests | CI.md (tooling) | Yes |
| Security scan | CI.md (tooling) | Yes |
| Commit lint | CI.md (tooling) | Yes |
| Size check | CI.md (tooling) | Yes |
| Dependency audit | CI.md (tooling) | Advisory (non-blocking for transitive) |

Check execution order: fast checks first. Fail fast, don't waste compute.

1. Format + lint (seconds)
2. Type check (seconds to minutes)
3. Unit tests (minutes)
4. Integration tests (minutes)
5. Security + dependency (parallel with tests)

### Dispatch QA gate

The dispatch pipeline applies an additional QA layer before merge eligibility. QA verdicts are control signals that drive the dispatch loop, not reports for humans to read.

| Verdict | Merge eligible | Next action |
|---------|:--------------:|-------------|
| PASS | Yes | Proceeds to merge policy evaluation |
| PARTIAL | No (held) | Corrective prompts auto-generated, dependents continue |
| FAIL | No (held) | Dependents blocked, diagnostic generated |

QA evaluation: per-criterion assessment against the prompt's acceptance criteria, evaluated from the PR diff. Details in WORKFLOW.md (verification stage).

---

## Merge policy

### Automated merge (dispatch pipeline)

The CI manager auto-merges PRs that satisfy both the PR merge gate and the dispatch QA gate. Merge decisions are tiered by blast radius and content:

| Condition | Action |
|-----------|--------|
| QA PASS + CI green + single-module blast radius | Auto-merge |
| QA PASS + CI green + multi-module blast radius | Merge, notify architect for observation triage |
| QA PARTIAL or hold flag | Hold for architect review |
| Touches public API surface | Hold for architect review |
| R-type prompt (research, no PR) | Move prompt to done/ |

### Manual merge

PRs not created by the dispatch pipeline follow standard review:

- Require CI pass (all gates above)
- Require 1 approval (human or bot for automated PRs)

### Branch protection

| Rule | Applies to |
|------|-----------|
| Require PR for all changes | main |
| Require CI pass | main |
| No force push | main |
| No merge commits (rebase or squash only) | main |
| Branch auto-delete after merge | all feature branches |

---

## Release timing

### Wave-based releases

Version bumps happen per-wave, not per-PR. A wave is a coherent batch of work with a unifying theme. See RELEASES.md for versioning policy and changelog format.

Exceptions that get immediate PATCH bumps:
- Security fixes
- Critical runtime bugs (data loss, crash on startup)

### release-please cadence

release-please runs on an hourly schedule, not on every push to main. During active dispatch batches, 10-30 PRs/hour land; per-push runs waste CI minutes and create noise. The hourly run picks up all accumulated commits and updates a single release PR.

The release-please PR is the version gate. It is never auto-merged. Operator sign-off required.

### Deployment windows

Container auto-updates via `podman-auto-update.timer`: Sundays 4 AM. Pin auto-update schedules to low-traffic windows to contain blast radius of bad updates.

---

## Upgrade procedure

### Binary services

1. Back up (verify backup integrity before proceeding)
2. Stop service
3. Replace binary (verify checksum against published SHA256)
4. Start service
5. Health check (automated, with timeout)
6. Smoke test (one real request through the system)

### Container services

1. Pull new image (`podman pull`)
2. Stop container (`systemctl stop <name>-container`)
3. Remove old container (`podman rm <name>`)
4. Recreate with same volume mounts and network config
5. Start (`systemctl start <name>-container`)
6. Health check
7. Verify logs for startup errors

For auto-updated containers, steps 1-5 happen via `podman auto-update`. Verify health after the timer fires.

### Zero-downtime (when applicable)

For services requiring uptime:
- Blue-green deployment OR rolling restart
- Health check gates before traffic shift
- Automatic rollback on health check failure

---

## Rollback

Every deployment is rollback-safe. The rollback procedure is tested and documented.

### Binary rollback

1. Stop service
2. Swap binary (previous version preserved at known path or in prior GitHub release)
3. Start service
4. Health check

### Container rollback

1. Stop container
2. Remove container
3. Recreate with previous image tag
4. Start container
5. Health check

### Database rollback

Database migrations are forward-only. Rollback SQL is documented per migration but treated as emergency-only. Design migrations to be backward-compatible where possible.

### Requirements

- Previous binary/image preserved (not overwritten)
- Database migrations have documented rollback SQL
- Rollback procedure tested against actual deployment

---

## Health checks

Health checks serve two purposes: deployment readiness validation and ongoing service monitoring. This section covers what health checks a deployable service must expose. OPERATIONS.md covers monitoring dashboards, alerting thresholds, and runbook-level health verification.

### Required endpoints

Every long-running service exposes:

| Endpoint | Purpose | Failure meaning |
|----------|---------|----------------|
| Liveness | Is the process running? | Restart the service |
| Readiness | Can it handle requests? | Remove from load balancer, do not route traffic |
| Dependency | Are database, cache, external APIs reachable? | Investigate upstream, do not deploy dependents |

### Pre-deployment validation

Before starting or upgrading a service, validate resource availability:

| Check | What | Why |
|-------|------|-----|
| Disk space | Data directories have sufficient free space | Prevents write failures mid-operation |
| Port availability | Listen ports are free | Prevents bind errors at startup |
| Credential validity | Auth tokens work (single probe request) | Prevents cryptic 401s after minutes of operation |
| Network reachability | External dependencies respond | Surfaces network issues before user traffic |

### Container health checks

Every long-running container defines a health check. Services behind a reverse proxy additionally have the proxy verify backend health.

```bash
# Container-level health
podman healthcheck run <container-name>

# Service-level health
curl -sf http://localhost:<port>/health || exit 1
```

---

## Fleet management

The dispatch fleet runs multi-provider AI agents. Deployment concerns specific to fleet operations:

### Provider routing

Provider selection is derived from per-provider success rate data, not manually maintained routing tables. See WORKFLOW.md for dispatch orchestration details.

### Fleet health

Fleet health is aggregated to `/tmp/dispatch-fleet-health.json`. The steward daemon monitors PR status and takes corrective action (fix agents on failure, auto-merge on green).

### Worktree isolation

Every worker operates in a dedicated git worktree. No shared mutable state between workers. Worktrees are created per-prompt and cleaned up post-merge. This is a deployment invariant: parallel execution depends on isolation.

---

## Principles

**Gates are automated.** If a human must remember to check something before merge, it is not a gate. Gates are pipeline stages that block progression programmatically.

**Derive deployment config.** Blast radius derives from cargo metadata. Routing derives from success rates. Gate requirements derive from CI tooling configuration. Manually maintained deployment checklists drift from the actual pipeline. See STANDARDS.md (derive, don't maintain).

**Rollback is not optional.** Every deployment must have a tested rollback path. "We'll fix forward" is not a rollback plan. The cost of preserving the previous artifact is negligible; the cost of not having it during an incident is unbounded.

**Health checks gate traffic, not just uptime.** A service that is running but cannot handle requests must not receive traffic. Liveness and readiness are distinct signals with distinct responses.
