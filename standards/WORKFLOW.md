# Closed-loop agentic development

The human is the architect. Everything else is the system.

## The problem

Existing AI coding tools solve the single-session problem: one agent, one task, one PR. Software is not one task. It is a dependency graph of hundreds of tasks with ordering constraints, quality gates, cross-cutting concerns, and institutional knowledge that must survive across sessions.

The gap between "an agent can write a feature" and "agents can build a system" is not model capability. It is orchestration: who plans the work, who schedules it, who verifies it, who recovers from failure, and who ensures that knowledge from one session is available to the next.

This system closes every loop. A planner decomposes goals into structured prompts with machine-verifiable acceptance criteria. A dispatcher executes them in parallel across isolated worktrees. A QA gate evaluates each result against its criteria and controls what happens next: advance, correct, or block. A CI manager validates, auto-merges, and dispatches fix agents. An observation triage system converts ephemeral worker findings into tracked issues. The planner receives QA feedback and adjusts the plan.

The human approves plans, resolves ambiguity, and reviews held PRs. Everything between planning approval and merged code is autonomous.

## Core concepts

**Prompt:** The atomic unit of work. A structured document containing a directive, task specification, acceptance criteria, blast radius, and validation gate. Machine-parseable, QA-verifiable, self-contained. A well-written prompt produces correct work regardless of which agent executes it. A poorly-written prompt produces plausible waste regardless of agent capability.

**Execution plan:** A dependency DAG of prompts, partitioned into ordered batches. Prompts within a batch run in parallel. Batches run sequentially. Each prompt declares what it depends on and what it blocks. The plan is the schedule, the dependency tracker, and the progress dashboard in one artifact.

**Blast radius:** The set of files and modules a prompt is authorized to modify. Not advisory. QA flags changes outside this boundary. Fix agents are constrained to it. Parallel prompts in the same batch declare non-overlapping blast radii, preventing merge conflicts by construction.

**Observation:** Something a worker noticed outside its task scope: a bug in adjacent code, missing tests, stale docs, a better API shape. Captured in the PR body. Automatically triaged into tracked issues post-merge. The mechanism that prevents ephemeral sessions from losing institutional knowledge.

**Verdict:** QA's assessment of a completed prompt. PASS, PARTIAL, or FAIL. Verdicts are not reports to be read. They are control signals that drive the dispatch loop: advance, correct, or block.

## Roles

| Role | Responsibility | Intelligence |
|------|---------------|-------------|
| **Architect** | Final authority. Approves plans, resolves ambiguity, sets direction, reviews held PRs | Human |
| **Planner** | Decomposes goals into research and implementation prompts. Writes execution plans. Maintains standards. Hands off to dispatcher via structured API. Receives QA feedback and adjusts plans | AI (high-capability model) |
| **Dispatcher** | Long-running orchestration service. Watches prompt queue, launches parallel sessions, monitors progress, runs QA, gates waves, generates corrective prompts, feeds verdicts back to planner | Automated (SDK + AI for QA/correction) |
| **Worker** | Executes a single prompt in an isolated worktree. Writes code, runs tests, opens a PR, captures observations | AI (headless session) |
| **CI Manager** | Validates PRs continuously. Auto-merges on green (policy-gated). Dispatches fix agents on failure. Triages observations post-merge | Automated (daemon + AI for fixes/triage) |

## Pipeline

```
Plan ──> Prompts ──> Dispatch ──> Execute ──> QA Gate ──> Merge
  ^                     ^                       │            │
  │                     │     PARTIAL/FAIL      │            │
  │                     └─── corrective ◄───────┘            │
  │                          prompts                         │
  │                                                          │
  │   verdicts + observations                                │
  └──────────────────────────────── Triage ──> Issues ───────┘
                                     │
                                     └──> next batch (dependencies unblocked)
```

### Stage 1: planning

The planner produces three artifact types.

#### Research prompts (R-type)

Output is a document, not code. Findings with confidence levels, source citations, and actionable recommendations. Research establishes the facts that implementation prompts depend on. Architecture decisions are made here, not during implementation.

Structure:
- Research questions (explicit, numbered, with sub-questions)
- Output format (required elements per finding: recommendation, evidence, confidence level, alternatives considered and why rejected)
- Cross-references to prior research that should be verified or extended

Research prompts execute in parallel waves. Later waves reference earlier results. The planner writes later waves after reviewing earlier outputs, not before.

#### Execution plans

A dependency DAG partitioned into ordered batches:

