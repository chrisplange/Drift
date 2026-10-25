# Plan: constraints only where you add them, predictable drag, robust regions

*2026-10-21, against `main` at b8fb54f; step 1 is built on #138 (`claude/path-records`).* Companion to
[fills-and-strokes-audit.md](fills-and-strokes-audit.md), which covers the fill/shape model.

Three workstreams, in this order. Each ends in a property test suite that guards it, so a
fix in one place can't quietly break another.

---

## 1. Constraints only where you add them

**Goal:** geometry with no relations or dimensions never goes through the solver. Moving,
reshaping or pen-editing it behaves exactly like Illustrator: what you drag goes where the
cursor says, nothing else moves.

**Today:** a drag works out its scope with `drift_solver::connected`
(`editor.rs:488`), which is closed under *shared points and constraints*, and then
always calls `solve_in`. So a free rectangle (four lines, four shared points, no
relations) still runs Newton with length/direction/radius preferences, which is where
small surprises come from (a neighbour nudging, a preference creeping, #94-class drift).

**Steps**
1. **Classify the scope.** In `drift-solver`, add `fn constrained(doc, scope) -> bool` (or
   return a count with `connected`): does any constraint touch an entity in the scope?
   Coincidence through a *shared point id* is topology, not a constraint, and does not
   count.
2. **Direct path in the editor.** In the drag and edit paths (`editor.rs` `solve_in`,
   `solve`, and the calls near `:704`, `:976`, `:1846`), when the scope is unconstrained:
   apply the dragged positions directly, carry `carried` params by the same delta, and skip
   `drift_solver::solve`. No preferences, no iterations.
3. **Split mixed scopes.** A drag that touches both: solve only the constrained component
   (`connected` restricted to constraint edges), and move the free part directly. This is
   the common case "free decoration hanging off a dimensioned frame".
4. **Make the state visible.** Free geometry draws in its own colour/tag (the render
   already has blue/black/red); the first relation added to it is the moment it joins the
   solver. Undo of that relation returns it to free.
5. **Tests.** Extend `drag_props.rs` with: *for any sketch with no constraints, any drag
   moves only the dragged and carried params, by exactly the cursor delta* (bit-exact, not
   within tolerance). And: *adding then undoing a relation leaves free geometry
   bit-identical.*

**Done when:** the free-geometry property holds for all generated sketches, and dragging
unconstrained pen paths never calls the solver (assert via a counter in a debug build).

---

## 2. Predictable drag before clever drag

**Goal:** in constrained geometry, a drag moves the least it has to, ends where it would
end regardless of the path the cursor took, and never creeps.

**Today:** `SolveOptions` carries soft preferences (`radii`, `directions`, `lengths`,
`focus`) and patience constants (`SOFT_PATIENCE = 20`, `SETTLE_PATIENCE = 10`) as a
safety net. The #94 soft-settle work fixed the worst of it; #95, #90 and #102–#104 are
the residue, and `drag_props.rs` has ignored properties tracking them.

**Steps**
1. **Write the contract down** as properties, one per behaviour, before changing code:
   - *Minimal movement:* untouched components don't move at all (bit-exact).
   - *Path independence:* dragging A→B directly and A→C→B end in the same place (within
     tolerance) when nothing switches branch.
   - *No creep:* dragging out and back to the start returns the sketch to where it was.
   - *Rotation/translation invariance:* the same drag on a rotated sketch gives the
     rotated result (the #94 regularisation fix generalised).
   - *Determinism:* two runs give bit-identical results.
2. **Remove hash-order dependence.** Replace iterated `HashMap`/`HashSet` in the solver
   (`by_root` at `lib.rs:667`, `scales` at `lib.rs:874`, `free_directions`) with
   `BTreeMap`/sorted `Vec`. Cheap, and it makes the determinism property testable.
3. **Solve from the drag's start, not the last frame.** Each frame, solve from the
   positions at drag start with the cursor's current target, rather than from the previous
   frame's result. This is the single biggest lever for path independence and no-creep;
   fall back to the previous frame only when the start is too far to converge from
   (branch flip), and keep that branch for the rest of the drag.
4. **Fewer, ranked preferences.** Keep `dragged` > `carried` > `focus` as a strict
   hierarchy (weighted least-squares with separated weights, or a two-stage solve), and
   treat `radii/directions/lengths` as one "keep shape" preference with a single weight.
   Retire `SOFT_PATIENCE` once the properties hold without it.
5. **Un-ignore** the `drag_props.rs` properties for #94/#95/#90 one at a time; each one
   passing closes its issue.
6. **Performance guard.** Keep the perf baselines from the #94 work; solving from the drag
   start can cost more iterations, so check large sketches.

**Done when:** all properties in step 1 pass without `#[ignore]`, and #95, #90 and
#102–#104 are closed.

---

## 3. Regions as a robust subsystem

**Goal:** closed areas come out the same for the same picture, whatever tiny gaps,
near-coincident edges or overlaps the geometry has. Fills stop being guessed per case.

**Today:** `drift-geom/src/regions.rs` builds a planar arrangement (split at crossings,
prune dangling edges, trace faces clockwise). Tolerance is per call site
(`tolerance(magnitude)`, with `* 10.0` pads in places, `tol_area = 1e-12`), and fills
re-find their face after edits (#120, #101). The audit adds the bigger change: a *shape*
object that owns its fill (#129, #133–#135).

**Steps**
1. **One snap stage.** Before splitting, snap all edge ends and crossing points to a
   single vertex set with one explicit tolerance (`RegionTolerance { snap, min_edge,
   min_area }`) derived from the drawing's extent. Every later stage works on snapped
   vertices only, so there are no more scattered `tolerance(..) * 10.0` pads.
2. **Deterministic output.** Sort vertices, edges and faces canonically (by position, then
   id), so the same picture gives the same face order and fills can key on stable data.
3. **Stable face identity**, for the fills still on faces after step 4. Give each face a
   fingerprint (the set of source curve ids and sides bounding it). A fill keeps its
   fingerprint; after an edit it reattaches to the face with the best-matching
   fingerprint, then to the face containing its seed. This replaces the case-by-case
   "follow the outline / follow the seed" logic. Build it on #138's path records: a face
   bounded by one path's curves can key on the path.
4. **Shapes first, faces second** (from the audit). Mostly done in
   [#137](https://github.com/chrisplange/Drift/pull/137) (fills stacked and drawn with their
   shapes) and [#138](https://github.com/chrisplange/Drift/pull/138) (shapes drawn as one
   are recorded as paths, and their fills follow them alone). What's left is to make sure
   the arrangement is only used for Live-Paint-style fills over a web of lines (plans,
   hatching), and that steps 1–3 hold for those.
5. **Curves.** Make sure Béziers and ellipses enter the arrangement the same way as lines
   and arcs (exact where possible, adaptively flattened with tolerance tied to the snap
   tolerance otherwise).
6. **Tests.** A `regions_props.rs` proptest suite:
   - *Perturbation:* jittering vertices by less than `snap` doesn't change the face count
     or topology.
   - *Invariance:* translating, rotating or scaling the drawing gives the corresponding
     faces.
   - *Area conservation:* the faces' areas sum to the area of their union (holes counted).
   - *Order independence:* shuffling the input curves gives identical output.
   - *Gap/overlap fixtures:* the private import samples (see the sample files memory)
     produce the same faces before and after a refactor (golden fingerprints, kept outside
     the public repo).

**Done when:** the property suite passes, the open fill bugs reproduce as failing
fixtures first and then pass, and no call site in `regions.rs` uses its own tolerance.

**Status (2026-10-25):** steps 1, 2 and 6 are done (3a, #142). Step 3 (fingerprints) and
step 5 (curves) are planned in [trim-and-regions-plan.md](trim-and-regions-plan.md), as 3b
(§3) and B0 (§2).

---

## Order and sizing

| # | Workstream | Size | Depends on |
|---|---|---|---|
| 1 | Free geometry skips the solver | small (days) | — |
| 2a | Deterministic solver, written contract | small | — |
| 2b | Solve from drag start, ranked preferences | medium–large | 1, 2a |
| 3a | Snap stage, canonical order, region properties | medium | — |
| 3b | Face fingerprints (shapes model is #137/#138) | medium | 3a, #138 |

1, 2a and 3a are independent and can run in parallel sessions. Start with 1: it's the
smallest change with the biggest effect on how the app feels to a designer.
