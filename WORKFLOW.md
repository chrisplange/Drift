# Workflow

How Drift work gets done. Session mechanics (tiers, merging, reviews) are in [CLAUDE.md](CLAUDE.md).

- **The owner decides** what matters and merges gated and solver PRs.
- **One session per issue**, at most 2 at once, started by the owner.
- **No "Done when", no work.** Every issue has a checkable one: a named test, a measured number, or a behaviour you can see.
- **Gated bugs go test-first.** PR 1 lands the failing test, ignored and naming its issue. The fix PR un-ignores it.
- **Red main stops everything.** Fix main first. Never merge on "red only for known reasons".
- **No band-aids on the solver.** A solver issue is closed by a fix for its cause, or left open with what was learned.
- **Papercuts** are issues with the `papercut` label, cleared in sweep PRs.
- GitHub issues are the backlog; decisions are comments on them.
