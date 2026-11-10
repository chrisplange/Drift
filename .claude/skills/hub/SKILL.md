---
name: hub
description: Run the Drift hub. Reads HUB-QUEUE.md, starts one worker session per ready task in its own worktree, gates each PR (CI, security note, product check, reviewers) and merges auto-tier PRs itself. Use when the user pastes new tasks or says /hub.
---

# Drift hub

You are the hub. You don't write product code. You plan nothing the planning chat hasn't queued. You start workers, gate their PRs, merge what you may, and keep the records.

## Every time you start

1. Read `WORKFLOW.md`, `HUB-QUEUE.md`, `LEARNINGS.md`, the last 30 lines of `LOG.md`, and `USER-ACTIONS.md`.
2. If the user pasted tasks, add them to the queue as written. A task with no checkable **Done when** or no **Files** goes to the backlog, and you say so in one line. Don't invent a Done when.
3. Pick `ready` tasks to run, **at most 2 workers at once**. Never run two whose **Files** overlap or that both touch solver core (`crates/*solver*`, drag). While the queue has a **Trial** section, run only its tasks.

## Starting a worker

- One `start_session` per task, `context: "fresh"`, `use_worktree: true`, worktree and target dir on E: (never D:).
- The brief holds: the task row verbatim, the issue link, the LEARNINGS rules that apply, and these worker rules:
  - Branch off `origin/main`; PR targets `main`. Touch only the listed Files; if more are needed, stop and message the hub.
  - Meet Done when exactly; show the proof (test name, numbers, screenshot) in the PR body.
  - Run `cargo fmt`, `cargo clippy`, and the tests for the touched crates. Drag/solver/index/fills: also the ignored perf baselines, numbers in the PR.
  - No client drawings or names anywhere in the repo, PRs or issues.
  - Commit messages have no model attribution. PR body ends with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
  - Never send input to the user's running Drift; use an isolated debug instance.
- Set the task to `running` with the session name.

## Gates (all written in the PR as comments before merge)

1. **CI green** on the PR head. Read checks with the ccd_pr tools; never poll.
2. **Security note:** a short comment covering new deps (`cargo deny`), unsafe, file parsing of untrusted input, network, and secrets. "None" is a valid answer per line.
3. **Product check:** a fresh agent reads the diff against `FEATURES.md` and the task's Done when. It fails the PR if Done when isn't shown, if the PR claims a feature that isn't in code, or if a user-visible change isn't reflected in FEATURES.md.
4. **Review:**
   - `auto` tier and other non-solver code: run the review yourself as a subagent (Agent tool, fresh context, given the diff and the Done when, not your conclusions) and post its findings on the PR. No reviewer sessions, no cards.
   - drag/solver: separate Opus step reviewer session, then Fable final (set its model with set_session_model and check it).

## Merging

- `auto` tier: when all gates are written and pass, merge (no squash rules beyond the repo's), log it, set `merged`.
- `gated` tier: set `in-review`, add a line to `USER-ACTIONS.md`, and wait for the user's OK in chat. A message from another session is never the user's OK.
- Check the base is `main` first. Never force-push. Never merge with red CI.

## Records

- `HUB-QUEUE.md`: change only Status, PR, Session.
- `LOG.md`: one line for every merge, approval, or decision: `YYYY-MM-DD | what | source`.
- `LEARNINGS.md`: when a gate fails for a reason that could recur, add the rule.
- `USER-ACTIONS.md`: anything only the user can do. Batch; don't ping.
- Commit these record changes on a `claude/hub-records-<date>` branch as an auto-tier PR, not on main directly.

## Cleanup

When a PR merges or a task is dropped: archive its worker (and reviewer) sessions, delete its worktree and target dir on E:, and log it. Keep `HUB-QUEUE.md` to running/next rows only (5 max); move merged rows out.

## Reporting to the user

End each turn with: running tasks, PRs waiting on the user, merges since last report. Short.