```
Batch 1: [001]                        # foundation, blocks everything
Batch 2: [002, 003]                   # parallel, both depend on B1
Batch 3: [004, 005, 006, 007]         # 4 parallel, all depend on B2
Batch 4: [008, 011]                   # depend on specific B3 outputs
Batch 5: [009, 010]                   # depend on 008
Batch 6: [012 -> 013 -> 014]          # sequential tail
```

Each prompt entry includes:

| Field | Purpose |
|-------|---------|
| Depends on | Which prompts must complete first |
| Blocks | Which prompts are waiting on this one |
| Blast radius | Files and modules in scope |
| Model tier | Capability level required |
| Status | queued / dispatched / PR# / merged |

The plan is a living document. The dispatcher feeds QA verdicts and observation summaries back to the planner after each batch. The planner updates status, re-scopes later batches based on what was learned, and adjusts prompt specificity if the corrective prompt rate is rising.

#### Implementation prompts (P-type)

Output is a PR. The prompt is specific enough that two different agents executing the same prompt would produce structurally equivalent results. File listings, type definitions, function signatures, step-by-step validation commands. Ambiguity in a prompt produces ambiguity in the output. The planner's job is to eliminate ambiguity before dispatch.

### Stage 2: prompt structure

Every implementation prompt is rendered from a template. The template is a function, not a convention. Missing sections are errors, not review findings.

```python
def render_prompt(
    task: str,
    branch: str,
    context: str,
    acceptance_criteria: list[str],
    blast_radius: list[str],
    standards: list[str] | None = None,
    not_in_scope: list[str] | None = None,
    pitfalls: list[str] | None = None,
) -> str:
```

Structural guarantees enforced at render time:
- `acceptance_criteria` must be non-empty (no prompt without verifiable outcomes)
- `blast_radius` must be non-empty (no prompt without scope declaration)
- `task` must start with an imperative verb (enforced by prefix check)
- `branch` must match conventional prefix pattern

The rendered output:

```markdown
# Task: <one-line description>

## Directive

Implement this task completely.
Write the code, run the tests, fix any issues, commit, push, open a PR.
Do not analyze or summarize the prompt. Execute it.

## Setup

git fetch origin && git log --oneline -3 origin/main
git worktree add ../worktrees/<branch> -b <branch> origin/main
cd ../worktrees/<branch>

## Standards

<auto-populated from project config>

## Context

<what exists, what's decided, relevant commits>
<SHORT. Only what the agent needs to start. Detail in the Task section.>

## Task

<exactly what to build. Imperative voice. Numbered steps if order matters.>
<what is explicitly OUT of scope>

## Acceptance criteria

- [ ] Concrete, verifiable outcome
- [ ] Each one is pass/fail testable
- [ ] QA evaluates the PR diff against these directly

## Blast radius

<files and modules in scope. Changes outside this set are flagged by QA.>

## Validation gate

<project-specific checks: formatter, linter, tests, doc generation>
git diff --stat  (self-review before PR)

## Observations

Capture anything noticed outside scope in the PR body:
Bug, Debt, Idea, Missing test, Doc gap.
Note file and line. Do not fix. Do not investigate deeply. Move on.
```

### Why each section exists

| Section | Purpose | What breaks without it |
|---------|---------|----------------------|
| Directive | Forces execution mode | Agent analyzes the prompt instead of implementing it |
| Setup | Worktree isolation | Agent works on main, corrupts shared state |
| Standards | Project conventions | Agent writes valid code that violates project norms |
| Context | Orients without overloading | Agent makes wrong assumptions or drowns in irrelevant detail |
| Task | Defines the work | Ambiguous output, scope creep |
| Acceptance Criteria | Machine-verifiable outcomes | QA has nothing to evaluate against |
| Blast Radius | Scope boundary | Agent modifies unrelated files, creates merge conflicts |
| Validation Gate | Pre-PR checks | Broken PRs waste CI and review cycles |
| Observations | Knowledge capture | Findings from ephemeral sessions are permanently lost |

#### Acceptance criteria specificity

Every acceptance criterion must reference a specific file, function, or structural change observable in a PR diff. QA evaluates criteria by examining the diff — criteria that require runtime execution, CI results, or external state produce systematic false negatives and waste corrective prompt cycles.

**Diff-visible criteria** (use these):

