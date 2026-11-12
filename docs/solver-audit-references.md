# Solver audit addendum: what outside solvers and papers say about PR 4, PR 5 and #400

*2026-11-12, against `main` at b95a141. Read-only: no product code changed, nothing run,
nothing measured. An addendum to [solver-audit.md](solver-audit.md). It overturns none of the
user's logged decisions. It does question one line of PR 5's design, the dragged parameters as
exact constraints (section 3.1), and puts that to the user.*

The audit's parked work is PR 4 (directed relations, #344), PR 5 (a drag is a continuation,
#362) and the stage-2 stall fix (#400). This note sets what other solvers and the literature do
beside each. Every claim about Drift cites a file on main. Every claim about an outside source
says how it was read (section 6). Two adversarial reviewers checked it over three rounds,
one against Drift's code and one against the sources; this version carries their corrections
and both agree with it.

## The short version

1. **PR 4: the audit's own example is the form FreeCAD uses, and it is the right one.** The
   audit suggests "the angle error itself from a directed cross/dot pair". FreeCAD's
   line-to-line angle is exactly that: one root, a slope that never vanishes, and a jump 180°
   from the root. FreeCAD leaves Parallel and Perpendicular with two roots, and no check for a
   flip was found in its solver, which is the gap #344 is. Drift's kept-direction measure is
   **not** a model for PR 4: it has the same jump and also loses its gradient at 90°.
2. **PR 5 leaves one question open: what a drag does when its point can't reach the pointer.**
   Read literally ("the dragged parameters as exact constraints"), PR 5 would refuse drags that
   go today and that the bar tests, such as a lever swung by a point that can't land. FreeCAD
   treats the pointer as a lower priority than the constraints, and task-priority methods have
   the same structure. That keeps those drags, but would turn drags the user has ruled must
   stop into drags that track. Neither reading matches today's behaviour. Section 3.1 sets out
   today's cases and asks the user which are to be kept.
3. **A fold is a stop, and the outside methods give a direct test for one.** Stopping at a
   fold is the binding drag rule and needs no following of the path round it. But how the fold
   is *detected* matters: the path's tangent says where it is, where shrinking steps only crawl
   toward it. That is a design point for PR 5.
4. **For #400 the references offer little.** FreeCAD builds the relations' null space
   outright, so its step does not leak out of the linearised constraints, but it gives up where
   they are rank-deficient, the case #400 is about.
5. **None of the sources read states a rule like Drift's "stop at the last valid shape".**
   The sample is small (section 6), so this is a statement about what was read, not about the
   field.

---

## 1. Where Drift stands on main

- **Angles.** Parallel, Perpendicular and Angle all lower to one equation with φ = 0, 90° or
  the typed angle (`system.rs:633–647`). Its residual is `sin(θ − φ)`, from the cross and dot
  products over the lines' current lengths (`equations.rs:150`, `:497–511`). It is zero at φ
  and at φ + 180°, and its gradient vanishes at φ ± 90°. This is the audit's cause (f).
- **Kept directions.** A line a solve keeps square is measured by `Shape::Direction`
  (`system.rs:151–176`, with a kept length l₀): `l₀·sin θ` while the line faces the kept way,
  and `±l₀·(2 − |sin θ|)` once it faces the other way, the sign following the sine. So it is
  zero only the right way round; its slope is `l₀·|cos θ|`, which is zero at ±90°; and it jumps
  from `+2·l₀` to `−2·l₀` turned end for end. (#400 changes this measure; this is main.)
- **Dragged points are heavy, not pinned.** `DRAGGED_MOBILITY` is 1e-6 (`system.rs:192`): an
  inverse weight, so moving a dragged point costs a million times what moving a free one does.
  Where the relations won't let it reach the target, it gives way. After the solve, `land`
  pins it at the target and solves again, and keeps that only if it fails nothing new
  (`lib.rs:877`). Otherwise the result with the point short of its target stands. A solve's
  `converged` has no term for the target (`lib.rs:3078`), except with the `fold` option.
- **When a frame that doesn't land is kept, and when it is refused.**
  - `drag_in_steps` accepts a step whose solve finished and turned nothing (`drag.rs:1825`).
    It does not ask whether the dragged points landed.
  - A solve that doesn't finish is refused. That includes one that runs to the iteration cap,
    which the audit records for 0.63 % of drag solves: drags that can't land, where "only the
    dragged points can bring the rows home" (audit, PR 3's notes). It also includes one that
    leaves a kept radius off which the free geometry could have kept (`unkept`,
    `lib.rs:3027`).
  - `breaks_held_shape` (`drag.rs:1095`) then refuses, landed or not, a frame that grew an arc
    or circle the drag doesn't move, or that both flung short and crushed a line it doesn't
    move; and, where the frame did not land, one that changed such a radius either way by more
    than ten times the pointer's move, or flung short or crushed such a line. A held radius
    that shrinks slowly goes.
  - The carry, fold and length retries each require landing.
- **Each frame solves from the drag's start**, not from the last frame (`Editor::drag`,
  `drag.rs:377–380`), so out and back returns. The path a frame follows is therefore the
  straight line from the start to the pointer now, not the way the pointer came.
