# Plan: trim first, regions 3b, Divide next, other Booleans later

*2026-10-25, against `main` at 05a2fc5 (format 42, reader 42); revised the same day after an
independent review.* Companion to [solver-drag-regions-plan.md](solver-drag-regions-plan.md) §3
(regions as a subsystem; 3a done in #142) and
[fills-and-strokes-audit.md](fills-and-strokes-audit.md) §3.4 (explicit combining). Walls W1
is being built while this is written. §2.4 says how B0 fits around it.

The audit's step 4 was "Divide, then the Booleans, then a Shape Builder tool". A first
draft of this plan designed all of that. Checked against the code, most of it exists already
as Trim plus the region finder plus `follow_paths`. So this plan does the smaller thing:

1. **B0 and 3b, on their own merits:** an arrangement that handles Béziers and ellipses
   properly, and face fingerprints for area fills.
2. **Trim fixes:** the gaps that make combining shapes by trimming slower than it should be.
3. **A script verb** for the same thing, so assistants and scripts can do it without
   computing click points.
4. **Divide, as the first Boolean to add after B0.** It's the one thing trim can't do.
5. **The other Booleans deferred**, with the notes worth keeping (§7).

---

## 1. What already combines shapes today

Every item here was read in the code at 05a2fc5.

| The Pathfinder operation | How Drift does it today | Where |
|---|---|---|
| **Add** two overlapping filled shapes | Trim the piece of each outline lying inside the other. Both paths open, their ends meet, `follow_paths` joins them into one closed path, and the topmost fill is kept (the other goes as `FillChange::Merged`). | `fills.rs:1438`; test `trimming_two_paths_into_one_outline_joins_them` |
| **Subtract** a shape lying inside another | Nothing to do. Islands are holes: a shape inside a path's outline is left out of its fill. | `path_region`, `fills.rs:1356`; test `a_paths_fill_follows_its_own_outline_alone` |
| **Subtract** a notch (a shape crossing an edge) | Trim the bottom shape's piece inside the top one, and the top one's piece outside the bottom. Two clicks per pair of crossings. The joined outline takes the topmost fill, so a Ctrl+[ first keeps the bottom's look. | `trim_at`, `modify.rs:622` |
| **Intersect** | Trim each outline's pieces outside the other. | same |
| **Divide** into hatching zones | The Fill tool fills the faces between lines. These are area fills, Live-Paint-style, which is what plans need. | `add_fill_at`, `fills.rs:1272` |
| **Divide** into independent pieces | **Not possible.** A curve is in one path only (`paths.rs:17`), and trimming never duplicates a shared edge. See §6. | — |
| **Shape Builder**'s drag | The Trim tool's drag removes every piece the pointer crosses ("power trim"). It's the same gesture on the removed pieces rather than the kept faces. | `TrimTool::sweep`, `tools/curve_ops.rs:31` |

Trim handles every curve kind: lines, arcs, circles, ellipses, elliptical arcs and Béziers
(Bézier pieces through `bezier_piece`, with smooth joins forgotten at the cut). It also does
what Pathfinder doesn't:

- **The surviving piece keeps its id** (`set_entity_kind`), so dimensions and relations on
  it survive.
- **Relations the trim breaks** are taken off and named (`finish_reshape`, `modify.rs:892`).
  A dimension or Fix that no longer holds refuses the trim.
- **You remove what you see.** There's no mode and no guessing about stacking order, and
  the result is never surprising.

**Why not the Pathfinder model.** It's modal (pick, then choose the right one of five
buttons). Its results depend on stacking order. It throws away the originals and their
relations. In Drift it would be a second way to reshape outlines, running beside
`follow_paths` and `follow_fills`, which would have to stay out of its way. The first draft
found one place where they wouldn't (§7.2).

---

## 2. B0: one arrangement, exact for Béziers and ellipses

### 2.1 What's wrong today

`regions_with` (`drift-geom/src/regions.rs:661`) builds a planar arrangement and keeps only
the resulting `Region`s. `Edge` has two variants, `Line` and `Arc`, and it is `Copy`.
Ellipses and Béziers enter as fixed chords: 48 per ellipse and 24 per Bézier, with a stop at
each crossing (`chords_through`, `:627`). And `vertices_on` (`:571`) runs only lines, arcs
and circles through nearby vertices. In practice:

- **Two coincident Bézier or ellipse pieces with different parameter ranges** (a pen shape
  trimmed against a copy of itself, an ellipse arc lying on a whole ellipse) get different
  chords. One piece's chord points aren't vertices on the other, so the deduplication by
  middle never fires, and they leave **sliver faces** instead of one shared edge.
- **Pieces carry no parameter range**, so nothing downstream can know which part of which
  curve bounds a face. 3b needs this, and so does Divide.
- **A line ending on a Bézier's interior** relies on `intersect` to become a junction.
  `intersect_numeric` snaps an end hit within tolerance onto the end exactly, so this
  usually works. It fails only when Newton doesn't converge from the chord approach, which
  is rare. B0's test for it is written **first**, and may well pass today. B0's case doesn't
  rest on it.
- **A self-crossing cubic** (the pen makes these by accident) gets no vertex where it
  crosses itself, because `split` intersects only pairs of different curves (`:406`). So
  its faces are wrong.

This is plan §3 step 5, left open by 3a.

### 2.2 What B0 does

- **`arrangement(curves, tol) -> Arrangement`**, factored out of `regions_with`:
  - `verts`: canonical, by (x, y);
  - `edges`: canonical, each with its `from`/`to` vertices, its geometry, and every input
    curve `along` it as a `Span { source, t0, t1 }`. Direction is carried on the span, not
    the edge, so a reversed coincident piece merges too.
  - `faces`: the bounded faces as half-edge loops (outer and holes), each with the `host`
    face its component sits in.

  `regions_with` becomes `arrangement(..)` plus building `Region`s.
- **`Edge::Curve { source: usize, t0: f64, t1: f64, kind: CurveRef }`**: one edge per
  stretch of an ellipse or Bézier between two real vertices (ends, crossings, snapped-on
  vertices). It replaces one edge per chord.
  - `CurveRef` holds the exact curve: a cubic's four points, or an ellipse's centre, axes
    and angles. **`Edge` stays `Copy`.** There is no stored polyline; it is flattened on
    demand.
- **Three uses, three tolerances.** Flattening to `tol.snap` (1e-9 × extent) would need
  about 10⁴ chords per curve, and every containment test and frame would walk them. So:
  - **Area** is exact by Green's theorem: closed-form for a cubic, and for an elliptical
    sector. `area2` stays exact, so `faces_conserve_area` stays tight.
  - **Containment** uses a loose polygon, at about the angle step arcs already use
    (`polygon(0.02)`). A point within flattening error of the boundary is answered by
    `near_outline`, which already exists.
  - **Drawing** uses the renderer's own tolerance, as arcs do today.

  `tol.snap` is used only for vertex placement.
- **Departure at a vertex** is the first non-zero of the first, second and third
  derivatives at that end. A handle collapsed onto its anchor (from imports, or a pen click
  without a drag) has a zero first derivative. Taking `atan2(0, 0)` would put the edge at a
  random place in the fan and mis-trace every face at that vertex.
- **`vertices_on` for Béziers and ellipses**, by projection (`nearest_on_bezier`,
  `nearest_on_ellipse`) within snap. A vertex near two places on a tight S-curve projects
  to the nearest one only. That's rare and accepted.
- **A self-intersection pass for cubics.** Each cubic is split at its parameter midpoint,
  the two halves are intersected with the existing `intersect`, and the shared midpoint is
  excluded. Each hit is a cut stop on the curve, at both of its parameters.
- **Deduplication** keys a curved edge by its unordered vertex pair and its point at
  mid-parameter. Sub-pieces of one curve are affine reparametrisations, so coincident pieces
  evaluate to the same middle up to rounding and merge into one edge. Independently drawn
  curves that merely look alike stay apart.

### 2.3 Done when

- **Bit-identical output for lines, arcs and circles.** This is a test, not a one-off: the
  old `regions_with` output over `crates/drift-io/tests/fixtures/*.drift` is recorded as JSON
  before the refactor, and compared after it. Golden fingerprints from the private samples
  are kept outside the repo (plan §3 step 6).
- New properties in `regions_props.rs`:
  - two coincident pieces of one Bézier or ellipse, either way round, are one edge, with no
    slivers;
  - a pen shape with a collapsed handle at a corner has the same faces as the same shape
    with a tiny handle;
  - a self-crossing cubic closed by a line gives its two lobes as faces;
  - the existing perturbation, invariance, area and order properties hold for drawings with
    Béziers and ellipses.
- Unit tests:
  - a line ending on a Bézier's interior is a junction (written first);
  - a Fill tool test on a pen shape closed by a line ending on its curve.
- **Performance:**
  - `perf_editor_pipeline` (its "find closed regions" step, `perf_tests.rs:139`) and
    `perf_import_commit` (`:594`), with no regression beyond noise;
  - a new `perf_regions_curves` case, a drawing of pen shapes and ellipses, which neither
    existing case has.

**Size M**, with the flattening, departure and self-crossing points decided above. Left
open, they would make it L.

### 2.4 B0 and W1

W1's work (uncommitted, branch `claude/walls-w1-ace1e7`) edits `regions.rs`: it makes
`Loop::contains` public and adds `Region::from_loops`. It also adds
`drift-geom/src/walls/`, which builds `Loop`s from `Edge::Line` and `Edge::Arc` by hand.
So:

- **B0 is API-compatible with W1's use of `Edge`, `Loop` and `Region::from_loops`.** `Edge`
  stays `Copy`. `Edge::Curve` is a new variant that W1's code never produces. `Loop` keeps
  its fields. W1's `match`es get a `Curve` arm only where the compiler asks.
- **B0 lands after W1, or is rebased onto it.** A B0 session can start now, on a branch cut
  from W1's once it's pushed. Or it can start from main and rebase when W1 merges, keeping
  its `regions.rs` diff away from W1's two hunks.

---

## 3. 3b: face fingerprints for area fills

### 3.1 What's wrong today

A shape's fill is found from its path and needs no guessing. An **area fill** (no path; a
face among shared lines, for hatching plans) is found again on every edit by a ladder of
guesses in `fill_shapes` (`fills.rs:325`) and `follow_fills` (`:433`):