| Instead of | Write |
|-----------|-------|
| "`cargo test -p X` passes" | "Test file `src/X/tests.rs` contains test function `test_Y` covering case Z" |
| "All tests pass" | "Module `X` contains `#[cfg(test)] mod tests` with tests for A, B, and C" |
| "`cargo clippy` clean" | "No `unwrap()` calls outside test modules; errors use `snafu::ResultExt`" |
| "Closes #NNN" | "Function `X` in `path/to/file.rs` returns `Result<T, Error>` (fixes #NNN behavior)" |
| "Issue #NNN requirements are satisfied" | "Struct `Config` has field `timeout: Duration`; `process()` respects it via `tokio::time::timeout`" |
| "CI green" | "Validation gate commands listed in Validation Gate section produce no errors" |
| "No regressions" | "Existing public API signatures in `src/lib.rs` are unchanged" |

**Why this matters:** The QA evaluator examines the PR diff, not runtime output. A criterion like "`cargo test` passes" looks like a FAIL from the diff (no evidence of test execution results), even when the code is correct. The prompt validation pipeline rejects non-diff-visible criteria before dispatch.

**Rule:** If a criterion requires executing a command or checking a system outside the PR diff to evaluate, it is not diff-visible. Rewrite it to describe the artifact that executing the command would verify.

#### Framing rules

- **Imperative first.** "Implement..." not "Here is the task..."
- **Action before context.** Without the directive, models default to commentary and exploration
- **"Task" not "Scope."** "Scope" reads as a briefing to analyze. "Task" reads as work to do
- **Minimal context.** Only what the agent needs to start. Detailed specs belong in the Task section
- **Explicit exclusions.** "Do NOT modify X" prevents scope creep more reliably than "only modify Y"

#### Model selection

| Capability tier | Use | Examples |
|----------------|-----|---------|
| High | Design decisions, multi-module changes, complex logic, research | New module architecture, API design, cross-cutting refactors |
| Standard | Mechanical work, single-module changes, fixes, evaluation | Formatting, lint fixes, conflict resolution, CI repair, QA |

### Stage 3: infrastructure

#### Local

Dev machine. Sufficient for small batches, single-project work, and prompt development iteration.

#### Cloud build server

For large batches. Ephemeral: provisioned per-project, destroyed after. The system should not depend on server persistence.

**Bootstrap** is an idempotent script. Run once on a fresh instance, safe to re-run on partial failure:

- Language toolchain with fast linker and build cache
- Agent runtime (Claude Code for headless execution)
- CLI tools for PR operations (gh)
- Repo clone at env-configured path
- Worker agent config: git safety rules, code standards, scope constraints
- Background dependency build to pre-warm caches

**Path configuration:** Code repo paths are environment variables. Prompts and planning docs live in the kanon repo itself.

```bash
ALETHEIA_REPO=$HOME/aletheia              # code repo clone
HARMONIA_REPO=$HOME/harmonia              # code repo clone
AKROASIS_REPO=$HOME/akroasis             # code repo clone
DISPATCH_STATE=$HOME/.dispatch             # state database (runtime, not in repo)
```

Portable between machines by cloning kanon and setting repo paths. No hardcoded absolute paths. No symlinks.

**Post-bootstrap:** Copy auth credentials, authenticate CLI tools, start dispatching.

### Stage 4: dispatch

The dispatcher is a long-running service built on the Claude Code SDK for headless agent session management. It watches the prompt queue, resolves dependencies against the execution plan, and forms groups for parallel dispatch.

#### Invocation

The service watches the prompt queue directory continuously. Prompts that appear in `queue/` with satisfied dependencies are picked up automatically. Manual dispatch is also supported for ad-hoc work:

```bash
kanon dispatch "98 and 99 in parallel, then 100 and 101"
```

Natural language spec parsed into ordered groups:
- `"98 and 99 in parallel, then 100"` -> `[[98, 99], [100]]`
- `"95 then 96 and 97"` -> `[[95], [96, 97]]`
- `"102"` -> `[[102]]`

In service mode, the dispatcher reads the execution plan, determines which prompts have satisfied dependencies (all "Depends on" entries are in `done/`), and forms groups from prompts at the same dependency depth. No manual spec needed.

#### Orchestration loop

