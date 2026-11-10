# Hub queue

The task list the hub runs from. The planning chat adds tasks; the hub only changes **Status**, **PR** and **Session**. Checkers verify each PR against its task's **Done when**.

## Rules for a task entry

- **ID** `H-<n>`, never reused. Link the GitHub issue if there is one.
- **Done when** must be checkable: a named test that passes, a measured number, or a behaviour you can see in the app. "Works well" is not a Done when.
- **Files** lists the files or areas the task touches. The hub never runs two tasks whose Files overlap.
- **Tier** picks the gate (see `.claude/skills/hub/SKILL.md`):
  - `auto`: papercuts, UI fillers, docs, tests, refactors with no behaviour change. The hub merges once the gates pass.
  - `gated`: drag/solver, file format, save/load, importers/exporters, release, CI/workflows, closed-source move. Waits for the user's OK in chat.
- **Status**: `ready` → `running` → `in-review` → `merged` / `blocked` / `dropped`.

## Trial (first run)

Only the three tasks below may run until they merge cleanly. Then self-merge of `auto` tasks is on for good (log it in LOG.md).

| ID | Issue | Task | Done when | Files | Tier | Status | PR | Session |
|---|---|---|---|---|---|---|---|---|
| H-1 | #264 | Shift+D then Esc must not leave a copy stacked on the original | A smoke test does Shift+D, Esc and asserts the entity count is unchanged; test passes | `crates/drift-edit/src/modify.rs`, `crates/drift-edit/src/tools/select.rs`, smoke test in `crates/drift-app` | auto | ready | | |
| H-2 | #337 | Relation glyphs readable and off the transform handles | Glyphs drawn at least the size set in the issue and offset so no glyph rect overlaps a handle rect; screenshot at 3440x1440 in the PR | relation glyph drawing in `crates/drift-app` canvas | auto | ready | | |
| H-3 | #242 | Auto horizontal/vertical inference window widened for sketched input | Window is a named constant (value from the issue); unit test: a 2.5° line gets Horizontal, an 8° line does not | `crates/drift-core/src/rules.rs` | auto | ready | | |

## Backlog (not yet ready: needs a Done when or the user's pick)

Open issues on 2026-11-09, grouped. The planning chat promotes one to a task by writing its Done when, Files and Tier.

- **Solver / drag (gated):** #280 (next, ask P4+P2 first), #362, #366, #344, #349, #350, #352, #351, #343, #347, #295, #244, #243, #306, #305, #313
- **Big-drawing perf (gated):** #373, #372, #371, #370, #253, #285, #307
- **Fills / sheets:** #345, #326, #369, #367 (auto), #338
- **Import / export (gated):** #245, #289
- **Flaky tests (auto):** #368, #353, #297, #296
- **Enhancements:** #314, #231, #183, #187, #186, #291, #290
