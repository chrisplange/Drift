"""The solver's acceptance bar, item 8: the sketch corpus (#187).

Sweeps every drawing in `scripts/solver-bar/corpus/` (see its README): every point and curve
that can move, dragged along ten pointer paths, every frame judged (rows met, no arc sweep
flip, no way-round flip, held radii, no jump, the stop rule). The checks and their letters
are in `crates/drift-edit/src/drag_props/corpus.rs`.

    python scripts/solver-bar/corpus.py run [--jobs N] [--out DIR] [--exe PATH] [--update]
    python scripts/solver-bar/corpus.py diff BASE/record.txt PR/record.txt
    python scripts/solver-bar/corpus.py baseline --from record.txt --platform linux

`run` builds the drift-edit test binary in release (unless --exe), runs the sweep in N
processes, and writes DIR/record.txt (one line per drag) and DIR/summary.md (the table for
a PR body). It then compares every drag's verdict with `corpus-baseline.txt`, the verdicts
main gives, and exits with status 1 if any differs: a drag that newly fails a check, or one
that no longer fails it (the PR that fixes a drag updates the baseline with --update, so a
later PR can't break it again unseen). On Linux and macOS the rows of
`corpus-baseline.<platform>.txt` stand in for the baseline's (round-off decides a drag at a
fold, and libm differs); --update there writes that file. `run` says which files it judged
against. --update refuses a sweep that isn't whole (one narrowed by CORPUS_ONLY,
CORPUS_TARGET, CORPUS_PATH or CORPUS_PRIVATE_ONLY, or one missing rows the baseline has).

`baseline --from` writes a platform's rows from a record made there: the Linux rows from the
`record.txt` of CI's `Sketch corpus (ubuntu)` job (its `solver-bar-corpus-linux` artifact),
with no Linux machine needed. Only one machine per platform is measured (Windows: the
owner's PC; Linux: CI's ubuntu-24.04 image); another CPU, a new runner image or toolchain,
or macOS may differ by a row, and those rows come from that machine's record the same way.

`diff` compares two records, drag by drag: checks newly failed and newly passed, frames
refused, and where the drag ended (a digest of the dragged part's geometry, to the bit).
It also lists drags that got worse at a check they already failed: more frames failing it,
or for `J` a worst jump at least twice as far. Exit status 1 if a drag newly fails a check
or got worse at one it fails.

With `DRIFT_CORPUS_PRIVATE` set to a folder of `.drift` files, those are swept too, as
`private-` and a hash of the file's contents. Their lines go to DIR/private.txt, never into the baseline, and
every table counts them in aggregate only: the repository is public.
"""

import argparse
import collections
import concurrent.futures
import json
import os
import pathlib
import platform
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
HERE = pathlib.Path(__file__).resolve().parent
TEST = "drag_props::corpus::corpus_sweep"

PATHS = ["short-e", "short-n", "short-w", "short-s", "long-ne", "long-nw", "long-sw",
         "long-se", "circle", "through"]

CHECKS = [
    ("R", "rows", "rows met"),
    ("S", "sweep", "no arc sweep flip"),
    ("W", "way", "no way-round flip"),
    ("L", "rev", "no line turned end for end"),
    ("G", "grew", "held radii never grow"),
    ("J", "jumps", "no jump over 50x the pointer"),
    ("X", "stop", "a refused frame changes nothing"),
    ("C", None, "the release keeps the relations"),
    ("P", None, "out and back returns"),
    ("O", None, "nothing outside the part moves"),
]


def build(features=None):
    """Build drift-edit's test binary in release; its path."""
    cmd = ["cargo", "test", "--release", "-p", "drift-edit", "--lib", "--no-run",
           "--message-format=json-render-diagnostics"]
    if features:
        cmd += ["--features", features]
    proc = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, text=True, encoding="utf-8")
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