```
for each group (from plan or manual spec):

    check abort sentinel
    sync repo to main branch

    launch all prompts in group as parallel async sessions
        inject launch preamble + prompt file path
        headless execution, permission bypass, isolated worktree
        monitor via async progress queue
        on max_turns: graduated resume

    collect results:
        extract PR URLs from session output
        cross-reference with PR list (catch PRs by branch name match)

    QA gate:
        for each PR with acceptance criteria:
            fetch diff + metadata
            evaluate per-criterion via standard-capability model
            produce verdict: PASS / PARTIAL / FAIL

    act on verdicts:
        PASS    -> mark complete, eligible for auto-merge
        PARTIAL -> mark complete, generate corrective prompts, queue
        FAIL    -> block dependent prompts, continue independents

    write results to state database
    sync repo for next group
```

#### Session parameters

| Setting | Value | Rationale |
|---------|-------|-----------|
| Model | Highest available | Implementation quality |
| Effort | High | Complex tasks need deep reasoning |
| Max turns | 80 initial | Sufficient for most implementation prompts |
| Permission mode | Bypass | Headless execution, no human to approve tool use |
| Poll interval | 15s | Progress monitoring granularity |
| Stuck threshold | 5 min | Detect agents that stopped making progress |

#### Graduated resume

When a session exhausts its turn budget, the dispatcher resumes it with progressively narrower scope. This avoids the all-or-nothing failure mode where a 90%-complete session is marked failed.

| Attempt | Turns | Instruction |
|---------|-------|-------------|
| Initial | 80 | Full prompt |
| Resume 1 | 40 | "Finish what you started. Do NOT start over." |
| Resume 2 | 20 | "Complete ONLY remaining acceptance criteria. List what's done vs. remaining before continuing." |
| Exhausted | -- | Mark STUCK. Generate diagnostic (criteria status, last activity). Diagnostic becomes input for a follow-up prompt |

The narrowing is deliberate. Resume 1 assumes the agent is close and needs more runway. Resume 2 assumes the agent may have lost focus and forces re-orientation. After exhaustion, the system generates a diagnostic rather than retrying blindly: a fresh prompt scoped to exactly what remains.

#### Launch preamble

Injected before every prompt. Establishes execution discipline:

- Read the prompt file completely before writing code
- Review project standards and conventions
- Create a checklist from acceptance criteria, track completion
- Work in the worktree, never on the default branch
- Verify assumptions by reading code before modifying it
- Run targeted tests after each change
- Run full validation gate before creating PR
- Scope constraint: push branch and create PR as only remote interaction

#### State database

SQLite at `$DISPATCH_STATE/state.db`:

| Table | Purpose |
|-------|---------|
| `dispatches` | Spec, groups, start/end time, overall status |
| `sessions` | Per-prompt: session ID, PR number, turns, QA verdict |
| `ci_validations` | Per-PR: head SHA, check results, timestamp. Auto-pruned by age |

The JSON manifest exported after each dispatch is a view, not the source of truth. The database enables queries the manifest cannot: historical prompt performance, retry rates, per-project patterns.

### Stage 5: verification

Three independent layers. Together they close the loop: no work lands without validation, no finding is lost, no failure goes unaddressed.

#### Layer 1: QA gate (per-group, controls dispatch flow)

Runs after each dispatch group completes. The verdicts determine what happens next.

**Process:**
1. Fetch PR diff (truncated at 80KB boundaries if massive)
2. Fetch PR metadata: changed files, title, body
3. Evaluate via standard-capability model (plan mode, 1 turn, structured JSON output)
4. Per-criterion assessment: PASS with evidence, or FAIL with reason
5. Flag: changes outside declared blast radius, project-specific anti-patterns
6. Aggregate: PASS (all criteria met), PARTIAL (some met), FAIL (critical criteria unmet)

**Verdicts drive the dispatch loop.** See DEPLOYMENT.md (deployment gates) for the verdict-to-action mapping and merge eligibility rules.

| Verdict | Next group | Follow-up |
|---------|-----------|-----------|
| PASS | Proceeds | Merge policy evaluation (DEPLOYMENT.md) |
| PARTIAL | Proceeds | Corrective prompts auto-generated |
| FAIL | Dependents blocked, independents continue | Diagnostic generated |

**Corrective prompt generation:** On PARTIAL or FAIL, the dispatcher generates a targeted fix prompt. Inputs: original prompt, failed criteria, the PR diff, and QA evidence. A standard-capability model writes a new prompt scoped to exactly the unmet criteria. This queues as a follow-up wave that runs before the next planned batch.

The corrective prompt is not a retry. It is a new, narrower prompt informed by what the original worker actually produced. It targets the delta, not the whole task.

#### Layer 2: CI manager (per-PR, continuous)

A single daemon process that validates open PRs on a polling loop. Four behaviors, each configurable:

