"""Read the solver bar's census logs (docs/solver-audit.md, section 5, items 2, 3 and 7).

    python scripts/solver-bar/analyse.py similarity DIR [BASE_VARIANT,PR_VARIANT,...]
    python scripts/solver-bar/analyse.py census BASE_DIR [PR_DIR]

`similarity`: DIR holds `<variant>-*.txt`, the `A ...` lines of
`drag_props::census::similarity_census` (one file per chunk of cases). For each variant:
drags per place, failures (a different number of refused frames or a different commit,
`rdiff`, or an end more than 1e-6 of the sketch's size off the origin's, mapped), drags that
broke a property (`ERR`), sketches that couldn't be committed there (`BUILD`), how the
solver's component solves ended, and how failures go with patience, cap and damping exits.
Variants default to `main`. With two or more, each is also compared with the first by
(case, place): newly failing, newly passing, ok -> ERR, ok -> BUILD, and pairs missing; exit
status 1 if a variant is missing pairs the first has, or broke a property the first didn't.
It also reads the audit's raw logs (E:\\Drift\\solver-audit-logs, `census2`), whose lines
also carry the rows' conditioning at each exit (`sig`, `sb`).

`census`: each DIR holds `<place>.txt`, the `C ...` lines of `drag_props::census::drag_census`
or `drag_census_seeded` at one place each (the file name is the place). Alone, it counts per
place: drags, stops (drags with a refused frame), drags not committed, ERR, BUILD, drags
ending with a collapsed arc, and, for the binding drag rule, drags that went while an arc's
radius changed and drags that grew a radius. With PR_DIR, it prints both builds' counts at
each place and compares them drag by drag: cases missing from the PR's census (or only in
it), drags that broke a property (ok -> ERR) or no longer build, new stops (went -> stop),
stops that now go, split by whether an arc's radius changed as it went (a kept radius never
may), stops whose number of refused frames changed, commits that changed, arcs that now
collapse or no longer do, and ends that moved; each listed by case. Exit status 1 if a
place or a case is missing from PR_DIR, or a drag broke a property it didn't on BASE.
"""

import collections
import glob
import math
import os
import re
import sys

NAMES = ["settled", "patience", "cap", "damping", "nostep"]
# Since solver PR 3 the lines carry four exits: the stop test (rows hold, the kept shapes
# stationary), a conflict (the rows off at a least-squares stationary point), the iteration
# cap (the safety net) and no step (a residual that isn't a number).
NAMES_PR3 = ["settled", "conflict", "cap", "nostep", "-"]


def names(layout):
    """The exits' names for lines that carry `layout` of them."""
    return NAMES_PR3 if layout == 4 else NAMES


def off_the_stop_test(ex, layout):
    """Solves that ended other than by a principled stop: patience, the cap and damping blow-ups
    before PR 3 (indices 1 to 3), the cap and no step since (2 and 3)."""
    return sum(ex[2:4]) if layout == 4 else sum(ex[1:4])
PLACES = ["s0.1", "s3.7", "s10", "x1m", "x100m", "x1km"]

SIM = re.compile(
    r"A (\d+) (\S+) (?:BUILD|(\S+) steps (\d+) (?:ERR ?(.*)|ref (\d+) com (\S+) rdiff (\d) "
    r"dev (\S+) rel (\S+) ?(.*)))"
)


def ints(s):
    return [int(x) for x in s.split(",")]


def stats(st):
    """Exit counts and the rest from a line's tail (new and audit formats)."""
    d = {"ex": [0] * 5, "it": 0, "solves": 0, "oex": [0] * 5, "sig": math.inf, "sb": None,
         "layout": 5}
    for key in ("ex", "oex"):
        m = re.search(rf"(?:^| ){key} (\S+)", st)
        if m:
            v = ints(m[1])
            d[key] = (v + [0] * 5)[:5]
            d["layout"] = len(v)
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

        layout = next((r["layout"] for r in R.values() if "layout" in r), 5)
        nm = names(layout)

        def exits(where):
            ex = [0] * 5
            for (c, t), r in R.items():
                if not r["build"] and where(t):
                    ex = [a + b for a, b in zip(ex, r["ex"])]
            s = sum(ex) or 1
            return ", ".join(f"{nm[i]} {100 * ex[i] / s:.2f}%" for i in range(layout)) + f" of {sum(ex)}"

        # (Shares of every component solve in the drag frames of the drags run there: the
        # origin row pools the origin's drags and the three scales'.)
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
                heur = off_the_stop_test(a["ex"], layout) + off_the_stop_test(r["ex"], layout) > 0
                (fail if failing(r) else ok).append(heur)
        if fail:
            what = "a cap or no-step" if layout == 4 else "a patience, cap or damping"
            print(f"of {len(fail)} drags failing similarity, {100 * sum(fail) / len(fail):.1f}% had "
                  f"{what} exit (in it or its origin twin), against "
                  f"{100 * sum(ok) / max(len(ok), 1):.1f}% of {len(ok)} passing")
        worst = sorted((t, c) for (c, t), r in R.items() if not r["err"] and t in PLACES and failing(r))
        print("failing drags (case place): " + " ".join(f"{c} {t}" for t, c in worst))
    return compare_similarity(data, variants)


