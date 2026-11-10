# Hub queue

Only what is running or next: 5 rows at most. GitHub issues are the backlog; merged rows move to LOG.md. The planning chat adds rows; the hub changes only **Status**, **PR** and **Session**. Rules: `WORKFLOW.md`.

## Row format

- **ID** `H-<n>`, never reused (H-1..H-5 are used).
- **Done when** must be checkable: a named test, a measured number, or a behaviour you can see.
- **Files**: what it touches, including callers of what it changes and their tests.
- **Tier**: `auto` (hub merges after the gates) or `gated` (waits for the user).
- **Status**: `ready` → `running` → `in-review` → `merged` / `blocked` / `dropped`.

| ID | Issue | Task | Done when | Files | Tier | Status | PR | Session |
|---|---|---|---|---|---|---|---|---|

Empty. Next: batch 1 (label `batch-1`: #367, #326, #245, #338, #306). The planning chat writes their rows.
