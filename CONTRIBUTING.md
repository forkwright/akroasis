# Contributing to Akroasis

Akroasis is a Rust workspace for multi-domain signal intelligence - radio, mesh, SDR, proximity, network defense, OSINT - correlated through a single typed signal model. It uses the self-hosted kanon forge as the authoritative PR surface. GitHub stays bidirectionally mirrored for external discoverability, but PRs live on the forge.

## Push target

```
origin = <forge>/forkwright/akroasis.git            (authoritative)
github = git@github.com:forkwright/akroasis.git     (mirror)
```

`<forge>` is this box's forge address. It is per-box state, not a property of this repository, so
it is not written down here — read it from your own `git remote -v`, or from the kanon MCP
binding that the session-start topology probe reports.

Push to `origin`. The forge post-receive hook runs CI (`.kanon-ci.toml`) and mirrors merge commits to GitHub via the pr-sync worker.

## Opening a PR

Two paths, same effect:

**Stoa UI.** Open `<forge>/prs/forkwright/akroasis`, click "New PR", pick base + head refs, review diff, submit.

**CLI.**

```bash
git push origin HEAD:refs/heads/<branch>
kanon pr open <branch> --title "..." --body "..."
```

`kanon pr open` prints the new PR number and its forge URL.

## Review

Comments and approvals land through stoa. The merge button activates when all gates report green:

- CI status `Pass` (every stage in `.kanon-ci.toml` exits zero, or the stage's `fail_on` predicate reports success).
- Independent verifier `Ok` (03f-e reproduces the headline claims from a fresh checkout of the head sha).
- A `Gate-Passed: kanon <version>` trailer is present on the tip commit of the PR branch, or the merge will append one.

## Merging

```bash
kanon pr merge <pr_number>
```

or the forge merge button. Default strategy is `squash`; `--strategy ff` or `--strategy rebase` are supported. The merge commit carries the `Gate-Passed` trailer.

Do not merge via GitHub. The GitHub mirror is read-only from the contributor's perspective: any merge performed there races the forge pr-sync worker and drops the trailer.

## External contributors

The GitHub mirror at `github.com/forkwright/akroasis` works as before. A PR opened on GitHub is ingested into the forge via the 05d bidirectional sync and then follows the normal review path above. The merge still happens on the forge; GitHub closes when the mirror sync observes the merge commit on `main`.

## Fallback

If the forge is unreachable, push to `github` and open a GitHub PR. When the forge is back, its pr-sync worker picks up the PR and continues from there. This is an escape hatch, not a preferred path - use it only when the forge is actually down.

## CI configuration

`.kanon-ci.toml` at the repo root defines the pipeline - the full Rust gate with per-stage concurrency capped at 8 so parallel rustc + nextest stay under the memory budget on the CI host:

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets --jobs 8`
- `cargo clippy --workspace --all-targets --jobs 8 -- -D warnings`
- `cargo nextest run --workspace --build-jobs 8 --test-threads 8`
- `kanon lint . --summary`

Keep `.kanon-ci.toml` in sync with `crates/archeion/src/ci_config.rs::default_rust_gate` when the upstream default changes - only the `--jobs` / `--test-threads` flags should differ.

Run the same gate locally before pushing:

```bash
kanon gate --stamp
```

`--stamp` appends the `Gate-Passed: kanon <version>` trailer to HEAD when every stage exits clean.

## Branch naming and commit format

Per `CLAUDE.md`: `feat/`, `fix/`, `docs/`, `refactor/`, `test/`, `cleanup/`. Commit messages are `category(scope): description`. Squash merges keep main linear.

## Backend-before-frontend ship-order

Do not expose a CLI or UI surface for verb V before V's backing subsystem is callable end-to-end and reaches an operator-visible success state. Surfaces ship after their dependencies, never before.

Where the dependency does not yet exist, the contributor has two acceptable shapes:

- Omit the surface entirely until the subsystem lands.
- Stub the handler with a typed loud-error return (e.g., `NotImplementedYet`) whose message names the tracking issue. Never ship a silent fake success path (a print-only stub, an `awaiting ACK` message with no network work, an `Ok(())` short-circuit).

Anti-pattern examples tracked in this repo:

- #121 - `akroasis serve` daemon subcommand existed with no transport implementation; other subcommands funneled users toward it.
- #122 - radio hardware adapters are `StubHardware`-only; the Baofeng protocol is unwired but advertised.
- #123 - `mesh send` printed `awaiting ACK` while doing no network work because no daemon received the packet.

Reviewers should block PRs that add a new surface ahead of its subsystem until either the subsystem lands in the same PR or the surface is downgraded to a loud-error stub per above.
