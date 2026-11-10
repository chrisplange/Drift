"""Read the solver bar's census logs (docs/solver-audit.md, section 5, items 2 and 3).

    python scripts/solver-bar/analyse.py similarity DIR [VARIANT,...]
    python scripts/solver-bar/analyse.py census BASE_DIR [PR_DIR]

`similarity`: DIR holds `<variant>-*.txt`, the `A ...` lines of
`drag_props::census::similarity_census` (one file per chunk of cases). For each variant:
drags per place, failures (a different number of refused frames or a different commit,
`rdiff`, or an end more than 1e-6 of the sketch's size off the origin's, mapped), drags that
broke a property (`ERR`), sketches that couldn't be committed there (`BUILD`), how the
solver's component solves ended, and how failures go with patience, cap and damping exits.
Variants default to `main`. It also reads the audit's raw logs (E:\\Drift\\solver-audit-logs,
`census2`), whose lines also carry the rows' conditioning at each exit (`sig`, `sb`).

`census`: each DIR holds `<place>.txt`, the `C ...` lines of `drag_props::census::drag_census`
or `drag_census_seeded` at one place each (the file name is the place). Alone, it counts per
place: drags, drags with a refused frame (stops), drags not committed, ERR and BUILD. With
PR_DIR, it compares the two builds drag by drag at each place: every new stop
(went -> stop), every stop that now goes, every commit that changed, and ends that moved.
"""

import collections
import glob
import math
import os
import re
import sys

NAMES = ["settled", "patience", "cap", "damping", "nostep"]
PLACES = ["s0.1", "s3.7", "s10", "x1m", "x100m", "x1km"]

SIM = re.compile(
    r"A (\d+) (\S+) (?:BUILD|(\S+) steps (\d+) (?:ERR ?(.*)|ref (\d+) com (\S+) rdiff (\d) "
    r"dev (\S+) rel (\S+) ?(.*)))"
)


def ints(s):
    return [int(x) for x in s.split(",")]


def stats(st):
    """Exit counts and the rest from a line's tail (new and audit formats)."""
    d = {"ex": [0] * 5, "it": 0, "solves": 0, "oex": [0] * 5, "sig": math.inf, "sb": None}
    for key in ("ex", "oex"):
        m = re.search(rf"(?:^| ){key} (\S+)", st)
        if m:
            v = ints(m[1])
            d[key] = (v + [0] * 5)[:5]
    m = re.search(r" it (\d+)", st)
    d["it"] = int(m[1]) if m else 0
    m = re.search(r"solves (\d+)", st)
    d["solves"] = int(m[1]) if m else sum(d["ex"])
    m = re.search(r"sig (\S+)", st)
    if m:
        d["sig"] = float(m[1])
    m = re.search(r"sb (\S+)", st)
    if m:
        d["sb"] = [ints(x) for x in m[1].split("/")]
    return d


def load_similarity(d, v):
    rows = {}
    for f in sorted(glob.glob(os.path.join(d, f"{v}-*.txt"))):
        with open(f, encoding="utf-8") as fh:
            for line in fh:
                i = line.find("A ")
                m = SIM.match(line[i:]) if i >= 0 else None
                if not m:
                    continue
                case, tag = int(m[1]), m[2]
                if m[3] is None:
                    rows[(case, tag)] = dict(build=True, err=True)
                elif m[6] is None:
                    rows[(case, tag)] = dict(build=False, err=True, **stats(m[5] or ""))
                else:
                    rows[(case, tag)] = dict(
                        build=False, err=False, steps=int(m[4]), ref=int(m[6]),
                        com=m[7] == "true", rdiff=int(m[8]), dev=float(m[9]),
                        rel=float(m[10]), **stats(m[11]),
                    )
    return rows


def failing(r):
    return r["rdiff"] or r["rel"] > 1e-6