1. the face bounded by exactly its curves (`Areas::exact`);
2. its whole area ignoring crossing curves (`uncrossed_region`, the `divided` flag);
3. `reshaped_whole`;
4. `successor`;
5. `sharing_at(near)`;
6. the face at its seed;
7. otherwise lost, or dropped.

Each rung was added for a bug (#120, #101, #133, #134, #135), and each new case means
another rung. The audit said not to keep adding them (§4 there).

### 3.2 Fingerprints

- **A face's fingerprint** is the set of `(EntityId, Side)` pairs for the curves bounding it,
  outer loop and holes kept apart. `Side` is Left or Right of the curve's own direction. It
  comes free from B0's half-edges.
  - The boundary set alone already tells most faces apart, including the two rooms either
    side of one wall line. `Side` decides only when two faces are bounded by the same set of
    curves (a lens between two arcs, the two lobes of a figure-eight).
- **Not stored in the file.** A fill already stores its boundary set and seed. The
  fingerprint is derived and cached. No format change.
- **Built before the edit, not when something draws.** `last_fill_shapes` is filled by
  `fill_shapes()`, which runs when something draws or asks. A headless editor (MCP, a
  script, a load followed at once by an edit, every unit test) would reach `follow_fills`
  with no fingerprint, and fall back to the ladder 3b removes. So at `begin()`, fills that
  have no fingerprint get one from the **pre-edit** arrangement, which `fill_shapes` already
  caches by revision.

### 3.3 Reattaching after an edit

Each edit says what became of curves, through a **substitution map**
`old curve → [(new curve, same direction?)]`:

- Trim, Split, Fillet and Chamfer build it explicitly. The id keeps the near piece, and
  `rest` is the far piece.
- Other edits get it from the journal (`reshaped_in_transaction`, `created_in_transaction`):
  a curve reshaped in place maps to itself, and a deleted one to nothing.
- **A curve regenerated by `follow_walls` with the same key maps to itself.** Keyed ids are
  stable (walls-design.md §2.3). Without this rule, a room's area fill on wall faces would
  go through the seed fallback on every wall edit.

The fill's old fingerprint is mapped through the substitution. The candidate faces (those
sharing at least one mapped curve) are scored by the **length-weighted overlap** of their
`(curve, side)` pairs:

