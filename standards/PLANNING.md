# Planning

> Standards for project planning, phase management, and decision tracking. Applies to all forkwright projects managed through kanon.

---

## Philosophy

Planning serves execution. Every planning artifact exists to answer one question an agent or operator will ask during work. If a document doesn't answer a question someone will actually ask, it doesn't belong.

Planning is not project management. No timelines, no story points, no sprint ceremonies, no resource allocation. One operator, AI agents, and a compiler. Complexity that requires a team is a bug.

Plans are living documents. They change as work reveals reality. Stale plans are worse than no plans because they actively mislead. Update or archive -- never leave a plan that contradicts the codebase.

---

## Directory structure

Every project in `projects/{name}/` follows this structure:

```
projects/{name}/
├── CLAUDE.md           # Agent context -- architecture, patterns, decisions, gotchas
├── vision.md           # Why the project exists, design principles, strategic moat
├── STATE.md            # Current position, locked decisions, active blockers
├── ROADMAP.md          # Phase index -- lean table of phases with goals and success criteria
├── phases/
│   ├── 01-{name}/
│   │   ├── PLAN.md     # Full phase detail: scope, requirements, decisions, open questions
│   │   └── SUMMARY.md  # Post-completion: what shipped, what was learned, what changed
│   ├── 02-{name}/
│   │   ├── PLAN.md
│   │   └── SUMMARY.md
│   └── NN-{name}/      # Active phase -- has PLAN.md but no SUMMARY.md yet
│       └── PLAN.md
└── planning/
    └── archive/        # Legacy docs (old roadmap.md, backlog.md, etc.)
```

### Required files

| File | Purpose | Update frequency |
|------|---------|-----------------|
| `CLAUDE.md` | Agent orientation. Architecture, key types, common tasks, gotchas. | On every major architectural change |
| `vision.md` | Soul of the project. Philosophy, principles, moat. Why it exists. | Rarely -- when the fundamental direction shifts |
| `STATE.md` | Where we are right now. Current phase, locked decisions, blockers. | Every session -- this is the "resume point" |
| `ROADMAP.md` | Phase index. What's done, what's active, what's planned. | When phases complete or new phases are identified |

### Phase lifecycle

```
Identified → PLAN.md written → Executing → SUMMARY.md written → Complete
```

1. **Identified**: phase appears in ROADMAP.md with a goal and success criteria
2. **Planned**: `phases/NN-name/PLAN.md` written with full detail
3. **Executing**: prompts generated from PLAN.md, dispatched, PRs merging
4. **Complete**: `SUMMARY.md` written, STATE.md updated, ROADMAP.md marked done

A phase is complete when its success criteria are met, not when all planned tasks are done. If criteria are met with fewer tasks, the phase is done. If criteria need more tasks than planned, the plan was wrong -- update it.

---

## File formats

### ROADMAP.md

The roadmap is an index. It answers: "what phases exist and what's their status?" It does NOT contain implementation detail, PR lists, prompt counts, or timelines.

```markdown
# Roadmap -- {Project}

## Active

### Phase nN: {Name}
**Goal:** {One sentence -- what must be TRUE when this phase completes}

Success criteria:
- {Observable behavior or state, not a task}
- {Another observable behavior}

## Planned

### Phase nN: {Name}
**Goal:** {One sentence}

Success criteria:
- {Observable behavior}

## Completed

### Phase nN: {Name} ✓
**Goal:** {One sentence}
**Completed:** {date}
```

Rules:
- One table or list per section (active, planned, completed)
- Success criteria are observable outcomes, not tasks ("users can search facts by entity" not "implement search endpoint")
- No PR numbers, no prompt numbers, no LOC counts, no timelines
- Completed phases link to their `phases/NN-name/SUMMARY.md` for detail
- The goal is: an agent reads this and knows what the project is building and what's next

### vision.md

The vision is the soul. It answers: "why does this project exist and what principles guide decisions?"

```markdown
# Vision -- {Project}

## Purpose
{Why this project exists. What problem it solves. Who it serves.}

## Principles
{Design principles that guide every decision. Not aspirational -- these are constraints.}

## Strategic moat
{What makes this project defensible. What a competitor would need to replicate.}

## Non-goals
{What this project deliberately does NOT do. Boundaries.}
```

Rules:
- Philosophy, not features. "Privacy as architecture" not "XChaCha20Poly1305 encryption"
- Stable across releases. If vision changes quarterly, it's not a vision
- Short -- one page. If it takes more, the vision isn't clear enough

### STATE.md