```bash
AUTO_MERGE=true          # squash-merge green PRs (tiered policy)
MAX_FIX_AGENTS=1         # concurrent fix agent sessions (0 = CI-only mode)
POLL_INTERVAL=45         # seconds between PR list checks
MAX_CONCURRENT=2         # parallel validation jobs
```

**Validation per PR:**
1. Detached worktree from main
2. Trial merge (detect conflicts before building)
3. Validation gate: formatter, linter, tests
4. Post results as PR comment with pass/fail table and collapsible failure details

**Fix agent dispatch on failure:**

| Failure | Fix scope |
|---------|----------|
| Merge conflict | Rebase, resolve, push |
| Formatting | Run formatter, commit, push |
| Lint | Fix root cause (no suppression), push |
| Test failure | Investigate cause, fix the bug (not the test), push |

**Fix agent constraints:**
- **File allowlist:** Derived from the PR's changed files. May only modify files already in the diff, plus test files for those modules. No exploring
- **Diff size cap:** If the fix exceeds 2x the original change scope, abort and flag for human review. This catches agents that "fix" a test failure by rewriting half the module
- **Model tier:** Always standard capability. Fix agents do mechanical work, not design

**Tiered auto-merge:** See DEPLOYMENT.md (merge policy) for the full merge decision table. The CI manager evaluates each PR against the tiered policy after validation passes.

#### Layer 3: observation triage (post-merge, automated)

Every merged PR is scanned for observations. Every observation becomes a tracked item. The pipeline:

1. Parse `## Observations` section from PR body
2. Classify each entry: Bug, Debt, Idea, Missing test, Doc gap
3. Deduplicate against open issues (title/description similarity)
4. Route to issue tracker:
 - Bug: individual issue, labeled appropriately
 - Debt/Idea: append to project backlog issue
 - Missing test: bundle into test-coverage tracking issue
 - Doc gap: bundle into docs tracking issue

One standard-model call per merged PR. This is the mechanism that converts ephemeral worker observations into durable project knowledge. Without it, an agent that notices a bug in adjacent code during a refactor has no way to propagate that finding. With it, every observation surfaces automatically.

### Stage 6: completion

Each batch follows the full cycle:

```
Dispatch group
  -> Workers execute in parallel worktrees
    -> QA gate evaluates each PR against acceptance criteria
      -> CI Manager validates build/lint/test
        -> Merge (green + policy) or hold (PARTIAL/FAIL/policy)
          -> Observation triage (post-merge -> issue tracker)
            -> Next group proceeds (dependencies unblocked)

PARTIAL -> corrective prompts queued, run before next planned batch
FAIL    -> dependents blocked, diagnostic generated, independents continue
STUCK   -> diagnostic -> follow-up prompt in next wave
```

Post-merge housekeeping:
1. Prompt moved to `done/`
2. Execution plan updated with PR number and status
3. Repo synced for next group
4. Planner may re-scope later batches based on observations, QA findings, or corrective prompt outcomes

### Multi-Project dispatch

The dispatcher manages prompts across projects simultaneously. Each project carries its own:
- Prompt queue and execution plan
- Repo clone and worktree root
- Standards, validation gate, and pitfalls list
- State database partition

Dispatcher overhead is minimal: it manages sessions and evaluates results. It does not hold project context or execute implementation work.

## When to use this

This methodology has overhead. Prompt generation, execution plans, QA evaluation, observation triage. That overhead is fixed per prompt regardless of task complexity. The system is worthwhile when:

**Use it:**
- 5+ related tasks with dependency ordering
- Multi-module or cross-cutting changes
- Projects with established standards that must be enforced consistently
- Work that benefits from parallelism (independent tasks that would be sequential with a single developer)
- Long-running development phases where institutional knowledge must survive across sessions

**Don't use it:**
- Single-file bug fixes. Just open an agent session and fix it
- Exploratory prototyping where the goal is unclear. The system requires well-defined acceptance criteria
- UI/UX iteration where the feedback loop is visual, not testable
- Tasks with fewer than 3 steps. The overhead of prompt generation exceeds the implementation effort
- Greenfield projects before any architecture decisions exist. Run R-type prompts first, then switch to the full pipeline once the foundation is defined

The complexity threshold: if you would write a project plan before starting the work manually, the work is complex enough for this system. If you would just start coding, it is not.

## Failure modes

The system can fail at every stage. Recognizing failure modes and their remediation is what separates a methodology from a script.

