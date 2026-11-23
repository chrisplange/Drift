"""Write docs/solver-heuristics.md, the heuristic ledger (#187), from its rows
(`ledger_rows.py`: what each heuristic is and protects, written by hand) and the ablation's
numbers (`ablation.json`, as `ablate.py` leaves it in its --out folder; the committed copy
is the run the ledger quotes, with each switch's list of newly failing drags cut to 25).

    python scripts/solver-bar/ledger.py [ablation.json] [docs/solver-heuristics.md] [commit]

After an `ablate.py` run of some switches, copy their entries into `ablation.json` (or the
whole file, after a full run) and run this again.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from ledger_rows import NOT_SWITCHED, SECTIONS  # noqa: E402

HERE = pathlib.Path(__file__).resolve().parent
args = sys.argv[1:]
result = json.loads(pathlib.Path(args[0] if args else HERE / "ablation.json").read_text(encoding="utf-8"))
out = pathlib.Path(args[1] if len(args) > 1 else HERE.parents[1] / "docs" / "solver-heuristics.md")
commit = args[2] if len(args) > 2 else result["base"]["commit"]
base = result["base"]
LETTERS = "RSWLGJXCPO"


def short(test):
    parts = test.split("::")
    return parts[-1]


def guards(r):
    t = r.get("tests")
    if t is None:
        return "not run", False
    failed = t["solver"] + t["edit"]
    extra = ""
    if t["hung"] or not t["finished"]:
        extra = " A test run didn't finish" + (
            " (" + ", ".join(f"`{short(x)}`" for x in t["hung"][:2]) + ")" if t["hung"] else "") + "."
    if not failed:
        return "none fails" + ("." + extra if extra else ""), bool(extra)
    names = ", ".join(f"`{short(x)}`" for x in failed[:4])
    more = f" and {len(failed) - 4} more" if len(failed) > 4 else ""
    return f"{len(failed)}: {names}{more}.{extra}", True


def corpus_cell(r):
    c = r.get("corpus")
    if c is None:
        return "not run", False, False
    by = ", ".join(f"{n} `{l}`" for l, n in c["worse"].items() if n)
    cell = f"{c['newly_failing']} newly fail" + (f" ({by})" if by else "")
    if c["newly_passing"]:
        cell += f"; {c['newly_passing']} newly pass"
    if c["missing"]:
        cell += f"; {c['missing']} drags not finished"
    cell += f"; {c['moved']} end elsewhere"
    solves = c["cost"]["solves"] / max(1, base["cost"]["solves"])
    refused = c["cost"]["refused"] - base["cost"]["refused"]
    cell += f"; solves ×{solves:.2f}"
    if refused:
        cell += f", frames refused {refused:+d}"
    if c["refusals_differ"] and not c["moved"]:
        cell += f"; {c['refusals_differ']} drags refuse other frames"
    return cell, bool(c["newly_failing"] or c["missing"]), (c["moved"], c["refusals_differ"])


lines = []
w = lines.append
w("# The solver's heuristics: what each protects, and what breaks without it")
w("")
w(f"*The ledger #187 asked for. Measured on `main` at {commit} (Windows, release), by "
  "`scripts/solver-bar/ablate.py`. It replaces section 4 of "
  "[solver-audit.md](solver-audit.md), which predates the audit's PR 3, PR 5 and #489.*")
w("")
w("Every rule and constant layered onto the solve (`newton` in "
  "`crates/drift-solver/src/lib.rs`) and the drag (`crates/drift-edit/src/editor/drag.rs`) "
  "is listed here with what it protects, the tests that guard it, and what the sketch corpus "
  "does without it.")
w("")
w("## How it was measured")
w("")
w("Each heuristic has a switch (`drift_solver::ablation::Switch`). The switches exist only "
  "in builds with the test-only `ablate` feature; in every other build each is `false` at "
  "compile time, and the code is as it was. With a switch thrown (`DRIFT_ABLATE=<name>`), "
  "its rule is off, or its constant changed as the row says. For each switch in turn:")
w("")
w("- **Guarding tests**: the normal tests of drift-solver and drift-edit were run, and the "
  "ones that fail are listed. A heuristic no test fails without has no guard.")
w("- **Corpus**: the sketch corpus (the solver bar's item 8: "
  f"{base['drags']:,} drags of its {base['sketches']} sketches, every frame judged) was swept and "
  "compared, drag by drag, with the same build with nothing thrown. \"Newly fail\" counts "
  "drags that fail a check they passed, by check: `R` rows met, `S` arc sweep flip, `W` "
  "way-round flip, `L` line turned end for end, `G` held radius grew, `J` jump over 50 "
  "times the pointer, `X` refused frame moved geometry, `C` the release, `P` out and back, "
  "`O` outside the part. \"End elsewhere\" counts drags whose part ended anywhere else, to "
  "the last bit. \"Solves\" is the sweep's drag solves against the base's.")
w("")
w("**Verdict.** *Breaks*: a test fails or a corpus drag newly fails a check. *Nothing "
  "breaks; drags end elsewhere*: no test and no check fails, but results change, so the "
  "rule does something nothing here measures as better or worse. *Nothing breaks; other "
  "frames refused*: every drag ends where it did, to the bit, but some refuse different "
  "frames on the way. ***Nothing breaks without "
  "it***: no test fails, no check fails, every drag ends where it did, to the bit, and "
  "refuses the same frames: on "
  "this evidence the rule is dead weight, or guards a case neither the tests nor the corpus "
  "holds (the row says which is likelier).")
w("")
w("What the verdicts can't see: the corpus sits at the origin at one size (the similarity "
  "census, item 2 of the bar, is what measures size and place), it has no Wall-tool walls, "
  "and its drags are a pointer's (typed moves, nudges and relation adding are other "
  "paths). A rule that only acts there reads as dead here. **No heuristic should be deleted "
  "on this table alone**: a row's flag is where to look, and the deletion is a PR judged by "
  "the whole bar.")
w("")
w("To run it again (hours for every switch; `--only` for a few):")
w("")
w("```")
w("python scripts/solver-bar/ablate.py --tests --out DIR [--only strides,fold-try]")
w("```")
w("")
w("This page is written by `scripts/solver-bar/ledger.py` from `ledger_rows.py` (the columns "
  "written by hand: what each heuristic is and protects) and `ablation.json` (the run's "
  "numbers). Change a row there, not here.")
w("")
flagged = []
breaks = []
moved_only = []
refused_only = []
for title, rows in SECTIONS:
    w(f"## {title}")
    w("")
    w("| Heuristic or constant (switch) | Where | What it protects | Tests that fail without it | Corpus without it | Verdict |")
    w("|---|---|---|---|---|---|")
    for name, what, where, protects in rows:
        r = result.get(name, {})
        g, g_breaks = guards(r)
        c, c_breaks, (c_moved, c_refusals) = corpus_cell(r) if r else ("not run", False, (0, 0))
        if not r:
            verdict = "not run"
        elif g_breaks or c_breaks:
            verdict = "breaks"
            breaks.append(name)
        elif c_moved:
            verdict = "nothing breaks; drags end elsewhere"
            moved_only.append(name)
        elif c_refusals:
            verdict = "nothing breaks; other frames refused"
            refused_only.append(name)
        else:
            verdict = "**nothing breaks without it**"
            flagged.append(name)
        w(f"| {what} (`{name}`) | {where} | {protects} | {g} | {c} | {verdict} |")
    w("")

w("## Constants and rules with no switch")
w("")
w("| Constant or rule | Where | What it is for | Why it has no switch |")
w("|---|---|---|---|")
for what, where, why, no in NOT_SWITCHED:
    w(f"| {what} | {where} | {why} | {no} |")
w("")
w("## Reading")
w("")
w(f"Of {sum(len(r) for _, r in SECTIONS)} switches: {len(breaks)} break a test or a corpus "
  f"check when thrown, {len(moved_only)} change where drags end without breaking either, "
  f"{len(refused_only)} change only which frames are refused, "
  f"and {len(flagged)} change nothing at all.")
w("")
if flagged:
    w("**Nothing breaks without it** (candidates to look at, not to delete on this alone): "
      + ", ".join(f"`{n}`" for n in flagged) + ".")
    w("")
if moved_only:
    w("**Nothing breaks, drags end elsewhere** (the rule acts, and nothing here says "
      "whether for better): " + ", ".join(f"`{n}`" for n in moved_only) + ".")
    w("")
if refused_only:
    w("**Nothing breaks, other frames refused** (every drag ends where it did; the rule "
      "changes the way there): " + ", ".join(f"`{n}`" for n in refused_only) + ".")
    w("")
WHY_DEAD = {
    "edit-preference-bound": "an edit's rule (PR 6), and the sweep's only edits are its releases, which begin with their shapes met. Its case (a line kept square stretched 40 km) has no test: **it needs one before anyone touches it**",
    "leave-storage": "it only acts where the floor of ulps is above the bar, about a million part-sizes from the origin (#377's seeded census, 100 km out). The corpus and the normal tests are at the origin: the similarity and seeded censuses are its guard, not these",
    "settle-meets-relations": "its case is one census drag (7930). No test pins it and no corpus drag reaches it: **the likeliest real candidate for a pinned test, or for deletion once the census agrees**",
    "lowered-line-flip": "walls only (relations lowered onto a wall's centreline). No corpus sketch has a Wall-tool wall, and `face_dimensions_hold_through_drags`' light run doesn't reach it: the heavy matrix's wall property is its guard",
}
all_rows = [(n, what) for _, rows in SECTIONS for n, what, _, _ in rows]
w("### Nothing breaks without it: why, row by row")
w("")
for n in flagged:
    w(f"- `{n}`: {WHY_DEAD.get(n, 'not explained')}.")
w("")
unguarded = [n for n, _ in all_rows
             if result.get(n, {}).get("tests") is not None
             and not (result[n]["tests"]["solver"] + result[n]["tests"]["edit"])]
w("### No test guards it")
w("")
with_corpus = [n for n in unguarded if result[n]["corpus"]["newly_failing"]]
without = [n for n in unguarded if not result[n]["corpus"]["newly_failing"]]
w("No normal test of drift-solver or drift-edit fails with these thrown.")
w("")
w("- **Guarded by the corpus alone** (corpus drags newly fail, no test does): "
  + ", ".join(f"`{n}` ({result[n]['corpus']['newly_failing']} drags)" for n in with_corpus)
  + ". Each wants a pinned test of its own, taken from one of those drags.")
w("- **Guarded by nothing here**: " + ", ".join(f"`{n}`" for n in without) + ".")
w("")
w("(The ignored pins, the heavy matrix and the censuses weren't run per switch: a rule "
  "guarded only there shows here as unguarded.)")
w("")
helped = [(n, result[n]["corpus"]) for n, _ in all_rows
          if result.get(n, {}).get("corpus") and result[n]["corpus"]["newly_passing"] >= 5]
helped.sort(key=lambda x: x[1]["newly_passing"] - x[1]["newly_failing"], reverse=True)
w("### Off, drags that fail on main pass")
w("")
failing = base["failing"]
w(f"Main fails {failing['drags']} of the corpus's drags ("
  + ", ".join(f"{n} `{l}`" for l, n in failing["by"].items() if n)
  + "; #526, #344, #362). "
  "With these thrown, some of those pass. That is a lead on where those failures come from, "
  "not a case for deleting the rule: each row also shows what it costs.")
w("")
w("| Switch | Newly pass (by check) | Newly fail | Tests that fail | End elsewhere |")
w("|---|---|---|---|---|")
for n, c in helped:
    by = ", ".join(f"{k} `{l}`" for l, k in c["better"].items() if k)
    t = result[n]["tests"]
    w(f"| `{n}` | {c['newly_passing']} ({by}) | {c['newly_failing']} | "
      f"{len(t['solver']) + len(t['edit'])} | {c['moved']} |")
w("")
w("### Cost")
w("")
w(f"Drag solves over the whole sweep, against {base['cost']['solves']:,} with nothing thrown, where a switch "
  "moves them by more than a tenth:")
w("")
w("| Switch | Solves | Frames refused | Solves ending at the iteration cap |")
w("|---|---|---|---|")
for n, _ in all_rows:
    c = result.get(n, {}).get("corpus")
    if not c:
        continue
    x = c["cost"]["solves"] / base["cost"]["solves"]
    if x > 1.1 or x < 0.9:
        w(f"| `{n}` | ×{x:.2f} | {c['cost']['refused'] - base['cost']['refused']:+d} | "
          f"{c['cost']['cap'] - base['cost']['cap']:+d} |")
w("")
w("## Appendix: every test that fails, switch by switch")
w("")
w("drift-solver's tests first, then drift-edit's (`drag_props::`, `held_shapes::`, `seeds::`, "
  "the editor's own), by name.")
w("")
for n, _ in all_rows:
    t = result.get(n, {}).get("tests")
    if not t or not (t["solver"] + t["edit"]):
        continue
    names = ", ".join(f"`{short(x)}`" for x in t["solver"] + t["edit"])
    w(f"- **`{n}`** ({len(t['solver']) + len(t['edit'])}): {names}")
w("")
out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
print(f"wrote {out}: {len(breaks)} break, {len(moved_only)} moved only, "
      f"{len(refused_only)} refused only, {len(flagged)} nothing")