def similarity(d, variants):
    data = {v: load_similarity(d, v) for v in variants}
    for v in variants:
        R = data[v]
        cases = sorted(c for (c, t), r in R.items() if t == "id" and not r["err"])
        print(f"\n=== {v}: {len(cases)} cases with an origin drag")
        print("| place | drags | fail similarity | % | refused differently | end off >1e-6 | ERR | BUILD |")
        print("|---|---|---|---|---|---|---|---|")
        for t in PLACES:
            n = rd = off = err = 0
            for c in cases:
                r = R.get((c, t))
                if r is None or r["build"]:
                    continue
                if r["err"]:
                    err += 1
                    continue
                n += 1
                rd += r["rdiff"]
                off += (not r["rdiff"]) and r["rel"] > 1e-6
            build = sum(1 for (c, tt), r in R.items() if tt == t and r["build"])
            bad = rd + off
            print(f"| {t} | {n} | {bad} | {100 * bad / max(n, 1):.2f} | {rd} | {off} | {err} | {build} |")
        build0 = sum(1 for (c, t), r in R.items() if t == "id" and r["build"])
        err0 = [c for (c, t), r in R.items() if t == "id" and r["err"] and not r["build"]]
        print(f"origin: BUILD {build0}, ERR {len(err0)}" + (f" (cases {err0[:20]})" if err0 else ""))
        errs = sorted((c, t) for (c, t), r in R.items() if r["err"] and not r["build"])
        if errs:
            print(f"ERR lines (a property broke): {len(errs)}; first: {errs[:30]}")

        def exits(where):
            ex = [0] * 5
            for (c, t), r in R.items():
                if not r["build"] and where(t):
                    ex = [a + b for a, b in zip(ex, r["ex"])]
            s = sum(ex) or 1
            return ", ".join(f"{NAMES[i]} {100 * ex[i] / s:.2f}%" for i in range(5)) + f" of {sum(ex)}"

        print("drag solves' exits, at the origin (id and scales): " + exits(lambda t: t in ("id", "s0.1", "s3.7", "s10")))
        for t in ("x1m", "x100m", "x1km"):
            print(f"drag solves' exits, {t}: " + exits(lambda tt, t=t: tt == t))
        spf = sorted(r["solves"] / max(r["steps"], 1) for r in R.values()
                     if not r["err"] and r["solves"] > 0)
        if spf:
            q = lambda p: spf[min(len(spf) - 1, int(p * len(spf)))]
            print(f"component solves per frame (drags that solve): median {q(0.5):.1f} "
                  f"p90 {q(0.9):.1f} p99 {q(0.99):.1f} max {spf[-1]:.0f}")
        if any(r.get("sb") for r in R.values()):
            sb = [[0] * 6 for _ in range(5)]
            for r in R.values():
                if r.get("sb"):
                    for i in range(5):
                        for j in range(6):
                            sb[i][j] += r["sb"][i][j]
            print("rows' conditioning at exit (<1e-12, 1e-12..1e-9, 1e-9..1e-6, 1e-6..1e-3, >=1e-3, unmeasured):")
            for i in range(5):
                if sum(sb[i]):
                    m = sum(sb[i][:5]) or 1
                    print(f"  {NAMES[i]:9s} {sb[i]}  <1e-6 {100 * sum(sb[i][:3]) / m:.1f}%  <1e-3 {100 * sum(sb[i][:4]) / m:.1f}%")
        fail, ok = [], []
        for c in cases:
            a = R[(c, "id")]
            for t in PLACES:
                r = R.get((c, t))
                if r is None or r["err"]:
                    continue
                heur = sum(a["ex"][1:4]) + sum(r["ex"][1:4]) > 0
                (fail if failing(r) else ok).append(heur)
        if fail:
            print(f"of {len(fail)} drags failing similarity, {100 * sum(fail) / len(fail):.1f}% had a "
                  f"patience, cap or damping exit (in it or its origin twin), against "
                  f"{100 * sum(ok) / max(len(ok), 1):.1f}% of {len(ok)} passing")
        worst = sorted(((r["rel"], c, t) for (c, t), r in R.items()
                        if not r["err"] and t in PLACES and failing(r)), reverse=True)
        print("failing drags (case place): " + " ".join(f"{c} {t}" for _, c, t in sorted(worst, key=lambda x: (x[2], x[1]))))


