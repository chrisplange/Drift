"""The heuristic ledger's ablation (#187): every heuristic switched off in turn.

Builds drift-edit's and drift-solver's test binaries with the test-only `ablate` feature
(`drift_solver::ablation`), then for each switch runs, with `DRIFT_ABLATE=<switch>`:

- the sketch corpus (as `corpus.py run` does), diffed against the same binary with nothing
  switched off;
- with --tests, both crates' normal tests: the ones that fail are the tests that guard the
  heuristic.

    python scripts/solver-bar/ablate.py [--jobs N] [--out DIR] [--only a,b,c] [--tests] [--skip-corpus]

DIR/<switch>/record.txt is the corpus record with that switch thrown, DIR/<switch>/tests.txt
the tests that failed, DIR/ablation.json everything counted, and DIR/ablation.md the table
`docs/solver-heuristics.md` quotes. A run of every switch takes hours: it is not part of
the nightly bar. Re-run it (or `--only` the switches concerned) when a heuristic is added,
changed or proposed for deletion.

The first run, with nothing thrown, must give the corpus baseline's verdicts: a build with
the feature on and no switch thrown behaves as a build without it.
"""

import argparse
import concurrent.futures
import json
import os
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import corpus  # noqa: E402


def build(package, features):
    """Build `package`'s lib test binary in release with `features`; its path."""
    cmd = ["cargo", "test", "--release", "-p", package, "--lib", "--no-run",
           "--features", features, "--message-format=json-render-diagnostics"]
    proc = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, text=True, encoding="utf-8")
    if proc.returncode != 0:
        sys.exit(f"the {package} test build failed (exit {proc.returncode}): its errors are above")
    exe = None
    for line in proc.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if (msg.get("reason") == "compiler-artifact" and msg.get("executable")
                and msg["target"]["name"] == package.replace("-", "_")):
            exe = msg["executable"]
    if not exe:
        sys.exit(f"no {package} test binary found")
    return exe


def switches(exe):
    """The switches' names, as the solver lists them."""
    src = (ROOT / "crates" / "drift-solver" / "src" / "ablation.rs").read_text(encoding="utf-8")
    names = re.findall(r'Switch::\w+ => "([a-z0-9-]+)"', src)
    if not names:
        sys.exit("no switches found in drift-solver's ablation.rs")
    return names


def sweep(exe, out, jobs, switch, timeout):
    """Run the corpus with `switch` thrown (None: nothing); its drags, and shards that broke."""
    out.mkdir(parents=True, exist_ok=True)
    env = {"DRIFT_ABLATE": switch} if switch else {}
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        results = list(pool.map(
            lambda k: corpus.run_shard(exe, k, jobs, out, env, timeout), range(jobs)))
    drags = {}
    for _, _, text in results:
        for line in corpus.record_lines(text):
            if line.startswith("D "):
                d = corpus.Drag(line)
                drags[d.key] = d
    (out / "record.txt").write_text(
        "\n".join(drags[k].line for k in corpus.sorted_keys(drags)) + "\n",
        encoding="utf-8", newline="\n")
    return drags, [(k, code) for k, code, _ in results if code != 0]


def run_tests(exe, switch, timeout, threads):
    """The normal tests of `exe` that fail with `switch` thrown (None: nothing), or a note."""
    env = dict(os.environ)
    if switch:
        env["DRIFT_ABLATE"] = switch
    try:
        proc = subprocess.run(
            [exe, f"--test-threads={threads}"], cwd=ROOT, env=env, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace",
            timeout=timeout)
        text = proc.stdout
    except subprocess.TimeoutExpired as e:
        text = e.stdout or ""
        if isinstance(text, bytes):
            text = text.decode("utf-8", "replace")
        text += "\n(timed out)\n"
    failed = sorted(set(re.findall(r"^test (\S+) \.\.\. FAILED$", text, re.M)))
    # (A test still running when the run was cut off, or one that aborted the process.)
    started = set(re.findall(r"^test (\S+) has been running", text, re.M))
    done = set(re.findall(r"^test (\S+) \.\.\. (?:ok|FAILED|ignored)", text, re.M))
    hung = sorted(started - done) if "(timed out)" in text else []
    finished = bool(re.search(r"^test result:", text, re.M))
    return failed, hung, finished, text