State is the resume point. It answers: "where are we and what decisions are locked?"

```markdown
# State -- {Project}

## Current phase
Phase NN: {Name}
Status: {executing / blocked / planning}

## Locked decisions
{Decisions that are final and must not be revisited without explicit operator approval}

- {Decision}: {rationale}
- {Decision}: {rationale}

## Active blockers
{What's preventing progress right now}

- {Blocker}: {what's needed to unblock}

## Recent context
{Brief notes from the most recent work session -- what was done, what's next}
```

Rules:
- Updated every work session
- Locked decisions are non-negotiable for agents -- if an agent wants to revisit, it must surface this to the operator
- Blockers are actionable -- each has a path to resolution
- Recent context prevents the "what was I doing?" problem across sessions

### PLAN.md (per phase)

The plan is the full detail for a phase. It answers: "what exactly needs to happen to complete this phase?"

```markdown
# Phase NN: {Name}

## Goal
{What must be TRUE when this phase completes -- same as ROADMAP.md}

## Success criteria
{Same as ROADMAP.md -- the contract}

## Scope
{What's in scope and what's explicitly out}

### In scope
- {Specific deliverable}
- {Specific deliverable}

### Out of scope
- {Thing that might seem in scope but isn't}

## Requirements
{Detailed requirements that success criteria decompose into}

- {REQ-01}: {Specific, testable requirement}
- {REQ-02}: {Specific, testable requirement}

## Decisions
{Architectural and technical decisions made for this phase}

| Decision | Choice | Rationale |
|----------|--------|-----------|
| {What was decided} | {What was chosen} | {Why} |

## Open questions
{Things that need to be resolved during execution}

- {Question}: {context, options if known}

## Dependencies
{What must be true before this phase can start}

- {Dependency}: {status}
```

Rules:
- PLAN.md is the source of truth for generating prompts
- Requirements are specific and testable -- "add X endpoint" not "improve the API"
- Decisions are final once written (move to STATE.md locked decisions if cross-phase)
- Open questions are resolved during execution -- update the plan when they're answered
- No implementation detail that belongs in code comments or CLAUDE.md

### SUMMARY.md (per phase)

The summary is the historical record. It answers: "what actually happened in this phase?"

```markdown
# Phase NN: {Name} -- Summary

## Outcome
{Did the phase meet its success criteria? What shipped?}

## Key changes
{The most important things that changed, not an exhaustive PR list}

- {Change}: {impact}
- {Change}: {impact}

## Decisions made
{Decisions that emerged during execution -- these feed future phases}

- {Decision}: {rationale}

## Lessons
{What we learned that applies to future work}

- {Lesson}

## Metrics
{If relevant: LOC, test count, performance numbers -- facts not vanity metrics}
```

Rules:
- Written after all success criteria are met
- Captures decisions and lessons -- the institutional knowledge
- Not an exhaustive log -- key changes that matter for understanding the project
- Feeds into training data capture (kanon training/ JSONL)

---

## Relationship to other kanon systems

### Prompts

Prompts live in `kanon/workflow/prompts/{queue,done}/`. They are generated from PLAN.md content but are separate artifacts. A PLAN.md describes what needs to happen; prompts are the dispatched work units.

Naming: `NNN-type-project-description.md`

### Research

Research lives in `kanon/workflow/research/{inbox,archive}/`. Research informs PLAN.md content. When research findings are integrated into a phase plan, the research document moves to archive.

### Standards

Standards live in `kanon/crates/basanos/standards/`. They define how code is written. Plans define what code is written. Plans reference standards ("follow RUST.md") but don't duplicate them.

### Training data

Phase summaries, prompt results, and QA evaluations feed `kanon/workflow/training/` JSONL files. The planning system produces training data as a byproduct.

---

## Migration from legacy structure

Projects with existing `roadmap.md` + `backlog.md` + `requirements.md` migrate as follows:

1. Create `STATE.md` from current roadmap "Current Phase" section
2. Create lean `ROADMAP.md` from roadmap phase list (strip PR tables, counts, timelines)
3. Completed phases → `phases/NN-name/SUMMARY.md` (key outcomes, not full PR history)
4. Active phase → `phases/NN-name/PLAN.md` (scope, requirements, decisions)
5. Backlog items → future phases in ROADMAP.md
6. `requirements.md` → fold into relevant phase PLAN.md files
7. `vision.md` → keep, update if stale
8. Archive old `roadmap.md`, `backlog.md`, `requirements.md` in `planning/archive/`

The migration preserves all information -- nothing is deleted, just reorganized.