def run_shard(exe, k, n, out, env_extra, timeout):
    env = dict(os.environ, CORPUS_SHARD=f"{k}/{n}", **env_extra)
    log = out / f"shard-{k}.log"
    try:
        proc = subprocess.run(
            [exe, TEST, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
            cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, encoding="utf-8", errors="replace", timeout=timeout,
        )
        text, code = proc.stdout, proc.returncode
    except subprocess.TimeoutExpired as e:
        text = e.stdout or ""
        if isinstance(text, bytes):
            text = text.decode("utf-8", "replace")
        code = "timeout"
    log.write_text(text, encoding="utf-8")
    return k, code, text


def record_lines(text):
    """The record's lines in a sweep's output. (The test harness prints the test's name with
    no line end, so the first line it prints follows that on the same line.)"""
    out = []
    for line in text.splitlines():
        line = line.split(" ... ", 1)[-1] if line.startswith("test ") else line
        if line[:2] in ("D ", "H "):
            out.append(line)
    return out


class Drag:
    """One `D` line of a record."""

    def __init__(self, line):
        w = line.split()
        self.sketch, self.target, self.path, self.verdict = w[1], w[2], w[3], w[4]
        self.fields = dict(zip(w[5::2], w[6::2]))
        self.line = line

    @property
    def key(self):
        return (self.sketch, self.target, self.path)

    @property
    def private(self):
        return self.sketch.startswith("private-")

    def failed(self):
        return set() if self.verdict == "ok" else set(self.verdict)

    def n(self, field):
        return int(self.fields.get(field, 0))


def read_record(path):
    """A record's drags by (sketch, target, path), and its `H` lines."""
    drags, heads = {}, []
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        if line.startswith("D "):
            d = Drag(line)
            drags[d.key] = d
        elif line.startswith("H "):
            heads.append(line)
    return drags, heads


def target_order(target):
    digits = "".join(c for c in target if c.isdigit())
    return int(digits) if digits else 0


def sorted_keys(drags):
    return sorted(drags, key=lambda k: (k[0], target_order(k[1]), PATHS.index(k[2])))


def baseline_rows(drags):
    """The baseline's lines for `drags` (public ones): `<sketch> <target> <verdict x 10>`."""
    rows = collections.OrderedDict()
    for key in sorted_keys(drags):
        d = drags[key]
        if d.private:
            continue
        rows.setdefault((d.sketch, d.target), {})[d.path] = d.verdict
    return {k: [v.get(p, "-") for p in PATHS] for k, v in rows.items()}


def read_baseline(path):
    rows = {}
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].split()
        if len(line) == 2 + len(PATHS):
            rows[(line[0], line[1])] = line[2:]
    return rows


# What narrows a sweep to part of the corpus: such a run can't be a baseline.
NARROWING = ("CORPUS_ONLY", "CORPUS_TARGET", "CORPUS_PATH", "CORPUS_PRIVATE_ONLY")


def write_baseline(path, rows, head):
    lines = [f"# {h}" for h in head]
    lines += [f"{s} {t} {' '.join(v)}" for (s, t), v in rows.items()]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def this_platform():
    return {"Windows": "windows", "Linux": "linux", "Darwin": "macos"}.get(
        platform.system(), platform.system().lower())


def platform_file(name):
    return HERE / f"corpus-baseline.{name}.txt"


def expected_rows(name=None):
    """The verdicts expected on platform `name` (here by default): the baseline's, with that
    platform's own rows over them; and the files they came from."""
    name = name or this_platform()
    rows = read_baseline(HERE / "corpus-baseline.txt")
    files = ["corpus-baseline.txt"]
    own = platform_file(name)
    if own.exists():
        rows.update(read_baseline(own))
        files.append(own.name)
    return rows, files


def partial(got, narrowed):
    """Why `got` (a sweep's baseline rows) can't be written as the baseline, or None."""
    if narrowed:
        return f"the sweep was narrowed by {', '.join(narrowed)}"
    base = read_baseline(HERE / "corpus-baseline.txt")
    missing = [k for k in base if k not in got]
    if missing:
        return (f"{len(missing)} target(s) the baseline has weren't swept "
                f"(first: {missing[0][0]} {missing[0][1]})")
    gaps = [k for k, v in got.items() if "-" in v]
    if gaps:
        return f"{len(gaps)} target(s) weren't swept along every path (first: {gaps[0][0]} {gaps[0][1]})"
    return None


def write_platform_rows(name, got):
    """Write platform `name`'s rows: those of `got` that differ from corpus-baseline.txt."""
    base = read_baseline(HERE / "corpus-baseline.txt")
    own = {k: v for k, v in got.items() if base.get(k) != v}
    write_baseline(
        platform_file(name), own,
        [f"The corpus baseline's rows that differ on {name} (see corpus-baseline.txt)."])
    return len(own)