def compare_similarity(data, variants):
    """Each variant after the first against the first, (case, place) by (case, place); the
    exit status."""
    status = 0
    A = data[variants[0]]
    for v in variants[1:]:
        B = data[v]
        lists = collections.defaultdict(list)
        for key in sorted(set(A) | set(B)):
            a, b = A.get(key), B.get(key)
            if b is None:
                lists["missing"].append(key)
                continue
            if a is None:
                lists["only"].append(key)
                continue
            if not a["err"] and b["err"] and not b["build"]:
                lists["ok -> ERR"].append(key)
            if not a["build"] and b["build"]:
                lists["ok -> BUILD"].append(key)
            if key[1] not in PLACES or a["err"] or b["err"]:
                continue
            if failing(b) and not failing(a):
                lists["newly failing"].append(key)
            elif failing(a) and not failing(b):
                lists["newly passing"].append(key)
        print(f"\n=== {v} against {variants[0]}, by (case, place)")
        for name, label in (("newly failing", "newly failing"), ("newly passing", "newly passing"),
                            ("ok -> ERR", "ok -> ERR"), ("ok -> BUILD", "ok -> BUILD"),
                            ("missing", f"missing in {v}"), ("only", f"only in {v}")):
            keys = lists[name]
            print(f"{label}: {len(keys)}" + (": " + " ".join(f"{c} {t}" for c, t in keys) if keys else ""))
        if lists["missing"] or lists["ok -> ERR"]:
            status = 1
            print(f"INCOMPLETE OR BROKEN: {v} is missing pairs or broke a property where {variants[0]} didn't.")
    return status


CEN = re.compile(
    r"C (\d+)(.*?)(?: (BUILD)$| ERR(.*)$| refused (\d+) committed (\S+) collapsed (\d+) far (\S+) "
    r"drag (\S+) (.*?)(?: radii (\S*))? \| (.*)$)"
)
# An arc's radius counts as changed past this share of itself (or of 1 mm, if smaller).
RADIUS_BAR = 1e-9


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
                pts = [tuple(map(float, p.split(","))) for p in m[12].split(";") if p]
                radii = [tuple(map(float, r.split(">"))) for r in (m[11] or "").split(";") if r]
                rows[case] = dict(kind="ok", refused=int(m[5]), committed=m[6] == "true",
                                  collapsed=int(m[7]), far=float(m[8]), pts=pts, radii=radii,
                                  has_radii=m[11] is not None, **stats(m[10]))
    return rows


def radius_changed(r):
    """Whether an arc's radius changed over the drag; None if the line doesn't say."""
    if not r["has_radii"]:
        return None
    return any(abs(b - a) > RADIUS_BAR * max(1.0, a) for a, b in r["radii"])


def radius_grew(r):
    if not r["has_radii"]:
        return None
    return any(b - a > RADIUS_BAR * max(1.0, a) for a, b in r["radii"])


def distance(place):
    """A place's name (`0`, `1m`, `100km`, or anything else) as a sort key."""
    m = re.fullmatch(r"([\d.]+)(km|m|mm)?", place)
    if not m:
        return (1, place)
    return (0, float(m[1]) * {"km": 1e6, "m": 1e3, "mm": 1.0, None: 1.0}[m[2]])


def places(d):
    names = [os.path.splitext(os.path.basename(f))[0] for f in glob.glob(os.path.join(d, "*.txt"))]
    return sorted(names, key=distance)


COUNTS = ("drags", "stops", "not committed", "ERR", "BUILD", "collapsed",
          "went, a radius changed", "a radius grew")


def counts(R):
    """One build's counts at one place, in the order of COUNTS."""
    ok = [r for r in R.values() if r["kind"] == "ok"]
    return [
        len(ok),
        sum(r["refused"] > 0 for r in ok),
        sum(not r["committed"] for r in ok),
        sum(r["kind"] == "err" for r in R.values()),
        sum(r["kind"] == "build" for r in R.values()),
        sum(r["collapsed"] > 0 for r in ok),
        sum(r["refused"] == 0 and bool(radius_changed(r)) for r in ok),
        sum(bool(radius_grew(r)) for r in ok),
    ]


