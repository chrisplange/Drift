"""The solver's acceptance bar, item 1 (and 6 on Linux): the heavy drag matrix.

Runs every `*_heavy` drag property with seeds 1 to 20 (release, `--exact`, one process per
property and seed), and compares the failures with `expected.txt`, the failures that are
traced and accepted (see docs/solver-audit.md, section 5). Exit status 1 if:
- a pair fails that isn't expected;
- a pair expected to fail passes (the PR that fixes a seed removes its line, so a later PR
  can't break it again unseen);
- a run doesn't finish within --timeout seconds (a drag that loops).

    python scripts/solver-bar/heavy.py [--jobs N] [--out DIR] [--seeds 1-20] [--exe PATH]

Without --exe it builds the drift-edit test binary in release first; a failed build prints
the compiler's errors. Each run's log goes to DIR/<property>-<seed>.log; DIR/summary.md is
the table for a PR body.
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
    # (json-render-diagnostics: the compiler's errors go to stderr, rendered as usual, and
    # only the artifact messages come as JSON on stdout.)
    proc = subprocess.run(
        ["cargo", "test", "--release", "-p", "drift-edit", "--lib", "--no-run",
         "--message-format=json-render-diagnostics"],
        cwd=ROOT, stdout=subprocess.PIPE, text=True, encoding="utf-8",
    )
    if proc.returncode != 0:
        sys.exit(f"the drift-edit test build failed (exit {proc.returncode}): its errors are above")
    exe = None
    for line in proc.stdout.splitlines():
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


def run(exe, out, prop, seed, timeout):
    """Run one property at one seed; (property, seed, passed, note)."""
    env = dict(os.environ, PROPTEST_RNG_SEED=str(seed), PROPTEST_DISABLE_FAILURE_PERSISTENCE="1")
    log_path = out / f"{prop}-{seed}.log"
    try:
        proc = subprocess.run(
            [exe, f"drag_props::{prop}", "--exact", "--ignored", "--test-threads=1"],
            env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
            encoding="utf-8", errors="replace", timeout=timeout,
        )
    except subprocess.TimeoutExpired as e:
        text = e.stdout if isinstance(e.stdout, str) else (e.stdout or b"").decode("utf-8", "replace")
        log_path.write_text(text + f"\n#TIMEOUT after {timeout:.0f} s\n", encoding="utf-8")
        return prop, seed, False, "timed out"
    log = proc.stdout + f"\n#RC {proc.returncode}\n"
    log_path.write_text(log, encoding="utf-8")
    ran = "1 passed" in log or "1 failed" in log
    return prop, seed, proc.returncode == 0 and ran, "" if ran else "didn't run"


def main():
    # (Windows consoles default to cp1252.)
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    ap.add_argument("--out", default=str(ROOT / "target" / "solver-bar" / "heavy"))
    ap.add_argument("--seeds", default="1-20")
    ap.add_argument("--exe")
    ap.add_argument("--timeout", type=float, default=1800, help="seconds per run (default 1800)")
    args = ap.parse_args()
    lo, _, hi = args.seeds.partition("-")
    seeds = range(int(lo), int(hi or lo) + 1)
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    exe = args.exe or build()
    want = expected()

    results, notes = {}, {}
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        jobs = [pool.submit(run, exe, out, p, s, args.timeout) for p in PROPERTIES for s in seeds]
        for job in concurrent.futures.as_completed(jobs):
            prop, seed, ok, note = job.result()
            results[(prop, seed)] = ok
            if note:
                notes[(prop, seed)] = note

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
        lines += ["", "**Expected to fail but passed** (remove its line from expected.txt): "
                  + ", ".join(f"`{p}` {s}" for p, s in fixed)]
    if notes:
        lines += ["", "Runs that didn't finish: " + ", ".join(f"`{p}` {s} ({n})" for (p, s), n in sorted(notes.items()))]
    summary = "\n".join(lines) + "\n"
    (out / "summary.md").write_text(summary, encoding="utf-8")
    print(summary)
    step = os.environ.get("GITHUB_STEP_SUMMARY")
    if step:
        with open(step, "a", encoding="utf-8") as f:
            f.write(summary)
    if new:
        print("Not expected: " + ", ".join(f"{p} {s}" for p, s in new))
    if fixed:
        print("Expected to fail but passed: " + ", ".join(f"{p} {s}" for p, s in fixed))
    if new or fixed:
        sys.exit(1)


if __name__ == "__main__":
    main()
