# QA

> The audit playbook. Defines when audits run, what they contain, what "passing" looks like, how findings are tracked over time, and how the process improves itself. Per-PR checks are handled by the dispatch pipeline and CI, not this document.

---

## Purpose

The goal is perfection. Every audit asks: is this the best it can be? If not, file the issue. The backlog is the distance between where the code is and where it should be. Measuring that distance honestly is the audit's job.

## Principles

**The standard is perfection.** Audit against the full standards in this directory. Every rule in every standard document applies. Reference standards by name (RUST.md, WRITING.md, SECURITY.md, etc.) rather than restating rules here. This document defines process. The criteria live in the standards.

**Every finding becomes a structured issue.** A well-researched GitHub issue on the relevant repository with: what's wrong, where it is (file + line), which standard it violates, and how to fix it. If the finding is about tooling or standards, log it on kanon.

**Every audit produces training data.** Each audit writes structured JSON to `workflow/training/audits/` capturing findings, scores, and context. This data trains future models to audit better, write cleaner code, and align with ecosystem standards. See [Training data output](#training-data-output).

**The audit audits itself.** If the linter misses something a standard requires, file on kanon. If a standard is ambiguous or missing a case, file on kanon. If the audit process is inefficient, file on kanon. Tooling and standards improve every cycle.

**Verify, don't trust.** If a doc says the system does X, check the code. If an issue is closed, verify the fix exists. If the linter says clean, check whether it's running all rules.

**Nothing is too small.** Every violation gets filed. Dismissing things as minor means they never get fixed.

**DRY applies to everything.** Duplicated constants, repeated boilerplate, copy-pasted logic, redundant documentation, restated rules: all are audit targets. Parameterize. Modularize. Reference, don't restate.

**Audit from every angle.** Mechanical linting catches syntax. LLM review catches design and prose quality. User testing catches UX. External comparison catches ambition gaps. Closed-issue verification catches regressions.

**Compare against the best.** Audit against best-in-class projects in the same domain. File issues for every gap. Update standards with every new pattern.

**Test from the user's seat.** Deploy the system. Use it as an operator would. File every friction point.

**Question your own completeness.** After every pass: what haven't I checked? What angle haven't I considered? The audit is done when you can't think of another question.

---

## Audit scheduling

WHY: An audit that runs "when someone remembers" doesn't run. Fixed cadences convert intent into habit. Frequency scales with risk: high-churn repos accumulate violations faster.

| Tier | Repos | Periodic | Full |
|------|-------|----------|------|
| Core | kanon, aletheia | Weekly | Monthly |
| Supporting | harmonia, akroasis, thumos | Biweekly | Quarterly |
| Standards-only | basanos/standards | On change | On change |

**Periodic audits** cover violation baseline, privacy/secrets, and standards enforcement gaps (see [Periodic audit](#periodic-audit)). Run them at the cadence above. Skip only if zero commits since the last run — and log the skip.

**Full audits** cover every phase. Schedule them at the cadence above. A full audit also runs after any major release (semver minor or major bump) or architectural change (new crate, crate split, dependency direction change).

**On-change audits** for standards-only repos trigger when any `.md` file in the standards directory is modified. The audit verifies internal consistency (no contradictions between standards), link validity, and claim accuracy.

**Batch execution.** Use `kanon audit --all` to run periodic audits across every repo in the fleet. Use `kanon audit --all --full` for full audits. If batch tooling does not yet exist, file on kanon to track it — do not silently run audits manually at scale.

WHY: Manual execution across five repos is error-prone and creates inconsistent audit timestamps. Batch tooling ensures every repo is audited with the same standards version in the same pass.

---

## Baseline scores

WHY: Without a defined target, audits produce findings but no verdict. Baselines separate "improving" from "acceptable" and create a gate that blocks regressions.

Each audit summary record includes category scores (see [Training data output](#training-data-output)). These are the minimum thresholds:

| Category | Minimum | Target | Gate? |
|----------|---------|--------|-------|
| Writing | B | A- | Yes |
| Safety | B+ | A | Yes |
| Architecture | B | A- | Yes |
| Testing | B | B+ | No |
| Security | B+ | A | Yes |
| Operations | B- | B+ | No |

**Gate** means a repo scoring below the minimum on a gated category blocks the next release. Non-gated categories are tracked and trended but do not block.

**How scores map to grades.** Scores derive from the violation density (violations per 1K lines) and severity distribution within each category. The grading function lives in basanos and is the single source of truth. Do not hardcode grade thresholds in this document — reference the implementation.

WHY: Hardcoded thresholds in prose diverge from the code that actually computes them. The table above defines policy (what the minimums are); the code defines mechanics (how a score becomes a grade).

**Ratchet rule.** Once a repo reaches a score, the minimum for that repo ratchets to that score. Baselines only move up. If a repo achieves A- in Security, A- becomes its new minimum. Store per-repo baselines in `workflow/baselines/{repo}.toml`.

WHY: Without a ratchet, repos oscillate. A team fixes violations to reach A-, then regresses to B+ next quarter. The ratchet converts every improvement into a permanent floor.

**New repos.** A repo's first full audit establishes its initial baseline. No gating applies until the second audit. The first audit is measurement, not judgment.

---

## Quality metric tracking

WHY: Point-in-time audits tell you where you are. Trends tell you whether you're getting better or worse. Without historical tracking, the same violations get rediscovered every cycle.

### What to track

| Metric | Source | Granularity |
|--------|--------|-------------|
| Total violation count | `kanon lint --summary` | Per repo, per audit |
| Violation count by severity | Audit summary record | Per repo, per audit |
| Violation count by standard | Audit JSONL findings | Per repo, per audit |
| Category scores | Audit summary record | Per repo, per audit |
| Delta from previous audit | Computed from consecutive summaries | Per repo, per audit |
| Time to resolve (filed → closed) | GitHub issue timestamps | Per repo, rolling |
| Standards coverage | Enforcement gap analysis | Per repo, per full audit |

### Storage

Audit JSONL files in `workflow/training/audits/` are the raw data. Per-repo baselines in `workflow/baselines/{repo}.toml` are the derived policy state. Both are committed to the kanon repo.

WHY: Committing audit data to the repo makes trends visible in git history, reviewable in PRs, and available to any tool without external service dependencies.

### Trend analysis

After each audit, compare against the previous three audits for the same repo and tier:

- **Violation delta.** Is the total count decreasing? If not, investigate. A flat or rising count means new violations are being introduced as fast as old ones are fixed.
- **Category score movement.** Any category that dropped since the last audit gets a tracking issue explaining why and what will reverse it.
- **Stale findings.** Any finding that appears in three consecutive audits without a corresponding open issue is a process failure. File it and flag the gap.
- **Resolution velocity.** Track the median time from issue filed to issue closed for audit findings. If median exceeds 30 days, the audit is producing findings faster than the team resolves them — adjust audit scope or increase fix bandwidth.

WHY: Measuring resolution velocity prevents the failure mode where audits produce an ever-growing backlog that everyone ignores. The audit must produce actionable work at a sustainable rate.

### Reporting

Each audit run appends to the repo's trend. `kanon audit-report --repo <name>` renders the trend as a table showing the last 6 audits with deltas. If this tooling does not yet exist, file on kanon — do not build ad-hoc scripts that become invisible workarounds.

---

## Prerequisites

```bash
cargo install cargo-fuzz cargo-outdated cargo-deny cargo-audit
rustup toolchain install nightly  # for fuzz
# gitleaks: https://github.com/gitleaks/gitleaks/releases
```

---

## Per-PR checks (handled by dispatch)

Format, clippy, tests, commit lint, and CI security scans run automatically via the kanon dispatch pipeline and GitHub CI. Not part of QA audits. See `crates/phronesis/` and `.github/workflows/`.

---

## Periodic audit

Covers what dispatch does NOT check: violation trends, privacy drift, and standards enforcement gaps.

### 1. violation baseline

```bash
kanon lint /path/to/repo --summary
```

All checker modules. Compare violation count against tracking issue baseline. Direction matters: trending down?

### 2. privacy and secrets

```bash
kanon scan /path/to/repo
gitleaks detect --source /path/to/repo
```

- No API keys, tokens, or credentials (even example ones that look real)
- No personal identifiers in code, comments, test fixtures, or commit messages
- No internal hostnames, IPs, or infrastructure details
- No customer data or employer-identifying information
- No private file paths revealing system layout
- `.gitignore` covers: `.env`, `credentials/`, `*.key`, `*.pem`, `instance/`

Public repo: assume everything committed is permanently visible.

### 3. standards enforcement gap

Compare standards docs against `crates/basanos/src/rules/` modules. Any rule without a linter check needs a kanon issue explaining why it can't be automated or tracking the work to automate it.

---

## Full audit

All phases of the periodic audit, plus the following.

### Phase 1: automated

#### 1.1 full test suite

```bash
cargo test --workspace --all-features
```

Per-crate if all-features OOMs. Document which commands produce full coverage.

#### 1.2 fuzz targets

```bash
rustup run nightly cargo fuzz list
rustup run nightly cargo fuzz run <target> -- -max_total_time=60
```

Requires nightly toolchain. Each target 60 seconds. Crashes are bugs.

#### 1.3 binary smoke test

Build release binary. Init. Start. Health check. Create session. Send message. Verify response. Stop. The full deploy pipeline end-to-end.

#### 1.4 dependency audit

```bash
cargo tree -d
cargo outdated
```

Flag duplicates. Flag stale deps.

#### 1.5 supply chain audit

```bash
cargo deny check
cargo audit
```

cargo-deny checks advisories, license compliance, banned crates, and source verification. cargo-audit checks the RUSTSEC advisory database. Both are zero-config with a deny.toml.

#### 1.6 shell script lint

```bash
shellcheck scripts/*.sh
```

Catches portability issues (GNU-only flags, quoting bugs) and POSIX compliance.

### Phase 2: writing and docs (LLM-assisted)

#### 2.1 writing quality

Linter writing checks, then LLM review of: AI pattern detection beyond keywords, information density, opening sentence quality, verb strength, hedging. Sample: changed files since last audit plus 5 random unchanged files.

#### 2.2 doc accuracy

Changed docs since last audit: do code references point to real files? Do numbers match reality? Are examples runnable?

#### 2.3 cLAUDE.md freshness

Does CLAUDE.md match the codebase? Paths correct? CLI subcommands current?

#### 2.4 doc claims verification

Cross-check documented claims against ground truth:

- Version numbers in docs match `Cargo.toml` `version` field
- Config field names in CONFIGURATION.md (or equivalent) match the config struct definition
- CLI subcommands listed in CLAUDE.md match `--help` output (compare `kanon --help` tree against prose)
- Internal doc links (`[text](path)`) resolve to existing files

Flag every mismatch as a separate finding. Documentation that lies is worse than no documentation.

### Phase 3: code quality (LLM-assisted)

#### 3.1 dead code

`#[allow(dead_code)]` justified? Commented-out blocks deletable? TODOs reference open issues? Empty match arms explained?

#### 3.2 error handling

Sample 10 error paths. Each: explains what went wrong AND how to fix it? Context propagates correctly?

#### 3.3 unsafe audit

Every `unsafe` block: SAFETY comment? Invariant correct? Replaceable with safe code?

#### 3.4 public API surface

New public items justified? Could any be `pub(crate)`? Documented?

### Phase 4: architecture (LLM-assisted)

#### 4.1 dependency direction

No upward dependencies. Check with `cargo tree`.

#### 4.2 crate boundary review

Any crate past 800 files or 50K lines?

#### 4.3 per-crate cLAUDE.md accuracy

Referenced types exist? Module paths correct? Common tasks accurate?

#### 4.4 repository structure audit

See STANDARDS.md § Repository Hygiene for the principles. Check:
- Every root file justified? (required at root by its consuming tool?)
- Shell scripts that should be native subcommands?
- Config files that belong with their consuming system?
- Empty directories, orphaned files, stale templates?
- Operational artifacts mixed with source code?
- Standards/data files separated from the tool that uses them?

### Phase 5: security (LLM-assisted)

#### 5.1 credential handling

New credential paths using 0600? Secrets redacted in logs? `std::fs::write` to config paths without explicit permissions?

#### 5.2 input validation

New HTTP endpoints validate at boundary? New tool inputs check allowed_roots? New queries parameterized?

#### 5.3 sandbox

New `tokio::spawn` without `.instrument()`? New process spawning without ProcessGuard? New file ops bypassing FileSystem trait?

#### 5.4 CSRF and auth default audit

Compare security-relevant code defaults against documented defaults. Mismatches here are critical: if DEPLOYMENT.md says CSRF is "enabled by default" but the code initializes it disabled, operators configure based on the docs and ship with a silent security hole.

Check:
- Auth mode default (e.g., `AuthMode::None` vs. `AuthMode::Required`) matches documented default
- CSRF protection enabled or disabled by default
- Sandbox enforcement on or off by default
- Credential file permissions (must be 0600 if written)
- Token preview length (must not expose full token in logs or UI)

### Phase 6: operational readiness

Runbook covers new components? New features emit metrics? Backup restore tested?

### Phase 7: regression

#### 7.1 closed issue verification

Sample 10 recently closed issues. Verify fix exists in current code.

#### 7.2 unported feature check

Review open unported-feature issues. Any inadvertently implemented? Close if so.

### Phase 8: external benchmark

Pick one well-respected project. Audit against kanon standards. Compare. File gaps. Update standards.

---

## Audit outputs

Every audit produces three things:

### 1. issues on the target repo

Each finding: GitHub issue with title (conventional format), body (what's wrong, file + line, standard violated, fix). One issue per finding. Reference standard by name.

### 2. issues on kanon

Standards unclear or missing a case. Linter rule that should exist. Audit step that was inefficient. Standard that contradicts another. Tool that's broken or gives false positives.

### 3. training data

Each audit writes a JSONL file to `workflow/training/audits/`:

**Filename:** `{repo}_{date}_{tier}.jsonl` (e.g., `aletheia_2026-03-20_full.jsonl`)

**Schema (one line per finding):**

```json
{
  "ts": "2026-03-20T14:30:00Z",
  "repo": "forkwright/aletheia",
  "tier": "full",
  "phase": "3.1",
  "rule": "RUST.md/dead-code",
  "file": "crates/nous/src/recall.rs",
  "line": 142,
  "severity": "medium",
  "finding": "Commented-out code block (15 lines) with no TODO or issue reference",
  "fix": "Delete or file issue with reference",
  "context": "fn score_candidates() contains disabled similarity threshold logic"
}
```

**Summary record (last line):**

```json
{
  "ts": "2026-03-20T16:00:00Z",
  "repo": "forkwright/aletheia",
  "tier": "full",
  "type": "summary",
  "version": "v0.13.0",
  "violations_total": 2221,
  "violations_delta": -126,
  "issues_filed": 15,
  "kanon_issues_filed": 3,
  "phases_completed": [1,2,3,4,5,6,7],
  "scores": {
    "writing": "A-",
    "safety": "A",
    "architecture": "B+",
    "testing": "B",
    "security": "B+",
    "operations": "B"
  }
}
```

This data feeds model fine-tuning for: code review, standards compliance detection, issue writing, and audit automation. Every audit makes the next audit's tooling better.

---

## Standards reference

Audit checks compliance against the full kanon standards library:

| Standard | Scope |
|----------|-------|
| STANDARDS.md | Universal principles |
| RUST.md | Rust language |
| PYTHON.md | Python language |
| SHELL.md | Shell scripts |
| WRITING.md | All prose and documentation |
| ARCHITECTURE.md | Structure, dependencies, API surface |
| TESTING.md | Test organization, coverage, infrastructure |
| SECURITY.md | Credentials, validation, sandboxing |
| OPERATIONS.md | Runbooks, monitoring, backup, deployment |
| API.md | HTTP endpoints, CLI, error responses |
| CI.md | Required checks, release process |
| YAML.md | YAML formatting, GitHub Actions workflow structure |
| PERFORMANCE.md | Resource budgets, benchmarks |
| STORAGE.md | Database, migrations, connections |
| TOML.md | TOML formatting, structure, Cargo.toml conventions |
| PROTOBUF.md | Protobuf schema design, naming, compatibility |

The standards are the criteria. This document is the process.