- **The branch test is after the fact.** `Orientation` (`lib.rs:1253`) records arc sweeps,
  tangent sides and distance signs before a step and compares after; a step that turned
  something is halved, up to six deep (`MAX_SPLITS`, `drag.rs:63`).
- **Stage 2** minimises the kept shapes over the relations' null space by projected conjugate
  gradients in the mobility-weighted metric, the dragged and carried left out, with the
  projection `μ·A⁻¹` (`lib.rs:1855–1899`). Main already projects twice for its stop test
  (`lib.rs:2352–2373`).

## 2. PR 4: directed relations

**The plan** (audit, PR 4). Lower Parallel, Perpendicular and Angle with their way round,
frozen where they were met, "to a residual with one root and no fold short of 180° (for
example, the angle error itself from a directed cross/dot pair, with the branch fixed)". The
solver reports σmin of the rows at its exit.

**What FreeCAD's planegcs does** (read in `Constraints.cpp`):

| Relation | Residual | Roots per turn | Slope |
|---|---|---|---|
| Parallel | cross product, scaled by one over the two lengths as they were when the scale was set | 2 (0° and 180°) | Vanishes at ±90° |
| Perpendicular | dot product, scaled the same way | 2 | Vanishes at 0° and 180° |
| Line-to-line angle | `atan2` of line 2's direction turned back by line 1's angle plus the datum: the angle error itself | 1 | 1 everywhere; the value jumps by 360° at 180° from the root |
| Point-to-line distance | signed distance minus or plus the datum | 1 | Side is a flag given when the relation is made |

- No way-round flag is passed for a line-to-line angle, Parallel or Perpendicular. An angle's
  way round comes from each line's stored start and end.
- Tangency and point-to-line distance take a side: the stored one if the caller names it, the
  current geometry's otherwise.
- **There is no limit specific to how far a line turns in a step.** The angle relation has a
  `maxStep` of 10°, but it limits a step in the angle's own value, and only when that value is
  an unknown. The default solver (DogLeg) never calls `maxStep`; it has its trust region on
  the step's size and nothing else.
- No comparison of a result with the one before it was found in the solver.

**What SolveSpace documents** (its pages, not its source). A new angle relation "chooses
arbitrarily which supplementary angle to constrain", and a menu command switches it. "If
multiple solutions exist, then the initial position of the sketch determines which is chosen",
by the method's basin of attraction, "though some cases are handled specially" (not said
which). Its pages do not give its residuals.

**What this means for PR 4.**

- **The audit's example stands, with a precedent.** The angle error from a directed cross and
  dot pair is planegcs's line-to-line angle. It meets the audit's test: one root, no fold
  short of 180°.
- **PR 4 goes further than planegcs, and should.** planegcs applies that form to Angle only,
  leaves Parallel and Perpendicular two-rooted, and no test for a flip was found in it.
- **Do not model PR 4 on `Shape::Direction`.** It looks like in-house precedent for a
  one-rooted direction measure, but its gradient vanishes at ±90° (section 1), which is the
  fold PR 4 exists to remove. As a row scaled to a unit gradient it would blow up there, and
  a row with a residual and no gradient is what stage 1 ends on as a conflict.
