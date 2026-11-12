#!/usr/bin/env python3
"""Post the "Solver review" commit status for a pull request.

A solver-tier PR (see .github/tiers.txt) may merge only after an independent review. The
review is marked by a comment from the repository owner's account that contains

    <!-- drift-review: approve <full head commit SHA> -->

The status is green when the PR isn't solver-tier, or when such a comment names the PR's
current head commit. A new push changes the head, so an older approval no longer counts.
Comments from anyone else are ignored, so on this public repository no one but the owner's
account can approve.

Run by .github/workflows/tier.yml on pull request and comment events. It reads only the
GitHub API, never the PR's code. Needs GH_TOKEN with `statuses: write` and `PR` (number).
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tier as tiers  # noqa: E402

CONTEXT = "Solver review"
MARKER = re.compile(r"<!--\s*drift-review:\s*approve\s+([0-9a-f]{40})\s*-->")
APPROVERS = {"OWNER"}


def find_approval(comments: list[dict], head_sha: str) -> dict | None:
    """The newest owner comment that approves `head_sha`, or None.

    Each comment is {"body", "author_association", "user"}; PR review bodies use the same
    shape.
    """
    for c in reversed(comments):
        if c.get("author_association") not in APPROVERS:
            continue
        for sha in MARKER.findall(c.get("body") or ""):
            if sha == head_sha:
                return c
    return None


def decide(tier: str, comments: list[dict], head_sha: str) -> tuple[str, str]:
    """Return (state, description) for the status."""
    if tier != "solver":
        return "success", f"Not needed: {tier}-tier change"
    c = find_approval(comments, head_sha)
    if c:
        return "success", f"Approved by {c.get('user', {}).get('login', 'owner')} for {head_sha[:7]}"
    return "failure", f"Solver-tier: needs the owner's drift-review marker for {head_sha[:7]}"


def gh_json_lines(path: str, jq: str) -> list[dict]:
    out = subprocess.check_output(["gh", "api", "--paginate", path, "--jq", jq], text=True)
    return [json.loads(line) for line in out.splitlines() if line.strip()]


def main() -> int:
    repo = os.environ["GITHUB_REPOSITORY"]
    pr = os.environ["PR"]
    info = json.loads(subprocess.check_output(
        ["gh", "api", f"repos/{repo}/pulls/{pr}", "--jq",
         "{sha: .head.sha, state: .state, url: .html_url}"], text=True))
    if info["state"] != "open":
        print(f"PR #{pr} is {info['state']}; nothing to do")
        return 0
    head = info["sha"]

    rules = tiers.parse_rules(tiers.DEFAULT_RULES.read_text(encoding="utf-8"))
    tier, _ = tiers.classify(tiers.pr_files(pr, repo), rules)

    shape = "{body: .body, author_association: .author_association, user: {login: .user.login}} | @json"
    comments = gh_json_lines(f"repos/{repo}/issues/{pr}/comments?per_page=100", ".[] | " + shape)
    comments += gh_json_lines(f"repos/{repo}/pulls/{pr}/reviews?per_page=100", ".[] | " + shape)

    state, desc = decide(tier, comments, head)
    print(f"PR #{pr} head {head} tier {tier}: {state} ({desc})")
    subprocess.check_call([
        "gh", "api", "-X", "POST", f"repos/{repo}/statuses/{head}",
        "-f", f"state={state}", "-f", f"context={CONTEXT}",
        "-f", f"description={desc[:140]}", "-f", f"target_url={info['url']}",
    ], stdout=subprocess.DEVNULL)
    return 0


if __name__ == "__main__":
    sys.exit(main())
