# Log

One line per decision, approval or merge, newest last. Format: `YYYY-MM-DD | what | source`. Source is where it came from: "user in chat", "planning chat", "hub", a PR or an issue. Never edit old lines; correct with a new one.

2026-11-04 | Fresh reviewer session per PR, tiered by risk (drag/solver Opus + Fable final; other code Opus 2 steps; papercuts step 2; UI fillers none) | user in chat
2026-11-02 | Standing approval: a task session cleared by its final reviewer may commit and open its PR itself | user in chat
2026-10-30 | No session writes to drive D:; durable notes go to E:\Drift\handoff or this repo | user in chat
2026-10-26 | Freeze: finish in-flight features, then usability and bug fixing; no new feature areas | user in chat
2026-11-09 | Adopt hub setup: planning chat + /hub session + one worktree per task; HUB-QUEUE/LOG/LEARNINGS/FEATURES in repo | user in chat
2026-11-09 | Tiered self-merge: hub merges `auto` tasks once gates pass; `gated` tasks wait for the user's OK | user in chat
2026-11-09 | Trial first: H-1..H-3 only, then self-merge of `auto` is on for good | user in chat
2026-11-10 | Solver audit decision 1: adopt the redesign in docs/solver-audit.md and keep the solver patch freeze until each issue's closing PR | user in chat
2026-11-10 | Solver audit decision 2: one tolerance for every solve, 1e-9 x a quarter of the part's size, in the part's own frame | user in chat
2026-11-10 | Solver audit decision 3: replace P2 ("far out unchanged") with "the same drag at any size and place, to about a million part-sizes out" | user in chat
2026-11-10 | Solver audit decision 4: held radii are held exactly in a drag (stop where they can't be kept); hanging lines stay soft, crushed-line stop kept | user in chat
2026-11-10 | Solver audit decision 5: commits keep shapes (#350), as the redesign's PR 6, after PR 3, with a before/after | user in chat
2026-11-10 | Solver audit decision 6: #377's accepted costs (D3, the hinge PQ2) are superseded by the redesign's PR 2 | user in chat
2026-11-10 | Solver audit decision 7: #295 is a walls bug, not solver; relabelled drawing + batch-2 (no walls label); its fix adds the four saved seeds | user in chat
2026-11-10 | Solver audit decision 8: file the 1 km commit refusal; filed as #387, closed by the redesign's PR 2 | user in chat
2026-11-10 | Solver audit decision 9: the heavy matrix and the pins run on ubuntu in a scheduled job (#187 step 4) | user in chat
