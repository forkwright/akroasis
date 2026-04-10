# Agent docs

> Standards for repository context files: CLAUDE.md, AGENTS.md, llms.txt, and similar static instruction files read by agent harnesses. Provider-agnostic principles that apply to any AI coding agent.

**Scope:** Static context files checked into repositories or placed in user config directories. Does not cover API prompt construction (system messages, user messages, tool calls); see [PROMPTING.md](PROMPTING.md) for those.

---

## Principle

Agent context files are instructions, not documentation. Every line costs tokens and competes for attention with the agent's built-in behavior. The goal is to change agent behavior on things it would otherwise get wrong -- not to describe the codebase.

---

## Inclusion filter

Before adding any content, it must pass all four criteria:

1. **Not discoverable** -- the agent cannot find this from README, configs, help commands, or reading code
2. **Actionable** -- the agent can execute a specific action based on this instruction
3. **Prevents silent failure** -- without this, the agent would produce a plausible but wrong result
4. **Broadly applicable** -- this applies to most tasks, not just edge cases

Content that fails any criterion does not belong in the context file.

---

## File types and placement

### CLAUDE.md (Claude Code)

Tool-specific context for Claude Code sessions. Loaded automatically from:
- `~/.claude/CLAUDE.md` -- global (all sessions)
- `./CLAUDE.md` -- project root (checked into git)
- Parent directories -- monorepo support
- Child directories -- loaded when agent works in that directory

### AGENTS.md (cross-tool standard)

Open standard (agents.md, Linux Foundation). Read by Claude Code, Cursor, Windsurf, Copilot, Codex, Jules, and others. Place at project root or any subdirectory -- closest file to edited code takes precedence.

Use AGENTS.md for cross-tool content. Use CLAUDE.md for Claude Code-specific features (hooks, skills, Claude-specific behavioral directives).

### llms.txt (website/documentation)

Navigation index for LLMs consuming documentation sites. Not for code repos -- for websites. Format:
- H1: project name
- Blockquote: summary
- H2 sections: lists of `[name](url): description` links
- `/llms.txt` for navigation, `/llms-full.txt` for full content

---

## Structure

### Required sections (in this order)

**1. Non-standard tooling** -- commands the agent cannot guess

```markdown
## Commands
- Build: `cargo build --release`
- Test: `cargo nextest run --workspace`
- Lint: `kanon lint . --summary`
- Gate: `kanon gate` (must pass before pushing)
```

This is the single highest-value section. ETH Zurich research shows agents use mentioned tools 2.5x more often. Repository-specific tools (`kanon`, `uv`, custom scripts) see 1.6x usage increase.

**2. Non-obvious rules** -- things the agent would get wrong

```markdown
## Rules
- Use `#[expect(lint, reason = "...")]` not `#[allow]` -- because #[expect] warns when the suppression becomes stale
- All PRs need `Gate-Passed: kanon 0.1.0` in the commit body -- the CI verify-gate check requires this
- Desktop crate is excluded from workspace -- build standalone with `cargo check -p theatron-desktop`
```

Every rule explains WHY. Without the explanation, the agent follows the rule rigidly but cannot generalize to novel situations.

**3. Architecture decisions** -- non-obvious structural choices

Only include decisions the agent would violate without knowing:

```markdown
## Architecture
- Errors: snafu with .context() and Location tracking (not thiserror, not anyhow)
- IDs: newtypes for all domain IDs (not raw String/u64)
- Async: Tokio actor model, not shared mutable state
```

Do NOT include: crate-by-crate descriptions, file trees, module maps. Agents explore codebases well without guidance.

**4. Boundaries** -- three-tier permission framework

```markdown
## Boundaries
Always: run cargo fmt before committing, stay within blast radius
Ask first: changes to public API surface, database migrations
Never: push to upstream, delete branches without checking, use --admin to bypass CI
```

### Optional sections

**Code examples** -- one concrete example outperforms lengthy descriptions. Show the desired pattern, not a paragraph explaining it.

**Environment quirks** -- required env vars, PATH additions, platform-specific setup that would cause silent failure.

**Testing specifics** -- non-standard test runners, required flags, fixtures that must exist.

---

## Size limits

| Scope | Target | Maximum |
|-------|--------|---------|
| Always-loaded root file | Under 100 lines | 300 lines |
| Per-crate/per-directory file | Under 50 lines | 100 lines |
| Combined across all loaded files | Under 200 lines | 500 lines |

Research shows agent performance degrades when context files exceed ~500 words in always-loaded mode. The agent's system prompt already contains ~50 instructions -- every additional instruction competes for adherence.

---

## Voice

General prompt voice rules (positive over negative, WHY on every rule, dial back aggressive emphasis) apply here too -- see [PROMPTING.md § Voice](PROMPTING.md#voice). The following are specific to context files:

- **Direct and terse** -- every word costs tokens in always-loaded files; context files compete for attention more than one-shot prompts
- **Concrete over abstract** -- "run `kanon gate`" not "ensure CI passes" because agents cannot act on vague instructions without additional lookup
- **Imperative mood** -- "add tests for new public functions" not "tests should be added" because imperative framing maps directly to agent actions

---

## Anti-patterns

- **Codebase overviews** -- agents navigate codebases well without guidance. Over 90% of files with overviews showed no measurable navigation improvement.
- **LLM-generated content** -- ETH Zurich research shows LLM-generated context files reduce success rates by 3% while increasing cost 20%+. Manually craft every line.
- **Linter-enforceable rules** -- use actual linters and hooks. Asking an agent to check formatting wastes context on something a tool does better.
- **README duplication** -- if it's in the README, package.json, or config files, agents already find it.
- **Generic boilerplate** -- "write clean code", "follow best practices", "be thorough" are noise. Agents already try to do these things.
- **Task-specific workarounds** -- context files are loaded for every session. Content that applies to one task wastes tokens on all others.
- **Frequent updates** -- if content changes weekly, it doesn't belong in a context file. Use dynamic injection (MCP, tools) instead.

---

## Per-crate CLAUDE.md template

For subdirectory context files in monorepos:

```markdown
# {crate-name}

{One-sentence purpose.}

## Commands
- Check: `cargo check -p {crate-name}`
- Test: `cargo test -p {crate-name}`

## Gotchas
- {Thing that would cause silent failure if not known}
```

Under 30 lines. Only include what passes the 4-question filter.

---

## llms.txt template

```markdown
# Project Name

> One-sentence project description with key technical details.

## Documentation
- [Architecture](docs/ARCHITECTURE.md): crate dependencies and data flow
- [API Reference](docs/API.md): HTTP endpoints and CLI commands
- [Deployment](docs/DEPLOYMENT.md): setup and configuration

## Optional
- [Changelog](CHANGELOG.md): version history
- [Contributing](CONTRIBUTING.md): development workflow
```

---

## Verification

After writing or updating an agent context file:

1. Read it as if you know nothing about the project -- is every instruction actionable without additional context?
2. For each line: does it pass all 4 inclusion criteria? Remove if not.
3. Check line count against limits. If over, prioritize by failure severity -- which instructions prevent the worst mistakes?
4. Test with a fresh agent session -- does the agent follow the instructions? If not, the file is too long or the instructions are too vague.
