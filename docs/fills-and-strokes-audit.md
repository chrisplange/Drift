# Fills, strokes and overlapping geometry: an audit

*2026-10-20, against `main` at b8fb54f (after PR #128).*

The goal: fills and strokes that behave the way a designer expects from Illustrator or
Affinity Designer, without giving up what makes Drift a CAD sketcher (shared edges, hatching
of areas among many lines, constraints).

The short version: **Drift today is permanently in Illustrator's "Live Paint" mode.** Every
fill is a *face* of the planar arrangement of all the curves on the canvas, not a property
of a shape. That is exactly right for hatching a plan drawn as a web of shared wall lines,
and exactly wrong for "draw a blue rectangle over a red circle". Every one of the open fill
bugs (#129, #133, #134, #135) and the whole "Covers 2 areas" apparatus come from code trying
to guess *which object* a fill belongs to from a soup of curves that doesn't record objects.
The fix is not to replace the face model but to add the thing it is missing: a **shape**
(a closed path that owns its fill and its stroke), and to make the renderer stack objects.

---

## 1. What the code does today

### 1.1 Geometry is a sketch of curves, not objects

- An entity is one point or one curve: line, arc, circle, ellipse, cubic Bézier
  (`crates/drift-core/src/shape.rs:19`). A rectangle is four lines and four shared points.
  A pen path is a chain of Béziers and lines that share end points (`tools/pen.rs:112`).
- Nothing records "these five curves are one closed path". `Group` is an unordered set of
  members used for picking and moving (`drift-core/src/groups.rs:18`); it has no order, no
  closed flag, and no appearance.
- Stroke appearance lives on each curve: colour, weight, line type, each "by layer" when
  unset (`Appearance`, `layers.rs:181`). There is no stroke opacity, no alignment
  (centre / inside / outside), no cap or join setting (arcs use round caps in the shader;
  corners of separate lines are whatever the two segments' ends overlap to), and no
  "stroke: none". The nearest thing to no stroke is *construction*, which also means
  "doesn't print" and "doesn't bound areas" (with an exception below).

### 1.2 A fill is a face, found again on every edit

- `Fill` (`drift-core/src/drafting.rs:62`) stores a sorted *set* of boundary curve ids, a
  seed point, style, layer, draw order and the `divided` flag. Holes are "whatever lies
  inside".
- Regions are the faces of the planar arrangement of *every* solid curve in the drawing
  (`Editor::regions`, `editor.rs:1139`, over `drift_geom::regions::regions`). Curves are
  split where they cross; each face is bounded by pieces of possibly many entities.
- `Editor::fill_shapes` (`fills.rs:296`) re-finds every fill each revision through a
  cascade of guesses: a face bounded by exactly its curves → the area among its own curves
  ignoring curves that cross them (`uncrossed_region`) → a face whose outline still has all
  its surviving curves (`successor`) → the face around its seed if it shares a curve → the
  last shape it had, flagged `lost` → the face at the seed.
- `follow_fills` (`fills.rs:371`) runs inside every edit transaction and rewrites the
  fill's boundary set to whatever the cascade found, plus the Divided / Refound / Dropped
  notices and the `fit` / `split` / `keep whole` actions.

This cascade is the heart of the trouble. It is roughly 400 lines of heuristics whose job is
to recover object identity after the fact, and each fix for one case creates the next bug:

| Issue | What the heuristic gets wrong |
|---|---|
| #120 (closed) | A fill grabbed the neighbour's overlapping lines because faces don't know whose lines they are. |
| #133 | Same again on a tie: two shapes sharing half a side, the "fewest cuts" tie-break hands the piece to the wrong shape. |
| #134 | A rectangle drawn twice: deleting one copy drops the fill, because the fill's *set* of ids is gone, though the seed is still enclosed. |
| #135 | A fill whose area is gone jumps to a neighbouring face its leftover lines happen to bound. |
| #129 | Deleting a wall between two filled rooms leaves two fills stacked on the merged face, because both seeds are now in the same face. |

None of these is a bug in the geometry; each is the inevitable result of inferring "which
object" from faces.

### 1.3 Fill and overlap depend on the order you did things in

Because a new fill is bounded by the *face* under the click, the same picture produces
different fills depending on history:

- Draw rectangle A, fill it, then draw rectangle B across it: A's fill stays whole (the
  `uncrossed_region` path, "crossed" flag) and B's lines just sit on top. Good.
- Draw A and B first, then click inside A: the fill is bounded by A's lines *and the pieces
  of B's lines*, and covers only the A-minus-B face. Then click in the lens: a third fill.
  Three fills, nine boundary pieces, none of them "the rectangle".

In Illustrator both cases are two objects stacked; the lens is just where they overlap.

### 1.4 Fills can never cover a stroke

Draw order is fixed as: all fills (by layer, then `order`) → underlay pictures → all curves
(by layer) (`drift-render/src/lib.rs`, `renderer.rs:379`, `editor.rs:2949`). So:

- A white rectangle placed over a drawing does not hide the lines under it. PDF import
  works around this by *trimming* covered lines at import and hiding the pieces on a
  "Covered by fills" layer (`pdf_import.rs:836`). That is a one-time hack, not a model.
- A filled shape drawn over another filled shape always shows the lower shape's outline
  through its fill, whatever its opacity.
- The default solid fill opacity is 0.35 (`drafting.rs:95`), so even "solid" fills look
  like tints and stacked fills blend. That suits a CAD overlay; it is the opposite of a
  design tool's default.
- Within a layer, fills have `order` (bring to front / send to back) but curves have no
  order at all: all strokes on a layer are equal, in entity-id order.

### 1.5 Selection and inspection are split in two

- Clicking inside a fill (not on a curve) picks the *fill* (`select.rs:741`);
  `selected_fills` is a separate set from `selection`. Moving a picked fill moves its
  outline and hole curves (`selected_fill_curves`), which feels close to moving an object.
- But the Inspector for a fill shows style, layer, order and area; the Inspector for
  curves shows stroke appearance and relations. Nowhere do you see one thing with a Stroke
  and a Fill, which is the Affinity / Illustrator mental model (the top bar is the closest:
  it shows a Stroke and a Fill swatch for the selection, `context_ui.rs:270`, but "Fill"
  there only recolours already-picked fills or the layer's fill colour).
- There is no "select a closed outline and choose Fill" (PLAN §5.12 lists it; it only
  exists for a picked *area* with area-picking on).
- Fills are not in `Group`s and aren't copied with Ctrl+C unless their whole outline is
  (`copy_fills_onto`), so "copy this shape" sometimes copies the fill and sometimes not.

### 1.6 What is already right, and worth keeping

- The region finder is good: exact arcs, tolerance handling, holes by containment,
  coincident edges, ellipses and Béziers as fine edges. This is the engine for hatching
  plans and for the boolean operations you don't have yet.
- Associativity: fills move with the solver, survive fillets, chamfers, splits, mirrors,
  patterns, blocks, undo and redo (`fill_tests.rs`). This is better than Illustrator, where
  a fill is just paint on a path.
- `uncrossed_region` is already "the region among this fill's own curves": it is 80 % of a
  shape fill. The code is closer to the right model than it looks.
- Hatches: exact clipping, drafting vs model scale, patterns, handles on the canvas.

---

## 2. The Illustrator / Affinity model, stated precisely

So the target is explicit:

1. **An object is a path with a fill and a stroke.** The path may be open or closed; it may
   be a compound path (several subpaths, holes by even-odd or non-zero rule). Fill and
   stroke each may be none. An open path with a fill is painted as if closed by a chord.
2. **Objects stack.** Each layer is a strict z-order of objects; within an object, the
   stroke draws over its own fill. An object's fill occludes *everything* below it,
   strokes included. Opacity and blend mode are per object.
3. **Overlap is purely visual.** Objects never cut, join or share geometry by lying on each
   other. Changing one never changes another's fill. Two rectangles that "share a wall" have
   two coincident edges.
4. **Combining is explicit.** Pathfinder (Illustrator) or the geometry operations
   (Affinity): Add, Subtract, Intersect, Exclude, Divide, and Shape Builder. They make new
   objects from the faces of the overlap. Live Paint is the opt-in mode where faces are
   painted directly.
5. **Selecting by the fill picks the object**, and moving it moves path, fill and stroke.
   The Appearance / Stroke-and-Fill panel edits both at once.

What Drift must keep that these tools don't have: shared edges between neighbouring areas
(one wall line, two rooms), constraints across shapes, and painting faces among many lines
(BHATCH). So the design is **two kinds of fill, both first class**:

- a **shape fill**: belongs to a closed path, bounded by exactly that path, independent of
  every other curve; this is the Illustrator object;
- an **area fill**: the current face-based hatch, for areas bounded by several shapes'
  lines; this is Live Paint / BHATCH, and stays as it is.

---

## 3. Recommendations, in the order to do them

### 3.1 Stack objects in the renderer (fills can cover strokes)

The visible half of the problem, and independent of the data model.

- Build one draw list per frame ordered by (layer rank, z within layer), where each fill
  is immediately followed by its own outline curves, and loose curves (not the outline of
  any fill) come at the top of their layer. Today `fill_graphics` and `items_styled`
  produce two separate lists; they need to produce interleaved *groups*.
- The renderer draws fills and lines in two pipelines with one pass each
  (`renderer.rs:379`). Interleaving means either alternating pipelines per group (cheap
  with wgpu: set pipeline, draw a range, repeat; hundreds of switches are fine) or a depth
  buffer with a z per group. Alternating per group is simpler and keeps WebGL2 parity.
- Solid fill default opacity: 1.0 for a shape fill; keep 0.35 as the default for area fills
  and hatch tints if you want the CAD look there. Make the hatch background opaque too.
- Give curves a z within their layer so a stroke drawn later can sit above a fill drawn
  later still. The simplest rule that matches Illustrator: every fill and every loose curve
  gets an `order` on its layer; "bring to front / send to back / forward / backward" apply
  to whatever is picked.
- Then the PDF importer's covering-trim becomes optional (keep it behind the existing
  import choice; the stacked renderer already shows the right picture).
- SVG and PDF output must emit the same interleaved order (`output.rs:383` currently
  records fills first). Export already writes fills as even-odd paths, so only ordering
  changes.

Effort: a few days. Risk: low. It fixes "a white shape doesn't cover" and "my fill shows the
lower shape's lines" outright.

### 3.2 Add a `Shape` record: a closed path that owns its fill and stroke

The structural fix. A new document table alongside groups:

```
Shape {
    curves: Vec<EntityId>,   // in order around the loop; each shares an end point with the next
    closed: bool,
    fill: Option<FillId>,    // a Fill whose boundary is exactly these curves
    holes: Vec<Vec<EntityId>>, // optional: a compound path's inner loops
    layer, order,
}
```

- **Creation:** the Rectangle, Polygon, Circle, Ellipse, Slot and Pen tools (on close)
  make a Shape for what they drew. Import (SVG, PDF, AI, DXF polylines and hatch
  boundaries) makes one per path. Existing drawings get none; nothing changes for them.
- **Its fill is found from its own curves only.** `fill_shapes` for a shape fill becomes:
  regions among `shape.curves` (plus explicit holes) → the one face. No cascade. If the
  loop no longer closes (a curve deleted, a point dragged open), the fill is `lost` and is
  drawn closed by a chord from the gap's ends, as Illustrator paints an open path, with the
  existing "Re-pick boundary" offer. Islands inside a shape are *not* holes unless they are
  in `holes` (Illustrator), with a per-fill "exclude islands" switch for CAD use.
- **Edits that keep the loop a loop keep the shape:** a fillet, chamfer or split replaces
  or splits entries in `curves` in place (the editor already knows which curves an edit
  created and replaced: `created_in_transaction`, `reshaped_in_transaction`). This replaces
  `successor` and `reshaped_whole` with a direct substitution, and it is exact rather than
  a guess.
- **A line drawn across a shape does nothing to it.** No "Covers 2 areas", no `divided`
  flag, no preference. Those stay only for area fills.
- **Delete the shape → delete its fill.** Delete one curve of it → lost fill, as above.
  Delete a neighbour's copy of a coincident edge → nothing happens (#134, #135 vanish).
  Overlapping sides (#133) can't grab the neighbour's line because the fill never looks at
  it. Deleting a shared wall between two shapes (#129): both loops open, both fills lost
  and chord-closed; the user merges them with Add (3.4) or re-picks. That is the honest
  answer; the current silent stacking is the worst one.
- **Area fills keep today's code path** but lose most of the cascade: an area fill is a
  face of the arrangement; if the face around its seed changes, it follows the seed; if
  there's none, it's lost. `uncrossed_region` and the divided-fill machinery are what a
  shape fill makes unnecessary, so retire them once shape fills exist instead of
  maintaining both.

Effort: the biggest item, perhaps two weeks including file format, undo journal, blocks,
copy/paste and the MCP tools. It removes more code than it adds in `fills.rs`.

### 3.3 One object in the Inspector: Stroke and Fill together

- Clicking inside a shape's fill picks the *shape*: its curves go in `selection`, its fill
  in `selected_fills`, and both are shown as one thing. Clicking its outline does the same.
  (Area fills keep picking the fill alone.)
- The Inspector and the top bar show, for a shape: **Stroke** (colour, weight, line type,
  None) and **Fill** (None, Solid colour + opacity, Hatch…). Affinity's two swatches.
- "Fill" on a selection of curves that form a closed loop (with or without a Shape record)
  makes a Shape and a fill: the missing "select closed boundary curves and choose Fill".
- **Stroke: None** as a real appearance value on a curve, distinct from construction: it
  doesn't draw or print, but it bounds areas, snaps and solves. That replaces the
  construction-edges-that-bound-fills exception (`fill_edges`, `editor.rs:1208`) and the
  import option "Construction: they shape the fill, and don't print" with the thing those
  are imitating.
- Groups should carry fills: a group's member list includes its shapes' fills, so
  copy, move, mirror and pattern never leave a fill behind.

### 3.4 Explicit combining: Add, Subtract, Intersect, Exclude, Divide

With shapes, overlap is visual, so combining must be a command. The region engine already
computes the faces of any overlap, so these are mostly plumbing:

- **Divide**: split every curve at crossings (the Split tool does one; do all in the
  selection), then make one Shape per face, each with the topmost original's fill. This is
  the Pathfinder Divide and the Shape Builder's raw material.
- **Add / Subtract / Intersect / Exclude**: Divide, then keep the faces that are inside
  the right operands (test each face's interior point against the operand loops), merge
  adjacent kept faces by dropping their shared edges, and build one Shape (with holes) from
  what's left. Constraints on the consumed curves go, as in Illustrator; the result is new
  geometry on the top operand's layer with its appearance.
- A **Shape Builder** style tool (hover a face, click to fill it, drag across faces to
  merge) is the Fill tool's hover highlight plus Divide; it would feel natural to a
  designer and is cheap once Divide exists.

### 3.5 Smaller stroke-model gaps

Worth doing after the above, roughly in this order of visibility:

- **Joins at corners.** A rectangle is four separate lines; at a heavy weight the corners
  show whatever the two round-capped segments overlap to. A shape with an ordered loop can
  be stroked as one polyline with proper mitre/round/bevel joins in the SDF pipeline.
- **Stroke opacity** (per curve or per shape), to match fill opacity.
- **Stroke alignment** (centre / inside / outside) for shapes only; it is meaningless on a
  loose line.
- **Dash patterns in paper units with caps** already exist as line types; a custom dash
  array can wait.

---

## 4. What not to do

- **Don't replace faces with objects.** Hatching a plan drawn as shared wall lines, and
  picking an area among many shapes' lines, are things Illustrator can't do without Live
  Paint and that your users (architects, engineers) need. Keep area fills.
- **Don't add more cases to the follow-fills cascade.** Each of #133, #134, #135 and #129
  can be patched individually, and each patch will make a new one. Fixing them for real is
  3.2.
- **Don't try to get occlusion by trimming geometry** (the PDF-import approach) anywhere
  else. It changes the drawing to fake a rendering order.

---

## 5. Suggested sequence

1. **3.1 stacked rendering** (fills cover strokes; per-layer z; opaque default for solids).
   Visible at once, no format change beyond a `z` on curves.
2. **3.2 Shape records and shape fills**, with the tools creating them and import
   producing them. Close #129, #133, #134, #135 against it. File format bump.
3. **3.3 unified picking and Inspector**, Stroke: None, fills in groups.
4. **3.4 Divide, then the booleans, then a Shape Builder tool.**
5. **3.5 joins, stroke opacity, alignment.**

Each step is shippable on its own and each makes the next one smaller.

**Status (2026-10-22):** 1 and 2 are done (#137, #138). 1's per-layer z for curves came in
batch 2 (Arrange, format 40), with print, PDF, SVG and DXF following the canvas's stack.
3 is done: the Inspector, Stroke: none (a `LineType::None`), and a click inside a shape's
fill picks the shape (outline and fill; copies get their own path). Clicking one side of
the outline still picks that curve alone, and groups don't yet carry area fills for copy
and pattern.

**Status (2026-10-25), step 4:** Trim and `follow_paths` already combine shapes. Step 4 is
replaced by [trim-and-regions-plan.md](trim-and-regions-plan.md):
- the trim fixes and a script verb come first;
- Divide comes next, after the region rework (§6 there);
- Add, Subtract, Intersect, Exclude and Shape Builder are deferred (§7 there).