def cost(drags):
    return {
        "solves": sum(d.n("solves") for d in drags.values()),
        "refused": sum(d.n("ref") for d in drags.values()),
        "cap": sum(int(d.fields.get("ex", "0,0,0,0").split(",")[2]) for d in drags.values()),
    }


def compare(base, got):
    """How the corpus with a switch thrown differs from the base run."""
    letters = [c[0] for c in corpus.CHECKS]
    worse = {l: 0 for l in letters}
    better = {l: 0 for l in letters}
    cases = []
    moved = 0
    refusals = 0
    for key in corpus.sorted_keys(base):
        if key not in got:
            continue
        a, b = base[key], got[key]
        for l in b.failed() - a.failed():
            worse[l] += 1
        for l in a.failed() - b.failed():
            better[l] += 1
        if b.failed() - a.failed():
            cases.append(f"{key[0]} {key[1]} {key[2]}: {a.verdict} -> {b.verdict}")
        moved += a.fields.get("end") != b.fields.get("end")
        refusals += a.n("ref") != b.n("ref")
    return {
        "drags": len(base),
        "missing": len([k for k in base if k not in got]),
        "worse": worse,
        "better": better,
        "newly_failing": sum(1 for k in base if k in got and got[k].failed() - base[k].failed()),
        "newly_passing": sum(1 for k in base if k in got and base[k].failed() - got[k].failed()),
        "moved": moved,
        "refusals_differ": refusals,
        "cases": cases,
        "cost": cost(got),
    }


def table(result, base_cost, names):
    """The ablation as a markdown table, a row a switch."""
    lines = [
        "| Switch | Guarding tests that fail | Corpus drags newly failing (by check) | Newly passing | Ended elsewhere | Solves | Verdict |",
        "|---|---|---|---|---|---|---|",
    ]
    for name in names:
        r = result.get(name)
        if not r:
            continue
        c = r.get("corpus")
        tests = r.get("tests")
        if tests is None:
            guard = "not run"
        else:
            failed = tests["solver"] + tests["edit"]
            hung = tests["hung"]
            guard = str(len(failed)) if failed else "none"
            if failed:
                short = [t.split("::")[-1] for t in failed[:3]]
                guard += ": " + ", ".join(f"`{t}`" for t in short) + (" …" if len(failed) > 3 else "")
            if hung or not tests["finished"]:
                guard += " (a run didn't finish" + (": " + ", ".join(f"`{t.split('::')[-1]}`" for t in hung[:2]) if hung else "") + ")"
        if c is None:
            lines.append(f"| `{name}` | {guard} | not run | | | | |")
            continue
        by = ", ".join(f"{n} `{l}`" for l, n in c["worse"].items() if n)
        newly = f"{c['newly_failing']}" + (f" ({by})" if by else "")
        if c["missing"]:
            newly += f"; {c['missing']} drags not finished"
        solves = c["cost"]["solves"] / max(1, base_cost["solves"])
        breaks = c["newly_failing"] or c["missing"] or (tests and (tests["solver"] or tests["edit"] or tests["hung"] or not tests["finished"]))
        if breaks:
            verdict = "breaks"
        elif c["moved"] or c["refusals_differ"]:
            verdict = "nothing breaks; drags end elsewhere"
        else:
            verdict = "**nothing breaks without it**"
        lines.append(
            f"| `{name}` | {guard} | {newly} | {c['newly_passing']} | {c['moved']} of {c['drags']} "
            f"| x{solves:.2f} | {verdict} |")
    return "\n".join(lines) + "\n"


