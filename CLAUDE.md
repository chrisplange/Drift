# Drift: notes for Claude Code sessions

Drift is a Rust 2D parametric CAD sketcher. How work is run: [WORKFLOW.md](WORKFLOW.md).
What Drift does: [FEATURES.md](FEATURES.md). Plan (only §1–3 current): [PLAN.md](PLAN.md).

## Start every task
- `git fetch origin`, then work in a fresh worktree branched from `origin/main` (worktrees
  live on `E:\Drift\worktrees`). A checkout can be stale; never plan from it.
- One issue per session. The issue's "Done when" is the finish line.

## Build and test (from CONTRIBUTING.md)
```
cargo fmt --all
cargo clippy --workspace --all-targets
cargo clippy --target wasm32-unknown-unknown -p drift-web
cargo test --workspace
```
Smoke suite alone: `cargo test -p drift-app smoke_`.

## Tiers
A PR's tier comes from the paths it changes. [.github/tiers.txt](.github/tiers.txt) is the
source of truth (`scripts/tier.py` computes it); the summary below is a guide only. Highest
tier touched wins.

| Tier | Paths (rough summary; see tiers.txt) | Merged by |
|---|---|---|
| solver | drift-solver, drift-core constraint/tolerance, drag_props, the editor's drag/solve files, scripts/solver-bar | owner, after `Solver review` is green |
| gated | drift-io, file format, Cargo.toml/lock, .github, packaging, app save/export/recovery, AI link, updater, CLAUDE.md, WORKFLOW.md, .claude | owner |
| auto | everything else | itself, on green CI |

`[heavy]` in tiers.txt is not a tier: it marks the broader paths that also run the heavy drag matrix.

- **Sessions never merge.** Auto PRs merge themselves on green; the owner merges the rest.
- **A branch that falls behind main** is brought up to date by its own session with
  `gh pr update-branch`. The auto-merge job can't do it: a push made with its token starts no CI.
- **Solver-tier PRs** get a separate reviewer session (Opus; solver/drag also Fable). The
  reviewer posts findings on the PR. The owner's approval comment carries
  `<!-- drift-review: approve <head-sha> -->`, which turns `Solver review` green.
- **Decisions** go as a comment on the issue they're about. LOG.md is frozen history.
- [docs/solver-audit.md](docs/solver-audit.md) and its decisions stand.

## Rules learned
- After any Cargo.lock change, run `python scripts/notices.py` and commit
  THIRD-PARTY-NOTICES.txt in the same PR.
- A PR touching drag, solver, index or fills runs the perf baselines
  (`cargo test -- --ignored`) and puts the numbers in the PR body.
- Before claiming Drift lacks a feature, check FEATURES.md and grep the code.
- No wall-clock asserts in normal tests; perf budgets live in the ignored baselines.
- Check that the PR's base is `main`.