BASELINE_HEAD = [
    "The sketch corpus's baseline (solver bar item 8, #187): the verdict main gives each drag.",
    "One line per dragged point (p) or curve (c): <sketch> <target> and its verdict along each",
    "path, in order: " + " ".join(PATHS) + ".",
    "`ok`, or a letter per check failed (crates/drift-edit/src/drag_props/corpus.rs): R rows,",
    "S arc sweep flip, W way-round flip, L line turned end for end, G held radius grew, J jump,",
    "X refused frame moved geometry, C release, P out and back, O outside the part.",
    "Measured on Windows; corpus-baseline.<platform>.txt holds the rows that differ elsewhere.",
    "Written by `python scripts/solver-bar/corpus.py run --update`; corpus.py fails on any",
    "verdict that differs.",
]


def summary(drags, heads):
    """The table for a PR body: per group of sketches, drags and how many fail each check."""
    public = [d for d in drags.values() if not d.private]
    private = [d for d in drags.values() if d.private]
    lines = []

    def table(title, group):
        by = collections.OrderedDict()
        for d in sorted(group, key=lambda d: d.sketch):
            name = "private" if d.private else d.sketch.split("-")[0]
            by.setdefault(name, []).append(d)
        letters = [c[0] for c in CHECKS]
        lines.append(f"### {title}")
        lines.append("")
        lines.append("| Sketches | Drags | ok | " + " | ".join(letters) +
                     " | Frames | Refused | Solves | Cap exits |")
        lines.append("|---|---|---|" + "---|" * len(letters) + "---|---|---|---|")
        rows = list(by.items())
        if len(rows) > 1:
            rows.append(("**all**", group))
        for name, ds in rows:
            fails = [sum(1 for d in ds if l in d.failed()) for l in letters]
            cap = sum(int(d.fields.get("ex", "0,0,0,0").split(",")[2]) for d in ds)
            sketches = len({d.sketch for d in ds})
            lines.append(
                f"| {name} ({sketches}) | {len(ds)} | {sum(1 for d in ds if d.verdict == 'ok')} | "
                + " | ".join(str(f) for f in fails)
                + f" | {sum(d.n('frames') for d in ds)} | {sum(d.n('ref') for d in ds)}"
                + f" | {sum(d.n('solves') for d in ds)} | {cap} |")
        lines.append("")

    table("The corpus", public)
    if private:
        table("Private drawings (in aggregate)", private)
    lines.append("Checks: " + "; ".join(f"`{l}` {what}" for l, _, what in CHECKS) + ".")
    lines.append("")
    noted = [("own", "frames an arc held by a dragged point swept over 90 degrees"),
             ("shrank", "frames a held radius was smaller than it began"),
             ("thru", "frames a line was squeezed through no length"),
             ("res", "drags that stopped and went on again")]
    lines.append("Noted, not judged: " + "; ".join(
        f"{sum(d.n(f) for d in drags.values())} {what}" for f, what in noted) + ".")
    unopened = [h for h in heads if " LOAD" in h]
    if unopened:
        lines.append("")
        lines.append(f"{len(unopened)} drawing(s) didn't open with their relations met and "
                     "weren't swept" + (": " + ", ".join(h.split()[1] for h in unopened
                                                         if not h.split()[1].startswith("private-"))
                                        if any(not h.split()[1].startswith("private-")
                                               for h in unopened) else "") + ".")
    return "\n".join(lines) + "\n"


