# Hub queue

What is running or next: 5 rows at most (see `WORKFLOW.md`). GitHub issues are the backlog; the batch labels below say what comes next. The coordinator changes **Status**, **PR** and **Session**; workers and reviewers check each PR against its **Done when**.

Last updated 2026-11-12.

## Rules for a task entry

- **ID** `H-<n>`, never reused. Link the GitHub issue.
- **Done when** must be checkable: a named test that passes, a measured number, or a behaviour you can see in the app.
- **Files** lists the files or areas the task touches, with their callers and those callers' tests. Two tasks whose Files overlap never run at once.
- **Tier**: `auto` (papercuts, UI, docs, tests, refactors) merges once CI is green and the review notes are on the PR; `gated` (solver/drag, file format, save/load, import/export, release, CI) waits for the user's OK.
- **Status**: `offered` → `ready` → `running` → `in-review` → `merged` / `blocked` / `dropped`.

## Running and next

| ID | Issue | Task | Done when | Files | Tier | Status | PR | Session |
|---|---|---|---|---|---|---|---|---|
| H-9 | #349 (PR 3 review items 1 and 2) | Solver stage-2 stall: steps keep to the rows' solutions and stop on the step | Heavy drag matrix matches `expected.txt`; free-move seeds 3, 6, 17, 18, 19 pass again; no perf line slower than main; Linux matches Windows | `crates/drift-solver/` `newton` second stage, `Shape::Direction` | gated | running (draft) | #400 | solver-fix-the-stage-2-s |
| H-7 | #350, #284 (commit side) | Solver redesign PR 6: commits keep shapes | Commits keep line lengths and directions and arc radii where the relations leave room, ranked below relations and named geometry; before/after on the relation-adding census (#333's corpus) in the PR | `crates/drift-solver/`, `crates/drift-edit/` editor `solve.rs` and its tests | gated | in-review | #399 | solver-pr6 |

Solver scope is PRs 2, 3 and 6 only; 2 and 3 are merged. PR 1 (#352), PR 4 (#344) and PR 5 (#362) are parked as `later`. After PR 6, work returns to the usability and bug batches; batch-1 waits on its Done when lines from the planning chat.

## Done

| ID | Issue | Task | PR | Merged |
|---|---|---|---|---|
| H-8 | — | Main green after PR 3: whole-part slide removed ("stop the drag"), circles held as arcs, Linux `held_shapes` and Windows `regions_props` fixed; free-move seeds 3, 6, 17, 18, 19 now expected failures | #398 | 2026-11-12 |
| H-1 | #264 | Shift+D then Esc leaves no copy stacked on the original (smoke test) | #379 | 2026-11-10 |
| H-2 | #337 | Relation glyphs 18 pt and kept off the transform handles | #382, review fixes #385 | 2026-11-10 |
| H-3 | #242 | Auto horizontal/vertical window widened to 3° for sketched lines | #380 | 2026-11-10 |
| H-4 | #343, #187 (corpus half) | Solver redesign PR 0: the acceptance bar as code, tests only (Linux heavy 277 ok / 3) | #389 | 2026-11-10 |
| H-5 | #366, #387, #280 (five pins) | Solver redesign PR 2: one frame, one bar (`tangent_and_parallel` stays ignored, owned by PR 3) | #391 | 2026-11-10 |
| H-6 | #349, #280 | Solver redesign PR 3: a principled stop (held arcs never grow in a drag; `tangent_and_parallel` un-ignored; all seven #280 pins pass; merged without the separate Opus/Fable reviews) | #394 | 2026-11-11 |
| — | #386 | Solver audit: root causes and redesign plan; all nine decisions accepted (LOG.md) | #386 | 2026-11-10 |

The trial is over: self-merge of `auto` tasks is on. #264 stays open for its follow-up (Shift+D with no pointer, or on a locked layer). #337 is still open after its PRs merged; close it.

## Batches (issue labels, 2026-11-10)

Work a batch at a time, up to 5 issues, in this order: papercuts, flaky tests, perf, solver. Each issue gets its Done when in the planning chat before it starts.

- **batch-1** (papercuts): #367, #338, #326, #306, #245
- **batch-2** (flaky tests): #368, #353, #297, #296, #295 (walls join bug, not solver; its fix adds the four saved seeds)
- **batch-3** (big-drawing perf): #373, #372, #371, #370, #285, #253
- **solver-audit** (PR 6 closes #350): #350, #351 (left open by PR 3: ~0.3 s on main and PR 3), #347 (blocked by #313), #187
- **solver, not in an audit PR**: #392 (solve says converged, analyze disagrees; fails on main)
- **later**: #352, #344, #362 (parked solver PRs 1, 4 and 5; still labelled solver-audit), #369, #345, #314, #313, #307, #305, #291, #290, #289, #244, #243, #231, #183