CHANGES = ("missing in PR", "only in PR", "ok -> ERR", "ok -> BUILD", "went -> stop",
           "stop -> went, radii kept", "stop -> went, a radius changed",
           "stop, refusals changed", "commit changed", "collapse changed", "ends moved >1e-9")

NOTE = (
    "stops: drags with a refused frame. collapsed: drags ending with an arc crushed or closed. "
    "went, a radius changed: drags that refused no frame while an arc's radius changed by more "
    f"than {RADIUS_BAR} of itself; a drag of an arc's own end may change its radius, a kept "
    "radius may not (the binding drag rule), so each such case is one to check. "
    "a radius grew: drags ending with an arc's radius larger than it began."
)


def census(base, pr):
    """Print the table; the exit status."""
    if not pr:
        print("| place | " + " | ".join(COUNTS) + " |")
        print("|---|" + "---|" * len(COUNTS))
        for p in places(base):
            row = counts(load_census(os.path.join(base, p + ".txt")))
            print(f"| {p} | " + " | ".join(map(str, row)) + " |")
        print("\n" + NOTE)
        return 0
    status = 0
    print("| place | build | " + " | ".join(COUNTS) + " | " + " | ".join(CHANGES) + " |")
    print("|---|---|" + "---|" * (len(COUNTS) + len(CHANGES)))
    detail = []
    for p in places(base) + [q for q in places(pr) if q not in places(base)]:
        fa, fb = os.path.join(base, p + ".txt"), os.path.join(pr, p + ".txt")
        if not (os.path.exists(fa) and os.path.exists(fb)):
            print(f"| {p} | **{'PR' if os.path.exists(fa) else 'BASE'} has no census here** |")
            status = 1
            continue
        A, B = load_census(fa), load_census(fb)
        n = collections.Counter()

        def note(key, c, text):
            n[key] += 1
            detail.append(f"{p} case {c}: {text}")

        for c in sorted(set(A) | set(B)):
            a, b = A.get(c), B.get(c)
            if b is None:
                note("missing in PR", c, "missing in PR")
                continue
            if a is None:
                note("only in PR", c, "only in PR")
                continue
            if a["kind"] != b["kind"]:
                key = {("ok", "err"): "ok -> ERR", ("ok", "build"): "ok -> BUILD"}.get((a["kind"], b["kind"]))
                if key:
                    note(key, c, key)
                else:
                    detail.append(f"{p} case {c}: {a['kind']} -> {b['kind']}")
                continue
            if a["kind"] != "ok":
                continue
            if a["refused"] == 0 and b["refused"] > 0:
                note("went -> stop", c, f"went -> stop ({b['refused']} frames refused)")
            elif a["refused"] > 0 and b["refused"] == 0:
                if radius_changed(b) is False:
                    note("stop -> went, radii kept", c, "stop -> went, radii kept")
                else:
                    note("stop -> went, a radius changed", c,
                         "stop -> went with an arc's radius changed: check the binding drag rule")
            elif a["refused"] != b["refused"]:
                note("stop, refusals changed", c, f"stop, {a['refused']} -> {b['refused']} frames refused")
            if a["committed"] != b["committed"]:
                note("commit changed", c, f"committed {a['committed']} -> {b['committed']}")
            if (a["collapsed"] > 0) != (b["collapsed"] > 0):
                note("collapse changed", c, f"collapsed arcs {a['collapsed']} -> {b['collapsed']}")
            d = max((math.hypot(x[0] - y[0], x[1] - y[1]) for x, y in zip(a["pts"], b["pts"])), default=0.0)
            if len(a["pts"]) != len(b["pts"]) or d > 1e-9 * max(1.0, a["far"]):
                note("ends moved >1e-9", c, f"end moved {d:.3e}")
        if n["missing in PR"] or n["ok -> ERR"]:
            status = 1
        print(f"| {p} | BASE | " + " | ".join(map(str, counts(A))) + " |" + " |" * len(CHANGES))
        print(f"| {p} | PR | " + " | ".join(map(str, counts(B))) + " | "
              + " | ".join(str(n[k]) for k in CHANGES) + " |")
    for line in detail:
        print("  " + line)
    print("\n" + NOTE)
    if status:
        print("\nINCOMPLETE OR BROKEN: the PR's census is missing a place or cases, or a drag broke "
              "a property it didn't on BASE.")
    return status


def main():
    # (Windows consoles default to cp1252.)
    sys.stdout.reconfigure(encoding="utf-8")
    if len(sys.argv) < 3 or sys.argv[1] not in ("similarity", "census"):
        sys.exit(__doc__)
    if sys.argv[1] == "similarity":
        sys.exit(similarity(sys.argv[2], sys.argv[3].split(",") if len(sys.argv) > 3 else ["main"]))
    sys.exit(census(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None))


if __name__ == "__main__":
    main()