- The best score of **at least one half** wins.
- **Ties go to the face around the seed.** That includes the half-split case (a wall split
  between two equal rooms, a shape trimmed to its half), where rounding would otherwise
  decide.
- With no candidate, the fill takes the face at its seed if that face shares a curve with
  it. Otherwise it is lost, or dropped if the edit deleted its curves.

This one scored match replaces rungs 1 and 3–5. The user's explicit rules stay on top,
unchanged:

- a curve drawn across an intact outline never shrinks a fill (`uncrossed_region` and the
  `divided` flag);
- `follow_divided_fills`;
- the #134 "drawn twice" rule in `delete_curves`;
- the topmost-wins merge.

### 3.4 Done when

- **Every existing `fill_tests.rs` test passes unchanged.** That includes the #129, #133,
  #134 and #135 close-outs. #133's
  `staggered_rectangles_sharing_part_of_a_side_keep_their_own_lines` and #134's
  `a_rectangle_drawn_twice_keeps_its_fill_when_either_copy_goes` were enabled in 1c46e52.
- New properties:
  - an edit that doesn't touch a face keeps its fingerprint;
  - a split of a bounding curve maps through the substitution to the same face;
  - the half-split case follows the seed, with the seed on either side;
  - the result doesn't depend on the order the faces come in.
