"""The solver's acceptance bar, item 1 (and 6 on Linux): the heavy drag matrix.

Runs every `*_heavy` drag property with seeds 1 to 20 (release, `--exact`, one process per
property and seed), and compares the failures with `expected.txt`, the failures that are
traced and accepted (see docs/solver-audit.md, section 5). Exit status 1 if a pair fails
that isn't expected; a pair expected to fail that passes is reported, not failed.

    python scripts/solver-bar/heavy.py [--jobs N] [--out DIR] [--seeds 1-20] [--exe PATH]

Without --exe it builds the drift-edit test binary in release first. Each run's log goes to
DIR/<property>-<seed>.log; DIR/summary.md is the table for a PR body.
"""

import argparse
import concurrent.futures
import json
import os
import pathlib
import platform
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
HERE = pathlib.Path(__file__).resolve().parent

PROPERTIES = [
    "drag_meets_relations_heavy",
    "move_lands_exactly_heavy",
    "free_move_is_accepted_heavy",
    "move_keeps_untouched_lengths_heavy",
    "drag_is_invariant_under_similarity_heavy",
    "small_drag_is_continuous_heavy",
    "select_all_moves_rigidly_heavy",
    "free_point_drag_moves_only_it_heavy",
    "drag_leaves_the_unreached_alone_heavy",
    "drag_leaves_the_unrelated_alone_heavy",
    "drag_is_path_independent_heavy",
    "drag_out_and_back_does_not_creep_heavy",
    "drag_is_deterministic_heavy",
    "face_dimensions_hold_through_drags_heavy",
]


def build():
    """Build drift-edit's test binary in release; its path."""
    out = subprocess.run(
        ["cargo", "test", "--release", "-p", "drift-edit", "--lib", "--no-run",
         "--message-format=json"],
        cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True, encoding="utf-8",
    ).stdout
    exe = None
    for line in out.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if (msg.get("reason") == "compiler-artifact" and msg.get("executable")
                and msg["target"]["name"] == "drift_edit"):
            exe = msg["executable"]
    if not exe:
        sys.exit("no drift-edit test binary found")
    return exe


def expected():
    """(property, seed) pairs expected to fail on this platform, from expected.txt.

    Each line: `<property> <seed> <platforms> # why`, platforms `all` or a comma list of
    `linux`, `windows`, `macos`.
    """
    here = {"Linux": "linux", "Windows": "windows", "Darwin": "macos"}.get(platform.system(), "")
    pairs = {}
    path = HERE / "expected.txt"
    for raw in path.read_text(encoding="utf-8").splitlines():
        line, _, why = raw.partition("#")
        parts = line.split()
        if not parts:
            continue
        prop, seed, where = parts[0], int(parts[1]), parts[2] if len(parts) > 2 else "all"
        if where == "all" or here in where.split(","):
            pairs[(prop, seed)] = why.strip()
    return pairs


def run(exe, out, prop, seed):
    env = dict(os.environ, PROPTEST_RNG_SEED=str(seed), PROPTEST_DISABLE_FAILURE_PERSISTENCE="1")
    proc = subprocess.run(
        [exe, f"drag_props::{prop}", "--exact", "--ignored", "--test-threads=1"],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        encoding="utf-8", errors="replace",
    )
    log = proc.stdout + f"\n#RC {proc.returncode}\n"
    (out / f"{prop}-{seed}.log").write_text(log, encoding="utf-8")
    ran = "1 passed" in log or "1 failed" in log
    return prop, seed, proc.returncode == 0 and ran


def main():
    # (Windows consoles default to cp1252.)
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--out", default=str(ROOT / "target" / "solver-bar" / "heavy"))
    ap.add_argument("--seeds", default="1-20")
    ap.add_argument("--exe")
    args = ap.parse_args()
    lo, _, hi = args.seeds.partition("-")
    seeds = range(int(lo), int(hi or lo) + 1)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    exe = args.exe or build()
    want = expected()

    results = {}
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        jobs = [pool.submit(run, exe, out, p, s) for p in PROPERTIES for s in seeds]
        for job in concurrent.futures.as_completed(jobs):
            prop, seed, ok = job.result()
            results[(prop, seed)] = ok

    fails = sorted(k for k, ok in results.items() if not ok)
    new = [k for k in fails if k not in want]
    fixed = sorted(k for k in want if k[1] in seeds and results.get(k))
    lines = [
        f"Heavy matrix ({platform.system()}): {len(results) - len(fails)} ok / {len(fails)} fail",
        "",
        "| Property | Failing seeds | Not expected |",
        "|---|---|---|",
    ]
    for prop in PROPERTIES:
        f = [s for p, s in fails if p == prop]
        n = [s for p, s in new if p == prop]
        lines.append(f"| `{prop}` | {', '.join(map(str, f)) or 'none'} | {', '.join(map(str, n)) or ''} |")
    if fixed:
        lines += ["", "Expected to fail but passed: " + ", ".join(f"`{p}` {s}" for p, s in fixed)]
    summary = "\n".join(lines) + "\n"
    (out / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)
    step = os.environ.get("GITHUB_STEP_SUMMARY")
    if step:
        with open(step, "a", encoding="utf-8") as f:
            f.write(summary)
    if new:
        print("Not expected: " + ", ".join(f"{p} {s}" for p, s in new))
        sys.exit(1)


if __name__ == "__main__":
    main()