- **The jump at 180° is left to deal with.** Any signed one-rooted measure of an angle must
  jump somewhere; that is topology, not a choice of formula. A step that carries a line past
  180° from its frozen way round sees the residual change sign with no root between. Nothing
  was found in planegcs that deals with this. PR 4 should say what Drift does: a test on the accepted step (as
  `Orientation` does for tangent sides today), a bound on how far a directed line may turn in
  one step, or both. A turn bound would be a new rule, so under decision 1 it needs to be part
  of PR 4's or PR 5's reviewed design, not added beside it.
- **Parallel has no way round of its own; a typed Angle does.** Two lines are parallel facing
  either way, so for Parallel and Perpendicular the frozen way round can only come from the
  geometry. A typed Angle is different: 30° and 210° are different statements. planegcs settles
  it by the lines' stored start and end. PR 4 should state which it is for Drift. (How
  `drift-core` defines a typed Angle's direction was not read for this note.)
- **Storing the way round** in the drawing, as planegcs does for a tangent's side, would let a
  drawing saved the wrong way round be told from one meant that way. The audit freezes it from
  the geometry instead, which needs no format change. Nothing read argues for changing that
  now.

## 3. PR 5: a drag is a continuation

**The plan** (audit, PR 5). Follow the path t ∈ [0, 1] from the drag's start to the pointer
with a predictor and a corrector; the corrector is PR 3's solve "with the dragged parameters
as exact constraints, not heavy weights"; step size "set by the corrector's contraction and by
σmin"; and "at a fold, at a bifurcation, or where a held radius would have to change, it stops
at the last valid t".

### 3.1 The open question: a dragged point that can't reach the pointer

With exact targets, the target at t is the start plus t times the offset. A dragged point the
relations leave one freedom (on a line, on a circle, at a fixed distance from a pivot) can
reach that target only where the straight line of targets meets its freedom: along it, if the
offset happens to lie that way, and otherwise at most at an isolated t. So in general no
solution exists just past t = 0, the last valid t is 0, and the frame is refused.

Today is not that simple. What a drag does when its point can't land depends on what holds it
back:

| # | Case | Today on main | Source |
|---|---|---|---|
| 1 | A point at a fixed distance from a fixed pivot, on a line that swings (a lever), dragged off its circle | **Goes.** The point swings round; "the dragged point can't land" | `held_shapes::a_lever_still_goes_at_any_advantage` (`held_shapes.rs:309–365`) |
| 2 | A point on a line whose ends are fixed, pointer off the line | **Goes**, sliding along the line. By reading the code only; no test found. The least certain row | — |
| 3 | A point on a circle or arc with a fixed centre and a free radius, dragged inward | **Goes.** The round's radius follows the point down | `a_point_dragged_across_its_circles_centre_is_refused_only_on_it` (`editor/tests.rs:1310–1391`); also `held_shapes::slow_radius_growth_stops_too` ("the other way, shrinking it, goes") |
| 4 | The same, dragged outward past the round's radius | **Refused**, every frame: "the drag never grows a radius it doesn't move" | Same test; LOG 2026-11-11 |
| 5 | Two arcs sharing an end, radii 8 and 12, one centre fixed, the other dragged further than they reach (e13) | **Refused** past the last shape where they meet. By the solver's `unkept` report, not by `breaks_held_shape` | `a_drag_the_kept_radii_cannot_follow_still_stops` (`drag_props.rs:2876–2885`) |
| 6 | A can't-land drag whose solve does not finish inside the cap | **Refused** (0.63 % of drag solves) | Audit, PR 3's notes |
| 7 | A drag that can't land and either crushes a line it doesn't move to nothing, or shortens it by more than 50 times the pointer's move while flinging an end more than 100 times it | **Refused**; short of that, it goes | `held_shapes::` pin and crank tests (`held_shapes.rs:482–630`) |

