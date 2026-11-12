# Status

What is running, what is next, what waits on the user, and the rules learned the hard way. It's kept short on purpose. GitHub issues and their labels are the backlog, and open PRs show what is in flight. The lead session updates this file once per batch, not once per event.

Last updated 2026-11-12.

## Running

| Issue | What | PR | Tier |
|---|---|---|---|
| #264 (follow-up) | Shift+D leaves no stray copy when the pointer is off the canvas or on a locked layer | #412 | auto |
| #338 | A Trim drag across walls trims them whole (decided 11-12: trim properly) | #413 | auto |
| #367 + PAPERCUTS.md rows | Papercut sweep: wall Space "center", scale box, sheet-scale hint, hatch menu, door jamb (small fixes only) | open | auto |

## Next

1. **batch-1, the rest:**
   - #245, gated. Decided 11-12: export with no sheets asks how to place the drawing.
   - #306, auto with a perf gate.
2. **batch-2, flaky tests:** #295, #296, #297 (also its macOS `f64::clamp` panic), #353, #409, #368. That's six, so one waits.
3. **batch-3, big-drawing perf:** #373, #372, #371, #370, #285, #253.
4. **Papercuts that need their own issue and a design talk:**
   - mirror with a temporary axis;
   - adding and editing annotations in sheet space (the user's two most-needed);
   - click-to-cycle through overlapping entities;
   - live on-canvas text;
   - the right-panel review;
   - layer 0 policy;
   - geometry merging across layers;
   - keeping an uploaded logo;
   - duplicating a sheet view.

Before a task starts, the lead confirms its Done when with the user.

## Parked

- **Solver.** The audit's PRs 2, 3 and 6 are merged.
  - #400 (stage-2 stall) is a draft, parked by the user. #407 is decided together with it.
  - PRs 1, 4 and 5 (#352, #344, #362) are `later`.
  - `docs/solver-audit-references.md` (#408) says "Nothing now".
  - Also open: #351, #347, #187.
- **`later`:** #326 (jagged fill edges, skipped 11-12), #369, #345, #314, #313, #307, #305, #291, #290, #289, #244, #243, #231, #183.

## Waiting on the user

- [ ] Merge `gated` PRs once their reviews are on them.
- [ ] Closed-source move: move issues to the private tracker, then make the repo private.
- [ ] Next release: approve the version bump PR and push the gated tag (`docs/releasing.md`).
- [ ] Website: deploy What's new after each release.
- [ ] When PR 5 is picked up: answer the four questions in `docs/solver-audit-references.md` section 3.1.
- [ ] Upstream report to acadrust about the clockwise hatch-arc fix (#289), if wanted.
- [ ] #337: trim it and queue it, or close it and file a new issue.

## Rules learned

| Date | What went wrong | Rule |
|---|---|---|
| 2026-11-12 | A plan from a chat with no repo access was written from a stale view, and cost two workers and two closed PRs | Nothing becomes work until the lead has checked it against the live repo and confirmed its Done when with the user |
| 2026-11-12 | The PR 6 worker started its own session (#400) | Workers never start sessions; they report the need to the lead |
| 2026-11-10 | A dependency bump (#381) failed the Third-party notices job | After any Cargo.lock change, run `python scripts/notices.py` and commit THIRD-PARTY-NOTICES.txt in the same PR |
| 2026-11-04 | A Fable reviewer silently started on Opus | After starting a reviewer with a model, check its model |
| 2026-11-04 | Long-lived reviewers anchored on old rulings and grew 36 GB targets | One fresh reviewer per PR; delete its target after merge |
| 2026-10-31 | A product comparison claimed "Drift lacks X" for things Drift had | Check FEATURES.md and grep the code before claiming a feature is missing |
| 2026-10-30 | Tests asserting wall-clock budgets fail on a busy machine (#368) | No hard wall-clock asserts in normal tests; perf budgets live in ignored baselines |
| 2026-10-29 | #253's drag work passed CI but regressed a drag 14→265 ms | Any PR touching drag, solver, index or fills runs the ignored perf baselines (`cargo test -- --ignored`) and puts the numbers in the PR |
| 2026-10-28 | A stacked PR (#45) landed on a side branch | Check that a PR's base is `main` before merging |
| 2026-10-27 | Vague tasks caused rework | Every task has a checkable Done when before it runs |