### Planner failures

**Bad prompts.** The most consequential failure. A prompt with vague acceptance criteria produces a PR that passes QA ly but does not actually satisfy the intent. A prompt with an incorrect blast radius produces a PR that modifies the right files for the wrong reasons.

*Signal:* High PARTIAL rate (>20% of prompts in a batch). Corrective prompts are fixing criteria failures, not agent mistakes.

*Remediation:* The problem is upstream. Stop dispatching. Review the prompts that produced PARTIAL results. The acceptance criteria are likely under-specified or the task decomposition is wrong. The planner needs to re-scope, not the workers.

**Over-decomposition.** Too many prompts, each too small. The overhead of worktree setup, PR creation, QA evaluation, and merge exceeds the implementation time. The dependency DAG becomes a bottleneck: many sequential batches of 1-2 prompts.

*Signal:* Average prompt execution time is under 5 minutes. Batches are mostly single-prompt. The merge queue is the constraint.

*Remediation:* Merge related prompts. A prompt that creates a module and a prompt that writes its tests should be one prompt. The granularity sweet spot is: one prompt per logical unit of work that can be verified independently.

**Under-decomposition.** Too few prompts, each too large. Workers hit max_turns and STUCK. QA cannot evaluate because the diff is enormous. Blast radii overlap between prompts in the same batch because the tasks are not cleanly separable.

*Signal:* High STUCK rate. Resume attempts fail. QA diffs are truncated at 80KB.

*Remediation:* Split. If a prompt touches more than one module boundary, it should be two prompts in sequential batches.

### Dispatch failures

**Cascade blocking.** A FAIL verdict on a foundational prompt (Batch 1, everything depends on it) blocks the entire pipeline. The corrective prompt also fails. Progress halts.

*Signal:* Dispatch loop stalled. All groups blocked on a single prompt.

*Remediation:* Escalate to architect. This is a case where the task itself may be mis-specified, the codebase is in an unexpected state, or the prompt is asking for something the agent cannot do. Human judgment required.