- A headless test: load a drawing with area fills, edit at once (no draw in between), and the
  fills follow by fingerprint, not by the seed fallback.
- `follow_fills` is shorter than it was.

**Size M.** The real dependency is B0's spans. W1's change to `fills.rs` is small and far
from `follow_fills`, so there's no need to wait for W1 beyond what B0 already waits for.

---

## 4. Trim fixes

Small changes that make Trim the comfortable way to combine shapes. Each is its own commit,
and together they make one PR.

### T1. A drag-trim is one undo step

**Today:** `TrimTool` trims the piece under the pointer at `DragStart`, then `sweep` calls
`trim_at` for each crossed piece, and each commits. The code says so: "Each piece crossed is
its own undo step; failures are skipped quietly" (`curve_ops.rs:63`). A drag across six
pieces takes six Ctrl+Z to undo. Halfway through, the fills sit lost on open paths, and a
piece that can't be trimmed (a dimension holds it) is skipped without a word.

**Change:**
- At `DragStart`, **before** that first trim (`curve_ops.rs:96`), the tool notes
  `history.recorded()`.
- At `DragEnd`, it squashes everything since into one step labelled "Trim"
  (`History::squash_last(n, label)`, `drift-core/src/document.rs:3858`). This is the same
  pattern `Session::step` uses for agent calls.
- The pieces still commit one by one, so each trim sees the previous one's result, and
  `follow_paths` and `follow_fills` run after each. Only the history changes.
- A trim that fails has already rolled back, including one refused with
  `EditError::Conflict` (`finish`, `modify.rs:949`). It counts as skipped, not as a step to
  squash.
- The notices are gathered over the drag. At the end there is one summary:
  > Trim: 6 pieces. Took off what no longer held: Length. 1 piece wasn't trimmed: a
  > length holds it.

**Tests:**
- `tools/tests.rs`: `a_drag_trim_is_one_undo_step`, where one Ctrl+Z restores every piece,
  fill and path, including the one trimmed at `DragStart`.
- `a_drag_trim_reports_the_pieces_it_skipped`, with a dimension holding one piece.
- A smoke test (§8).

### T2. Trim against the picked curves

**Today:** `cuts()` (`modify.rs:499`) asks the spatial index for every curve near the one
being trimmed, construction lines included. So an unrelated grid line, reference line or
neighbour crossing two shapes adds pieces between them. Trimming one shape into another
takes extra clicks, and a drag can trim more than meant.

