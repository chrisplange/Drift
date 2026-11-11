# Workflow

How Drift work gets done. Short on purpose. If this and another note disagree, this wins.

## Who does what
- **The user** plans in the Claude Project (no code there), approves `gated` PRs, and clears `USER-ACTIONS.md` when it suits them.
- **The hub** (one Claude Code session, started by the user from the sidebar, running `/hub`) runs tasks, gates PRs and keeps the records.
- **Workers**: at most 2 at a time, one per task, each in its own worktree on E:.

## Where things live
- **GitHub issues** are the only backlog.
- **`HUB-QUEUE.md`** holds only what is running or next: 5 rows at most.
- **`LOG.md`** has one line per decision, approval or merge.
- **`LEARNINGS.md`** records what went wrong and the rule that came out of it.
- **`FEATURES.md`** lists what Drift does, each line pointing to code.
- **`USER-ACTIONS.md`** lists things only the user can do.
- **`PAPERCUTS.md`** is the inbox for small usability annoyances, one line each. The hub and workers log papercuts there instead of opening issues; the user can report one in a few words to the coordinator or the notes session. A planned batch groups open papercuts into one issue with a Done when.

## Rules
1. **No "Done when", no work.** An issue gets a checkable Done when (a named test, a measured number, or a behaviour you can see) before anyone starts it.
2. **Files listed up front.** A task lists the files it touches, including the code that calls what it changes and those callers' tests. The hub never runs two tasks whose files overlap.
3. **Two tiers:**
   - `auto`: papercuts, UI, docs, tests, refactors. The hub merges once CI is green and a security note, a product check and a review are on the PR.
   - `gated`: solver/drag, file format, save/load, import/export, release, CI. These wait for the user.
4. **Reviews:** `auto` PRs are reviewed by an agent inside the hub, posted on the PR. Solver and drag PRs get separate Opus and Fable reviewer sessions.
5. **Red main stops everything.** Fix main first. Never merge on "red only for known reasons".
6. **No band-aids on the solver.** A solver issue is closed by a fix for its cause, or left open with what was learned. Ignoring a test is only for proven flakiness, with a note pointing to the issue.
7. **Clean up as you go.** When a PR merges, archive its sessions and delete its worktree and target.
8. **Batch the user.** Don't ping for each thing. Add it to `USER-ACTIONS.md` or the hub's turn report.

## Clearing the issues log
Work in batches of up to 5, in this order: papercuts, then flaky tests, then perf, then solver. Each batch gets its Done when lines in the planning chat first.