So today's rule, as built: **relations give way** (rows 1 and 2); **a held radius never grows, and
may shrink (in a frame that doesn't land, no faster than ten times the pointer)** (rows 3
and 4); **a held line may lengthen, and may shorten at the
geometry's own pace, but is not flung short or crushed** (row 7).
Rows 1 and 2 differ from row 6 only in whether the solve finishes inside its cap, which is not
a rule anyone chose. The 2026-11-12 ruling (a round set Equal to a fixed-centre round stops when
dragged off its tangent line) belongs with row 4; no test of it was found in what was read.

Two readings of PR 5 are on the table, and neither reproduces that table:

- **Pointer exact (the audit as written).** Rows 1 and 2 become refusals, and so do the
  can't-land drags of row 7 that go today (the crank, the pin's early frames). Rows 1 and 7
  are in the bar: item 7 names `held_shapes::`, so PR 5 as written fails its own acceptance
  test. The refused cases of rows 4 to 7 stay refused, 6 now for a stated reason (the audit:
  "PR 5's exact targets make these a conflict").
- **Pointer as a lower priority.** The relations, and the radii a drag holds, are exact; the
  dragged point is brought as near the pointer as they allow. This is what planegcs does
  (below). Rows 1 and 2 go, as today. But rows 4, 5 and 6 would no longer be refused: a held
  radius never *has* to change if the pointer may simply not be met, so the geometry would
  track the nearest allowed place where today the frame is refused and nothing moves. That is
  a different meaning of "stops" from the one the user ruled on (2026-11-03, 2026-11-11,
  2026-11-12) and that the tests above assert. It also adds a jump of its own: with a point
  held on a circle, the nearest place flips to the far side as the pointer crosses the centre.
  Row 7's rate rule has no place in it at all.

**Row 3 changes under both**, if decision 4 ("held radii are held exactly in a drag") covers
it: the circle would keep its size and the point would be refused or swing round. Whether it
does is open. The radius in row 3 is one the drag holds (`find_held_shapes`,
`drag.rs:1025–1059`), but PR 3, built after decision 4, kept "a radius the drag itself
dictates follows", and the shrink is tested and merged.

**What the references do.**

- **planegcs** splits a drag's system in two. The sketch's own constraints are the main
  system; the drag's temporary constraints (the dragged point tied to a target) are tagged as
  lower priority. For the part of the sketch that has both, it minimises the temporaries'
  error subject to the main system holding, by a sequential quadratic programming step. Its
  own comment calls the first "of higher priority than the second".
- **Task-priority control in robotics** has the same structure, one linearised step at a
  time: a lower task is done as well as it can be without disturbing a higher one (Siciliano
  and Slotine 1991). Escande, Mansard and Wieber (2014) solve any number of such levels as one
  problem. Their texts were not read for this note; this is their standard content.
- **SolveSpace** solves an under-constrained sketch "in a least squares sense" and says that
  "tends to yield 'less surprising' results when dragging points". Its technology page also
  says a sketch of a linkage "will fail to converge when the real linkage would bind". Its
  pages do not say how the dragged point is ranked.
- **ezpz** has priorities, but all or nothing: by a reviewer's reading of its `lib.rs`, it
  solves each priority level and below from scratch and returns the last one fully met. A
  lower level that can't be fully met is dropped, not met as nearly as possible.

So planegcs supports "the pointer gives way to the constraints", and the task-priority
methods describe the same structure. Nothing read has a held shape that makes a drag refuse,
a one-sided rule like "may shrink, never grow", or a rate rule.

**A possible mechanism, with its limits.** A solve with the pointer at a lower priority has,
for each exact constraint, a multiplier: the rate, to first order, at which the point could get
nearer the pointer if that constraint were let go a little, with a sign for which way. In principle that could state row 4's rule in PR 5's
terms: refuse the frame where a held radius carries a multiplier whose sign means "the pointer
wants this radius larger". The sign matters; a two-sided test would refuse row 3 as well. This
is an idea, not a proposal, and it has known holes:

- **Multipliers are unique only where the exact constraints are independent.** Drift allows
  redundant relations, and a held radius that a relation also fixes (a radius dimension, an
  Equal to a fixed round, which is the 2026-11-12 case) shares its load with that relation in
  no defined proportion.
