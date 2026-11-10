# Learnings

What went wrong and the rule that came out of it. Add one when a review, revert or rework finds a cause. Each rule must be something a checker can act on.

| Date | What went wrong | Rule |
|---|---|---|
| 2026-10-29 | #253 drag work passed CI but regressed a drag 14→265 ms; only found by running the ignored perf baselines | Any PR touching drag, solver, index or fills runs `cargo test -- --ignored` perf baselines and puts the numbers in the PR |
| 2026-10-31 | Product comparison claimed "Drift lacks X" for things Drift had | Before claiming a feature is missing, check FEATURES.md and grep the code |
| 2026-10-28 | A stacked PR (#45) landed on a side branch | Check a PR's base is `main` before merging |
| 2026-11-04 | A Fable reviewer silently started on Opus | After starting a reviewer with a model, check its model and set it with set_session_model |
| 2026-11-04 | Long-lived reviewers anchored on old rulings and grew 36 GB targets | One fresh reviewer per PR; delete its target after merge |
| 2026-10-30 | Tests asserting wall-clock budgets fail on a busy machine (#368) | No hard wall-clock asserts in normal tests; perf budgets live in ignored baselines |
| 2026-11-10 | A dependency bump (#381) failed the Third-party notices job | After any Cargo.lock change, run `python scripts/notices.py` and commit THIRD-PARTY-NOTICES.txt in the same PR |
| 2026-10-27 | Vague tasks caused rework | Every task has a checkable Done when before it runs |
