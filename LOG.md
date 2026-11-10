# Log

One line per decision, approval or merge, newest last. Format: `YYYY-MM-DD | what | source`. Source is where it came from: "user in chat", "planning chat", "hub", a PR or an issue. Never edit old lines; correct with a new one.

2026-11-04 | Fresh reviewer session per PR, tiered by risk (drag/solver Opus + Fable final; other code Opus 2 steps; papercuts step 2; UI fillers none) | user in chat
2026-11-02 | Standing approval: a task session cleared by its final reviewer may commit and open its PR itself | user in chat
2026-10-30 | No session writes to drive D:; durable notes go to E:\Drift\handoff or this repo | user in chat
2026-10-26 | Freeze: finish in-flight features, then usability and bug fixing; no new feature areas | user in chat
2026-11-09 | Adopt hub setup: planning chat + /hub session + one worktree per task; HUB-QUEUE/LOG/LEARNINGS/FEATURES in repo | user in chat
2026-11-09 | Tiered self-merge: hub merges `auto` tasks once gates pass; `gated` tasks wait for the user's OK | user in chat
2026-11-09 | Trial first: H-1..H-3 only, then self-merge of `auto` is on for good | user in chat
2026-11-09 | H-3 (#242) Files widened: crates/drift-app/src/agent/geometry.rs (use rules helper) + agent/schema.rs (MCP text 1→3 degrees); interactive line snap unchanged | hub
2026-11-09 | H-3 (#242) Files widened: crates/drift-app/src/agent/script/tests.rs fixtures redrawn >3° off (test-only) for the new window | hub
2026-11-10 | main CI red, not caused by trial PRs: (1) cargo deny RUSTSEC-2026-0457/0461/0462 on rmcp 3.5.1 (apps/mcp); (2) Test (ubuntu) drift-edit drag_props::seeds::e_result_depends_on_size_tangent_and_parallel fails at seeds.rs:1747 after #377; trial merges held until user decides | hub
2026-11-10 | H-1 PR #379 test-only (fix already in #310); security note pass; product check + Opus reviewer pending | hub
2026-11-10 | H-1 PR #379 product check PASS (issuecomment-6759439465) | hub
2026-11-10 | Queue H-4 (rmcp advisory bump) and H-5 (ubuntu drag seed since #377) as gated main-CI fixes before trial merges | user in chat
2026-11-10 | H-3 PR #380 opened; security note pass; product check + Opus reviewer pending | hub
2026-11-10 | H-3 PR #380 product check PASS (issuecomment-6759594256) | hub
2026-11-10 | H-4 and H-5 dropped, superseded by PR #381 (rmcp 3.9.0 + notices; flaky #280 seed marked #[ignore]); workers told not to open PRs and to report anything #381 misses; merging #381 is the user's call | Drift coding setup session
2026-11-10 | H-2 PR #382 opened | hub
2026-11-10 | #381 merged (main CI fix, user's call); duplicate H-4 PR #383 closed; #379 #380 #382 updated from main | hub
2026-11-10 | H-2 PR #382 product check PASS (issuecomment-6759645929; pie-menu spill-over to be noted in body) | hub
2026-11-10 | H-5 worker finding (ulp-chaotic seed case; possible HashMap-order non-determinism) saved for #280 in side-session notes h5-finding-for-280.md; issue comment not posted (auto mode blocked) | hub
2026-11-10 | H-1 PR #379 Opus review APPROVE (issuecomment-6759619034; test fails 16 vs 8 with #310 fix off). Reviewer gaps for planning chat: Shift+D with no pointer, and Shift+D on a locked layer, still leave a stacked copy (#264) | hub
2026-11-10 | H-2 PR #382 body updated with full pie-menu side effect | hub
2026-11-10 | H-3 PR #380 Opus review APPROVE (issuecomment-6759730028) | hub
2026-11-10 | Trial done: #379 (H-1), #380 (H-3), #382 (H-2) merged; self-merge of `auto` tasks is on | hub / user in chat
2026-11-10 | Cleanup + fresh start: WORKFLOW.md; hub max 2 workers, subagent reviews; memory trimmed; issue triage applied (labels solver-audit, batch-1..3, later; #186 closed) | user in chat
2026-11-10 | Follow-up for #264 from the #379 review: Shift+D with no pointer, and Shift+D on a locked layer, still leave a stacked copy | hub
