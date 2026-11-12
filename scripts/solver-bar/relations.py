"""Compare two runs of the relation-adding census (drag_props::relation_census).

    python scripts/solver-bar/relations.py BASE.txt PR.txt [--list N]

Prints, per relation kind and overall: how many cases each build added and refused (and why),
cases one build adds and the other refuses, and for the cases both add: how often a line
changed length or an arc radius (the geometry the relation names, and the rest), how far
points moved, and how far the rest's lines turned. `--list N` lists up to N cases per change
class, for tracing. Exit status 1 if PR newly refuses a case BASE adds.
"""

import sys
from collections import Counter, defaultdict


def read(path):
    out = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            parts = line.split()
            if not parts or parts[0] != "R":
                continue
            case, kind, act, result = int(parts[1]), parts[2], parts[3], parts[4]
            vals = {}
            rest = parts[5:]
            for k, v in zip(rest[::2], rest[1::2]):
                vals[k] = float(v)
            out[case] = (kind, act, result, vals)
    return out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    listing = 0
    if "--list" in sys.argv:
        listing = int(sys.argv[sys.argv.index("--list") + 1])
        args = [a for a in args if a != str(listing)]
    base, pr = read(args[0]), read(args[1])
    cases = sorted(set(base) & set(pr))
    missing = sorted(set(base) - set(pr))
    if missing:
        print(f"PR is missing {len(missing)} cases its base has")

    results = defaultdict(lambda: [Counter(), Counter()])
    newly_refused, newly_added = [], []
    both = []
    for c in cases:
        kb, ab, rb, vb = base[c]
        kp, ap, rp, vp = pr[c]
        assert (kb, ab) == (kp, ap), f"case {c} differs in what it adds"
        if rb in ("BUILD", "NOPICK"):
            continue
        results[(kb, ab)][0][rb] += 1
        results[(kb, ab)][1][rp] += 1
        if rb == "ok" and rp != "ok":
            newly_refused.append((c, kb, ab, rp))
        elif rb != "ok" and rp == "ok":
            newly_added.append((c, kb, ab, rb))
        elif rb == "ok":
            both.append((c, kb, ab, vb, vp))

    print("Added / refused, base -> PR, per relation and active element:")
    kinds = sorted({k for k, _ in results})
    for k in kinds:
        for a in ("none", "act"):
            if (k, a) not in results:
                continue
            b, p = results[(k, a)]
            fmt = lambda r: " ".join(f"{x} {r[x]}" for x in sorted(r))
            print(f"  {k:13} {a:4}  base: {fmt(b):40}  PR: {fmt(p)}")
    total_b, total_p = Counter(), Counter()
    for b, p in results.values():
        total_b.update(b)
        total_p.update(p)
    print(f"  {'all':18}  base: {dict(total_b)}  PR: {dict(total_p)}")
    print(f"Newly refused: {len(newly_refused)}; newly added: {len(newly_added)}")
    for c, k, a, why in newly_refused[: max(listing, 20)]:
        print(f"  NEWLY REFUSED {c} {k} {a}: {why}")
    for c, k, a, why in newly_added[:listing]:
        print(f"  newly added {c} {k} {a} (base: {why})")

    def share(rows, test):
        n = sum(1 for r in rows if test(r))
        return f"{n} ({100.0 * n / max(len(rows), 1):.1f} %)"

    print(f"\nBoth added: {len(both)} cases")
    for label, key in [
        ("named lines changed length", "nl"),
        ("rest lines changed length", "rl"),
        ("named arcs changed radius", "na"),
        ("rest arcs changed radius", "ra"),
    ]:
        b = share(both, lambda r: r[3].get(key, 0) > 0)
        p = share(both, lambda r: r[4].get(key, 0) > 0)
        print(f"  {label:30} base {b:16} PR {p}")
    b = share(both, lambda r: r[3].get("rturn", 0) > 0.01)
    p = share(both, lambda r: r[4].get("rturn", 0) > 0.01)
    print(f"  {'rest lines turned > 0.01°':30} base {b:16} PR {p}")
    b = share(both, lambda r: r[3].get("rturn", 0) > 90)
    p = share(both, lambda r: r[4].get("rturn", 0) > 90)
    print(f"  {'rest lines flipped (> 90°)':30} base {b:16} PR {p}")

    def median(xs):
        xs = sorted(xs)
        return xs[len(xs) // 2] if xs else 0.0

    for label, key in [
        ("farthest point moved, mm", "mv"),
        ("points moved", "pts"),
        ("named lines' length change, mm", "ndl"),
        ("rest lines' length change, mm", "rdl"),
    ]:
        mb = [r[3].get(key, 0) for r in both]
        mp = [r[4].get(key, 0) for r in both]
        print(
            f"  {label:30} median base {median(mb):.4g} PR {median(mp):.4g};"
            f" max base {max(mb, default=0):.4g} PR {max(mp, default=0):.4g}"
        )

    # Per case: what changed between the builds.
    same = [r for r in both if r[3] == r[4]]
    print(f"  identical result: {len(same)}")
    classes = defaultdict(list)
    for c, k, a, vb, vp in both:
        if vb == vp:
            continue
        if vb.get("nl", 0) > 0 and vp.get("nl", 0) == 0:
            classes["named line no longer changes length"].append((c, k, a, vb, vp))
        if vb.get("nl", 0) == 0 and vp.get("nl", 0) > 0:
            classes["named line newly changes length"].append((c, k, a, vb, vp))
        if vb.get("rl", 0) > 0 and vp.get("rl", 0) == 0:
            classes["rest line no longer changes length"].append((c, k, a, vb, vp))
        if vb.get("rl", 0) == 0 and vp.get("rl", 0) > 0:
            classes["rest line newly changes length"].append((c, k, a, vb, vp))
        if vb.get("ra", 0) == 0 and vp.get("ra", 0) > 0:
            classes["rest arc newly changes radius"].append((c, k, a, vb, vp))
        if vb.get("na", 0) == 0 and vp.get("na", 0) > 0:
            classes["named arc newly changes radius"].append((c, k, a, vb, vp))
        if vp.get("rturn", 0) > 90 >= vb.get("rturn", 0):
            classes["rest line newly flipped"].append((c, k, a, vb, vp))
        if vp.get("mv", 0) > 2 * vb.get("mv", 0) + 1.0:
            classes["moves farther (> 2x + 1 mm)"].append((c, k, a, vb, vp))
        if vb.get("mv", 0) > 2 * vp.get("mv", 0) + 1.0:
            classes["moves less (< 1/2 - 1 mm)"].append((c, k, a, vb, vp))
    print("\nChange classes (cases both add):")
    for label in sorted(classes):
        rows = classes[label]
        by_kind = Counter(f"{k}/{a}" for _, k, a, _, _ in rows)
        print(f"  {label}: {len(rows)}  {dict(by_kind.most_common(6))}")
        for c, k, a, vb, vp in rows[:listing]:
            keys = ("mv", "pts", "nl", "ndl", "rl", "rdl", "na", "nda", "ra", "rda", "rturn")
            fb = " ".join(f"{x}={vb.get(x, 0):g}" for x in keys)
            fp = " ".join(f"{x}={vp.get(x, 0):g}" for x in keys)
            print(f"    {c} {k} {a}\n      base {fb}\n      PR   {fp}")
    return 1 if newly_refused else 0


if __name__ == "__main__":
    sys.exit(main())