- **In a coupled sketch nearly everything is loaded.** When the point can't land, every exact
  constraint between it and the pointer's obstacle carries some multiplier, as every member of
  a truss carries some force. "A relation is in the way" and "a held shape is in the way" are
  not exclusive; both at once is the ordinary case.
- **So it needs a threshold**, and a multiplier's size depends on how its row is scaled and on
  the drawing's size. That is a stop decided near round-off, the class of rule the audit's
  hypothesis (b) is about.
- **It says nothing about row 7**, which is about lines and rates, nor about a frame that
  lands (all multipliers are then zero), which `breaks_held_shape` can still refuse today.

**For the user.** The mechanism is PR 5's to design. What it must produce is the user's to
say, and the table is the question. Before PR 5 is designed:

1. **Rows 1 and 2:** should a drag whose point is kept from the pointer only by relations
   still go, following as near as it can, as the lever does today? (PR 5 as written says no.)
2. **Rows 4 and 5:** should those frames still be refused outright, the geometry staying
   where the last frame left it, and not track the nearest allowed shape? (A lower-priority
   pointer, by itself, says no.)
3. **Row 3:** does "held exactly" include a round whose radius the dragged point dictates, so
   that the inward drag stops shrinking the circle?
4. **Row 7:** decision 4 keeps the crushed-line stop. Does the pace rule (a held line flung
   short faster than 100 times the pointer) stay as well?

This note's recommendation: answer yes to 1 and 2, treat the table, as the user answers 3 and
4 and with row 6 settled by design and not by the cap, as PR 5's specification, and add each
row that lacks one as a test before PR 5 starts. That means PR 5's corrector
cannot be "exact targets" as written, nor a plain lower-priority pointer; its design has to
say how it tells row 1 from row 4. The note has no proven way to do that.

### 3.2 Folds: stop, and how to know one

- **A fold is a stop.** Path-following methods can follow a path round a turning point by
  stepping along the path instead of along the parameter (pseudo-arclength continuation,
  Keller's term; Allgower and Georg cover the family). For a drag the parameter is how far
  along the line from the start to the pointer the target has gone. Past a fold there is no
  solution further that way on the branch being followed, so there is nothing to show. That
  is the binding drag rule and what the audit says.
- **Detecting the fold is the design point.** Stepping in t and shortening the step as the
  corrector struggles only approaches a fold; the path nears it like a square root, and where
  the stepping gives up is set by a smallest step or a threshold. That is the kind of stop the
  audit found to be the main cause of results that depend on size (hypothesis (b)).
- **The path's tangent gives a direct test, at a price.** Take the system with t as one more
  unknown. Its tangent, the direction along the path with t included, has a t-component that
  falls to zero at a simple fold and changes sign past it. This is not the predictor the audit
  describes if that predictor is the rate of change with t, which has no t-component to read
  and grows without bound at the fold. It needs the tangent of the augmented system (one
  bordered solve per step), and to see the sign change it needs one step taken along the
  path, past the fold, whose result is used to locate the fold and never shown. So PR 5 can
  stop at a fold without following the path round it, but a clean test is not free.
- **Which system.** Most dragged sketches are under-constrained, so the relations and target
  rows alone do not define a path. The two-stage solve does, by picking the least change and
  then the kept shapes. The path, its tangent and its conditioning are that whole selection's,
  with the pointer's rank in it whichever way 3.1 is answered. It is PR 5's to compute. PR 4's
  promise (σmin of the rows, from the factor) is not that, and should not be stretched to
  cover it.
- **σmin alone does not say "fold".** It falls at a fold, at a bifurcation, at a redundant
  relation, and wherever the rows are merely ill-conditioned; the audit measured near-singular
  rows at 3.3 % of ordinary exits. A redundant row is singular all along the path, a fold or
  bifurcation at one place on it. So dependent rows have to be dropped by a rank decision
  first, and what is left told apart by the tangent: it turns back at a fold, and is not
  unique at a bifurcation.
- **The path is the chord.** Each frame follows the straight line from the drag's start to
  the pointer now (section 1). So a pointer the user has taken round an obstacle is still
  stopped by a fold on the straight line, and where a frame stops can differ from the last
  frame's as the line swings. That follows from solving from the start, which the audit keeps.
- **Stopping at a bifurcation** is right for the same reason as at a fold: nothing says which
  branch to take.

### 3.3 Start from the drag's start

planegcs saves a reference solution when a drag begins and starts every solve of the drag
from it, as Drift does. It also has an option, off by default, to take a new reference once
the pointer is 20 times the first move's length from the last one; its comment says the solve
is "getting too far away from the original solution". That is weak evidence its authors met
trouble with always starting from the beginning over a long drag. It is the same trouble the
audit's continuation is meant to answer by other means (following the path instead of leaping
to its end). Nothing read argues for solving from the last frame.