def cmd_run(args):
    out = pathlib.Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    exe = args.exe or build(args.features)
    env_extra = dict(kv.split("=", 1) for kv in args.env)
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        results = list(pool.map(
            lambda k: run_shard(exe, k, args.jobs, out, env_extra, args.timeout),
            range(args.jobs)))
    broken = [(k, code) for k, code, _ in results if code != 0]
    lines = [l for _, _, text in results for l in record_lines(text)]
    drags = {}
    for l in lines:
        if l.startswith("D "):
            d = Drag(l)
            drags[d.key] = d
    heads = sorted(l for l in lines if l.startswith("H "))
    keys = sorted_keys(drags)
    (out / "record.txt").write_text(
        "\n".join([h for h in heads if not h.split()[1].startswith("private-")]
                  + [drags[k].line for k in keys if not drags[k].private]) + "\n",
        encoding="utf-8", newline="\n")
    if any(d.private for d in drags.values()):
        (out / "private.txt").write_text(
            "\n".join([h for h in heads if h.split()[1].startswith("private-")]
                      + [drags[k].line for k in keys if drags[k].private]) + "\n",
            encoding="utf-8", newline="\n")
    text = summary(drags, heads)
    (out / "summary.md").write_text(text, encoding="utf-8", newline="\n")
    print(text)
    if broken:
        for k, code in broken:
            print(f"shard {k} ended with {code}: see {out / f'shard-{k}.log'}")
        return 1
    got = baseline_rows(drags)
    if args.update:
        narrowed = [k for k in NARROWING if env_extra.get(k, os.environ.get(k))]
        why = partial(got, narrowed)
        if why:
            print(f"not updating the baseline: {why}. --update takes a whole sweep.")
            return 1
        if this_platform() == "windows":
            write_baseline(HERE / "corpus-baseline.txt", got, BASELINE_HEAD)
        else:
            write_platform_rows(this_platform(), got)
        print("baseline updated")
        return 0
    if args.no_judge:
        return 0
    want, files = expected_rows()
    print(f"judged against {' + '.join(files)} ({this_platform()})")
    changed = []
    for key in sorted(set(got) | set(want), key=lambda k: (k[0], target_order(k[1]))):
        g, w = got.get(key), want.get(key)
        if g is None or w is None:
            changed.append(f"{key[0]} {key[1]}: " + ("not in the baseline" if w is None else "not swept"))
            continue
        for path, a, b in zip(PATHS, w, g):
            if a != b:
                worse = set(b.replace("ok", "")) - set(a.replace("ok", ""))
                changed.append(f"{key[0]} {key[1]} {path}: {a} -> {b}"
                               + (f"  (newly fails {''.join(sorted(worse))})" if worse else "  (fixed)"))
    if changed:
        print(f"{len(changed)} verdict(s) differ from the baseline:")
        for line in changed[:200]:
            print("  " + line)
        if len(changed) > 200:
            print(f"  ... and {len(changed) - 200} more")
        print("replay one with CORPUS_ONLY=<sketch> CORPUS_TARGET=<e12> CORPUS_PATH=<path> "
              "CORPUS_TRACE=1; if the change is meant, run with --update")
        return 1
    print(f"all {sum(len(v) for v in got.values())} verdicts as the baseline has them")
    return 0


