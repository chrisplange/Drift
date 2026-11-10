# Hub queue

What is running or next: 5 rows at most (see `WORKFLOW.md`). GitHub issues are the backlog; the batch labels below say what comes next. The coordinator changes **Status**, **PR** and **Session**; workers and reviewers check each PR against its **Done when**.

Last updated 2026-11-10.

## Rules for a task entry

- **ID** `H-<n>`, never reused. Link the GitHub issue.
- **Done when** must be checkable: a named test that passes, a measured number, or a behaviour you can see in the app.
- **Files** lists the files or areas the task touches, with their callers and those callers' tests. Two tasks whose Files overlap never run at once.
- **Tier**: `auto` (papercuts, UI, docs, tests, refactors) merges once CI is green and the review notes are on the PR; `gated` (solver/drag, file format, save/load, import/export, release, CI) waits for the user's OK.
- **Status**: `offered` → `ready` → `running` → `in-review` → `merged` / `blocked` / `dropped`.

## Running and next

| ID | Issue | Task | Done when | Files | Tier | Status | PR | Session |
|---|---|---|---|---|---|---|---|---|
| H-4 | #343, #187 (corpus half) | Solver redesign PR 0: the bar as code, tests only (`docs/solver-audit.md` §5) | Similarity census committed as an ignored test; exit-reason counters as test-only instrumentation; `analyse.py` committed; drag properties check structural rows (an arc's end on its circle, midpoints); main's numbers for all seven parts of the bar recorded in the PR; nothing deleted | `crates/drift-solver/` (tests and test-only instrumentation), `crates/drift-edit/` `drag_props.rs` and `drag_props/`, `tools/analyse.py` | gated | offered (waits for the user's go) | | |

After PR 0: PR 1 (#352) may go at any time, then PR 2. One solver PR in review at a time.

## Done

| ID | Issue | Task | PR | Merged |
|---|---|---|---|---|
| H-1 | #264 | Shift+D then Esc leaves no copy stacked on the original (smoke test) | #379 | 2026-11-10 |
| H-2 | #337 | Relation glyphs 18 pt and kept off the transform handles | #382, review fixes #385 | 2026-11-10 |
| H-3 | #242 | Auto horizontal/vertical window widened to 3° for sketched lines | #380 | 2026-11-10 |
| — | #386 | Solver audit: root causes and redesign plan; all nine decisions accepted (LOG.md) | #386 | 2026-11-10 |

The trial is over: self-merge of `auto` tasks is on. #264 and #337 are still open on GitHub after their PRs merged; close them.

## Batches (issue labels, 2026-11-10)

Work a batch at a time, up to 5 issues, in this order: papercuts, flaky tests, perf, solver. Each issue gets its Done when in the planning chat before it starts.

- **batch-1** (papercuts): #367, #338, #326, #306, #245
- **batch-2** (flaky tests): #368, #353, #297, #296, #295 (walls join bug, not solver; its fix adds the four saved seeds)
- **batch-3** (big-drawing perf): #373, #372, #371, #370, #285, #253
- **solver-audit** (closed only by the redesign PRs 0–6): #387, #366, #362, #352, #351, #350, #349, #347 (blocked by #313), #344, #343, #280, #187
- **later**: #369, #345, #314, #313, #307, #305, #291, #290, #289, #244, #243, #231, #183
