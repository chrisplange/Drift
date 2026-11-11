# Solver audit: root causes and a redesign plan

*2026-11-10, read-only audit of `main` at b800322. No product code changed. Measured with a
probe build that is not committed (see [How this was measured](#how-this-was-measured)).*

## The short version

Since mid-October every solver or drag fix (#94, A2, A3, #278, #281, #282, #284, #377) has added
a rule on top of the last one. The solver's `lib.rs` grew from 937 lines on 15 October to
2,672; 39 commits have touched it or the drag code since. 7 drag tests are pinned and ignored.
The audit tested the four hypotheses from the rethink handoff against the code and with new
measurements.

| Hypothesis | Verdict | Strongest number |
|---|---|---|
| (a) Absolute coordinates instead of a part-local, size-normalised frame | **Holds**, with one correction: the frame only works together with a tolerance that sits above round-off | Similarity failures 1 km out: **4.01 % → 0.30 %**; 100 m out: **1.06 % → 0.08 %** (frame + size bar on every solve) |
| (b) Heuristic stopping (patience, caps) instead of a convergence test | **Holds**, and it is the strongest | **97.4 %** of drags that fail similarity had a patience, cap or damping blow-up exit, against 4.3 % of drags that pass |
| (c) No fold or near-singular detection, so round-off picks the branch | **Holds in part** | Near-singular rows (σmin/σmax < 1e-3) at **22 %** of cap exits and **21 %** of damping blow-ups, against 3.3 % of normal exits. Not the cause of patience exits |
| (d) Drags reuse the commit solver with bolt-ons, not a continuation solve | **Holds** (structure and cost); it is the only hypothesis that reaches #362 | Four retry ladders, two whole-frame retries and three budgets; p99 49 solves a frame, max 494. #362's seven seeds fail **identically** with (a) fixed |

Three more causes turned up: (e) the solver's idea of "least change" is not the user's (#350,
#284); (f) the angle equation has a spurious root and a fold of its own (#344); (g) the editor
re-derives frames and floors by hand around the solver (`landed`, `breaks_held_shape`,
`length_slack`, the 1 mm floor in `Yardstick::scale`).

Also found: **main can't commit 551 of 10,561 random sketches placed 1 km from the origin**
(44 at 100 m). Their relations hold. With a local frame on every solve, all of them commit.
Filed as #387.

The proposal is six gated PRs (section 5). Together they delete the near/far hinge, the patience
rule, the cap-as-conflict test, fallback (b), the landing passes, the halving ladders, the fold
and length tries and their budgets, and the speed thresholds. They keep the binding drag rule.
Section 6 lists the nine decisions; the user took all of them, as recommended, on 2026-11-10.

---

## How this was measured

**Code read:** `crates/drift-solver/` (all of it), `drift-edit/src/editor/solve.rs`,
`editor/drag.rs`, `drag_props.rs`, `drag_props/seeds.rs`, `drift-core/src/constraint.rs`, and the
#377 notes in `E:\Drift\handoff\2026-11-09-coord-8\` and `E:\Drift\drag-size-logs\`.

**Probe build:** a scratch worktree of b800322 with switches read from the environment. With
every switch off it reproduces main exactly: the same 6 of 7 ignored pins fail, and everything
else passes. The switches:

| Variant | What it does |
|---|---|
| `main` | Today's code. |
| `lf` | A drag part's solve runs in a local frame: its points translated to the part's centre (rounded to a power of two of its size, so the shift is exact). Every stop is measured against 1e-12 × A/4 (A is the part's size), with no hinge. |
| `t3` | No frame. Every drag stop is measured against 1e-9 × A/4, with no hinge. |
| `lft3` | `lf` with the bar at 1e-9 × A/4. |
| `lfa` | `lft3` for **every** solve, the commit's too. |
| `nh` | No frame, no hinge, 1e-12 × A/4 everywhere. Shows what the hinge protects. |

It also counts how each component's Newton solve ends (settled, `SETTLE_PATIENCE`, the
100-iteration cap, or damping λ blowing past 1e12), and the conditioning of the rows at the exit
(σmin/σmax of the row-scaled Jacobian, by Jacobi eigenvalues, for components of up to 80 rows).

**Similarity census (new):** the same random drags as #365/#377's census (same generator and
seed, so case numbers match). Each drag is run at the origin, then at 0.1×, 3.7× and 10×, and
1 m, 100 m and 1 km out. Each result is compared with the origin's, mapped through the
transform. A drag **fails** if it refuses a different number of frames, or ends more than 1e-6 of
the drag's size away: the bar of the `drag_is_invariant_under_similarity` property. Census 1 is
6,000 cases (5,288 build), census 2 is 12,000 (10,561 build), so 74,000 drags per variant.

**Targeted runs:** the 7 ignored pins and all `seeds::`/`held_shapes::` tests under each
variant; `tangent_and_parallel` with its offset nudged by −6…+6 ulps (x) and −3, 0, +3 ulps (y),
39 drags at two sizes; census case 14647 frame by frame at 1 km and 1.41 km (#366);
`small_drag_is_continuous_heavy` seeds 23, 32, 34, 36, 44, 69, 86 (#362); #295's six saved `cc`
seeds; #349's held chain at 10 to 2,000 bars with the cap at 100, 1,000 and 10,000; the heavy
matrix (14 properties × seeds 1–20) on `main` and `lfa`.

Commands, scripts and raw logs: `E:\Drift\solver-audit-logs\` (`census.sh`, `census2.sh`,
`heavy.sh`, `tools\analyse.py`, `targeted\`). Probe source: `E:\Drift\worktrees\solver-audit-probe`
(uncommitted diff: `drift-solver/src/audit.rs`, hooks in `lib.rs`, `drag_props/zz_audit.rs`).

---

## 1. Every open `solver-audit` issue, mapped to a root cause

| Issue | What it is | Root cause | Evidence | Closed by |
|---|---|---|---|---|
| **#280** | Same drag, different result at another size. 7 pins: 2 left from before #377, 4 that #377 added, and `tangent_and_parallel`, re-ignored by #381 | (a) for 4 pins, (b) for 2, (c) for 1 | Per pin in section 2 | PR 2 + PR 3 |
| **#366** | 1 km out, a hanging line's length try lands across a fold for one frame | (a): main's coarse far-out finish stretches the line on the catch-up frame; then (d): the length try, a bolt-on, jumps the fold | Case 14647 at 1 km: main refuses frame 4, then jumps 3.05 mm and **7.59 mm**, and leaves the 30 mm line at 29.999999. `lfa`: no refusal, 1.523 mm every frame (the pointer's speed), lengths held, identical at 1 km and 1.41 km | PR 2 |
| **#362** | Seven continuity seeds still jump on small drags | (d), with (c): a single solve per frame picks a branch, and nothing tracks the path | All 7 fail under `lfa` with the **same point and the same move to 12 digits** as main (jumps of 15 to 524 mm for drags of 0.1 to 0.5 mm). Not size, not tolerance | PR 5 |
| **#344** | A big step flips a Parallel, Perpendicular or Angle pair to the other way round | (f): the residual sin(θ − φ) is zero at φ and at φ + π, and its gradient vanishes at φ ± 90°, a fold built into the equation (`equations.rs` ~485) | Code: the equation has two roots by construction; #331 dodges it only when a relation holds a dragged point | PR 4 |
| **#349** | Perpendicular refused as a conflict on a held chain | (b): "didn't converge in 100 iterations" is the conflict test | All 12 cases tried (10 to 2,000 bars, middle and end) are refused at cap 100; **all 12 are added at cap 10,000**, in 341 to 5,070 iterations. Not a conflict | PR 3 |
| **#350** | Parallel stretches the line it is made parallel to | (e): a commit's solve has no shape preference at all (`plain()` in `solve.rs` strips them), so the minimum-norm step moves whatever coordinate is cheapest, a length included | Code; same family as #284 | PR 6 (needs your call, decision 5) |
| **#351** | A failed active-element try costs ~1 s on a big drawing | (b): a try that can't converge only stops at the cap | Same mechanism as #349 | PR 3 |
| **#352** | Merged equal points start from their mean, which rounds | Numerics hygiene, independent of the hypotheses | Code: `system.rs` ~1209 divides a sum by the count | PR 1 |
| **#343** | Drag property tests can't see structural rows | Test gap: `unmet` skips rows no relation owns | Code: `lib.rs` ~589 `let Some(id) = row.source else { continue }` | PR 0 |
| **#347** | Active element through relations lowered onto wall faces | Not the solver: the editor's relation-adding policy, and blocked by #313 | — | Out of scope; stays blocked |
| **#295** | "Flaky" `face_dimensions_hold_through_drags` | **Not flaky and not the solver.** The 4 "join flipped" seeds fail on every run, on main and on `lfa` alike: walls' join bookkeeping (`drag_joins`, tees re-read at step 5 of a 1.3 to 1.5 m drag). The 2 "face relation" seeds no longer fail on main | Replays of the six saved `cc` lines | Walls fix, outside this plan (decision 7; relabelled `drawing` + `batch-2`, the repo has no walls label) |
| **#187** | Record what each heuristic protects; keep a corpus | Meta | Section 3 is the record; PR 0 is the corpus | PR 0 |

## 2. Every ignored or pinned drag test, mapped to a root cause

All 7 are in `drag_props/seeds.rs`. Each was run under every variant. `tangent_and_parallel`
passes on Windows and macOS and fails on Linux, so it shows as passing below.

| Pin | Recorded cause | Root cause | main | `lf` | `lft3` | `lfa` |
|---|---|---|---|---|---|---|
| `patience_and_landing` | Band pair: one copy near, one far | (a): the hinge itself | fail | pass | pass | pass |
| `crushed_at_one_size` | Band pair | (a): the hinge | fail | pass | pass | pass |
| `flung_centre` | Round-off at a degenerate fold, and a band pair | (a), then (c) | fail | pass | pass | pass |
| `at_the_hand_over` | Branch choice at 17.1 part-sizes, 19 mm (12338) | (a): the hinge's edge | fail | pass | pass | pass |
| `patience_cut_at_one_size` | Patience cut off today's bar at one size | (b): `SETTLE_PATIENCE` and fallback (b) | fail | fail | pass | pass |
| `crawl_at_the_cap` | A fold's crawl cut by the cap, 1 to 7 µm | (b) + (c): a crawl at a fold, cut by the cap | fail | fail | fail | fail |
| `tangent_and_parallel` | Refused at one size only, Linux by chance | (c) + (a): a fold amplifies an ulp of libm into a stop decision | pass* | pass | pass | pass |

\* On Windows. The chaos probe below is what decides it.

**`tangent_and_parallel`, the H-5 case, decided.** Its offset nudged by −6…+6 ulps in x and −3,
0, +3 in y (39 drags, each at 1× and ¼× turned 232°): on main **21 of 39** refuse a different
number of frames at the two sizes. Under `lfa`, **0 of 39** do. That is exactly the size of
perturbation glibc's sin, cos and atan2 make, so the Linux failure is (a) + (c) and not a
platform bug. H-5's other suspect, a hash map's random order, did not show: four separate
processes gave bit-identical results for all 78 drags (each process seeds its hash maps afresh).

**What the variants cost.** Each variant also fails some tests that pass on main. One of those
is a test of #377's mechanism (`a_landing_pass_reads_the_size_its_solve_began_with`: "a far solve
is never judged near"), which a redesign deletes by design. The rest are real and say where a
redesign has to be careful:
- `lf` fails `at_a_fold` (4e-5) and #284's `h_untouched_line_kept_at_any_size_and_place` at 1 km.
  Traced as far as the length try: at 1 km its strict solve doesn't go, where main's does. With
  the bar at 1e-9 (`lft3`) the same case passes. The reading: geometry is stored in absolute f64
  between solves (an ulp at 1 km is 1.2e-10 mm, 6e-12 of a 20 mm part), so no frame can hold a
  1e-12 × A/4 bar from one solve to the next. This is the correction to hypothesis (a): **a frame
  alone can't beat absolute storage; the bar has to sit above it.**
- `lft3` fails `h_untouched_line_kept_with_an_angle_relation...` at 100 m, `size_free_near_the_origin_at_other_similarities`
  (`at_a_fold` at 2×, 113°: 0.2 mm), and `held_shapes::an_old_conflict_beside_a_small_drag_changes_nothing`
  (29 of 32 frames where 32 are expected). All three involve the fold try, the length try or
  patience: the bolt-ons PRs 3 and 5 replace. They are the probe's open risks, not results.
- `lfa` fails those three and one more: `held_shapes::a_drag_that_cant_land_doesnt_grow_a_radius_it_doesnt_move`
  at 0.1× and 1 km. The drag is still refused and no radius grows (the binding rule holds); what
  fails is the bit-exact "nothing moved": the commit after the refused drag re-solves and moves
  points by about 1e-11 mm. That part is 2 mm across and 500,000 part-sizes out, which is where
  a 1e-9 bar meets storage rounding. So the floor doesn't vanish; it moves from 17.6 part-sizes
  to about a million. PR 2 keeps it, as a floor of a few ulps of the coordinates, and a solve that
  starts with its rows met writes nothing.

---

## 3. What the hypotheses look like in numbers

### (a) Absolute coordinates: holds, with a correction

Similarity census 2 (10,561 drags at each place; % failing):

| | 0.1× | 3.7× | 10× | 1 m | 100 m | 1 km | sketches that can't be committed 1 km out |
|---|---|---|---|---|---|---|---|
| `main` | 0.05 | 0.03 | 0.04 | 0.07 | **1.06** | **4.01** | **551** (and 44 at 100 m) |
| `lft3` (frame + bar, drag solves only) | 0.04 | 0.04 | 0.04 | 0.06 | 0.50 | 2.31 | 551 |
| `lfa` (frame + bar, every solve) | 0.04 | 0.04 | 0.04 | 0.06 | **0.08** | **0.30** | **0** |
| `lfall` (frame, 1e-12 bar, every solve) | 0.06 | 0.03 | 0.04 | 0.04 | 0.19 | 3.31 | 0 |

Census 1 (5,288 drags) adds: `lf` 0.80 % at 100 m and **5.67 %** at 1 km (worse than main);
`t3` (bar without frame) 1.10 % and 4.17 % (no better); `nh` (no hinge) **27 %** and **68 %**.

What this says:
- **Near the origin #377 already works.** Main fails about 1 drag in 2,500 at other sizes, and
  no variant improves on it. What is left there is (b) and (c).
- **Far out the frame and the bar have to come together.** The frame alone (`lf`, `lfall`)
  makes 1 km worse; the bar alone (`t3`) does nothing. Together on every solve (`lfa`) they cut
  1 km failures 13-fold, and damping blow-ups at 1 km from **12.8 %** of all component exits to
  **0.8 %** (origin: 0.5 %).
- **Why the hinge exists.** Without it (`nh`) a 1e-12 bar is unreachable far out: 68 % fail at
  1 km. Today's bar is 1e-12 of the coordinates, which is only about 4,500 ε: it sits on the
  round-off floor even at the origin. #377's 17.6 part-sizes is where 1e-12 × A/4 meets 64 ε of
  the coordinates. With a bar of 1e-9 × A/4 in a local frame, storage rounding only reaches the
  bar about a million part-sizes out (about 10 km for a 10 mm part). Past that a floor is still
  needed (section 2, `lfa`'s one extra failure), but no realistic drawing reaches it.
- **The 551 sketches.** Main refuses to commit 5.2 % of random sketches 1 km out, though their
  relations hold. Derived, not traced case by case: angle rows are held to a fixed 1e-12 (a
  sine), but a 10 mm line whose ends are stored to 1.2e-10 mm has its direction known only to
  about 1e-11, so the rows can't be met, and the commit fails as a conflict. `lfa` commits all of
  them. Filed as #387 (decision 8).

### (b) Heuristic stopping: holds, and it is the main cause

How component solves end in drags (census 2, main, 2.3 million solves):

| Where | settled | `SETTLE_PATIENCE` | 100-iteration cap | λ blow-up |
|---|---|---|---|---|
| at the origin | 94.84 % | 3.26 % | 1.36 % | 0.54 % |
| 1 km out | 84.56 % | 1.56 % | 1.06 % | **12.82 %** |
| `lfa`, 1 km out | 95.85 % | 2.13 % | 1.22 % | 0.79 % |

- Of the 532 drags in census 2 that fail similarity, **97.4 %** had at least one patience, cap
  or blow-up exit (in the drag or in its origin twin). Of the 62,239 that pass, 4.3 % did.
  Census 1: 99.6 % against 4.2 %.
- Under `lfa`, 70.7 % of the 58 drags still failing had such an exit: what is left is (b).
- #349 is (b) in its purest form: the "conflict" is the iteration cap (section 1).
- Patience is not a safety net in practice: it ends 3 % of all drag solves. It ends the crawl of
  a kept shape (radius, length, direction) that the minimum-change step closes only a share of
  per iteration. Those exits are not at singular rows: σmin/σmax < 1e-3 at 3.5 % of patience exits,
  the same as at normal exits (3.3 %). The crawl comes from how the preferences are solved, not
  from the relations.

### (c) No fold detection: holds in part

Conditioning of the rows at the exit (census 2, main):

| Exit | σmin/σmax < 1e-6 | < 1e-3 |
|---|---|---|
| settled | 2.9 % | 3.3 % |
| patience | 2.0 % | 3.5 % |
| cap | 1.7 % | **22.2 %** |
| λ blow-up | 21.3 % | **21.4 %** |

- Cap exits and blow-ups sit at near-singular rows 6 to 7 times as often as normal exits: there
  Gauss–Newton only converges linearly, the cap cuts it, and round-off decides where.
- Drags that fail similarity had a near-singular solve (σ < 1e-6) 3.3 times as often as those
  that pass (13.9 % against 4.2 %): real, but a minority.
- The `tangent_and_parallel` probe shows the mechanism: a fold turns ulps of libm into a
  different stop. Moving the stop off the round-off floor (frame + bar) removed it in all 39
  nudges; a fold test is still needed so a drag stops at a fold rather than lets round-off pick
  a branch (`crawl_at_the_cap`, #344).
- Tried and inconclusive: an unweighted null-space share of the kept shapes' gradients does not
  separate patience exits from normal ones. The crawl is in the mobility-weighted metric (dragged
  and carried unknowns are 1e-6 and 1e-4 as mobile), which that measure ignores.

### (d) Drags reuse the commit solver with bolt-ons: holds

One drag frame today (`editor/drag.rs`):

```
drag
└ per part: drag_from_start          one solve; if it wandered or failed: substeps of
  │                                  max(size/4, offset/16)
  └ drag_towards                     halve, up to 4 deep
    └ drag_frame
      ├ drag_frame_today
      │ ├ drag_frame_steps
      │ │ ├ drag_in_steps            halve, up to 6 deep, on an unfinished solve, a turned
      │ │ │                          arc or tangency, or a flipped line (Orientation)
      │ │ ├ carry turned curves      loop: push curves that even the smallest step turns
      │ │ └ carry_holding            carry the curves the dragged points are held on
      │ └ fold try                   the same again with SolveOptions::fold, own budget
      └ keep_lengths                 the same again for a hanging line, own 32-solve budget
└ breaks_held_shape                  refuse if a held radius moved > 10× the pointer, or a
                                     held line > 100×
```

and in the solver, per solve: `land` (pin the dragged, then the carried, and solve again).

- **Cost:** component solves per frame (census 2, drags that solve): median 2.0, p90 5.7,
  **p99 49.3, max 494**.
- **What only this reaches:** #362's seven seeds are untouched by (a) to 12 digits. They are
  branch jumps: a frame solves from the drag's start to the new target, and whatever solution
  the solve lands on is taken unless an orientation check, a speed threshold or a budget happens
  to catch it.
- **What is already right and stays:** each frame solves from the drag's start (plan 2b of
  `solver-drag-regions-plan.md`), so out and back returns; free geometry skips the solver; parts
  are solved apart.

### Other causes the evidence pointed to

- **(e) "Least change" isn't the user's least change (#350, #284).** A commit solves with no
  shape preference (`plain()`), so the weighted minimum-norm step moves the cheapest coordinate,
  which can be a named line's length. Drags have a keep-shape preference, commits don't, and the
  two rank things differently.
- **(f) An equation with a spurious root (#344).** `Equation::Angle`'s residual sin(θ − φ) has
  roots at φ and φ + π and a zero gradient at φ ± 90°. Parallel, Perpendicular and Angle all use
  it. The drag works around it with orientation checks and carries.
- **(g) Frames and floors re-derived by hand outside the solver.** `Yardstick::scale` floors at
  1 mm absolute (`c.max(1.0)`). The editor's `landed()` uses `tolerance::same(|c|)`,
  `breaks_held_shape` uses 1e-9 × offset or 64 ε × |c|, `length_slack` re-derives the quarter
  size and the 64 ε floor, and `LENGTH_TRY_TOLERANCE` copies the solver's tolerance. Each one is
  a place the same size question gets answered again, a little differently.
- **Not a cause:** hash-map order (see section 2), and the solver's own constants, which are
  otherwise scale-free (`mu = 1e-10 × max_diag`, the λ schedule, `NEGLIGIBLE_ROW`, the
  mobilities).

---

## 4. The heuristics layered on so far, and their fate

"Deleted" means the redesign has no such rule; "replaced" means a principled mechanism does its
job; "kept" means it stays as a plain design parameter.

| Heuristic | Where | Added | What it protects | Fate |
|---|---|---|---|---|
| Tolerance 1e-12 × max(1, \|c\|) | `Tolerances`, `Yardstick::scale` | early | Convergence | **Replaced** (PR 2): one bar, a fixed share of the part's size, in the part's frame |
| `Yardstick`, `NEAR_FLOOR` = 64 ε, the 17.6 part-size hinge, `drag_part`, `drag_size`, `drag_near` | `lib.rs` 224–297 | #377 | Size-free stops near the origin, today's far out | **Deleted** (PR 2) |
| Today/near split in `newton` (`cut_short`, `capped_loose`, `solve_judged(today)`) | `lib.rs` | #377 | Bound the work of a refused frame on a big part | **Deleted** (PR 2) |
| Fallback (b): a near patience cut counts only if today's bar holds | `newton` | #377 | Same | **Kept by PR 2** for every drag solve, held to `PATIENCE_SHARE` (1e-3) of the bar, with no floor (floored, it gave way some 140 part-sizes out). **Deleted** (PR 3), with `PATIENCE_SHARE` |
| λ /= 3 once rows hold (near drags only) | `newton` | #377 | Round-off steering the damping | **Extended by PR 2** to every drag solve (in a frame, every drag solve is what "near" was). **Deleted** (PR 3): once the rows hold, the relations' damping no longer runs |
| Angle rows' floor of ulps over their shorter line (`row_tolerance`) | `lib.rs` | PR 2 | Storage rounding of a line's direction far out (#387) | **Settled by PR 3**: an angle between lines holds to the length bar over its shorter line (its end within the bar of where the angle puts it), so it is held as a length is, with the length bar's floor, a million part-sizes out; no edge between. The unitless bar stays for an ellipse's tangency only |
| A fold solve isn't cut by patience before its rows hold to the patience share and every kept shape is met | `newton` | PR 2 | A fold solve finishes only where it meets everything; at the part's bar its rows hold sooner, and patience cut it mid-crawl (macOS CI) | **Deleted** with patience (PR 3). With `fold`, the shapes settle only where a step could gain them less than the bar squared |
| `SETTLE_PATIENCE` = 10 | `newton` | #94 | Bounds a kept shape's crawl | **Replaced** (PR 3): a stationarity test of the preferences on the relations' manifold (projected gradient in the weighted metric below the bar, or the step's most gain below the bar squared, or the region below the bar) |
| `MAX_ITERATIONS` = 100 as the conflict test | `newton` | early | A failing part stops | **Replaced** (PR 3): a least-squares stationary point (the region's best step brings the rows no more than their finest bar closer); the cap stays as a counted safety net. It still ends 0.63 % of the census's drag solves: drags that can't land, their dragged points giving way at their weight (see PR 3's notes) |
| λ > 1e12 × max_diag break; 30-try factor loop | `newton` | early | A step that can't be taken | **Replaced** (PR 3): a factor that fails is a refused step (the damping rises); the stationarity test ends a stalled solve |
| `still`/`at_rest`/`last_moved` ("a drag's solve isn't over while a step moves") | `newton` | #104 | Stops that depend on size | **Replaced** (PR 3): the frame and bar (PR 2) make the stop size-free, and the preferences' stage settles by stationarity |
| `FELT_PASSES` = 6, `FELT_ROUND_OFF`, `SECOND_ORDER` = 0.25, the soft step's 4 halvings | `newton` | #94, #103 | Keeping the preferences' step in the relations' null space | **Replaced** (PR 3): a two-stage solve, the relations first, then the preferences by projected conjugate gradients in their null space (projection `μ·A⁻¹`), re-projected onto them by Newton steps |
| `WANDERING_GAIN` = 0.25, `wandered`, `WANDERED` = 1 | `newton`, `drag.rs` | #103 | A solve that wanders round a fold triggers small steps | **Replaced** (PR 5): step-size control in the continuation |
| `KEEP_SHAPE` = 100, `UNFOCUSED_MOBILITY`, `DRAGGED/CARRIED_MOBILITY` | `newton`, `system.rs` | #94, #333 | Ranks of what moves | **Kept** as weights; dragged targets become exact constraints in a drag (PR 5), so the dragged weight goes |
| `land()` landing passes | `lib.rs` 521 | #94 era | The heavy, not pinned, dragged give a little | **Deleted** (PR 5): targets are exact |
| Step limit `across` (`drawing_across`, circles counted) | `newton` | #103, #280 | One iteration can't fling geometry | **Kept** as the trust region's upper bound |
| Fold steps: `SolveOptions::fold`, `FOLD_MET` = 1e-9, `fold_scale`, `fold_step` | `newton` | #282 | Finish a solve stalled at a fold | **Replaced** (PR 3 + 4). PR 3 deletes `fold_step` (the preferences' Gauss–Newton step meets a kept shape at a fold); `fold_scale`, `FOLD_MET` and the fold solve's strict finish stay for PR 4/5 |
| `drag_from_start` substeps (size/4, ≤ 16), `drag_towards` (≤ 4 halvings), `drag_in_steps` (`MAX_SPLITS` = 6) | `drag.rs` | #103, c99881c | Go round the shape, not across a fold | **Replaced** (PR 5). PR 2: its size counts each place once (points at one place counted twice moved its middle) |
| `DRAG_SOLVES` = 200, `UNFINISHED_WORK` = 6,400 | `drag.rs` | 1c46e52, #281 | Bound a frame's cost | **Replaced** (PR 5): one budget on the continuation's steps |
| Fold try (own budgets) | `drag_frame_today` | #282 | Retry with fold steps | **Deleted** (PR 5) |
| Length try: `LENGTH_TRY_SOLVES` = 32 (shared), `LENGTH_TRY_STRETCH` = 1e-6, `LENGTH_TRY_TOLERANCE`, `length_slack` | `keep_lengths` | #284, #365, #377 | A hanging line keeps its length | **Deleted** (PR 5): the continuation tracks the branch where the length is kept |
| `SHAPE_HOLD` = 10, `LINE_HOLD` = 100, `COLLAPSED_LINE` = 1e-6 | `breaks_held_shape` | #278 | The binding drag rule | **Replaced** (PR 5): held radii are held, and the continuation stops at the last valid shape (decision 4) |
| `Orientation` (arcs inside out, tangency sides, `SWEEP_JUMP`, `COLLAPSED_RADIUS`), flipped lines, `NO_WAY_ROUND` | `lib.rs`, `drag.rs` | c99881c, #280 | A step mustn't land on another branch | **Kept** as the continuation's branch test, run per predictor step instead of per halving |
| `touching` (tangencies frozen at drag start), `TOUCHING_END` | `system.rs` | #97 | Tangent side kept | **Kept**, and extended to Parallel/Perpendicular/Angle (PR 4) |
| Carry turned curves, `carry_holding` | `drag.rs` | #283, #282 | Push what can't give way | **Kept** as behaviour, re-expressed as which parameters the continuation moves |
| `stalled` second solve in `solve_settled` | `solve.rs` | #342 | A commit that ran out of iterations | **Deleted** (PR 3), with `solve_settled` and `plain`: a solve stops at its stop test, not short of it |
| `NEGLIGIBLE_ROW` = 1e-9, `row_scales` | `newton` | #94, #277 | Round-off rows don't steer the step | **Kept** |

---

## 5. The redesign, as a PR sequence

Each PR is `gated` (solver/drag). Each is judged against the same bar. Sized by scope and risk,
not time.

**The acceptance bar (every PR):**
1. **Heavy matrix**, 14 properties × seeds 1–20, release, `--exact`: no ok → FAIL that isn't
   traced and accepted. Main today: see [Heavy matrix](#heavy-matrix-today).
2. **Similarity census** (PR 0 adds it): failures at 0.1×, 3.7×, 10×, 1 m, 100 m and 1 km never
   worse than the PR's base at any place, and the PR's own target met.
3. **#377's census** (random 20,000 at 0, 1, 3, 10, 30 m, 100 m, 1 km; seeded 2,000 at those and
   100 km): every new stop and every went → stop traced; "stop, as now" holds.
4. **Pins:** each PR names the pins it un-ignores; none it doesn't name changes.
5. **Perf baselines** (`cargo test -- --ignored` perf, per LEARNINGS): `perf_big_drag`,
   `perf_big`, the tiled sketch at 200 and 1,000 copies, the refused 2,000 mm part.
6. **Linux:** the heavy matrix and the pins also run on ubuntu, since that is where libm differs.
7. **The binding drag rule:** a drag that kept radii can't follow stops at the last valid shape;
   radii are never grown. Checked by `held_shapes::` and the census's stop classes.

PR 0 puts the bar in `scripts/solver-bar/` (its README runs all seven). **Known gaps, from PR
0's review, to close before PR 3 or PR 4 is judged:**
- The heavy matrix and the censuses draw from one generator (`plan()`). It never draws Angle,
  Radius, Diameter, FixRadius, Symmetric, Collinear, Concentric, Coradial, Smooth, Corner,
  EqualDistance, H/V distances, PointLineDistance, circles or ellipses. PR 4 re-lowers Angle,
  and a radius dimension is what a kept radius meets most. A second generator, with its own
  seed so today's case numbers stay valid, adds at least Angle, Radius/Diameter, Symmetric and
  Collinear.
- No check sees an arc flip. A drag that carries an arc's end past its start turns a short arc
  into a near-full circle with every row still met. The second generator's properties add an
  arc-sweep continuity check.
- The similarity census translates and scales but never turns. Rotation, where libm's sin and
  cos come in, is covered only by the heavy similarity property.
- Item 7 in the census is a flag to check, not a verdict. A drag that goes while an arc's
  radius changes may be the user dragging that arc's own end. The census lists each one, and
  each must be traced.

### PR 0: the bar as code (tests only)
- Commit the similarity census as an ignored test, the exit-reason counters as test-only
  instrumentation, and `analyse.py` as a script.
- Make the drag properties check structural rows (an arc's end on its circle, midpoints).
- Record main's numbers for all seven parts of the bar in the PR.
- **Closes** #343; the corpus half of #187. **Deletes** nothing. **Risk:** low; it can only find
  more failures (#343's probe found an arc 1.18 mm off its circle that passed).

### PR 1: numerics hygiene
- #352: merged equal members start from that value.
- **Closes** #352. **Risk:** low; ulp-level changes, checked bit for bit against main on the census.

### PR 2: one frame, one bar
- Every solve, the commit's and analysis's too, runs each component in its own frame:
  translated to its centre, rounded to a power of two of its size. One bar, a fixed share of the
  component's size (recommendation: 1e-9 × A/4, decision 2), for every stop: rows, step size,
  kept shapes, `unmet`, `meets_within`. Angles keep a unitless bar, now reachable because the
  frame's coordinates are small. A floor of a few ulps of the stored coordinates stays (reached
  only about a million part-sizes out), and a solve whose rows already hold writes nothing.
- The editor's `landed`, `breaks_held_shape`'s landing test and `length_slack` read the same bar
  from the solver instead of re-deriving it.
- **Closes:** #366; #280's `patience_and_landing`, `crushed_at_one_size`, `flung_centre`,
  `at_the_hand_over` and `tangent_and_parallel` (`lfa` passes all five); the 1 km commit refusals.
- **Deletes:** `Yardstick`, `NEAR_FLOOR`, the hinge, `drag_part`/`drag_size`/`drag_near`,
  `solve_judged(today)`, the `c.max(1.0)` floor, `LENGTH_TRY_TOLERANCE` and `length_slack`'s floor
  logic, and the 64 ε floors in the editor.
- **Target:** similarity failures ≤ 0.1 % at 100 m and ≤ 0.3 % at 1 km (the probe: 0.08 % and
  0.30 %); no sketch refused at commit for being far out.
- **Risk:** medium-high. It changes when every solve stops, so far-out results change: that is
  the point, and it overturns P2 (decision 3). The probe's open risks are the four tests listed
  in section 2 and heavy similarity seeds 2, 14 and 18, mostly on the fold and length tries that
  PRs 3 and 5 replace. PR 2 lands before those, so it has to trace each of them itself.

### PR 3: a principled stop
- **Two stages:** first meet the relations (Gauss–Newton or Levenberg–Marquardt with a trust
  region, weighted by mobility), then, among their solutions, the preferences, by steps
  projected exactly into the relations' null space and re-projected onto them.
- **Stops:** the rows hold at the bar, and the preferences' projected gradient (in the weighted
  metric) is below the bar, or the predicted decrease is below the bar squared.
- **Conflict:** a least-squares stationary point (JᵀF ≈ 0) with the rows above the bar, judged
  by rank, not by running out of iterations. The cap stays as a safety net, counted, and the
  census must show it never firing.
- **Closes:** #349, #351; pins `patience_cut_at_one_size` and `crawl_at_the_cap`.
- **Deletes:** `SETTLE_PATIENCE`, fallback (b), the λ/3 rule, `FELT_PASSES`, `FELT_ROUND_OFF`,
  `SECOND_ORDER`, the soft step's halvings, `still`/`at_rest`, the λ blow-up break, the 30-try
  factor loop, the cap as conflict test, `solve_settled`'s second solve.
- **Target:** under 0.5 % of component solves end other than by the stop test (today 5.2 % at
  the origin); every census drag that fails similarity is traced.
- **Risk:** high. It is the solver's core loop, and how fast a crawl at a fold closes changes
  every drag that crawls today (3 % of solves). Perf is the second risk: #349's chain needs
  thousands of iterations today, so the step itself must get better (trust region, not tiny-μ
  minimum norm), not only the stop.

**PR 3's notes (as built).**
- **Stage 1** is Levenberg–Marquardt with three changes from main's step. The damping is a
  distance (the identity), not the mobility weighting: weighted, a step that swings a
  relation's focus far and the rest a hair is as far off its linearisation as any, and #349's
  chain crawled. The dragged and carried keep their weight in it: damped as a distance, a drag
  that can't land lets them give way, and `held_shapes::`' refused drags go. A step longer than
  the drawing is refused, not cut along its own direction (cut steps walked census case 2432
  437 mm across a 20 mm part, frame after frame). And each step gets a second-order correction
  (Newton steps to where its linearisation promised the rows), no larger than the step.
- **The kept shapes in stage 1's metric** weigh as much as their points (`KEEP_SHAPE` times the
  least of their points' weights), so under a relation's focus a line of the rest is kept against
  moving the rest, not against moving the focus.
- **Stage 2** is Steihaug's projected conjugate gradients in the mobility metric, the projection
  `μ·A⁻¹` with a refined solve (unrefined, round-off of a millionth of the step lands in the
  directions nothing feels: case 2911's origin copy 3.4e-3 off its twins). A trial is kept at a
  gain of a quarter, not any gain (any gain let round-off slide geometry along a direction the
  shapes hardly feel). It also settles where a step would bring the shapes' offsets, as one
  length, no more than a bar nearer: "below the bar squared" alone never ends at a shape the
  relations hold off (a cycle of 1e-12 gains on a misfit of 0.06).
- **Held radii.** A drag solve that leaves off a kept radius its free geometry can change (more
  than √μ of its gradient in the rows' null space) is unfinished: the drag stops at the last
  shape that kept it (e13 stops at 3.73 of its 8.15, where its circles stop meeting). A radius
  the drag itself dictates follows, as `held_shapes::` has it (a drag that lands may change a
  radius it doesn't move).
- **The bar where the solve stands.** Where the rows hold, they hold to the bar of the part as the
  solve leaves it too: a solve that made the part smaller said converged and an analysis of
  what it wrote didn't (`perturbed_satisfiable_sketches_converge`: base fails every 5,000-case
  run; PR 3 none in 15,000). And the floor of ulps judges stored values only: once a solve has
  moved them, they are its own and reach the unfloored bar (held to the floor, solves 100 km
  out stopped a hair inside it and rounded over it when stored).
- **The cap still fires** on 0.63 % of the census's drag solves (main: patience, cap and damping
  3.4 %). These are drags that can't land: only the dragged points can bring the rows home, and
  in the region they give way at their weight, about a hundredth a step. On main the same drags
  end at the cap too, and "unfinished" is what refuses them: the binding rule rests on it. A
  sharper "can't land" stop (the free geometry's own step leaves the rows off) took the cap to
  0.28 % but moved where drags at a fold stop between sizes; it was left out. PR 5's exact
  targets make these a conflict.
- **Held arcs never grow** in a drag, landed or not (the binding rule as the user states it):
  `breaks_held_shape` stops a frame that grows an arc none of whose points the drag moves by
  more than the bar or `FOLD_MET` of it. Shrinking still goes, as `held_shapes::` has it. This
  stops f3's move (#281), which PR 3's first build let through, and in #377's random census some
  160 drags a place that went before (stops 274 → 419).
- **A free move is never a stop.** A move of curves in a part nothing pins that the solve can't
  make (it lands where the least change leads, and from there the kept shapes' stage can stall
  at a fold short of moving the whole part) moves the whole part along instead, which lands every
  dragged point, meets every relation and keeps every shape exactly
  (`free_move_is_accepted_heavy` 6 and 17). A second-order correction of the kept shapes inside
  the first stage was tried first and cost #349's chain.
- **A drag's steps are bounded together** by the drawing's size at the solve's start (not each
  alone), in both stages: `flung_centre` threw a point 6 to 23 m out of a 20 mm sketch in 21 of
  39 one-ulp nudges (and failed on Linux and macOS). Commits are left unbounded, a dimension typed
  ten times longer moving geometry rightly far.

### PR 4: directed relations and fold awareness
- Parallel, Perpendicular and Angle are lowered with their way round, frozen where they were
  met (the drag start in a drag, the current geometry in a commit), to a residual with one root
  and no fold short of 180° (for example, the angle error itself from a directed cross/dot pair,
  with the branch fixed).
- The solver reports σmin of the rows at its exit (a cheap estimate from the factor), so a caller
  can tell "at a fold" from "met".
- **Closes:** #344; it is also what PR 5's fold test reads.
- **Deletes:** the drag-side orientation work-arounds for those pairs (#331's carry stays only
  where it is about behaviour).
- **Risk:** medium. A relation added the other way round to its current geometry must still
  flip it once, which needs a test of its own.

### PR 5: a drag is a continuation
- A drag frame follows the path t ∈ [0, 1] from the drag's start offset to the pointer:
  - **Predictor:** the tangent of the solution path (from the Jacobian at the last point).
  - **Corrector:** PR 3's solve, with the dragged parameters as exact constraints, not heavy
    weights.
  - **Step size:** set by the corrector's contraction and by σmin (PR 4). Orientation, flipped
    lines and held radii are checked at every accepted step.
- At a fold, at a bifurcation, or where a held radius would have to change, it stops at the last
  valid t: the binding rule, stated once.
- It still solves from the drag's start each frame (path independence) and part by part.
- **Closes:** #362, and #366 for good if PR 2 hasn't.
- **Deletes:** `land`, the heavy dragged weight, `drag_from_start`'s substep rule,
  `drag_towards`, `drag_in_steps`' halvings, `MAX_SPLITS`, `WANDERED`/`wandered`,
  `DRAG_SOLVES`, `UNFINISHED_WORK`, the fold try and its budgets, `keep_lengths` and every
  `LENGTH_TRY_*`, and `SHAPE_HOLD`/`LINE_HOLD`/`COLLAPSED_LINE` (if decision 4 goes that way).
- **Target:** all 14 heavy properties × 20 seeds pass (the 7 continuity seeds of #362 included),
  no pins left, perf level on `perf_big_drag`.
- **Risk:** highest. It rewrites `drag.rs`, and how a drag behaves at a fold becomes a
  deliberate rule instead of an accident. It needs PR 3 and PR 4 first.

### PR 6: the user's least change (behaviour, needs decision 5)
- Commits get the same keep-shape preference as drags, ranked below the relations and the
  named geometry: lines keep their lengths and directions, arcs their radii, where the
  relations leave room.
- **Closes:** #350, and the commit side of #284.
- **Risk:** medium. It changes what moves when a relation is added; it needs a before/after on
  the census of relation adding (#333's corpus).

**PR 6's notes (as built).**
- **What an edit keeps** (`Editor::kept_shapes`, measured as the geometry stands): every arc its
  radius and every line its direction; a line its length where keeping it moves nothing else
  (it hangs from the rest, as in a drag); what a new relation names takes up its change by
  moving and turning (its lines keep their lengths, not their directions; its arcs take the
  change in their radii, so #75's fillet still grows inside square lines). A size a dimension
  sets is the dimension's, not a preference. Keeping every line's length too was tried: a
  position isn't a preference, so the cheapest way to keep the rest's lengths was to slide the
  whole sketch (#75's outline moved 10 mm), which breaks "a relation moves what it relates".
- **Ranks, as weights in the second stage** (`SolveOptions::length_ranks`, normalised so the
  lightest is 1, so every shape still settles to the bar): a length weighs 1e-3 of a direction
  or radius, a named line's length 1e-3 of that. A line between two held ends keeps its
  direction and stretches along itself rather than turn. The first stage's metric weighs them
  the same way. Weights, not a strict order: where a length must change, the directions and
  radii beside it give way by about a thousandth of the change (micrometres).
- **A direction's offset** is `l₀·sin θ` at the length it was kept at, rising to `2·l₀` turned
  end for end. Main's `l·sin θ` was met by a reversed line (5.2 % of relations added flipped a
  line) and shrank with the line (a line a relation must turn was collapsed to meet it).
- **A commit's preference steps are bounded together** by the drawing's size from where the
  second stage began (the first stage stays unbounded): a line kept square was otherwise
  stretched 40 km to meet a line it nearly paralleled.
- **No relation is newly refused.** Where keeping shapes leaves a relation refused (the
  analysis reading a landing as collapsed or redundant at a special configuration), it is
  solved again exactly as before (`SolveOptions::shapes_as_before`), as #333 falls back for the
  active element.
- **Paths.** Every edit's solve keeps shapes: `commit` (so every tool, MCP call, script and
  Build Geometry edit), relation adding (`add_constraints`, `add_picked_constraints`, typed
  sizes), dimension and variable edits, and the solve on opening a drawing. Drag-like solves
  keep the drag's preferences (PR 5's scope): drag frames, `drag_radius`, `drag_offset`
  (an offset's distance is solved as a drag of its sides), `rotate_geometry`, and the moves
  and nudges that go through `drag`.

**Order and parallelism:** PR 0 first. PR 1 at any time. PR 2 next (the biggest measured win).
Then PR 3. PR 4 can go beside PR 3, since its files are the equations and lowering. PR 5 after
PR 3 and PR 4. PR 6 after PR 3. One solver PR is in review at a time; the others wait.

**Outside this plan:** #295 is walls' join bookkeeping (decision 7); #347 stays blocked by #313.

---

## Heavy matrix today

Heavy matrix, 14 properties × seeds 1–20, release, `--exact` (`heavy.sh`):

| Property | main fails | `lfa` fails |
|---|---|---|
| `drag_is_invariant_under_similarity_heavy` | 3, 4, 6, 19 | 2, 3, 14, 18, 19 |
| the other 13 | none | none |
| **Total** | **276 ok / 4** (the same as #377's PR) | **275 ok / 5** |

- `lfa` fixes similarity seeds 4 and 6, and newly fails 2, 14 and 18. Seeds 2 and 14 refuse a
  different number of frames at the two sizes (seed 2 is the drag that
  `e_result_depends_on_size_guard_for_a_wider_fold_try` already guards: a point on an arc, the
  fold try's territory); seed 18 ends 0.4 mm apart. Not traced further: the probe tests a
  direction, it isn't a candidate PR, and these are cases PR 2's own bar has to settle.
- `small_drag_is_continuous_heavy` passes seeds 1–20 on both; #362's failures are seeds 21–100.
- `face_dimensions_hold_through_drags_heavy` passes seeds 1–20 on both; #295's failures come from
  its own saved seeds (section 1).

---

## 6. Decisions

All nine were taken by the user on 2026-11-10, each as recommended (LOG.md).

1. **Adopt this direction and keep the patch freeze.** No more solver or drag patches outside
   this sequence; `solver-audit` issues stay paused until the PR that closes them.
   *Recommendation:* yes. Every rule added since #94 moved the failures rather than removing them.
   **Decided 2026-11-10: as recommended.**
2. **The tolerance.** One bar, a fixed share of each part's size, in place of 1e-12 of the
   coordinates. *Recommendation:* 1e-9 × A/4: 2.5 picometres on a 10 mm part, 0.25 nm on a 1 m
   one, far below anything drawn or exported. It is what the probe measured. 1e-10 would work out
   to about 100,000 part-sizes; 1e-12 does not work far out at all.
   **Decided 2026-11-10: as recommended.**
3. **"Far out unchanged" (P2 from #377).** PR 2 changes far-out results on purpose.
   *Recommendation:* replace P2 with "the same drag at any size and place, to about a million
   part-sizes from the origin".
   **Decided 2026-11-10: as recommended.**
4. **Held radii in a drag: hard or soft.** Today they are a soft preference, and a drag is
   refused only when it doesn't land and one moves more than 10× as fast as the pointer. In a
   continuation they can be held exactly, and the drag stops where it can't keep them, which is
   the binding rule read literally. That stops some drags that today let a held radius drift
   slowly. *Recommendation:* hard for radii. Hanging lines stay soft (carried at their length
   where possible), with the crushed-line stop kept.
   **Decided 2026-11-10: as recommended.**
5. **#350: commits keep shapes.** *Recommendation:* yes, as PR 6, after PR 3, with a before/after.
   **Decided 2026-11-10: as recommended.**
6. **#377's accepted costs (D3: 12338's 19 mm, 19595, the seeded fold churn; the hinge PQ2).**
   *Recommendation:* treat them as superseded by PR 2. They were costs of the hinge, and the
   hinge goes.
   **Decided 2026-11-10: as recommended.**
7. **#295 out of `solver-audit`.** It is deterministic walls bookkeeping, not a flaky solver
   test. *Recommendation:* relabel it as a walls bug. The walls PR that fixes it adds the four
   saved seeds to the regression file as its proof (adding them before the fix would turn main
   red).
   **Decided 2026-11-10: as recommended.** Done: #295 is relabelled `drawing` + `batch-2` (the repo has no walls label).
8. **File the 1 km commit refusal as an issue.** "A sketch whose relations hold can't be
   committed 1 km out (551 of 10,561 random sketches; 44 at 100 m)". *Recommendation:* file it,
   closed by PR 2.
   **Decided 2026-11-10: as recommended.** Done: filed as #387.
9. **Linux in the bar.** *Recommendation:* run the heavy matrix and the pins on ubuntu in a
   scheduled job (#187's step 4), so platform round-off is seen before a merge, not after.
   **Decided 2026-11-10: as recommended.**
