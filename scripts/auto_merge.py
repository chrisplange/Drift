#!/usr/bin/env python3
"""Merge an auto-tier pull request once every required check is green.

Run by .github/workflows/auto-merge.yml. Agent sessions never merge (the project's Claude
Code settings deny `gh pr merge`); this job is the only automatic merger, and it merges only
when all of these hold:

- the PR is open, not a draft, from a branch in this repository, by the repository owner;
- its head is still the commit the triggering run tested (when a run triggered it);
- scripts/tier.py, run fresh on the PR's files, says `auto` (labels are not trusted);
- it has no `hold` label;
- branch protection reports required checks for it, and every one of them passed (or was
  skipped by its own path filter). With no required checks at all, branch protection isn't
  on yet, and nothing is merged;
- GitHub says it can merge cleanly (`mergeStateStatus` CLEAN; a branch behind main must be
  updated by its session, since a push made with this job's token wouldn't start CI).

A merge made with GITHUB_TOKEN doesn't fire `push` workflows, so afterwards it dispatches CI
on main, and the website build when the PR touched what the website is built from.
Needs GH_TOKEN with contents, pull-requests and actions write, and PR (number) or HEAD_SHA.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tier as tiers  # noqa: E402

# Mirrors the push paths in .github/workflows/website.yml.
WEBSITE_PREFIXES = (
    "website/", "docs/guide/", "apps/web/", "crates/", "Cargo.toml", "Cargo.lock",
    "LICENSE.md", "THIRD-PARTY-NOTICES.txt", "scripts/check-demo-build.sh",
    ".github/workflows/website.yml",
)
PASSING = {"pass", "skipping"}


def blockers(pr: dict, owner: str, expected_sha: str | None, tier: str, checks: list[dict]) -> list[str]:
    """Every reason not to merge; an empty list means merge."""
    why = []
    if pr.get("state") != "OPEN":
        why.append(f"state is {pr.get('state')}")
    if pr.get("isDraft"):
        why.append("draft")
    if (pr.get("headRepositoryOwner") or {}).get("login") != owner:
        why.append("head branch is not in this repository")
    if (pr.get("author") or {}).get("login") != owner:
        why.append(f"author is {(pr.get('author') or {}).get('login')}, not {owner}")
    if expected_sha and pr.get("headRefOid") != expected_sha:
        why.append("head moved since the run that triggered this")
    if tier != "auto":
        why.append(f"tier is {tier}")
    if any(l.get("name") == "hold" for l in pr.get("labels") or []):
        why.append("has the hold label")
    if not checks:
        why.append("no required checks reported (branch protection not on?)")
    else:
        bad = [f"{c.get('name')}={c.get('bucket')}" for c in checks if c.get("bucket") not in PASSING]
        if bad:
            why.append("required checks not green: " + ", ".join(bad))
    if pr.get("mergeStateStatus") != "CLEAN":
        why.append(f"merge state is {pr.get('mergeStateStatus')}")
    return why


def touches_website(files: list[str]) -> bool:
    return any(f.startswith(WEBSITE_PREFIXES) for f in files)


def gh(*args: str) -> str:
    return subprocess.check_output(["gh", *args], text=True)


def main() -> int:
    repo = os.environ["GITHUB_REPOSITORY"]
    owner = repo.split("/")[0]
    head_sha = os.environ.get("HEAD_SHA") or None
    pr = os.environ.get("PR") or ""
    if not pr:
        # A workflow_run's pull_requests list can be empty; find the open PR by its head.
        found = json.loads(gh("api", f"repos/{repo}/commits/{head_sha}/pulls", "--jq",
                              "[.[] | select(.state == \"open\") | .number]"))
        if len(found) != 1:
            print(f"no single open PR for {head_sha}: {found}")
            return 0
        pr = str(found[0])

    info = json.loads(gh("pr", "view", pr, "-R", repo, "--json",
                         "state,isDraft,headRepositoryOwner,author,headRefOid,labels,mergeStateStatus"))
    files = tiers.pr_files(pr, repo)
    rules = tiers.parse_rules(tiers.DEFAULT_RULES.read_text(encoding="utf-8"))
    tier, _ = tiers.classify(files, rules)

    # `gh pr checks` exits non-zero while checks are failing or pending; the JSON still comes.
    run = subprocess.run(["gh", "pr", "checks", pr, "-R", repo, "--required", "--json", "name,bucket"],
                         capture_output=True, text=True)
    try:
        checks = json.loads(run.stdout or "[]")
    except json.JSONDecodeError:
        checks = []

    why = blockers(info, owner, head_sha, tier, checks)
    if why:
        print(f"PR #{pr}: not merging: " + "; ".join(why))
        return 0

    sha = info["headRefOid"]
    print(f"PR #{pr}: auto tier, every required check green; merging {sha}")
    subprocess.check_call(["gh", "pr", "merge", pr, "-R", repo, "--merge", "--match-head-commit", sha])
    subprocess.check_call(["gh", "workflow", "run", "ci.yml", "-R", repo, "--ref", "main"])
    if touches_website(files):
        subprocess.check_call(["gh", "workflow", "run", "website.yml", "-R", repo, "--ref", "main"])
    return 0


if __name__ == "__main__":
    sys.exit(main())