**Change (AutoCAD's standard Trim, where cutting edges are picked first):**
- **The query takes the cutting set as a parameter:**
  `cuts_against(id, against: Option<&CuttingSet>)`, and `cuts(id)` is the `None` case.
  `trim_span`, `trim_preview` and `trim_at` take the same parameter. No hidden editor state
  changes what the query returns, so the tool, the MCP `against` field and the script verb
  (§5) share one query and can't drift apart.
- **The cutting set is keyed by path, not curve ids.** The far piece of a trimmed cutting
  curve is a new id (`rest`), so a list of curve ids goes stale halfway through a drag or a
  script. `CuttingSet` holds paths and loose curves:
  - a picked curve in a path stands for **that path's curves as they are now**
    (`doc.path_of`, kept current by `follow_paths`);
  - a picked loose curve stands for itself, and for the pieces made from it in this
    gesture. The tool records each `rest` it creates.
- **If curves are picked when the Trim tool starts**, they are the cutting set. The
  selection survives the tool switch (`set_tool` doesn't clear it).
  - The picked curves can be trimmed themselves: trimming two overlapping shapes into each
    other is "pick both, T, click (or drag across) the two inner pieces".
  - **A selection with no curves** (only points, fills or text) counts as nothing picked.
    A stray picked fill mustn't make every piece untrimmable.
- **With nothing picked**, Trim works as today: everything cuts.
- The hint says which: "Trimming against 8 picked curves · Esc for all curves". Esc clears
  the cutting set and the selection, and leaves the tool on. The highlight shows the cutting
  edges, so it's clear what counts.
- **Extend** gets the same parameter in `extend_target`. It keeps iterating every curve
  rather than the spatial index. An extension can reach any distance, so there are no
  bounds to query by.

**Built (T1 and T2, uncommitted).**
- `CuttingSet` and the `*_against` forms of `cuts`, `trim_span`, `trim_preview`, `trim_at`,
  `extend_target` and `extend_at` are in `modify.rs`.
- The Trim and Extend tools read the picked curves at their first input
  (`tools/curve_ops.rs`), and a drag-trim is one step with one summary.
- **A bug found on the way and fixed with it:** `follow_paths` let a path that a trim had
  left open chain through another open path's curves and take them, without closing. The
  other shape's fill then lost its path and became a loose area fill. When the outline
  closed, two fills were stacked on it, against "trimming filled shapes together leaves one
  fill". An open chain now takes only its own and new curves (`fills.rs`, `follow_paths`).
  The test `squares_trimmed_open_one_side_at_a_time_join_with_the_top_fill` fails without
  the fix.

**Tests:**
- `trimming_against_picked_curves_ignores_a_grid_line`: two filled circles and a line across
  both. With the circles picked, two clicks give one peanut, and the line is untouched.
- `a_drag_against_picked_shapes_follows_the_far_pieces`: a cutting curve trimmed early in
  the drag still cuts through its `rest`.
- `a_picked_fill_alone_leaves_trim_cutting_against_everything`.
- `with_nothing_picked_everything_cuts`, which pins today's behaviour.
- `extend_reaches_only_a_picked_boundary`, and `extend_ignores_an_unpicked_line_in_the_way`.

### T3. Which fill a join keeps: unchanged

When trimming closes open paths into one (`follow_paths`), **the topmost fill stays**, as
with `Merged` (#129) and Illustrator's Unite. A "most outline length" rule was considered
and rejected:

- Two equal shapes are a tie only up to rounding, so a test of "unchanged" can't be pinned.
- Near-equal shapes would flip the result on a tiny edit.

For a notch, Ctrl+[ on the bottom shape first keeps its look, and T2's hint can say so when
two filled shapes are the cutting set. A better rule, if one is ever wanted, belongs to an
explicit operation, not to `follow_paths`.

### T4. Hole ownership: filed, not in this plan

Islands look like holes, but a hole stays behind when its shape is moved by its fill. A path
fill moves only its path's curves (`selected_fill_curves`, `fills.rs:1218`). The fix is the
audit's compound path (§3.2 there: `holes`), a format addition that the tolerant reader
(#166) makes cheap. It's filed as an enhancement, **"Make hole / compound path"**:

- "Make hole" on a shape and an island picked together;
- `Path.holes`, which Divide's rings would also use (§6);
- a fixture.

It doesn't ride along with the trim fixes.

### T5. Notes for the same list

- `extend_target` refuses everything but lines and arcs (`modify.rs:980`: "only lines and
  arcs can be extended"). An architect extending an elliptical arc to a wall is refused.
  This isn't T2's job; it's filed beside T4.

---

## 5. A script verb: `trim … inside` / `outside`

**Today:** Drift script builds geometry declaratively (rooms, walls, arrays, relations,
fills) and has **no modify verbs** (`parse.rs:372`). Through MCP, an assistant can trim only
with `trim_extend_split {id, at, mode}`, which needs a point on the piece to remove, so it
has to compute pick points.

**The verb:**

```
trim A inside B      # remove A's pieces that lie inside closed shape B
trim A outside B     # remove A's pieces that lie outside B
```

"Inside" and "outside" name **what goes**. That's documented plainly, because an AutoCAD
reader might read `trim A inside B` as "trim A to B". A and B are names or ids: a shape, a
room's outline, `use e12 as …`, or a list. **B resolves to a path**, and "closed" means
`path.closed`. A lone circle is a closed path of one. With two lines, the combinations people
want come out of Trim's own rules:

```
rect slab 0,0 6 m x 4 m
circle bay 6 m,2 m r 1.2 m
trim slab inside bay      # union: both pieces inside the other go,
trim bay inside slab      # and follow_paths joins one shape
fill slab solid
```

| Want | Lines |
|---|---|
| Union | `trim A inside B`, `trim B inside A` |
| Notch (A minus B) | `trim A inside B`, `trim B outside A` |
| Overlap only | `trim A outside B`, `trim B outside A` |
| Hole | nothing: draw B inside A (islands are holes) |

This table goes into `docs/guide/ai/script.md` and into the script's `DESCRIPTION`, which
the MCP tool's description quotes.

**How it runs:**
- `Stmt::Trim { what, side, by }` in `parse.rs`, `STATEMENTS`, `names.rs`' check, and
  `DESCRIPTION`.
- `Runner::lower` resolves A and B to paths, then calls `trim_extend_split` with two new
  optional fields:
  - **`against`**, which is T2's cutting set, as path or curve ids;
  - **`keep: "inside" | "outside"` with `by`**. With these, the editor finds a curve's
    cuts against B only, and removes each piece on the unwanted side of B's region.
  - Each removal is a `trim_at` with B as the cutting set.
- **It iterates A's curves as they are after each trim**, until no curve of A's path has a
  piece on the unwanted side. The first trim of a side cut twice creates `rest`, and the
  next unwanted piece is on `rest`. Iterating the original id list would miss it.
- **The midpoint test is enough.** A piece between two consecutive cuts against B lies
  wholly on one side of B, by construction. So its midpoint decides, and no sampling is
  needed. The code says so, so nobody "improves" it later.
- The statement therefore goes through exactly Trim's semantics: ids kept, relations
  dropped and named, `follow_paths` joining. The whole script is one undo step, as every
  script is.
- **Errors and warnings:**
  - "bay isn't closed: trim … inside needs a closed shape" (an error);
  - "slab has nothing inside bay" (a warning, not an error, so a script stays runnable);
  - a piece a dimension holds is reported, as T1's summary does.
- **Tests:**
  - `script/tests.rs`: each row of the table, a side cut twice (the `rest` case), and the
    errors;
  - `agent/tests.rs`: `trim_extend_split` with `against`, and with `keep`/`by`.

**Size S–M**, after T2, whose cutting-set parameter it reuses.

**Built (uncommitted).** What changed from the design above:
- **B can't simply be "a closed path".** Once the first line trims A, A is open, so
  `trim B inside A` would have no inside to test against. This affects the union and the
  notch, and the table's second line depends on it. So the inside test is explicit:
  - `Editor::trim_by(what, by, area, inside)` takes an `Area` (closed polygons) instead of
    finding it from `by`;
  - the script remembers each name's area from before this script first trimmed it, and
    passes it as the MCP field `inside_of`.

  Nothing is hidden state: a run with `explain` replays exactly.
- **The MCP fields** on `trim_extend_split` are:
  - `against`;
  - `remove: inside|outside` with `by` (and no `at`), with optional `ids` for more curves of
    A;
  - `and_remove`, which does the second half in the same call against A as it was, so a
    union is one call;
  - `inside_of`.
- **"The shape of a curve"** (`Editor::shape_curves`) is its path or, for loose curves such
  as script and `add_geometry` rectangles, the closed loop it is in through shared end
  points.
- **A circle or whole ellipse** that lies wholly on the side to go, with fewer than two cuts,
  is deleted whole. Trim can't do that, because it needs two cuts on a closed curve.
- **After a trim, a script name** stands for what is left of it plus the pieces trims cut
  off it (`Runner::trimmed_into`).
- **Tests:**
  - `modify::tests` (union, notch and overlap by area; errors);
  - `agent::tests` (`against`; a union in one call and one step);
  - `script::tests` (parsing; the three rows; the error and the warning).
- **Not tested yet:** the warning for a piece a dimension holds. It goes through Trim's
  `finish_reshape` refusal, which `trim_by` records as skipped.

---

## 6. Divide: the first Boolean to add, after B0

Divide is the one Pathfinder operation that the target users lose and trim can't give.
Signage, packaging dielines and laser-cut parts split overlapping shapes into pieces and
move them apart. Architects do the same when hatching a site plan into zones that must be
edited separately later. Those users won't ask for "Pathfinder". They'll say Drift can't cut
a shape into parts.

With B0, Divide is cheap. It needs none of the other operations' machinery (inside-ness
labelling, look rules across operands, merging faces), and no format change except for
rings (below).

- **Input:** the picked closed shapes. Open curves picked with them are knives, consumed as
  in Illustrator.
- **The faces** are B0's arrangement of the picked curves only. Unselected lines never cut.
  Only faces inside at least one picked shape count.
- **Output:** for each face, one new curve per half-edge of its outer loop: an exact piece,
  from the edge's spans, through `modify::piece`. The curves get their own points, and the
  face is recorded with `add_shape`.
  - Neighbouring pieces get two coincident curves along the edge they share. A curve is in
    one path only, and snapping never welds.
  - Each piece takes the look (fill, stroke, layer, stack position) of the topmost picked
    shape covering it.
  - The pieces are grouped, as Illustrator does.
  - The originals, their fills and the knives are removed.
  - **Relations on the originals go.** The pieces are new geometry, which is honest for a
    cut-into-parts command. The notice names how many went.
- **Rings** (a face with a hole): until "Make hole / compound path" (T4) exists, the hole
  is a separate piece lying inside, so it shows as a hole by the islands rule.
- **`follow_paths`:** Divide creates paths, so it needs §7.2's `rank` fix in the same PR.
- **Walls:** wall-owned geometry is refused through `Editor::guard_owned`.
- **Commands:**
  - a registry row `modify.divide`, "Divide shapes", in the Modify menu;
  - aliases DIVIDE and PATHFINDER, and the word "break apart";
  - no default key;
  - one undo step.
- **Tests:**
  - the pieces' areas sum to the union's area;
  - no point is shared between pieces;
  - each piece's fill is the topmost covering one;
  - trimming one piece leaves its neighbour whole;
  - undo is one step;
  - a smoke test.

**Size M**, after B0, and after the trim fixes in priority.

---

## 7. The other Booleans: deferred

### 7.1 Why

With §1, the trim fixes, the script verb and Divide, the other four commands (Add,
Subtract, Intersect, Exclude) add little. They would cost a labelling engine, look rules
across operands, compound paths and a new place for the followers to fight over geometry.
They come back only if a real need shows up that trim and Divide can't meet. One example is
many-shape unions from imports, where trimming each overlap by hand is impractical, and the
import path may be the better place for that.

### 7.2 Notes kept from the first draft, for whoever picks this up

- **Engine:** label faces by **parity across edges** (crossing an edge toggles every operand
  with a curve along it), never by point-in-polygon. Exact results come from B0's spans
  through `modify::piece`.
- **Ids:** the longest piece keeps the id, and a curve that survives whole is untouched, as
  Trim does. Relations that still hold stay; unmet ones are dropped and named.
- **`follow_paths` friction:** an open neighbour path sharing a corner point can chain
  through curves created in the transaction and take them from a path created in that
  transaction. The cause is `rank` (`fills.rs:1486`), which gives created curves rank 1.
  **Any command that creates paths needs `rank` to give such curves `None`.** That includes
  Divide (§6). Trim doesn't create paths, so it can't trigger this today.
- **Walls:** refuse wall-owned geometry through `Editor::guard_owned` ("Wall faces follow
  the wall: Explode it first"). walls-design.md §3.4 says "Explode followed by Add" makes a
  one-off union of walls. With Add deferred, that becomes "Explode, then trim". The line is
  updated when walls-design.md is next edited after W1.
- **Keys:**
  - Ctrl+Alt is AltGr on Windows, so no Ctrl+Alt defaults.
  - Shift+M (Illustrator's Shape Builder) is free; plain M is Mirror.
  - Alt on the Fill tool is the eyedropper, so a Shape Builder would be its own tool.

---

## 8. Test plan

| Batch | Tests |
|---|---|
| B0 | The JSON before/after test for lines, arcs and circles. `regions_props.rs`: coincident curved pieces (either way round) as one edge; collapsed handle = tiny handle; self-crossing cubic lobes; the existing properties over drawings with Béziers and ellipses. Unit tests: line ending on a Bézier (written first); Fill tool on a pen shape. `perf_editor_pipeline`, `perf_import_commit`, new `perf_regions_curves`. |
| B1 (3b) | Every `fill_tests.rs` test unchanged. Fingerprint properties (§3.4), including the half-split case. The headless load-then-edit test. Substitution maps from Trim, Split, Fillet and Chamfer, each with a test that an area fill beside the edited curve stays on its face. |
| T1, T2 | `tools/tests.rs`: one undo step per drag (the `DragStart` piece included); skipped pieces reported; picked cutting edges ignore a grid line; the `rest` of a cutting curve still cuts; a picked fill alone means nothing picked; nothing picked = today; Extend to a picked boundary, past an unpicked line. |
| Smoke | `ui_tests/smoke/fills.rs` gains `smoke_trim_two_filled_shapes_into_one`. Two overlapping filled rectangles from the Rectangle tool, both picked, T, one drag across the two inner pieces. One fill on screen, and one Ctrl+Z gives both back. Listed in `docs/regression-checklist.md`'s Fills row. Divide adds `smoke_divide_two_squares` later. |
| S (script) | `script/tests.rs`: union, notch, overlap, a side cut twice, the errors. `agent/tests.rs`: `trim_extend_split` with `against` and with `keep`/`by`. |
| Divide | §6's list. |

---

## 9. Phasing

| Batch | Contents | Size | Depends on |
|---|---|---|---|
| **T: trim fixes** | §4 T1, T2, with Extend. `tools/curve_ops.rs` and `modify.rs` (`cuts_against`, `extend_target`). | **S–M** | none. **Can start now**; it doesn't touch W1's files. |
| **B0: arrangement** | §2. | **M** | 3a (merged). After W1, or rebased onto it (§2.4). |
| **B1: 3b fingerprints** | §3. | **M** | B0. |
| **S: script verb** | §5. | **S–M** | T2. |
| **D: Divide** | §6, with the `rank` fix. | **M** | B0. |
| Filed | "Make hole / compound path" (T4); Extend for ellipse arcs (T5). | — | — |
| Deferred | Add, Subtract, Intersect, Exclude, Shape Builder (§7). | — | a need from the user |

**Order:** T can start now. B0 starts once W1's branch is pushed (cut from it) or rebases on
W1 when it merges. After B0, B1 and D can run in parallel: D touches the editor and the
registry, and B1 touches `fills.rs`. S follows T.

---

## 10. Decisions

These were open questions in the first version, decided by the user after the review:

1. **T2 applies whenever curves are picked.** A selection with no curves counts as nothing
   picked.
2. **A join keeps the topmost fill (T3).** The "most outline" rule isn't adopted.
3. **Hole ownership is filed** as "Make hole / compound path" (T4), not built here.
4. **The verb is `trim`**, with "inside" and "outside" documented as what goes.
5. **This file is `trim-and-regions-plan.md`.** solver-drag-regions-plan.md §3 and
   fills-and-strokes-audit.md §5 step 4 have status lines pointing here.
