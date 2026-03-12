# Git Workflow

## Worktree Convention

All feature work happens in a dedicated git worktree:

```bash
git fetch origin
git worktree add /home/builder/akroasis-worktrees/<ticket-slug> -b <ticket-slug> origin/main
cd /home/builder/akroasis-worktrees/<ticket-slug>
```

Path pattern: `/home/builder/akroasis-worktrees/<ticket-slug>`
Branch pattern: `<ticket-slug>` (e.g. `feat/p0-01-workspace-scaffold`)

Always branch from `origin/main`. Never branch from a local branch.

## Commit Format

```
category(scope): short imperative description
```

- `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `style`
- Scope is the affected crate or area: `koinon`, `akroasis`, `ci`, `docs`
- Subject line ≤ 72 characters, no trailing period
- Body (optional): explain *why*, not *what*

Examples:
```
feat(koinon): add Frequency newtype with unit constructors
fix(akroasis): box figment::Error to reduce Error enum size
chore: add workspace clippy lints for unwrap/expect/panic
```

## Validation Gate

Run before opening a PR:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All three must pass with zero warnings/errors.

## PR Format

Title: `category(scope): short description` (same format as commit)

Body:
```markdown
## Summary
- Bullet points describing what changed and why

## Test plan
- [ ] cargo build && cargo test --workspace
- [ ] cargo clippy --workspace --all-targets -- -D warnings
- [ ] Manual verification steps if applicable
```
