# Releases

> Standards for versioning, release process, changelog format, and binary distribution. Applies to all shipped software across all forkwright repositories.

---

## Versioning

### SemVer 2.0

All projects follow [Semantic Versioning 2.0.0](https://semver.org/). Version format: `MAJOR.MINOR.PATCH`.

| Component | Meaning |
|-----------|---------|
| MAJOR | Incompatible API changes |
| MINOR | New functionality, backward-compatible |
| PATCH | Backward-compatible bug fixes |

### Pre-1.0 convention

Pre-1.0 projects use `0.MINOR.PATCH`:

| Bump | When | Examples |
|------|------|---------|
| PATCH (0.x.Y) | Bug fixes, security patches, doc fixes, test additions, lint compliance. No API or behavioral change. | Fix SQLite busy_timeout, fix TUI scroll, add missing_docs |
| MINOR (0.X.0) | New features, architectural changes, breaking internal API changes, crate splits, new crate additions. | Mneme split, desktop views, new tool types, embedding model upgrade |

MAJOR remains 0 until the project commits to a stable public API.

### Post-1.0 convention

| Bump | When |
|------|------|
| PATCH | Bug fixes, security patches, documentation corrections |
| MINOR | New features, deprecations (old API still works) |
| MAJOR | Removed or changed public API surface |

---

## When to bump

Version bumps happen **per-wave, not per-PR**. A wave is a coherent batch of work with a unifying theme (e.g. "Wave 10: Desktop + Mneme Split").

Exceptions:
- **Security fixes** get an immediate PATCH bump and release.
- **Critical runtime bugs** (data loss, crash on startup) get an immediate PATCH bump.

The version bump is a dedicated commit at the wave boundary:
1. Update `workspace.package.version` in root `Cargo.toml`
2. Let release-please handle CHANGELOG generation from conventional commits
3. Review and merge the release-please PR
4. Tag triggers binary build + publish

---

## Release process

### Automation via release-please

Every repository uses [release-please](https://github.com/googleapis/release-please) for automated release management. The flow:

1. Conventional commits land on main (`feat:`, `fix:`, `docs:`, etc.)
2. release-please opens (or updates) a single PR bumping version + updating CHANGELOG.md
3. Operator reviews the generated CHANGELOG, edits if needed, merges
4. Merge creates a git tag (`vX.Y.Z`)
5. Tag triggers `release.yml` workflow: build binaries, attach to GitHub release

release-please PRs are **never auto-merged**. They are the version gate requiring operator sign-off.

### Trigger frequency

Run release-please on an **hourly schedule**, not on every push to main. During active dispatch batches we merge 10-30 PRs/hour; running release-please per-push wastes CI minutes and creates noise. The hourly run picks up all accumulated commits and updates the single release PR.

Also enable `workflow_dispatch` for manual trigger when cutting a release immediately.

```yaml
on:
  schedule:
    - cron: "0 * * * *"
  workflow_dispatch:
```

### Changelog visibility

Show `feat`, `fix`, `perf`, `refactor`, and `docs` in the changelog. Hide `test`, `chore`, `ci`, `style`. Refactors and documentation are substantial work in this ecosystem (crate extractions, architecture changes, standards updates) -- hiding them misrepresents the release.

### Required configuration files

Every released repository must have:

| File | Purpose |
|------|---------|
| `release-please-config.json` | Package type, changelog sections, bump rules |
| `.release-please-manifest.json` | Current version (`{".": "X.Y.Z"}`) |
| `.github/workflows/release-please.yml` | Hourly schedule + workflow_dispatch trigger |
| `.github/workflows/release.yml` | Workflow that builds and publishes on tag creation |

### Version source of truth

`workspace.package.version` in the root `Cargo.toml` is the single version source. All crates in the workspace inherit it via `version.workspace = true`. Never version crates independently within a workspace.

---

## CHANGELOG format

release-please generates changelog entries from conventional commits. The format follows [Keep a Changelog](https://keepachangelog.com/):

```markdown
## [0.14.0] -- 2026-XX-XX

### Added
- Mneme crate split: eidos, krites, graphe, episteme
- Desktop app: 7 views with real functionality

### Changed
- theatron-core extracted from theatron-tui

### Fixed
- SQLite busy_timeout race under concurrent access

### Removed
- Legacy webchat shim
```

Sections used: **Added**, **Changed**, **Fixed**, **Removed**. Empty sections are omitted.

Commit types map to changelog sections:

| Commit type | CHANGELOG section |
|-------------|-------------------|
| `feat` | Added |
| `fix` | Fixed |
| `refactor`, `perf` | Changed |
| `docs`, `test`, `chore`, `ci` | Hidden (not in CHANGELOG) |
| `revert` | Removed or Changed (context-dependent) |

---

## Binary distribution

### Tarball structure

Each release produces tarballs, one per target:

```
aletheia-0.13.0-x86_64-unknown-linux-musl.tar.gz
├── aletheia              # Static binary
└── instance.example/     # Example config directory structure
```

Binary name matches the repository name. No version in the binary filename.

### Checksums

Every tarball has a matching SHA256 checksum file:

```
aletheia-0.13.0-x86_64-unknown-linux-musl.tar.gz.sha256
```

Checksum format: `<hash> <filename>` (two-space separator, matching `sha256sum` output).

### SBOM

Each release attaches a Software Bill of Materials in SPDX JSON format. Generated via `cargo sbom` or equivalent tooling.

---

## Target matrix

### Minimum (all shipped projects)

| Target | Runner | Method |
|--------|--------|--------|
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | cross |
| `aarch64-apple-darwin` | `macos-latest` | native (cargo build) |

musl produces fully static Linux binaries. macOS aarch64 covers Apple Silicon.

### Extended (when user demand exists)

| Target | Runner | Method |
|--------|--------|--------|
| `aarch64-unknown-linux-musl` | `ubuntu-latest` | cross |
| `x86_64-apple-darwin` | `macos-13` | native |

Add targets only when there are users on that platform. Do not speculatively build for platforms nobody uses.

---

## Named versions

Post-1.0, major versions get Greek names following the gnomon naming system. The name captures the essential character of the release, not a marketing slogan.

Pre-1.0 versions are numbered only.

---

## Rollback

Every release preserves the previous binary. The rollback procedure:

1. Stop service
2. Swap binary (previous version is at a known path or in the prior GitHub release)
3. Start service
4. Health check

Database migrations are forward-only. Rollback SQL is documented per migration but treated as emergency-only. Design migrations to be backward-compatible where possible.

---

## Emergency and hotfix releases

### When this applies

An emergency release is warranted when one or more of the following conditions exist:

| Condition | Examples |
|-----------|---------|
| Active security vulnerability | Exploitable CVE in a dependency, leaked credential requiring rotation, authentication bypass |
| Data loss or corruption risk | Migration bug destroying records, write-path silent truncation, backup integrity failure |
| Service-down with no workaround | Crash on startup, infinite loop on common input, dependency hard-failure |
| Severity P0 or P1 per OPERATIONS.md | See OPERATIONS.md incident response severity levels |

If the issue does not meet these criteria, it follows the normal wave-based release cadence. Do not use the emergency process for convenience.

WHY: The emergency process trades thoroughness for speed. Every use of it skips the wave boundary review that catches regressions. Overuse erodes the safety the normal process provides. Underuse leaves users exposed. The criteria above are the bright line.

Emergency releases ship immediately. They do not wait for in-progress waves to complete, for the hourly release-please schedule, or for unrelated PRs to merge. The hotfix is the priority.

### Hotfix process

Hotfixes branch from the latest release tag, not from main. This isolates the fix from unreleased work on main that has not been through the full release cycle.

#### Steps

1. **Create hotfix branch from the release tag.**
   ```bash
   git checkout -b hotfix/vX.Y.Z+1 vX.Y.Z
   ```

2. **Apply the minimal fix.** If the fix already exists as a commit on main, cherry-pick it onto the hotfix branch (`git cherry-pick <sha>`). If the fix does not exist yet, develop it directly on the hotfix branch. Either way, scope the change to the smallest diff that resolves the issue. No refactors, no drive-by improvements, no unrelated fixes. Every additional line is additional risk in an already high-risk situation.

3. **Run the full CI gate.** All PR merge gates from DEPLOYMENT.md apply. The emergency process does not skip CI. If CI is broken, fix CI first.

WHY: An untested hotfix has a meaningful probability of making the incident worse. The time spent on CI is insurance against a second incident while the first is still open. The only thing worse than a P0 is two P0s.

4. **Bump version as an immediate PATCH.** Update `workspace.package.version` in root `Cargo.toml`. The version bump commit uses the conventional commit format: `fix: <description of the security/stability issue>`.

5. **Tag and release from the hotfix branch.** Manually trigger the release-please workflow (`workflow_dispatch`) rather than waiting for the hourly schedule. Verify the release artifacts are published and checksums match.

6. **Backport to main.** Cherry-pick the fix into main per the backport process below.

7. **Cherry-pick to any active release branches.** If other release branches exist, apply the fix there too. Do not leave known vulnerabilities in any supported version.

#### What the hotfix process does NOT change

- CI gates are not relaxed.
- Branch protection is not bypassed.
- Auto-merge is not used. Operator sign-off is required.
- The release-please PR still requires review.

WHY: Every gate that exists in the normal process exists because its absence caused an incident in the past or would foreseeably cause one. An emergency is the worst time to disable safety mechanisms.

### Backport to main

After the emergency release ships, the fix must land on main. The hotfix branch diverged from a release tag, not from main, so main does not have the fix.

#### When main is close to the release tag

Cherry-pick the fix commit(s) from the hotfix branch into a PR against main:

```bash
git checkout -b backport/hotfix-vX.Y.Z+1 origin/main
git cherry-pick <fix-commit-sha>
```

The backport PR follows normal merge gates. It does not use the emergency process.

#### When main has diverged significantly

If main has accumulated substantial changes since the release tag, a clean cherry-pick may not apply. In this case:

1. Create a new branch from main.
2. Re-implement the fix against the current main state.
3. Open a PR with the original hotfix PR cross-referenced.

The fix may look different on main -- different function signatures, different module structure. The intent is identical. The backport PR body must reference the original emergency release for traceability.

#### CHANGELOG marker

Document the emergency release in CHANGELOG.md with an explicit marker so it is distinguishable from normal wave releases:

```markdown
## [0.5.2] -- 2026-XX-XX [EMERGENCY]

### Fixed
- <description of the fix>

_Emergency release: hotfix from v0.5.1 for [P0 description]._
```

The `[EMERGENCY]` tag in the version header aids post-incident review and release auditing.

### Dispatch interaction

Emergency releases operate on a separate branch from the dispatch pipeline. The two processes run independently with specific coordination rules.

#### In-flight dispatches

Dispatches in progress continue against main. They are not rebased onto, blocked by, or redirected to the hotfix branch. The hotfix branch is short-lived and merges back to main via the backport process. Workers that started before the emergency continue uninterrupted.

WHY: Redirecting in-flight dispatches to a hotfix branch would require restarting sessions, invalidating worktrees, and re-evaluating QA verdicts. The cost exceeds the benefit. The backport merge into main resolves the divergence naturally.

#### Steward priority

The steward merges the hotfix backport PR with priority over queued dispatch PRs. If the steward is polling and both a hotfix backport PR and dispatch PRs are green, the hotfix backport PR merges first. Dispatch PRs re-validate against the new main HEAD after the hotfix lands.

WHY: The hotfix fixes a P0/P1 issue. Dispatch PRs are feature work. Delaying the hotfix merge to preserve dispatch PR ordering defeats the purpose of the emergency process. Dispatch PRs that conflict with the hotfix get rebased by the fix agent, which is the normal conflict resolution path.

### Rollback procedures

Rollback is the first response to a bad release, not a last resort. If a release causes a P0/P1 incident and the fix is not immediately obvious, roll back first, then diagnose.

#### Decision framework

| Situation | Action |
|-----------|--------|
| Root cause identified, fix is < 20 lines, CI can run in < 15 min | Hotfix forward |
| Root cause unclear or fix is complex | Roll back, then investigate |
| Database migration has run | Assess: if migration is backward-compatible, roll back binary only. If not, execute documented rollback SQL under operator supervision. |
| Multiple services affected | Roll back in reverse dependency order (leaf services first, shared dependencies last) |

WHY: The instinct during an incident is to push forward -- "we're almost there, just one more fix." This instinct produces cascading failures. Rolling back restores known-good state immediately. Diagnosis under pressure with a broken system running produces worse fixes than diagnosis at leisure with the system stable.

#### Binary rollback

1. Stop service.
2. Replace binary with previous version (preserved at known path or downloaded from prior GitHub release).
3. Start service.
4. Run health check. Verify liveness, readiness, and dependency endpoints per DEPLOYMENT.md.
5. Run smoke test: one real request through the full path.

#### Container rollback

1. Stop container (`systemctl stop <name>-container`).
2. Remove container (`podman rm <name>`).
3. Recreate with previous image tag.
4. Start container (`systemctl start <name>-container`).
5. Run health check and verify logs for startup errors.

#### Database rollback

Database migrations are forward-only by default. Rollback SQL exists per migration for emergency use only.

1. **Verify the rollback SQL exists** for the migration in question. If it does not, this is a P0 gap -- write it now under four-eyes review.
2. **Back up the current database state** before executing rollback SQL. A failed rollback on top of a failed migration is unrecoverable without a backup.
3. **Execute rollback SQL** under operator supervision. Never automate migration rollback -- the operator must verify each statement's effect.
4. **Verify data integrity** after rollback. Run the service's integrity checks or manually verify critical tables.

WHY: Forward-only migrations exist because backward migrations are inherently dangerous -- they can drop columns, delete data, and violate constraints that new data depends on. Executing them is a conscious, supervised decision, not an automated recovery step.

### Communication

Timely, accurate communication during an incident is as important as the technical fix. Silence erodes trust faster than bad news.

#### Templates

**Incident declaration** (post to team channel and any affected-user channels immediately upon P0/P1 classification):

```
INCIDENT: [P0/P1] [brief description]
IMPACT: [what is broken, who is affected]
STATUS: Investigating / Mitigating / Monitoring
NEXT UPDATE: [time, no more than 30 minutes from now]
```

**Status update** (every 30 minutes for P0, every 60 minutes for P1, until resolved):

```
UPDATE: [P0/P1] [brief description]
STATUS: [current status]
ACTIONS TAKEN: [what has been done since last update]
NEXT STEPS: [what happens next]
NEXT UPDATE: [time]
```

**Resolution notice** (when service is restored):

```
RESOLVED: [P0/P1] [brief description]
RESOLUTION: [what fixed it: rollback / hotfix vX.Y.Z / config change]
DURATION: [time from detection to resolution]
POST-MORTEM: Scheduled within 48 hours.
```

WHY: Templated communication removes decision fatigue during an incident. The operator should be thinking about the fix, not about how to phrase a status update. Fixed intervals prevent both radio silence and noisy over-communication.

#### Notification channels

| Audience | Channel | When |
|----------|---------|------|
| Team (operators) | Team chat / ntfy | Every update |
| Affected users | Status page / email | Declaration, major updates, resolution |
| Stakeholders | Summary email | Resolution and post-mortem |

### Post-incident review

Every emergency release triggers a post-incident review within 48 hours. This is not optional and is not waived based on severity resolution speed.

#### Required sections

| Section | Contents |
|---------|----------|
| Timeline | Timestamped sequence: detection, classification, actions taken, resolution. Source from logs and chat history, not memory. |
| Root cause | The technical cause AND the systemic cause. "A bug in X" is the technical cause. "No test coverage for Y path" or "migration lacked rollback SQL" is the systemic cause. |
| Impact | What broke, for how long, for how many users/systems. Quantify where possible. |
| Detection | How the incident was detected. If a human noticed before monitoring did, that is a monitoring gap to address. |
| Response evaluation | What went well, what went poorly, what was lucky. |
| Action items | Concrete, assigned, deadlined. Each action item prevents recurrence of either the root cause or a response gap. |

WHY: The post-incident review exists to make the system better, not to assign blame. "Human error" is never a root cause -- it is a symptom of a system that made the error possible. Every P0/P1 that does not produce systemic improvements is a wasted crisis.

#### Action item standards

- Every action item has an owner and a deadline.
- Action items are filed as issues in the relevant repository, not left in a document.
- Action items are tracked to completion. An action item that is filed and forgotten is worse than no action item -- it creates the illusion of improvement.
- Common action item categories: add test coverage, add monitoring/alerting, add rollback SQL, improve documentation, fix CI gap, add pre-flight check.

WHY: Unfiled action items decay into good intentions. Issues in the tracker are visible to the dispatch pipeline, get prioritized, and get assigned. The review document is an input to the tracker, not a substitute for it.

---

## Pre-release checklist

Before merging a release-please PR:

- [ ] All CI checks pass on main
- [ ] CHANGELOG entries accurately describe the wave's changes
- [ ] No unreleased breaking changes hiding behind feature flags
- [ ] Version bump magnitude matches the nature of changes
- [ ] Security advisories addressed (no unpatched CVEs in release)