def main():
    # (Windows consoles default to cp1252.)
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 4) - 2))
    ap.add_argument("--out", default=str(ROOT / "target" / "solver-bar" / "ablation"))
    ap.add_argument("--only", help="comma-separated switches (default: all)")
    ap.add_argument("--tests", action="store_true", help="also run both crates' tests per switch")
    ap.add_argument("--skip-corpus", action="store_true", help="tests only")
    ap.add_argument("--timeout", type=int, default=2400, help="seconds per corpus process")
    ap.add_argument("--test-timeout", type=int, default=1800, help="seconds per test run")
    args = ap.parse_args()
    out = pathlib.Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    # (The committed corpus only: the ledger quotes these tables.)
    os.environ.pop("DRIFT_CORPUS_PRIVATE", None)
    os.environ.pop("DRIFT_ABLATE", None)
    edit = build("drift-edit", "ablate")
    solver = build("drift-solver", "ablate") if args.tests else None
    names = switches(edit)
    todo = [n for n in names if not args.only or n in args.only.split(",")]
    unknown = [n for n in (args.only or "").split(",") if n and n not in names]
    if unknown:
        sys.exit(f"no switch named {unknown}")
    store = out / "ablation.json"
    result = json.loads(store.read_text(encoding="utf-8")) if store.exists() else {}

    base = None
    if not args.skip_corpus:
        base, broke = sweep(edit, out / "base", args.jobs, None, args.timeout)
        if broke:
            sys.exit(f"the base run broke: {broke}")
        want = corpus.expected_rows()
        got = corpus.baseline_rows(base)
        differ = [k for k in want if got.get(k) != want[k]]
        if differ:
            print(f"WARNING: with nothing thrown, {len(differ)} targets' verdicts differ from "
                  f"corpus-baseline.txt (first: {differ[0]})")
        result["base"] = {"cost": cost(base), "differ_from_baseline": len(differ)}
    if args.tests:
        base_failed = {}
        for which, exe in (("solver", solver), ("edit", edit)):
            failed, hung, finished, text = run_tests(exe, None, args.test_timeout, args.jobs)
            (out / "base").mkdir(exist_ok=True)
            (out / "base" / f"tests-{which}.log").write_text(text, encoding="utf-8")
            base_failed[which] = set(failed)
            if failed or hung or not finished:
                print(f"WARNING: with nothing thrown, {which} tests: failed {failed}, hung {hung}, finished {finished}")
        result.setdefault("base", {})["tests_failed"] = {k: sorted(v) for k, v in base_failed.items()}

    for n, name in enumerate(todo, 1):
        entry = result.setdefault(name, {})
        where = out / name
        where.mkdir(exist_ok=True)
        if not args.skip_corpus:
            got, broke = sweep(edit, where, args.jobs, name, args.timeout)
            entry["corpus"] = compare(base, got)
            entry["corpus"]["shards_broke"] = [str(b) for b in broke]
        if args.tests:
            tests = {"hung": [], "finished": True}
            for which, exe in (("solver", solver), ("edit", edit)):
                failed, hung, finished, text = run_tests(exe, name, args.test_timeout, args.jobs)
                (where / f"tests-{which}.log").write_text(text, encoding="utf-8")
                tests[which] = [t for t in failed if t not in base_failed[which]]
                tests["hung"] += hung
                tests["finished"] &= finished
            entry["tests"] = tests
            (where / "tests.txt").write_text(
                "\n".join(tests["solver"] + tests["edit"] + [f"(hung) {t}" for t in tests["hung"]]) + "\n",
                encoding="utf-8")
        store.write_text(json.dumps(result, indent=1), encoding="utf-8")
        c = entry.get("corpus")
        t = entry.get("tests")
        print(f"[{n}/{len(todo)}] {name}: "
              + (f"corpus {c['newly_failing']} newly failing, {c['moved']} ended elsewhere" if c else "")
              + (f"; tests failing {len(t['solver']) + len(t['edit'])}" if t else ""), flush=True)

    base_cost = result.get("base", {}).get("cost", {"solves": 1})
    (out / "ablation.md").write_text(table(result, base_cost, names), encoding="utf-8", newline="\n")
    print(f"wrote {out / 'ablation.md'}")


if __name__ == "__main__":
    main()