## 4. #400: the leaking projection

**The problem, from #400's description** (its branch was not read). Stage 2's projection
`μ·A⁻¹` lets through μ/σ² of each direction the relations feel with singular value σ. Where
the relations hold the shapes off, that swamps the step. #400 takes it off by projecting again,
up to four passes, citing Gould, Hribar and Nocedal. (That paper treats round-off in an exact
projection; applying it to a deliberately regularised one is an analogy, and repeated passes
do little for directions where σ² is not well above μ.)

**What planegcs does.** In its two-level solve it factors the transpose of the main system's
Jacobian by QR with full pivoting, takes the columns of Q past the rank as a basis Z of the
null space, and solves the lower-priority problem in those coordinates. A step built from Z
is in the null space of the linearised constraints to round-off. But it requires the Jacobian
to have full row rank: if not, the routine returns an error and the two-level solve stops
where it is (reporting success if the main system already holds, so the dragged point just
doesn't move). So it has no answer for the case #400 is about, where a direction is neither
clearly free nor clearly held. It also forms the full dense Q and carries a dense Hessian
estimate.

**What this is worth to Drift.** Not much, as it stands. A null-space basis would be new
code: Drift's `qr_rank` (`sparse.rs:298`) returns a rank and which columns can move, keeps no
Q, and does not pivot; `dense_rref` is test-only. The basis would have to be in stage 2's
mobility-weighted metric with the dragged and carried left out, not a plain orthonormal one.
And it would move #400's hard case into a rank threshold without removing it. If #400 is
resumed and its stop rules keep multiplying, an exact basis on small components is an
experiment to consider. This note does not recommend it ahead of finishing #400 as designed.

**Strict ranks or weights.** PR 6 ranks kept shapes by weights a thousand apart, and records
that directions give way "by about a thousandth of the change". The hierarchical methods above
would make the order strict. Nothing read says the thousandth matters to a user.

## 5. What the references don't offer

- **No stated stop rule.** No branch check was found in planegcs. When a solve fails during a move it
  does not apply the result, so as far as `Sketch.cpp` shows the drawn geometry stays as the
  last solve that worked left it (the solver's own values go back to the reference saved when the drag
  began); what
  the layers above do was not read. SolveSpace documents that
  a binding linkage fails to converge, and suggests stepping a dimension when a solve finds
  "an undesired solution". Neither states a rule like Drift's.
- **D-Cubed is closed.** Siemens's public posts say the 2D solver has "several features that
  can be used to influence how this geometry will move", and since version 72 a "localised
  solving" mode that limits a change "to the closest geometries". They do not say how.
- **ezpz** is Levenberg-Marquardt on the normal equations with a sparse Cholesky
  factorisation (`faer`), with a priority and weight per constraint (section 3.1). No drag
  target or branch handling in what was read. MIT licensed.
- **No one's numbers.** None of these sources publishes how often a drag fails or flips.
  Drift's censuses have no outside baseline.

## 6. Sources, and how far each can be trusted

Code was read for design only. Nothing here is copied from it.

| Source | How it was read | Licence |
|---|---|---|
| FreeCAD planegcs: `GCS.cpp`, `GCS.h`, `qp_eq.cpp`, `SubSystem.cpp`, `Constraints.cpp`, `Sketch.cpp` (main branch) | Source text, twice: by a fetch agent for the draft, then independently by the reviewer of the sources. Sections 2 to 4 state only what the reviewer confirmed in the source. | The files' SPDX line says LGPL-2.1-or-later (their older header text says Library GPL version 2 or later) |
| SolveSpace: `tech.pl`, `ref.pl` | Documentation pages only. All quotations confirmed verbatim by the reviewer. **Its source was not read**, and should not be by anyone who then writes Drift's solver. | GPLv3 (stated on its home page) |
| Zoo ezpz: `solver.rs`, `constraint_request.rs`, `lib.rs`, `Cargo.toml`, README, crate docs | Source and docs. The priority scheme is from the reviewer's reading of `lib.rs`. | MIT (`Cargo.toml`; the LICENSE file's name and year are unfilled) |
| Siemens D-Cubed 2D DCM | One post read in full (version 72.0, 2020), quotes confirmed. A 2022 introduction post, read only by the draft's fetch agent, said nothing on dragging | Closed |
| Papers and books (below) | Bibliographic details checked against index or publisher pages. **Their texts were not read.** Each is cited only for standard content. | — |