def diff_records(base, pr):
    """Lines describing how `pr` differs from `base`, and whether any drag newly fails."""
    out = []
    both = [k for k in sorted_keys(base) if k in pr]
    only_base = [k for k in base if k not in pr]
    only_pr = [k for k in pr if k not in base]
    worse, better, deeper, moved, refusals = [], [], [], 0, 0
    per_check_worse = collections.Counter()
    per_check_better = collections.Counter()
    for k in both:
        a, b = base[k], pr[k]
        fa, fb = a.failed(), b.failed()
        if fb - fa:
            worse.append((k, a, b))
            per_check_worse.update(fb - fa)
        if fa - fb:
            better.append((k, a, b))
            per_check_better.update(fa - fb)
        # Worse at a check it already failed: more frames failing it, or a jump twice as far.
        rose = [letter for letter, field, _ in CHECKS
                if field and letter in fa & fb and b.n(field) > a.n(field)]
        if "J" in fa & fb and "J" not in rose:
            wa, wb = float(a.fields.get("worst", 0)), float(b.fields.get("worst", 0))
            if wb >= 2 * wa:
                rose.append("J")
        if rose:
            deeper.append((k, a, b, rose))
        moved += a.fields.get("end") != b.fields.get("end")
        refusals += a.n("ref") != b.n("ref")
    out.append(f"{len(both)} drags in both records"
               + (f"; {len(only_base)} only in the base, {len(only_pr)} only in the PR"
                  if only_base or only_pr else ""))
    out.append(f"ended elsewhere (to the bit): {moved}; refused a different number of frames: {refusals}")
    out.append("")
    out.append("| Check | Newly failing | Newly passing | Failing in base | Failing in PR |")
    out.append("|---|---|---|---|---|")
    for letter, _, what in CHECKS:
        out.append(f"| `{letter}` {what} | {per_check_worse[letter]} | {per_check_better[letter]} | "
                   f"{sum(1 for k in both if letter in base[k].failed())} | "
                   f"{sum(1 for k in both if letter in pr[k].failed())} |")
    out.append("")
    for title, rows in (("Newly failing", worse), ("Newly passing", better)):
        public = [r for r in rows if not r[1].private]
        private = len(rows) - len(public)
        if rows:
            out.append(f"{title}: {len(rows)}" + (f" ({private} in private drawings)" if private else ""))
            for k, a, b in public[:60]:
                out.append(f"  {k[0]} {k[1]} {k[2]}: {a.verdict} -> {b.verdict}")
            if len(public) > 60:
                out.append(f"  ... and {len(public) - 60} more")
    if deeper:
        public = [r for r in deeper if not r[1].private]
        private = len(deeper) - len(public)
        out.append(f"Worse at a check already failing: {len(deeper)}"
                   + (f" ({private} in private drawings)" if private else ""))
        for k, a, b, rose in public[:60]:
            what = ", ".join(
                f"{letter} {a.fields.get(field)} -> {b.fields.get(field)} frames"
                if letter != "J" or b.n("jumps") > a.n("jumps")
                else f"J worst {a.fields.get('worst')} -> {b.fields.get('worst')}"
                for letter in rose
                for field in [next(f for l, f, _ in CHECKS if l == letter)])
            out.append(f"  {k[0]} {k[1]} {k[2]}: {b.verdict}: {what}")
        if len(public) > 60:
            out.append(f"  ... and {len(public) - 60} more")
    return out, bool(worse or deeper)


def cmd_diff(args):
    base, _ = read_record(args.base)
    pr, _ = read_record(args.pr)
    for extra, into in ((args.base, base), (args.pr, pr)):
        private = pathlib.Path(extra).with_name("private.txt")
        if private.exists():
            into.update(read_record(private)[0])
    lines, worse = diff_records(base, pr)
    print("\n".join(lines))
    return 1 if worse else 0


def cmd_baseline(args):
    """Write a platform's rows from a record made on it (CI's artifact, say)."""
    if args.platform == "windows":
        sys.exit("corpus-baseline.txt is Windows's: write it with `run --update` on Windows")
    drags, _ = read_record(args.record)
    got = baseline_rows(drags)
    why = partial(got, [])
    if why:
        sys.exit(f"not writing {platform_file(args.platform).name}: {why}")
    n = write_platform_rows(args.platform, got)
    print(f"{platform_file(args.platform).name}: {n} row(s) that differ from corpus-baseline.txt")
    return 0


def main():
    # (Windows consoles default to cp1252.)
    sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    run = sub.add_parser("run", help="run the sweep and judge it against the baseline")
    run.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 4) - 2))
    run.add_argument("--out", default=str(ROOT / "target" / "solver-bar" / "corpus"))
    run.add_argument("--exe", help="a drift-edit test binary built in release (skips the build)")
    run.add_argument("--features", help="cargo features for the build")
    run.add_argument("--env", action="append", default=[], metavar="KEY=VALUE",
                     help="set in the sweep's environment (repeatable)")
    run.add_argument("--timeout", type=int, default=3600, help="seconds per process")
    run.add_argument("--update", action="store_true", help="write the baseline from this run")
    run.add_argument("--no-judge", action="store_true", help="don't compare with the baseline")
    run.set_defaults(func=cmd_run)
    diff = sub.add_parser("diff", help="compare two records")
    diff.add_argument("base")
    diff.add_argument("pr")
    diff.set_defaults(func=cmd_diff)
    base = sub.add_parser("baseline", help="write a platform's rows from a record made there")
    base.add_argument("--from", dest="record", required=True, help="a record.txt from that platform")
    base.add_argument("--platform", required=True, help="linux, macos, ...")
    base.set_defaults(func=cmd_baseline)
    args = ap.parse_args()
    sys.exit(args.func(args))


if __name__ == "__main__":
    main()