CEN = re.compile(r"C (\d+)(.*?)(?: (BUILD)$| ERR(.*)$| refused (\d+) committed (\S+) collapsed (\d+) far (\S+) drag (\S+) (.*?) \| (.*)$)")


def load_census(f):
    rows = {}
    with open(f, encoding="utf-8") as fh:
        for line in fh:
            i = line.find("C ")
            m = CEN.match(line[i:].rstrip("\n")) if i >= 0 else None
            if not m:
                continue
            case = int(m[1])
            if m[3]:
                rows[case] = dict(kind="build")
            elif m[5] is None:
                rows[case] = dict(kind="err")
            else:
                pts = [tuple(map(float, p.split(","))) for p in m[11].split(";") if p]
                rows[case] = dict(kind="ok", refused=int(m[5]), committed=m[6] == "true",
                                  collapsed=int(m[7]), far=float(m[8]), pts=pts, **stats(m[10]))
    return rows


def distance(place):
    """A place's name (`0`, `1m`, `100km`, or anything else) as a sort key."""
    m = re.fullmatch(r"([\d.]+)(km|m|mm)?", place)
    if not m:
        return (1, place)
    return (0, float(m[1]) * {"km": 1e6, "m": 1e3, "mm": 1.0, None: 1.0}[m[2]])


def places(d):
    names = [os.path.splitext(os.path.basename(f))[0] for f in glob.glob(os.path.join(d, "*.txt"))]
    return sorted(names, key=distance)


def census(base, pr):
    print("| place | drags | stops (a refused frame) | not committed | ERR | BUILD |" + (" went -> stop | stop -> went | commit changed | ends moved >1e-9 |" if pr else ""))
    print("|---|---|---|---|---|---|" + ("---|---|---|---|" if pr else ""))
    detail = []
    for p in places(base):
        A = load_census(os.path.join(base, p + ".txt"))
        ok = [r for r in A.values() if r["kind"] == "ok"]
        row = (f"| {p} | {len(ok)} | {sum(r['refused'] > 0 for r in ok)} | "
               f"{sum(not r['committed'] for r in ok)} | {sum(r['kind'] == 'err' for r in A.values())} | "
               f"{sum(r['kind'] == 'build' for r in A.values())} |")
        if pr:
            f = os.path.join(pr, p + ".txt")
            B = load_census(f) if os.path.exists(f) else {}
            ws = sw = cc = mv = 0
            for c, a in sorted(A.items()):
                b = B.get(c)
                if not b or a["kind"] != "ok" or b["kind"] != "ok":
                    if b and a["kind"] != b["kind"]:
                        detail.append(f"{p} case {c}: {a['kind']} -> {b['kind']}")
                    continue
                if a["refused"] == 0 and b["refused"] > 0:
                    ws += 1
                    detail.append(f"{p} case {c}: went -> stop ({b['refused']} frames refused)")
                elif a["refused"] > 0 and b["refused"] == 0:
                    sw += 1
                    detail.append(f"{p} case {c}: stop -> went")
                if a["committed"] != b["committed"]:
                    cc += 1
                    detail.append(f"{p} case {c}: committed {a['committed']} -> {b['committed']}")
                d = max((math.hypot(x[0] - y[0], x[1] - y[1]) for x, y in zip(a["pts"], b["pts"])), default=0.0)
                if d > 1e-9 * max(1.0, a["far"]):
                    mv += 1
            row += f" {ws} | {sw} | {cc} | {mv} |"
        print(row)
    for line in detail:
        print("  " + line)


def main():
    # (Windows consoles default to cp1252.)
    sys.stdout.reconfigure(encoding="utf-8")
    if len(sys.argv) < 3 or sys.argv[1] not in ("similarity", "census"):
        sys.exit(__doc__)
    if sys.argv[1] == "similarity":
        similarity(sys.argv[2], sys.argv[3].split(",") if len(sys.argv) > 3 else ["main"])
    else:
        census(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None)


if __name__ == "__main__":
    main()
