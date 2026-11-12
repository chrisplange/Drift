#!/usr/bin/env python3
"""Work out a change's merge tier (auto, gated or solver) from the paths it touches.

The rules live in .github/tiers.txt. A change's tier is the strictest one any of its files
falls in. An empty file list is gated: when we can't tell what changed, a person looks.

Usage:
  python scripts/tier.py --base origin/main          # files changed on HEAD since the base
  python scripts/tier.py --pr 123                    # a pull request's files, via gh
  python scripts/tier.py --stdin < files.txt         # one path per line
Options:
  --explain          also print which file put the change in its tier
  --github-output    append `tier=<tier>` and `heavy=true|false` to $GITHUB_OUTPUT
  --rules PATH       a tiers file other than .github/tiers.txt
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ORDER = ["auto", "gated", "solver"]  # weakest to strictest
DEFAULT_RULES = Path(__file__).resolve().parent.parent / ".github" / "tiers.txt"


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    """`**` = any number of directories (or none), `*` = anything within one segment."""
    out = []
    i = 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        elif pattern[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    return re.compile("".join(out) + r"\Z")


def parse_rules(text: str) -> dict[str, list[tuple[str, re.Pattern[str]]]]:
    rules: dict[str, list[tuple[str, re.Pattern[str]]]] = {"solver": [], "gated": [], "heavy": []}
    section = None
    for n, raw in enumerate(text.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        m = re.fullmatch(r"\[(\w+)\]", line)
        if m:
            section = m.group(1)
            if section not in rules:
                raise ValueError(f"tiers file line {n}: unknown section [{section}]")
            continue
        if section is None:
            raise ValueError(f"tiers file line {n}: pattern before any [section]")
        rules[section].append((line, glob_to_regex(line)))
    return rules


def classify(files: list[str], rules) -> tuple[str, list[tuple[str, str, str]]]:
    """Return (tier, [(file, tier, pattern)]) for the files that raised the tier."""
    files = [f.strip().replace("\\", "/") for f in files if f.strip()]
    if not files:
        return "gated", [("(no files)", "gated", "an empty change is gated")]
    tier = "auto"
    hits = []
    for f in files:
        for name in ("solver", "gated"):
            pat = next((p for p, rx in rules[name] if rx.match(f)), None)
            if pat:
                hits.append((f, name, pat))
                if ORDER.index(name) > ORDER.index(tier):
                    tier = name
                break
    return tier, hits


def needs_heavy(files: list[str], rules) -> bool:
    """Whether the heavy drag matrix should run: any file in [heavy] or [solver].

    An empty file list runs it, the safe side.
    """
    files = [f.strip().replace("\\", "/") for f in files if f.strip()]
    if not files:
        return True
    pats = rules["heavy"] + rules["solver"]
    return any(rx.match(f) for f in files for _, rx in pats)


def pr_files(pr: str, repo: str | None) -> list[str]:
    """A PR's files, old names of renames included, from the GitHub API."""
    repo = repo or os.environ.get("GITHUB_REPOSITORY")
    if not repo:
        repo = subprocess.check_output(
            ["gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"], text=True
        ).strip()
    out = subprocess.check_output(
        ["gh", "api", "--paginate", f"repos/{repo}/pulls/{pr}/files?per_page=100",
         "--jq", ".[] | [.filename, (.previous_filename // \"\")] | @json"],
        text=True,
    )
    files = []
    for line in out.splitlines():
        name, prev = json.loads(line)
        files.append(name)
        if prev:
            files.append(prev)
    return files


def diff_files(base: str) -> list[str]:
    out = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", f"{base}...HEAD"], text=True
    )
    return out.splitlines()


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    src = ap.add_mutually_exclusive_group(required=True)
    src.add_argument("--base")
    src.add_argument("--pr")
    src.add_argument("--stdin", action="store_true")
    ap.add_argument("--repo")
    ap.add_argument("--rules", default=str(DEFAULT_RULES))
    ap.add_argument("--explain", action="store_true")
    ap.add_argument("--github-output", action="store_true")
    a = ap.parse_args(argv)

    rules = parse_rules(Path(a.rules).read_text(encoding="utf-8"))
    if a.stdin:
        files = sys.stdin.read().splitlines()
    elif a.pr:
        files = pr_files(a.pr, a.repo)
    else:
        files = diff_files(a.base)

    tier, hits = classify(files, rules)
    heavy = needs_heavy(files, rules)
    print(tier)
    if a.explain:
        for f, name, pat in hits:
            print(f"  {name:6} {f}  ({pat})", file=sys.stderr)
        print(f"  heavy drag matrix: {'yes' if heavy else 'no'}", file=sys.stderr)
    if a.github_output:
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as fh:
            fh.write(f"tier={tier}\nheavy={'true' if heavy else 'false'}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
