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
2026-11-10 | A master Drift Coordinator session now coordinates and automates agents and writes no code; a notes session keeps the records | user
2026-11-10 | Solver redesign scoped to PR 2, PR 3 and PR 6 of docs/solver-audit.md; PR 1 (#352), PR 4 (#344) and PR 5 (#362) parked as later. After PR 3, work returns to usability and bug batches | user in chat
2026-11-10 | Solver PR 0 (#389) merged: the acceptance bar is now on main (Linux heavy 277 ok / 3) | user in chat
2026-11-10 | Solver PR 2 (#391): user accepted the remaining failures: heavy similarity seeds 14 and 17 (PR 3), seed 6 on Linux (PR 4, parked), the e13 arc drag stopping at 29 instead of 32 frames (ignored pin, PR 5, parked) | user in chat
2026-11-11 | Solver PR 2 (#391) merged; tangent_and_parallel stays ignored, owned by PR 3 | user
2026-11-11 | Solver PR 3 (#394) sent back before review: fix the arc growing in f3's drag (binding rule), free-move refusals (seeds 6, 17) and the arc-ends slide. Accepted as is: cap at 0.63 % (fix in parked PR 5), #351 left open (measured ~0.3 s on main and PR 3), active element held while a chain bends (#333) | user in chat
2026-11-11 | Solver PR 3 (#394) merged by the user without the separate Opus/Fable reviews; held arcs never grow in a drag; tangent_and_parallel un-ignored; #349 and #280 closed (all seven #280 pins pass) | user
2026-11-11 | "Stop the drag" reaffirmed: PR 3 (#394) brought back a whole-part rigid slide when a drag frame fails (rejected 2026-11-03); it is being removed (main-fix) | user
2026-11-11 | Circles are held like arcs: "radii are never grown" covers circle radii too (main-fix) | user
2026-11-11 | Dragging a point held on a fixed-centre circle outward past its radius stops the drag, the same as arcs; no circle exemption (confirms gated PR #398) | user
2026-11-12 | A round set Equal to a fixed-centre round stops when dragged off its tangent line, for arcs and circles alike (merged #398 knowing it) | user
2026-11-12 | The stage-2 stall fix (draft #400) continues as its own gated PR after PR 6 (#399) merges; it is not part of PR 6 | user
2026-11-12 | Solver PR 6 (#399, edits keep shapes) merged before the separate Opus and Fable reviews ran; closes #350 | user