**QA false positives.** The QA evaluator marks a criterion as FAIL when the diff actually satisfies it, but the evidence is not obvious from the diff alone (e.g., the criterion is "all tests pass" and the test file wasn't modified because existing tests already cover it).

*Signal:* Corrective prompts that produce identical diffs to the original. The "fix" changes nothing because nothing was broken.

*Remediation:* The classifier now detects criteria requiring runtime execution or external state (e.g., "`cargo test` passes", "Closes #NNN") and auto-resolves them as NOT_VERIFIABLE instead of FAIL. The prompt validation pipeline rejects such criteria before dispatch. For remaining criteria, improve specificity: "Test file X exists with test function Y that covers case Z" is diff-visible. See the "Acceptance criteria specificity" section above for rewrite patterns.

**Agent drift.** A worker session wanders from the task. It refactors adjacent code, adds features not in the prompt, or "improves" things outside the blast radius.

*Signal:* QA flags files outside blast radius. Diff is large relative to task complexity.

*Remediation:* The directive and blast radius sections exist for this reason. If drift is recurring, strengthen the prompt template: add an explicit "Do NOT modify any file not listed in the Blast Radius section" line. The launch preamble should reinforce this. For persistent drift, reduce max_turns to force focus.

### CI failures

**Fix agent loops.** A fix agent pushes a change that triggers a new CI validation, which fails differently, which dispatches another fix agent. The loop continues until MAX_FIX_AGENTS is consumed or the diff size cap triggers.

*Signal:* Multiple fix commits on the same PR in rapid succession. PR comment thread growing with alternating fail/fix/fail.

*Remediation:* The diff size cap (2x original change) is the circuit breaker. If a fix agent's changes exceed this, the system aborts and flags for human review. If loops persist below the cap, reduce MAX_FIX_AGENTS to 0 for that PR (manual fix required).

**Merge conflict storms.** Multiple PRs from the same batch modify files that are technically non-overlapping but cause conflicts at the merge level (adjacent lines, import blocks, configuration files).

*Signal:* Multiple PRs in the same batch fail trial merge after the first one is merged.

*Remediation:* The CI Manager re-validates after each merge (new HEAD SHA triggers re-check). Fix agents handle the rebase. For recurring conflict patterns, the planner should add shared files (e.g., module index files, dependency manifests) to the pitfalls list as "coordinate with other prompts in this batch."

### Systemic failures

**Observation overflow.** Workers produce many observations. Automated triage creates many issues. The issue tracker becomes noise. Nobody reads the issues. Knowledge is technically captured but functionally lost.

*Signal:* Open issue count growing faster than close rate. Backlog issues with 50+ appended observations that nobody has reviewed.

*Remediation:* Triage quality matters more than triage completeness. Add severity thresholds: only auto-create issues for Bug observations. Debt/Idea/Missing test/Doc gap append to a single weekly digest issue rather than individual items. The architect reviews the digest, not the individual observations.

**Standards drift.** The project's standards evolve (new lint rules, new conventions) but the prompt template's standards section still references old docs. Workers produce PRs that pass the old standards but fail the new ones.

*Signal:* CI failures on prompts that the QA gate marked PASS. The disconnect is between what QA evaluates (acceptance criteria) and what CI enforces (toolchain rules).

*Remediation:* The prompt template's standards section is auto-populated from project config. When standards change, the config changes, and all future prompts get the new standards. The fix is to never hand-write the standards section. Always generate it.

## Pipeline health metrics

Not cost tracking. System health signals that indicate whether the pipeline itself is performing well or degrading.

| Metric | Healthy | Degraded | Action |
|--------|---------|----------|--------|
| **Corrective prompt rate** | <10% of prompts need correction | >20% | Planner is under-specifying. Review acceptance criteria quality |
| **Stuck rate** | <5% of sessions | >15% | Prompts are too large. Decompose further, or increase max_turns |
| **QA false positive rate** | <5% of FAIL verdicts are wrong | >15% | Acceptance criteria are not diff-visible. Run prompt validation to catch them pre-dispatch. See "Acceptance criteria specificity" |
| **Fix agent success rate** | >80% of fix agents resolve the failure | <50% | Failures are too complex for mechanical fixes. Reduce MAX_FIX_AGENTS, let corrective prompts handle it |
| **Prompt-to-merge cycle time** | Batch completes in one dispatch run | Frequent multi-wave corrections before merge | Prompt quality issue or dependency misordering in execution plan |
| **Observation-to-issue rate** | >90% of observations become tracked items | <50% triage, overflow accumulating | Tighten severity threshold, switch to digest mode |
| **Batch parallelism ratio** | Average batch size >3 | Most batches are 1-2 prompts | Dependency DAG is too serial. Re-scope to decouple prompts |

These metrics are queryable from the state database. Track them per-project. A project with a 30% corrective prompt rate has a planner problem, not a worker problem. A project with a 20% stuck rate has a decomposition problem, not a capacity problem.

## Prompt conventions

### File naming

`NNN-domain-description.md`

The number is the prompt's identity. The domain groups related work. The description is human-readable.

### Branch naming

Conventional prefixes matching commit categories:

| Prefix | Use |
|--------|-----|
| `feat/<name>` | New functionality |
| `fix/<description>` | Bug fix |
| `refactor/<scope>` | Restructuring without behavior change |
| `docs/<topic>` | Documentation |
| `chore/<topic>` | Dependencies, CI, tooling |

### Parallel coordination

When multiple workers execute simultaneously:
- Each gets a unique branch name (derived from prompt number and domain)
- Prompts in the same batch declare non-overlapping blast radii
- If file overlap is unavoidable, the prompts must be in different batches (sequenced)
- Each prompt notes: "Other sessions may be running. Do not modify: <list of files owned by other prompts in this batch>"

### Project-Specific pitfalls

Each project maintains a pitfalls list. The prompt renderer injects relevant entries. These are the hard-won lessons that prevent recurring mistakes:

- Feature gate requirements for optional dependencies
- Vendored/generated directories that must not be hand-modified
- Lint configuration requirements for new modules
- Banned dependencies (with approved alternatives and rationale)
- Test organization rules (where integration vs. unit tests live)
- Default visibility rules (restrictive exports by default)
- Build/test commands that are known to timeout or require special flags
- Files that are frequently conflict sources across parallel prompts

## Design principles

**Prompts are the product.** Model capability is necessary but not sufficient. A precise prompt produces correct work. A vague prompt produces plausible waste. Quality originates in the generation pipeline. The strongest available model cannot compensate for ambiguous acceptance criteria. Invest in prompt quality over agent capability.

**Execution framing, not analysis.** Every prompt opens with an imperative directive. Without it, language models default to commentary. The directive is the most important sentence in the prompt. "Implement X" produces implementation. "Here is a description of X" produces analysis.

**QA gates, not reports.** Verification verdicts are control signals, not documents. FAIL blocks dependent work. PARTIAL generates corrective prompts. The system acts on its own evaluations. No human reads a QA report and decides what to do next. The loop is closed.

**Corrective over rollback.** Failed QA generates fix prompts, not reverts. Work already done has value. A corrective prompt targeting two unmet criteria is cheaper than re-executing the entire original. The system builds forward, never backward.

**Isolation by default.** Every worker operates in a worktree. No shared mutable state. Blast radii enforce scope. Parallel prompts in the same batch are non-overlapping by construction. Merge conflicts are detected at CI time via trial merge, not discovered at review time.

**Nothing is lost.** Worker sessions are ephemeral. The system is not. Every observation a worker makes survives the session via mandatory PR body capture and automated triage. Every QA verdict feeds back to the planner. Every health metric is queryable. The system's memory is structural, not dependent on any single session's context.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ Architect (human)                                                │
│   Approves plans. Sets direction. Resolves ambiguity.            │
│   Reviews held PRs. Everything else is below this line.          │
└──────────────┬───────────────────────────────────────────────────┘
               │
               v
┌──────────────────────────────────────────────────────────────────┐
│ Planner (high-capability AI agent)                               │
│                                                                  │
│   Research prompts ──> findings ──> architecture decisions        │
│   Execution plans  ──> batched dependency DAGs                   │
│   Implementation prompts (rendered from template function)       │
│   Standards, conventions, pitfalls                               │
│   Receives QA verdicts + observations ──> adjusts plan           │
└──────────────┬───────────────────────────────────────────────────┘
               │ prompt queue + execution plan
               │ (verdicts flow back up)
               v
┌──────────────────────────────────────────────────────────────────┐
│ Dispatcher (orchestrator service, Claude Code SDK)               │
│                                                                  │
│   Watch prompt queue, resolve dependencies, form groups          │
│   Per group:                                                     │
│     Launch N parallel worker sessions (worktree-isolated)        │
│     Monitor progress (async, stuck detection)                    │
│     Graduated resume on max_turns (80 -> 40 -> 20 -> diagnostic) │
│     Collect PRs                                                  │
│     QA gate: per-criterion evaluation                            │
│       PASS    ──> next group                                     │
│       PARTIAL ──> next group + corrective prompts queued         │
│       FAIL    ──> block dependents + diagnostic                  │
│     Corrective prompt generation (scoped to unmet criteria)      │
│   State database                                                 │
└──────────────┬───────────────────────────────────────────────────┘
               │ PRs on feature branches
               v
┌──────────────────────────────────────────────────────────────────┐
│ CI Manager (daemon)                                              │
│                                                                  │
│   Poll open PRs                                                  │
│   Per PR:                                                        │
│     Trial merge ──> conflict detection                           │
│     Validation gate ──> format, lint, test                       │
│     Post results as PR comment                                   │
│     On green + policy ──> auto-merge (tiered)                    │
│     On failure ──> fix agent (file-constrained, diff-capped)     │
│   Post-merge:                                                    │
│     Observation triage ──> issue tracker                         │
└──────────────────────────────────────────────────────────────────┘
```

## Dependencies

| Component | Dependency | Purpose |
|-----------|-----------|---------|
| Dispatcher | Claude Code SDK | Headless agent session management (async query, resume, progress streaming) |
| Dispatcher | Python 3.12+ | Async orchestration |
| CI Manager | gh CLI | PR listing, diffing, commenting, merging |
| All | git | Worktree management, repo sync, branch operations |
| All | Project toolchain | Language-specific formatter, linter, test runner |

## Operational boundaries

**What the architect does:**
- Approves execution plans before dispatch begins
- Sets project direction and makes architecture decisions
- Reviews PRs held by tiered auto-merge policy
- Resolves ambiguity the planner flags

**What the architect does not do:**
- Schedule or sequence prompts (the execution plan handles this)
- Relay messages between planner and dispatcher (direct handoff via structured API)
- Triage observations (automated post-merge)
- Monitor worker sessions (dispatcher handles progress, stuck detection, and resume)
- Decide what to do about QA failures (verdicts are control signals, not reports)

**What triggers architect involvement:**
- Plan approval (before dispatch begins)
- FAIL verdict on a foundational prompt after corrective prompt also fails
- PR held by merge policy (multi-module, public API, PARTIAL verdict)
- Health metric degradation flagged by the system (rising corrective rate, rising stuck rate)

Everything else runs.