Not verified:

- FreeCAD's layers above `Sketch.cpp` (what a user sees on a failed drag frame; where the
  re-reference option is switched on).
- Lamure and Michelucci's text. It is listed because it applies continuation to geometric
  constraint solving; what it does at a turning point was not checked, and this note draws
  nothing from it.
- Section 3.1's row 2 (a point on a fixed line slides): from the code, no test, not run. Whether
  the app passes the radius itself as dragged on a rim drag (which would change row 3).
- How `drift-core` defines a typed Angle's direction; whether the app adjusts the pointer
  before calling `Editor::drag`.
- #400's branch.

**References**

- Allgower, E. L. and Georg, K. *Numerical Continuation Methods: An Introduction.* Springer,
  1990. Reissued as *Introduction to Numerical Continuation Methods*, SIAM Classics in Applied
  Mathematics 45, 2003.
- Borning, A., Freeman-Benson, B. and Wilson, M. "Constraint hierarchies." *Lisp and Symbolic
  Computation* 5(3), 1992, 223–270. (The general theory of required and preferred
  constraints; background to section 3.1.)
- Escande, A., Mansard, N. and Wieber, P.-B. "Hierarchical quadratic programming: fast online
  humanoid-robot motion generation." *International Journal of Robotics Research* 33(7), 2014,
  1006–1028.
- Gould, N. I. M., Hribar, M. E. and Nocedal, J. "On the solution of equality constrained
  quadratic programming problems arising in optimization." *SIAM Journal on Scientific
  Computing* 23(4), 2001, 1376–1395.
- Lamure, H. and Michelucci, D. "Solving geometric constraints by homotopy." *IEEE
  Transactions on Visualization and Computer Graphics* 2(1), 1996, 28–34.
- Siciliano, B. and Slotine, J.-J. E. "A general framework for managing multiple tasks in
  highly redundant robotic systems." *Fifth International Conference on Advanced Robotics*,
  1991, 1211–1216.
- FreeCAD: <https://github.com/FreeCAD/FreeCAD/tree/main/src/Mod/Sketcher/App/planegcs>
- SolveSpace: <https://solvespace.com/tech.pl>, <https://solvespace.com/ref.pl>
- ezpz: <https://github.com/KittyCAD/ezpz>
- Siemens: <https://blogs.sw.siemens.com/plm-components/d-cubed-2d-dcm-version-72-0/>

## 7. What to do with this

Nothing now: PR 4, PR 5 and #400 are parked. When one is taken up:

| When | Do |
|---|---|
| Before PR 5 is designed | Put section 3.1's four questions to the user. Add a test for each row of its table that lacks one (row 2 first: a point on a fixed line, dragged off it). |
| PR 4 | Keep the audit's angle-error form. Say what happens at the 180° jump, and where a typed Angle's way round comes from. |
| PR 5 | Build to the table in 3.1 as the user answers it; say how the design tells row 1 from row 4. Say how a fold is detected and told from a bifurcation and a redundant row (3.2). |
| #400, if resumed | Finish it as designed. An exact null-space basis is a fallback experiment, not a recommendation. |
