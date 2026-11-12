# Workflow

How Drift work gets done. Short on purpose. If this and another note disagree, this wins. Two layers since 2026-11-12.

## Who does what
- **The user** decides what matters, confirms each task's Done when, makes product calls, and merges `gated` PRs.
- **The lead**: one Claude Code session, the Drift Coordinator.
  - It plans with the user directly, against the live repo.
  - It starts workers.
  - It has a subagent review each PR and passes the findings to the worker.
  - It merges `auto` PRs.
  - It keeps the records.
  - It writes no product code.
- **Workers**: at most 2 at a time, one issue each, each in its own worktree on E:. A worker reports to the lead and never starts sessions.
- A planning chat outside the repo can be used for open-ended thinking. Nothing from it becomes work until the lead has checked it against the repo and the user has confirmed the Done when.

## Where things live
- **GitHub issues and labels** are the backlog and the status. Open PRs show what is in flight.
- **Decisions** are a comment on the issue they're about, posted by the lead when the user makes them. `LOG.md` is frozen at 2026-11-12, as history.
- **`PAPERCUTS.md`** is the inbox for small usability annoyances, one line each. The user reports one to the lead in a few words. Small ones are cleared in a sweep PR, which marks its rows fixed. Bigger ones become issues.
- **`FEATURES.md`** lists what Drift does, each line pointing to code.

There are no records-only PRs. Records change only inside the PR that does the work, such as a sweep marking its papercuts fixed, or a PR that changes this file.

## Rules
1. **No "Done when", no work.** An issue gets a checkable Done when (a named test, a measured number, or a behaviour you can see) before anyone starts it. The lead confirms it with the user in one message.
2. **No overlapping files.** Every task lists the files it touches. The lead never runs two tasks whose files overlap.
3. **Two tiers:**
   - `auto`: papercuts, UI, docs, tests, refactors. The lead merges once CI is green and the review, a security note and a product check are on the PR.
   - `gated`: solver/drag, file format, save/load, import/export, release, CI. These wait for the user.
4. **Reviews.** A review subagent from the lead reviews every PR. Solver and drag PRs also get separate Opus and Fable reviewer sessions.
5. **Red main stops everything.** Fix main first. Never merge on "red only for known reasons".
6. **No band-aids on the solver.** A solver issue is closed by a fix for its cause, or left open with what was learned. A test is only ignored for proven flakiness, with a note pointing to the issue.
7. **Clean up as you go.** When a PR merges, archive its sessions and delete its worktree and target.
8. **Batch the user.** Don't ping for each thing. Put it in the lead's turn report.

## Rules learned
Each rule came from something that went wrong.
- Nothing becomes work until the lead has checked it against the live repo and confirmed its Done when with the user. A plan written from a stale view cost two workers on 2026-11-12.
- Workers never start sessions. They report the need to the lead.
- After any Cargo.lock change, run `python scripts/notices.py` and commit THIRD-PARTY-NOTICES.txt in the same PR.
- After starting a reviewer with a model, check that it is really running that model.
- Use one fresh reviewer per PR, and delete its target after the merge.
- Before claiming Drift lacks a feature, check FEATURES.md and grep the code.
- No hard wall-clock asserts in normal tests. Perf budgets live in the ignored baselines.
- Any PR touching drag, solver, index or fills runs the ignored perf baselines (`cargo test -- --ignored`) and puts the numbers in the PR.
- Before merging, check that the PR's base is `main`.

## Clearing the issues log
Work in batches of up to 5. The order is papercuts, then flaky tests, then perf, then solver, which follows the audit's PR sequence (`docs/solver-audit.md`).
