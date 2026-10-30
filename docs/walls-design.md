# Walls: design for walls v1 and what follows

*2026-10-23, against `main` at 74c5b2f (file format 41). Revised 2026-10-24 following an
independent review (`D:\Development\Drift\handoff\walls-design-review.md`, outside the repo),
and refreshed 2026-10-25 against `main` at 3914842 (format 42 with the tolerant reader, #166;
the command registry, #167; Selection & move, #164).* Companion to
[fills-and-strokes-audit.md](fills-and-strokes-audit.md) (the shape and fill model this builds
on), [solver-drag-regions-plan.md](solver-drag-regions-plan.md) (§1, "constraints only where
you add them", which walls must respect) and [file-format.md](file-format.md) (the version
policy §2.6 follows).

Interactive walls were the top investment in the October audit. A prototype Wall tool exists
as a patch on a pre-path base (`D:\Development\Drift\handoff\wall-tool-explore-2026-10-16.patch`):
`tools/wall.rs` (the tool) and `walls.rs` (offsets, mitres, caps, `Justify`, `end_on_line`),
with 5 unit tests and 4 tool tests. It draws walls as loose lines held by hidden constraints
and cuts other walls' faces for T joins. This document keeps its geometry and its feel, and
replaces its model.

> **Frozen 2026-10-27.** Walls stop at W3 plus Walls from lines (W1.5): bug fixes only.
> W4 (rooms), W5b (script openings and rooms), W6 (wall types) and W7 (DXF for walls) are
> **cut, not deferred**: they are the path to BIM, which is a non-goal (PLAN.md §1). W3b, W3c
> and W5a are not scheduled. The sections below that describe the cut batches (rooms in §7,
> wall types in §5.3, W5b in §8) are kept as a record of the design, not as plans.

---

## 0. Decisions at a glance

| Question | Decision |
|---|---|
| What is a wall? | A `Wall` record in drift-core: a centreline (a `Path` of construction curves), a thickness, a justification, a poché, and joins at its ends. |
| Are faces real curves? | Yes: generated lines and arcs, **owned by the wall**, regenerated at commit with **stable ids**. Keys are by **station** (what starts a piece), not by count, so inserting a T or an opening renumbers nothing else. |
| How do walls join? | One **node rule** for every join: L, T, X and n-way are nodes where wall ends meet; each corner is the crossing of two neighbouring faces. |
| Per-join outlines or Boolean Add? | **Per-join outlines.** The wall engine re-chains each cluster's outline from its nodes. The chaining uses corner identity, with no tolerance. Ids stay stable. |
| How is poché drawn? | One path-bound `Fill` per outer outline loop of a **cluster** (joined walls of the same poché and layer). There is no face line inside a cluster, so Arrange order never shows one. |
| What does the solver see? | Nothing, unless you add a relation or dimension to a wall. **Generated entities are never solver unknowns.** A relation on a face is **lowered** to the centreline with a signed offset before the scope is worked out (W2). |
| Is thickness a driving dimension? | A **property dimension**: you can show it and type into it, and it writes the wall's thickness (a number or an expression). The solver never solves for it, but sees it as the offset in lowered relations. |
| Openings | An `Opening` record hosted on a wall: a station along it, an opening width, a library family. The symbol is generated at the host's thickness. |
| Rooms | A `Room` record: a name, number and tag point. Its area is the face around the tag among the room-side faces of every room-bounding wall, with openings closed at the face plane. |
| Keys | **W** for the Wall tool (thin lines moves to Shift+Alt+W). **W again** switches between straight and arc, and **click-drag** bends the next segment. **Space** flips the justification. A stays the Arc tool. All of these are bound through the **command registry**, which lands before W1. |
| Tool settings | Thickness (with presets), justify and poché sit on the tool bar and remember themselves across drawings, as the Fill switch does. Named **wall types** in the drawing come later (W6). |
| Other generated geometry | Walls set the protocol any generated geometry follows (offsets as objects, #247, next): a record in drift-core, a generator in drift-geom, a `follow_*` hook in `Editor::commit`, ownership through `Owners`. The checklist is [editor-split-design.md](editor-split-design.md) §6. |

---

## 1. Goals and non-goals

An architect drawing a 2D plan should be able to:

- **Draw walls with a thickness** by clicking a centreline (or either face) point by point,
  typing lengths and angles, and closing back on the start for a room.
- **Join them cleanly.** L corners mitre. A wall ending on another makes a T and the host's
  face opens. Crossing walls make an X. Three or more walls meeting at one point clean up as one
  node. A wall can end against a column, a slab edge or a grid line. Joins re-clean when
  anything moves.
- **Draw curved walls:** arcs through a third point, or bent by dragging, mixed with straight
  runs and joined into them.
- **Fill walls with poché:** solid or hatch, as one area per cluster of joined walls, with no
  lines drawn between walls of the same material.
- **Place openings:** doors and windows hosted in a wall. They cut it, carry a swing, come from
  the library in US sizes by default, with metric and national size sets (DIN 18101, BS 4787)
  as options, and slide along the wall.
- **Tag rooms** with a name, a number and a live area that follows the walls.
- **Stay parametric only where wanted:** dimension a wall's face to a grid, set a thickness by
  a variable, and leave everything else free.
- **Print and export** what is on screen: print, PDF, SVG and DXF.
- **Turn an imported plan's wall lines into walls** (pairs of parallel lines, broken at every
  opening, as AutoCAD drawings have them). This is batch W1.5, right after v1.

**Non-goals.** 3D, wall heights, BIM data (materials as data, fire ratings, schedules beyond a
room list) and IFC are out of scope. So are multi-layer compound walls (finish, insulation and
structure as separate bands), stairs and columns as a type. **Curtain walls:** v1's answer is
a thin wall (say 50 mm) with a hatch of a mullion pattern. **Columns:** draw them as shapes.
Where a column sits in a wall, give its sides inside the wall Stroke: none by hand, or wall a
closed rectangle with *Walls from curves* (a four-segment ring). Merging a column into a
wall's poché automatically needs type priority (W6). Some of these are later batches (§10).
None of them should be made harder by v1.

---

## 2. Data model

### 2.1 Records (drift-core)

New module `crates/drift-core/src/walls.rs`, with new tables in `Document`: walls (W1), property
dimensions (W1), openings (W3) and rooms (W4). Each is held in `Shared<BTreeMap<…>>` like the
other parts (cheap copies for Options, #160), and journalled like paths (`Key::Wall`,
`Key::PropertyDim`, `Key::Opening`, `Key::Room`, with matching `Record` variants).

```rust
/// Stable identifier of a wall, "w3".
pub struct WallId(pub u32);

pub struct Wall {
    /// The centreline: construction curves in order (lines and arcs), open or closed.
    /// A closed path is a ring of walls (a room drawn in one go).
    pub centre: PathId,
    /// Thickness in mm, and the expression it comes from, if any ("t_ext", "W / 20").
    /// Evaluated with the dimensions' expressions, in `apply_parameters`, before the solve.
    pub thickness: f64,
    pub thickness_expr: Option<String>,
    /// Where the wall lies against its centreline, looking along it.
    pub justify: Justify,           // Centre | Left | Right (the prototype's enum, moved here)
    /// None: faces only, no fill.
    pub poche: Option<FillStyle>,
    pub layer: LayerId,
    /// Where the wall's cluster fill stacks on its layer (as `Fill::order`). Faces don't
    /// carry an order: they draw with their fill.
    pub order: Option<i32>,
    /// The start and end of an open centreline (unused when closed).
    pub ends: [WallEnd; 2],
    /// Clean up with walls it crosses mid-segment (X joins). Default true.
    pub cross: bool,
    /// What the wall made, by key: see §2.3.
    pub made: Made,
}

pub struct WallEnd {
    /// Clean up with walls that meet this end (default true; "Don't join" in the Inspector).
    pub join: bool,
    /// This end sits on another wall's centreline (a T): which wall and which of its
    /// segments. An L or n-way join needs no record: the walls share the centreline point.
    pub tee: Option<(WallId, u16)>,
    /// This end stops against a curve that isn't a wall (a column, a slab edge, a grid
    /// line): both faces end where they cross it (the prototype's `end_on_line`).
    pub against: Option<EntityId>,
}

pub struct OpeningId(pub u32);      // "o2"
pub struct Opening { /* §6 */ }
pub struct RoomId(pub u32);         // "r5"
pub struct Room { /* §7 */ }
```

`FillStyle` (kind, colour, opacity, hatch weight) moves from `drift-app/src/fills.rs:99` to
drift-core, where its four field types already are. It moves rather than being duplicated as a
`Poche` type. The app keeps using it for the Fill tool.

**Why a run and not one record per straight segment**, as Revit has: the tool draws runs, a
closed room is one ring, and the prototype, the script (`room … walls 200`) and Path records
all think in runs. A wall's *segment* (its centreline curve `k`) is still addressable wherever
it matters: in face keys, T hosts and openings. A click picks the run, and the Inspector lists
each segment's length. **Split wall** (at a point) and **Join walls** (two runs that share an
end) convert between runs and pieces. When segments renumber, they re-key faces by mapping old
segments to new ones (§2.3).

**Every place that lists the tables by hand** must learn the new ones, or walls will be
silently left behind. This checklist is part of W1:

| Where | What to add |
|---|---|
| `Key`, `Record`, `get_record`, `Transaction::apply` (`document.rs`, the `Key`/`Record` matches) | the new tables (`walls`, `openings`, `rooms`, `property_dims`) |
| `Document::keys()` (`document.rs:2359`) | them too. It feeds `restore_parts` (whole-sketch Options) and `same_sketch`, and a table missing here doesn't restore with an Option. |
| `sketch_copy` | nothing (it clones the whole document), but check with a test |
| drift-io writer and reader | the `walls`, `openings` and `rooms` sections (§2.6) |
| `Document::delete` (`document.rs:1577`) | cascades: deleting a wall's last centreline curve deletes the wall, its generated entities and its openings. Deleting a host wall deletes its openings and clears tees on other walls. |
| `clipboard::copy_into` (copy, paste, pasting fragments) | copies wall records whose centreline was copied, remaps `WallId`, `PathId` and every id in `made` (or drops `made` and regenerates on paste) |
| `blocks.rs` `make_block` | walls in a selection are exploded into the block: faces and poché go in, centrelines are dropped |
| groups (`Group.members`) | `follow_walls` keeps membership: new generated entities join their wall's groups, and deleted ones leave them |

### 2.2 The centreline is ordinary geometry

The centreline curves are normal entities: lines and arcs with shared points, in a Path record
(`Path { curves, closed }`) marked as owned by the wall. They are drawn as construction (never
printed, never bounding areas). They are hidden unless the wall is picked or hovered, the Wall
or Dimension tool is active (then they show faintly, because those tools snap to them), or
**View → Wall centrelines** is on. The Dimension tool snaps to them even when they are hidden.

This one choice gives walls everything the sketch already has: snapping, dragging, the
free-geometry path (plan §1), relations and dimensions, Options, copy and paste, undo. When
Justify is Left or Right the centreline *is* a face line, so it is the line you clicked.
"Centreline" is the record's word for the reference line, whatever the justification. A typed
or read-out length is always the centreline's: the axis for Centre, the clicked face for Left
or Right.

### 2.3 Faces are generated, owned and keyed

A wall's faces, caps, jambs and the corner points between them are **real entities** (lines,
arcs and points) generated from the centreline, thickness, justification, joins and openings,
and owned by the wall. They are real entities, not drawing-time decoration, so that rendering,
stacking, picking, snapping, temporary dimensions, output (print, PDF, SVG, DXF), area fills
and the dimension tool all work on walls without wall-specific code.

What the generator makes is keyed, so regenerating **updates entities in place** instead of
deleting and re-adding them:

```rust
pub struct Made {
    pub curves: BTreeMap<FaceKey, EntityId>,
    pub points: BTreeMap<CornerKey, EntityId>,
    /// The outline loops and poché fills of the clusters this wall leads (§3.5).
    pub outlines: BTreeMap<LoopKey, PathId>,
    pub fills: BTreeMap<LoopKey, FillId>,
}

pub struct FaceKey {
    pub seg: u16,             // centreline segment
    pub part: Part,           // Left | Right | Cap(End) | Jamb(OpeningId, u8) | Butt(End) | Step(End) | Bevel(End)
    pub from: Station,        // what starts this piece along the segment
}

/// What starts a face piece, so pieces keep their keys when others are added before them.
pub enum Station {
    SegStart,                 // the segment's start (its node corner, or cap)
    Tee(WallId),              // the far corner of a T where that wall meets this face
    Cross(WallId, u16),       // the far corner of an X with that wall's segment
    Jamb(OpeningId, u8),      // an opening's far jamb
}

pub struct CornerKey { pub node: NodeKey, pub between: (EndKey, EndKey) }
```

**Key stability.** The rules:
- **Same key, same entity.** It keeps its id, and its geometry is updated (`set_entity_kind`,
  point moves). A new key gets a new entity, and a key that has gone deletes its entity.
- **Keys don't shift.** A piece is named by the station that starts it, so adding a T branch
  or an opening part-way along a face changes only the piece it lands in. The pieces after it
  keep their keys. Moving a T branch past another T keeps both keys, because each piece is
  named after the wall that starts it.
- **Split and Join re-key.** Split wall and Join walls (and inserting a point into a
  centreline) renumber `seg`. They re-key by mapping old segments to new ones. Relations on
  faces of a split segment follow the half they lie on, and are dropped with a notice where
  that's ambiguous.
- **Who owns a node's corners.** A node's corner points belong to the **lowest `WallId` at
  the node**. If that wall is deleted, the next lowest takes them over. Its points get new ids
  then, but the neighbours' face entities keep theirs.
- **Unchanged keys aren't written.** A key whose geometry is unchanged (bitwise) isn't touched.
  Otherwise every commit on a big cluster would journal thousands of unchanged records (§9.4).

Stable keys matter for more than dimensions. An area fill clicked inside a room (the Fill tool)
holds the inner face ids in its `boundary`, and rooms and relations depend on them too. W1's
property tests check that ids stay stable (§9.2), even though relations on faces only arrive in
W2.

**Who owns what.** `Document::owner(EntityId) -> Option<Owner>` (`Owner::Wall(WallId)`,
`Owner::Opening(OpeningId)`, or `Owner::Centre(WallId)` for centreline curves and points) is
an index rebuilt from the `made` maps and centreline paths. Every modify tool calls
**`Editor::guard_owned(ids) -> Result<Redirect, Notice>`** before acting:

| Operation | Centreline curve or point | Face piece | Corner point | Poché fill |
|---|---|---|---|---|
| Move, drag, grab | allowed: it is the wall's geometry | **redirect**: offset that segment's centreline sideways | **redirect**: move the node's centreline point | **redirect**: move the wall |
| Rotate, Scale (`transform.rs`), Mirror (`modify.rs`), arrays and patterns | allowed. Copies of a wall are walls (the record is copied, and regeneration makes faces). | redirect to the wall | redirect to the wall | redirect to the wall |
| Trim, Extend, Split | allowed on the centreline (the wall follows) | **refused**: "Wall faces follow the wall: trim its centreline, or Explode it" | refused | refused |
| Fillet, Chamfer | **allowed at a centreline corner**: it makes an arc or a short segment in the centreline path, which is the natural way to round a wall | refused | refused | refused |
| Delete | deletes the wall if it was the last curve; otherwise the run opens or splits | deletes the wall (asking first if it hosts openings) | deletes the wall | deletes the wall |
| Offset | allowed (new, unowned curves) | allowed (new, unowned curves) | — | — |
| Layer, construction | the wall's: changing it on any part changes the wall's layer. A wall moved to another layer gets the notice "its poché is now separate from walls on LAYER". | the wall's | the wall's | the wall's |
| Colour, weight, line type override | kept on that piece (survives keyed regeneration) | kept | — | the wall's poché style |
| Walls from curves | refused ("already a wall") | refused | — | — |
| Box selection | picks the wall | picks the wall | **never box-picked** (not movable on their own) | picks the wall |

**Explode** (right-click, Inspector) turns a wall into plain lines, arcs and a shape fill: the
record goes, and the entities stay as unowned geometry. It exists for one-off edits the wall
model can't express.

### 2.4 Regenerated at commit, as `follow_paths` is

`Editor::commit` (`editor.rs:572`) runs: solve → `refresh_driven` → … → `follow_paths` →
`follow_fills` → `carry_hatches` → `doc.commit`. Walls slot in **after the solve and before
`follow_paths`**:

```rust
// apply_parameters (before the solve) now also re-evaluates thickness_expr, opening offsets
// and widths, so lowered relations (§4.2) see the new offsets in the same solve.
let report = self.solve(&[]);
self.refresh_driven();
self.adopt_into_open_group();
self.make_new_curves_construction();
self.follow_walls();      // new: crates/drift-app/src/walls/regen.rs
self.follow_paths();      // skips paths owned by walls
self.follow_fills();      // skips fills owned by walls
self.carry_hatches();     // a cluster fill's `boundary` is its outline (see §3.5)
```

`Editor::follow_walls`:

1. **Dirty set.** These walls are dirty: walls whose centreline curves were created, reshaped
   or moved in the transaction (`created_in_transaction`, `reshaped_in_transaction`, moved
   points), walls whose record changed, walls whose `against` curve moved, and the walls joined
   to any of them (one hop through nodes, because a node's corners depend on every wall at it).
2. **Reattach.** Free tee ends are re-projected onto their host's centreline, and free
   `against` ends onto their curve (§3.3 rule 4).
3. **Rebuild nodes** for the dirty walls (§3.2), and the face pieces between their stations.
4. **Diff by key** against `made`: update changed entities, add new ones, delete gone ones.
   Unchanged keys aren't touched.
5. **Re-chain the outlines** of every cluster that holds a dirty wall (§3.4), and update their
   outline Paths and poché Fills (keyed too, §3.5).

All of this happens inside the open transaction, so the edit and its regeneration are one undo
step. Undo and redo restore the exact ids. Because the generator is a pure function of the
records and the centreline geometry, regeneration is idempotent, and the tests in §9 rely on
that. Entities that `follow_walls` moves are excluded from the far-off check
(`self.far_off_of(tx.entities())`, `editor.rs:594`): a neighbour's face moving with its node
isn't a surprise.

**Load.** Loading doesn't go through `commit`. `Editor::new` (`editor.rs:445`) runs
`apply_parameters`, `solve` and `doc.commit("Load")` directly. W1 adds `follow_walls` (over every
wall) to that Load transaction and compares the document before and after. A difference is
reported as "This drawing's walls were repaired on opening". There should be none, because the
file stores the generated entities.

**During a drag** nothing is committed. The canvas draws the dirty walls' faces from the same
geometry function (`drift_geom::walls::faces`) and hides their stored entities for the drag, as
the pen tool previews. Only the dragged walls and their node neighbours are recomputed per
frame. The stored faces of dirty walls are stale meanwhile, so **snapping leaves them out**
during the drag. The spatial index (`index.update_moved`, `editor/drag.rs`) would otherwise
snap to where a face *was*.

### 2.5 How walls relate to Path records and shape fills

- The **centreline** is a Path (open or closed) of construction curves.
- Each **outline loop** of a cluster (§3.4) is a Path of generated face curves, closed. A
  cluster with a room inside it has an outer loop and an inner loop. An opening that cuts a
  wall through splits a loop into two.
- The **poché** is a **path-bound `Fill`** (`fill.path = Some(outer_loop)`) in the wall's
  `FillStyle`, with `boundary` set to the loop's curves (as `carry_hatches` expects). It is a
  shape fill in the audit's sense: it stacks, prints like the screen, shows in the Inspector,
  and Arrange moves it.
- **How its region is found** differs from a drawn shape. `path_region` (`fills.rs:1356`)
  takes the curves that lie inside a path and don't cross it as holes. For a building-sized
  outer loop, `regions_among` (`fills.rs:1170`) would then test every piece of furniture inside
  the building's bounds against every outline curve, on every commit. So `fill_shapes` asks
  the wall engine first. For a wall-owned outline, the region is the outer loop with the
  cluster's inner loops as holes, known exactly from the geometry, with no arrangement. A
  column drawn inside a wall is never a hole by accident.
- `follow_paths` and `follow_fills` **skip** wall-owned paths and fills, so walls keep their own
  paths. A wall face never joins a drawn shape's path through a shared end point (the bug in
  §3.1).
- **Wall fills are exempt from the "a fill goes with its whole outline" rules.** Today a
  path-bound fill is treated as the shape of its whole outline (`Editor::shape_outline`,
  `fills.rs:987`; `sync_group_fills`; clipboard copies fills "whose whole outline was copied").
  A cluster's fill spans several walls, so those rules would pick, move, copy or delete the
  whole cluster when one wall is picked. Instead, picking a wall picks its centreline, its
  faces and, for the Inspector and Arrange only, its cluster's fill. Copy, delete and move
  act on the wall record, and regeneration remakes the fill.

**Picking.**
- **Order:** group, then wall, then entity. A click on a wall inside a closed group picks the
  group. Inside an entered group, it picks the wall, never a bare face.
- **Inside a merged poché,** at a node there is no boundary between walls, so the click picks
  the wall whose centreline is nearest the pointer. Ties go to the cluster's lead wall.
- **Pick filters** (`PickFilter`, #164): walls are picked under **Edges** (their faces) and
  **Fills** (their poché). v1 adds no new kind. Generated corner points are never picked under
  Points.

### 2.6 File format and migration

`docs/file-format.md` is the policy. Since #166 (format 42, `reader` 42), builds pass over
tables and fields they don't know and keep them when saving, so **an addition bumps nothing**
(rule 2). It needs a `vNN.M.drift` fixture and a line in file-format.md §3's "Additions without
a bump" table. Walls are designed to be additions:

- **New optional tables:** `walls`, `openings`, `rooms` and `property_dims` (§4.3), each
  `#[serde(default, skip_serializing_if = …)]`. A drawing without walls writes exactly what it
  wrote before.
- **New optional field:** `rules.size_set` (§6), unset by default.
- **No new kinds in existing tables.** That is rule 4's line between an addition and a breaking
  change: a new entity `type`, relation or annotation kind would make older builds refuse the
  file. Walls stay on the right side of it:
  - there is no new field on `Entity`, `Path` or `Fill`, because ownership is derived from the
    `made` maps;
  - faces are ordinary lines and arcs, and pochés are ordinary path-bound fills;
  - the property dimensions get their own table (§4.3) instead of a new annotation kind.
- **No `FORMAT_VERSION` or `READER_VERSION` bump, and no migration.** Each batch that adds a
  table writes its own fixture: W1 `v42.1.drift` (walls), W3 `v42.2` (openings, size set), W4
  `v42.3` (rooms), W6 the next (wall types). To make one, extend `fixture_drawing()` in
  `crates/drift-io/tests/format_fixtures.rs` with what the batch adds (W1: an L, a T, an X, a
  curved wall and a thickness dimension) and run `write_current_fixture`. Old fixtures are left
  as they are. Declaring every table up front in W1, which the review suggested to save bumps,
  is no longer needed.
- **Surviving an older build's edits (rule 3).** Here is how walls meet it. An older 42 build
  opens a drawing with walls and keeps the `walls` table, but it sees the faces as plain lines.
  So it can move, trim or delete them, delete a centreline, or delete a whole wall's geometry.
  The newer reader must repair what no longer fits, and **the Load check is that repair**
  (`follow_walls` over every wall in `Editor::new`'s Load transaction, §2.4):
  - a `made` id that is missing, or no longer a line or arc, is regenerated;
  - faces an older build moved or trimmed are regenerated from the centreline, and the notice
    says "Walls were repaired: N faces edited outside the wall were rebuilt";
  - a wall whose centreline path is gone is dropped, and its faces are left as plain geometry,
    so nothing visible is lost;
  - an opening on a missing wall, a tee or against pointing at something gone, and a room or
    property dimension whose target is gone are dropped or cleared;
  - a `made` entry whose id now belongs to an unrelated curve (ids aren't reused, so this can
    only come from a hand-edited file) is refused by the reader (rule 8).

  A property test edits a walls drawing as an older build would: it loads, strips the wall
  tables' meaning, applies random plain edits to the entities, then reattaches the carried
  tables. Opening the result must always give a valid drawing.
- **Writing.** Generated entities, outline paths and poché fills are written as ordinary
  `entities`, `paths` and `fills`, so a file opens without regenerating. The `walls` section is
  written **after** the entities it refers to. `made` maps are written as
  `{"L0@start": "e40", "R0@tee:w3": "e41", …}`, with key strings defined in drift-io.
- **Reading** validates every id in `made`, every `centre` path, every `tee`, `against` and
  opening host. It refuses what can't come from Drift with a message (rule 8: "readers
  check"), and leaves what an older build could have caused to the Load repair above.
- Walls drawn by the script before W5a are plain lines and stay so. W1.5's **Make walls from
  lines** can convert them, as it converts imported plans.

---

## 3. Joins

### 3.1 Why the prototype's T join can't stay

The prototype ends a wall on another wall's face by **reshaping the host's face**
(`set_entity_kind` to end at the first jamb) and **adding a new line** for the rest. Under the
current model that breaks in three ways:

1. **`follow_paths` swallows the new wall.** The host face is "reshaped in the transaction".
   The new wall's faces are "created in the transaction" and meet the reshaped face's new ends.
   `follow_paths` treats the host path as touched and chains through the created curves
   (rank 1), so the host's path re-closes *around the new wall*. It takes curves that belong
   to the new wall (a curve is in at most one path) and turns the host's poché into a T that
   nothing records.
2. **The join isn't stored anywhere.** Move the branch wall 300 mm along and the gap it cut
   stays where it was. Delete the branch and the host stays open: its fill is lost and
   chord-closed, and nothing can heal it, because only the geometry knew a join was there.
3. **The host's faces aren't the host's.** Regenerating the host from its own data would undo
   the cut. Not regenerating means the host can never re-thicken, re-justify or move.

The cure is the one the fills model took for shapes: record the object (the wall and its
joins) and derive the curves from it. Never infer the object from the curves.

### 3.2 One rule: nodes

Every join is a **node**: a point where wall *ends* meet.

| Join | How the node arises | Ends at the node |
|---|---|---|
| L, Y, n-way | Two or more centrelines share an end point (the same point id, which is topology, as shapes share points). | each wall's real end |
| T | `WallEnd::tee = Some(host, seg)`: the branch's end is on the host's centreline. | the branch's real end, plus the host split into two **virtual ends** at that station |
| X | Two centrelines in the same cluster cross mid-segment, both walls have `cross` on, and the crossing is found geometrically. | four virtual ends |
| Against | `WallEnd::against = Some(curve)` | one: both faces end on the curve, and there is no cap |
| Free end | No other end there | one: a **cap** |

For each end `i` at node `N`, take its outward direction `dᵢ` (the centreline's tangent
pointing away from `N`; for an arc, the tangent at `N`) and its two faces: `ccwᵢ`, on the
counter-clockwise side of `dᵢ`, and `cwᵢ`. Sort the ends by the angle of `dᵢ`. Then:

> **The corner between consecutive ends `i` and `j` (counter-clockwise from `i`) is where
> `ccwᵢ` crosses `cwⱼ`, nearest `N`.** `ccwᵢ` ends there, and so does `cwⱼ`.

```
   L (2 ends)                     T (branch + host's two virtual ends)

        B │   │                          B │   │
          │   │                            │   │
   ───────┘c₁ │                  ──────────┘c₁ └c₂──────────   host's near face, split at c₁, c₂
   A    N     │                  H₋        N         H₊
   ───────────┘c₂                ───────────────────────────   far face: H₋ and H₊ are opposite
                                                               (parallel faces), so no corner
```

One function, `drift_geom::walls::node(ends) -> Vec<Corner>`, covers every case:

- **L.** Two ends make two corners: the inside one and the outside mitre. Unequal thicknesses
  give the correct oblique mitre for free.
- **T.** The host's virtual ends are opposite, so their faces are parallel. On the far side
  the host face just continues: no corner, no split. On the near side, the two corners are
  where the branch's faces meet the host's face. The host's face is split *in the derived
  geometry* (into `FaceKey` pieces), not by editing anyone's curves.
- **X.** Four corners, and all four faces split.
- **Y and star joins.** As many corners as ends, with no special case.
- **Straight on.** Two consecutive ends count as straight on when `|angle − π| < 1e-6` rad.
  If their offsets match, the face passes straight through with no corner, and the run is
  continuous. If the offsets differ (a thickness change), each face ends on the normal
  through `N`, and a short **step** edge joins them. In an n-way node, exactly opposite
  directions are treated the same way. For example, a wall running straight through a node
  that also has a branch is a T expressed as three real ends.
- **Same direction.** Two ends leaving `N` in exactly the same direction (|angle| < 1e-6 rad)
  overlap. They don't join, and the notice offers to merge them (rule 5 below).
- **Tangent-continuous ends** (an arc running smoothly into a line or another arc, the most
  common curved join). The two faces touch at a double root, which round-off can turn into
  "no crossing". If `|dᵢ × dⱼ| < 1e-9` and the ends are opposite, the corner is `N` plus the
  normal times the offset, with no intersection needed.
- **Arcs** otherwise: faces are concentric arcs, and crossings are line–circle or
  circle–circle (the prototype's `mitre`), nearest `N`.
- **Mitre limit.** If a corner lies more than `MITRE_LIMIT × max(tᵢ, tⱼ)` from `N` (4, as SVG's
  default), the spike is cut square to the bisector at that distance. That makes a **bevel**:
  two corners and a bevel edge. A very sharp fork (two walls leaving within a degree or so) is
  capped at the limit, and the notice reports it.
- **Segments shorter than their thickness** (a 50 mm jog in a 200 mm wall). The corners from
  its start node and its end node cross on the inside face, so the inside piece would have
  negative length. When a piece's stations come out of order, the piece is dropped, and the
  neighbouring faces join at their own crossing (one corner instead of two), with a notice.
- **Free end.** A **cap** across the end, square to the centreline (angled caps: W6).
- **Against a curve.** Both faces end where they cross the curve (line, arc, circle; nearest
  `N`). The end edge lies along the curve and is drawn with **Stroke: none**, because the
  curve itself is the line. If a face misses the curve, the end is capped.
- **Too tight or no crossing.** If an arc's inner face radius `r − o` is below the snap
  tolerance, or faces don't meet at all, the end is capped. The notice says which wall and why.
  A wall is never dropped silently.

### 3.3 Cleanup rules

1. **What joins.** Ends join when the centrelines share the point (L, n-way), or when `tee` or
   `against` is set. An X forms between crossing walls of the same cluster key when both have
   `cross` on. An end with `join = false` stays capped and overlaps whatever it meets, and
   Arrange decides the look. `cross` is per wall, because a mid-segment crossing has no end to
   hang "Don't join" on.
2. **Cluster key** = (poché style, layer). Walls of the same material on the same layer merge
   into one area, with no lines between them. Walls of the same material on different layers
   (existing vs new, demolition) don't merge. The line between them is drawn, and that's
   deliberate.
3. **Different materials at a node** (keys differ): the thickest wall runs through. Ties go to
   the wall drawn first (lower `WallId`), and W6's type priority replaces this rule. The node
   is computed for the through wall as if the others weren't there. The others **butt**: their
   faces end where they cross the through wall's near face, and their end edge (`Part::Butt`)
   lies along it with **Stroke: none**. The through wall's face already draws that line, so
   it is never drawn twice and the DXF has no duplicates. If two different materials meet at
   an L with neither running through, they mitre, and the mitre edge is a drawn material line.
4. **Tees and against ends follow without the solver.** If the host or curve moves,
   `follow_walls` re-projects a free end onto it: the nearest point on the host's segment
   `seg` (or on the curve), clamped to it. That is topology-like behaviour, as shared points
   are, not a relation. If the end is in a constrained scope, the lowered relations handle it
   (§4.2). If the host is deleted, or the end is pulled clear of it (further than the host's
   half thickness plus the snap tolerance), `tee` or `against` is cleared and the end caps.
5. **A wall drawn along another** (overlapping and parallel) doesn't join, and the notice
   offers to merge them into one run.
6. **Welding.** Joins need shared point ids. A pasted, imported or scripted wall whose end
   only *coincides* with another's end is welded when it enters the drawing: ends within the
   drawing's gap tolerance (`gaptol`, a drawing setting since plan A) share one point.
7. **Determinism.** Nodes are processed in key order (`WallId`, segment, end). Angles are
   sorted with `total_cmp`, and exact ties break by `WallId`.

### 3.4 Outlines per join, not Boolean Add

There are two ways to get one area out of several walls:

- **(a) Per-join outlines.** The wall engine knows every corner exactly (§3.2). Each face is
  cut into pieces between consecutive *stations*: node corners, T corners, X corners and
  opening jambs. End edges at nodes are never generated, because they would lie inside the
  merged area. A cluster's outline is then just its pieces, caps, jambs, steps, bevels and butt
  edges, **chained by shared corner points**. Each corner is computed once per node and used by
  both sides, so the chaining works by identity, not tolerance.
- **(b) Boolean Add** (fills audit 3.4). Make each wall a plain rectangle or ring and union
  them with the region engine: Divide, keep the faces inside any operand, drop the shared
  edges.

**Decision: (a).** The reasons:

- **Architectural rules aren't unions.** A union can't express "the thicker wall runs through
  and the thinner butts with a material line", a mitre limit, a step at a thickness change, or
  "don't join this end". These are per-node rules, and (a) is where they live.
- **Stable ids.** Add makes new curves from faces every time, so a dimension or a room's area
  fill on a wall face would be lost on every edit. In (a) every piece has a `FaceKey`.
- **Exactness.** Chaining has no tolerance at all. Tolerance appears in only two places, both in
  the snap tolerance's terms: finding X crossings (a bounds-indexed pairwise test over the
  dirty walls' segments against their cluster), and releasing a tee or against end (§3.3). The
  regions work (plan §3b, face fingerprints) is still open, and walls shouldn't wait for it
  or stress it.
- **Cost is local.** An edit recomputes its walls' nodes (O(ends)) and re-chains the touched
  clusters (O(pieces in the cluster), microseconds even for a whole floor). There is no
  arrangement of everything nearby.
- **Add is a one-shot, destructive command** in the audit's design: it consumes its operands
  and their relations. Walls must stay live.

Boolean Add stays the right tool for drawn shapes, and walls never call it. Once it exists,
**Explode** followed by Add is the way to make a one-off union of walls.

### 3.5 Poché stacking: no face line between walls

- **Within a cluster there is no line to hide.** Shared end edges are never generated. The
  cluster has one outline per loop and one fill per outer loop, drawn as one draw group by
  `stack.rs` (fill, then its outline). The faces carry no `order` of their own
  (`Entity::order = None`): they draw with their fill. Arrange order within a cluster is
  meaningless and isn't consulted.
- **Clusters of different materials** abut exactly along a butt edge that is drawn once (by
  the through wall). Neither fill overlaps the other, so their relative order changes nothing.
  The drawn line also hides the anti-aliasing seam where two fills meet.
- **Unjoined overlaps** (`join = false`, different layers, or a wall over a picture) stack
  normally. A new wall goes on top of its layer, its poché covers what is beneath (strokes
  included, #137), and Arrange (Ctrl+] / Ctrl+[) changes it.
- **Arrange on a wall arranges its cluster.** The cluster fill's `order` is the highest
  `order` among its walls, and `Editor::arrange` (`stack.rs`) redirects a picked wall to its
  cluster fill. The Inspector says "Arranges the walls joined to it too".
- **Fill keys.** A cluster's outline paths and fills are recorded in the `made` of the
  cluster's **lead** wall (its lowest `WallId`), keyed by `LoopKey`. The outer loop holding the
  lead's first piece is loop 0, and the others are ordered by their lowest `FaceKey`. If the
  lead wall is deleted or leaves the cluster, the paths and fills move to the next lead's
  `made` **with their ids**. The user's overrides on the fill (a hatch angle, origin or scale
  set with #158's handles) survive.
- **Hatch alignment.** `carry_hatches` moves a hatch with its outline only when the whole
  outline moves as one. So a hatched poché keeps its pattern fixed in the world when one wall
  moves (as Revit's model patterns do), and follows when the whole plan moves. Two abutting
  clusters with the same hatch (on different layers) have separate origins, so the pattern
  jumps at the butt line. v1 accepts that; in W6 a wall type shares one origin.

---

## 4. Constraints

### 4.1 Constraints only where you add them

Plan §1: geometry with no relations never goes through the solver. It is implemented as
`drift_solver::needs_solve` (`drift-solver/src/lib.rs`). Walls follow it:

- **A new wall adds no relations.** The prototype's hidden `Parallel` + `PointLineDistance`
  pairs and its automatic Horizontal/Vertical go. Faces are parallel because they are
  *generated* parallel, not because the solver holds them. With **auto relations** on, the
  tool adds Horizontal/Vertical to *centreline* segments drawn level or plumb, exactly as the
  Line tool does, and no more.
- **Free straight walls drag directly.** A run of straight centreline segments with no
  relations is free geometry: `needs_solve` is false, `drag_solves` counts 0, and the wall moves
  exactly with the pointer while `follow_walls` regenerates. **Curved walls solve today,**
  because `needs_solve` counts any arc as "shaped" (an arc's ends must stay on its circle).
  That is acceptable for v1. Moving a whole arc rigidly (its three points by the same delta)
  keeps it an arc, so plan §1 could let that case through; that is a solver-plan follow-up,
  not a wall task.
- **Joins are topology, not relations.** L and n-way joins share a point. Tee and against ends
  re-project onto their host (§3.3 rule 4). Neither creates a constraint.

So a quickly sketched plan stays a drawing, and a plan you dimension becomes parametric where
you dimensioned it.

### 4.2 What the solver sees: lowering, never generated entities

**Rule: generated entities (faces, corners, caps, jambs) are never solver unknowns.**
`drift_solver::connected` closes a scope under shared points, and corner points are shared
between the faces of different walls. If faces were in the system, one dimension between two
faces would pull in their corners, the neighbours' faces ending at those corners, and so on
round the cluster. One "3.60 clear" would make the whole building the scope, which breaks
"nothing far away moves" and the drag properties (`move_keeps_untouched_lengths`,
`small_drag_is_continuous`).

So a relation stays **stored on the face** (by its id: the UI draws it there, and stable keys
keep it attached), but the solver sees it **lowered** onto the centreline. W2 adds a lowering
pass, `walls::lower(doc) -> Lowering`, which maps each affected `ConstraintId` to a constraint
over centreline entities with a signed offset. It runs before `connected`, `needs_solve` and
`System::build_in`, and all three read constraints through it.

| Relation on a face | Lowered to |
|---|---|
| Parallel, perpendicular, horizontal, vertical, angle | the same relation on the face's centreline segment (a face is parallel to its segment) |
| Collinear with a line | parallel to it + a signed offset distance |
| Distance from a point or point-like to a face line | signed distance from the point to the centreline segment, ± `o` |
| Distance between two parallel faces (of two walls, or a wall and a line) | signed distance between the centreline segments, with both offsets |
| Point on a face | signed offset distance `o` from the centreline segment (for an arc face: on the concentric circle at `r ± o`) |
| Radius or diameter of an arc face | the centreline arc's, ± `o` |
| Anything on a **face corner** (a node corner, cap end, jamb point) | **refused**: "A wall's corner follows its joins: dimension the face or the centreline". Driven (reference) dimensions and temporary dimensions to corners are fine. |
| A tee or against end in a constrained scope | point-on-curve: the branch's centreline end on the host's centreline segment, or on the curve, at the offset |

- **One new residual.** Signed offsets need one new residual family in `drift_solver::system`:
  `OffsetFrom { point, curve, offset }`, the signed distance of a point from a line or from an
  arc's circle. Lowered forms use it where today's unsigned `PointLineDistance` can't say
  which side. The side is taken from the current geometry when lowering, as
  orientation-dependent relations keep their branch.
- **Offsets come from the wall's thickness and justify** at solve time. A thickness edit
  changes the offset, so one solve followed by `follow_walls` is enough: a face held by a
  dimension stays put, and the centreline moves. Thickness expressions are evaluated in
  `apply_parameters`, before the solve (§2.4), so the solve sees the new value.
- **Faces don't need moving after the solve.** Faces were never solved, so `follow_walls` just
  regenerates them from the solved centreline. They agree with the lowered relation exactly,
  up to the solver's tolerance on the centreline.
- **The scope is the centrelines the relation touches** (and whatever they are constrained
  to), as for any other geometry. The drag properties apply unchanged, and W2's tests extend
  `drag_props.rs` with walls.

This makes the common architect's dimension work: *"3.60 m clear between finished faces"*. It is
a distance between two inner faces of two walls, and it moves the centrelines while each wall
keeps its thickness.

**In W1** (before lowering exists), relations and *driving* dimensions on faces are refused
with a notice ("Dimension the centreline, or wait for face dimensions"). *Driven* dimensions on
faces work, and so does anything on centrelines. The Dimension tool snaps to hidden centrelines,
as it snaps to construction lines. A click on a wall gives its **face** by default, as Revit
does; the centreline is the snap target when it is shown, or with **Alt**. W2 should ship in
the same release as W1, because driving face-to-face clear widths is the dimension architects
reach for first.

### 4.3 Thickness is a property dimension

Thickness is a property of the wall: a number, or an expression of variables (`t_ext`,
`t_int + 15`). It is **not** a solver unknown. You can solve a plan for a clear width, but not
for a thickness.

- **Showing it.** The Dimension tool, given the two faces of one straight wall segment, or
  **Show thickness** in the Inspector, places a **property dimension**:
  `PropertyDim { target: PropertyRef, place }`, drawn by the dimension renderer, with
  `PropertyRef::WallThickness(WallId)`. Its value is always the property. Property dimensions
  live in their own table, `property_dims`, not as a new kind in the annotations table. A new
  annotation kind would be a breaking change under file-format.md rule 4, while a new table is
  an addition (§2.6).
- **Driving it.** Typing into it writes `wall.thickness` / `thickness_expr` as one undo step.
  It edits the wall's property, so it never conflicts with relations. With face relations
  (W2), a face held by a dimension stays put and the centreline moves (§4.2). Otherwise the
  justified face stays put, and both faces move for Centre.
- **Variables.** Thickness expressions are re-evaluated in `apply_parameters`, so changing
  `t_ext` in the Variables panel re-thickens every wall that uses it. An Option that changes
  `t_ext` ghosts correctly.
- The same `PropertyDim` mechanism places and edits openings (`OpeningOffset`,
  `OpeningWidth`, §6). The solver never sees property dimensions.

---

## 5. Tool interaction

`ToolKind::Wall` is in `crates/drift-app/src/tools/wall.rs` (the prototype's `WallTool`,
ported). Its geometry is in `crates/drift-geom/src/walls.rs` (the prototype's `walls.rs`,
moved to drift-geom so drift-core stays geometry-free and the MCP side can use it later).
drift-geom must stay wasm-clean.

### 5.1 Kept from the prototype, and what changes

- **Clicking out a path.** Click the start, then each corner. A rubber-band preview of the whole
  run (both faces, caps, the poché tint) follows the pointer (`preview_with`, `preview_faces`).
- **Arcs.** In arc mode, click the end, then a point the wall curves through
  (`Seg::Arc(through)`). It is joined into the straight runs on either side (tangent
  continuity is §3.2's special case).
- **Bending by dragging** (new, the pen's feel). Click-drag out of the last corner instead of
  clicking: the next segment becomes an arc tangent to the drag direction, as the pen pulls a
  handle. Arc mode is for arcs through a known point; dragging is for quick curves.
- **Typed length and angle** (`fields`: Length, Angle; `target()`), and **X/Y locks**
  (`ToolKey::X`, `ToolKey::Y`, `line::steer`). Length is the centreline's (§2.2).
- **Closing on the start point** makes a closed centreline: a ring of walls with no caps. An end
  within `gaptol` of the start closes the ring, the same tolerance the Fill tool uses for gaps,
  so typed lengths that miss by 0.5 mm still close.
- **Undo granularity:** one undo step **per segment**, as the Line tool does. Each segment
  extends the run's centreline path and regenerates the run, so the join at the previous corner
  updates as you go. **Backspace** while drawing removes the last corner. **Enter or a
  double-click** finishes. **Esc** keeps what was drawn, as the Line tool does.
- **Starting or ending on another wall** joins it. On another wall's centreline *end* (or a
  corner node), the run shares that point (L, n-way). On its centreline or either face,
  mid-span, it records a T (`tee = (host, seg)`), with the end moved onto the host's
  centreline. The prototype's `line_under` becomes `wall_under`, which picks walls by their
  faces, poché or centreline.
- **Ending on a non-wall curve** (a column outline, a grid line, an imported line) records
  `against`. Both faces stop on it, as the prototype's `end_on_line` did.
- **Default thickness** when nothing is set follows the drawing's units: 4½″ imperial (a 2×4
  wall, finished), 100 mm metric (the prototype's `thickness()`).

### 5.2 Keys and the command registry

**The command registry (#167) is where tools are bound.** Every action is a row in
`crates/drift-app/src/commands.rs` (`table()`). The palette, the menus, key dispatch, the
shortcuts window and the guide's shortcut tables are all generated from it. Thin lines has
already moved from W to Shift+Alt+W, and a registry test keeps W free ("W is taken" in
`commands.rs`'s tests). What W1 adds:

- **The tool row**, next to the other Draw tools:

  ```rust
  tool("draw.wall", C::Draw, ToolKind::Wall)
      .keys(const { &[plain(Key::W)] })
      .aliases(&["WALL", "WA"]),
  ```

  This puts the Wall tool in the Draw menu and the palette (AutoCAD users type WALL), gives it
  W, and lists it in the shortcuts window and guide. The "W is taken" test changes to "W is
  the Wall tool".
- **W again** goes through `DriftApp::tool_key` (`app/keyboard.rs`), where C, A, F and U
  already cycle. Tool rows aren't idle-guarded, which is why A mid-polyline reaches it. W1
  adds a Wall branch there: with the Wall tool current, W calls `tool.key(ToolKey::Again)` to
  switch straight/arc for the next segment, *without* cancelling the run. The Slot branch
  cancels; walls must not.
- **The tool's own keys** stay in the tool, as the registry's boundary says (X/Y, typed
  values and Tab are tool keys too). They are listed in `OTHER_KEYS` under "While drawing"
  for the shortcuts window:

| Key | In the Wall tool | Why |
|---|---|---|
| **W** | Wall tool (registry row) | freed for walls by #167 |
| **W again** | Straight / arc for the next segment (the tool bar shows `Straight \| Arc`) | the registry's "pressed again" idiom, as for C, A, F and U |
| **A / L** mid-run | Arc next / straight next | the same keys the Line tool takes mid-polyline (`OTHER_KEYS`: "In a polyline: switch to a tangent arc, or back to lines"), so the habit carries over. Here A is a tool key, not the Arc tool. |
| **Space** | Flip justify, Left ↔ Right (Centre stays Centre); for an opening, cycle its swing (§6) | Revit's spacebar flips a wall while placing it |
| X / Y | Lock an axis | unchanged |
| Backspace | Remove the last corner | as other drawing tools |
| Tab | Next typed field, while typing | unchanged |

Outside a run, A is still the Arc tool. The prototype claimed A globally, and that can't stay.

**Space and the registry's repeat.** Since #167, *Enter or Space with nothing in hand repeats
the last command*. That applies only when the current tool is Select and isn't busy
(`idle` in `app/keyboard.rs`). With the Wall tool in hand Space never repeats, so the tool
gets it through `claimed(Key::Space, ToolKey::Space)`, next to the X/Y/G/B/R claims. Pressing
Space after leaving the Wall tool repeats it, which is AutoCAD's habit and fine.

**Space while typing goes to the field**, because imperial input uses it (`6 1/2"`, `12' 6"`).
`handle_typing` runs before `shortcuts` (`lib.rs:909`) and, once a field is active, takes every
text event and every key but Tab, Enter, Esc and Backspace. An idle Space never starts a field,
since fields start only with a digit, `.`, `,`, `-`, `(` or `=`. egui would also activate a
focused widget on Space, so **the tool bar must not keep keyboard focus**: `handle_typing`
surrenders focus for tools with fields, and the Wall tool has fields. A UI test in
`ui_tests/walls.rs` pins down all of this:
- W, W again and A/L mid-run;
- Space flipping in the Wall tool, Space typing into `6 1/2"`, and Space repeating the Wall
  tool from Select;
- Shift+Alt+W still toggling thin lines.

### 5.3 Where thickness, justify and poché live

**Decision for v1: tool bar state that remembers itself**, as the Fill switch does. That means
`Editor` fields carried into each new or opened drawing by `keep_prefs_from` (`editor.rs:463`)
and saved with the app's preferences, as `fill_new_shapes` is today (pref `newfill`,
`app/document.rs:188`). The new fields are `wall_thickness` (mm, 0 = by units) with its
expression, `wall_justify` and `wall_poche` (on or off; the style is the Fill tool's, as the
Fill switch uses it), saved as `wthick`, `wjust` and `wpoche`. The review read the Fill switch
as an editor field only. It is both, and walls do the same.

The tool bar (`app/context_ui.rs`):

`Thickness [ 200 mm ▾ ]` · `Left face | Centre | Right face` (Space flips) · `Straight | Arc` (W) · `☐ Poché [Solid ▾ ■]`

- **Thickness presets**, by the drawing's units. The field accepts any value or expression
  (`t_ext`, which is stored on the wall as `thickness_expr`), and its dropdown lists common
  sizes. US walls come first. Every preset is the **actual** finished thickness, labelled
  with the nominal construction it comes from, because nominal sizes are a trap: a "2×4" is
  3½″, and an "8-inch" block is 7⅝″.

  | Imperial preset | Actual |
  |---|---|
  | 2×4 stud, ½″ gypsum both sides | 4½″ (3½″ + ½″ + ½″) |
  | 2×4 stud, ⅝″ gypsum both sides | 4¾″ |
  | 2×6 stud, ½″ gypsum both sides | 6½″ (5½″ + ½″ + ½″) |
  | 2×6 stud, ⅝″ gypsum both sides | 6¾″ |
  | 6″ CMU | 5⅝″ |
  | 8″ CMU | 7⅝″ |
  | 12″ CMU | 11⅝″ |

  **Metric drawings** get the generic metric list: 100, 115, 140, 150, 200, 240, 300 and
  365 mm. The presets come from the drawing's size set (§6), so they follow its units by
  default, and a national set (DIN 18101, BS 4787, W3b) can carry its own list. Fractional
  input works (`4 1/2"`). In W6 the presets become wall types, where an exterior wall can add
  its sheathing and cladding.
- **Defaults follow the drawing's units.** Nothing here assumes metric. With no thickness
  set, a wall in an imperial drawing is 4½″ and one in a metric drawing is 100 mm (the
  prototype's `thickness()`). New drawings keep whatever units Drift gives them today.

**Why not drawing settings now.** Plan A (#144) moved into the drawing the rules that change
*what a drawing means*. A tool's starting values aren't that: each wall stores its own
thickness, justify and poché, so the drawing is complete without the tool bar's state. What
does belong in the drawing is a **named wall type** ("Ext 300 concrete", "Int 100 stud"): shared
by walls, editable in one place, and carrying a join priority. That is batch W6, where the
tool bar's thickness box becomes a type picker with "custom". v1 records are forward-compatible:
a wall without a type is a custom wall.

### 5.4 Editing after drawing

- **Picking** a face, the poché or the centreline picks the wall (§2.5). Shift adds walls.
- **Inspector** (`app/wall_ui.rs`, in the chain before `shape_inspector` in
  `app/shape_ui.rs`):
  - Thickness (a value or expression, with presets)
  - Justify
  - Poché (`fill_style_editor_or_none`)
  - Layer and Arrange
  - Ends: "Join" per end, plus "Joins walls it crosses"
  - Openings (a list, and Add)
  - Length per segment (a driven readout)
  - The actions **Show thickness**, **Split wall** and **Explode**.
- **Grips** on the picked wall:
  - centreline end points and nodes: move the shared point, so every wall at the node follows;
  - segment midpoints: offset the segment sideways;
  - a flip arrow for Justify.
- **Walls from curves:** right-click → *Walls from curves* on a picked chain or shape. It uses
  the existing curves (Tab chains, rectangles, pen paths) as the centreline: they become
  construction and get a wall at the tool bar's settings. That is the architect's "Pick
  lines" mode for a grid or a sketch. It doesn't help with an imported plan's *face* lines,
  which is what W1.5's **Make walls from lines** is for (§10).

---

## 6. Openings

```rust
pub struct Opening {
    pub wall: WallId,
    /// Where it sits along the wall: from an anchor, by an offset (the script's own words:
    /// `centred | offset 600 from bed.sw`).
    pub at: OpeningAt,              // Centred(seg) | From { anchor: Anchor, offset: f64, offset_expr: Option<String> }
    /// The structural opening (the cut in the wall), not the leaf.
    pub width: f64,
    pub width_expr: Option<String>,
    pub symbol: Symbol,             // Library { family, type_name } | Void (a cased opening)
    /// Which way it opens: hinge side, and which face it swings to.
    pub hand: Hand,                 // Left | Right
    pub facing: Side,               // the wall's Left or Right face
    pub layer: Option<LayerId>,     // None: the wall's
    pub made: Made,                 // jambs and symbol curves, keyed as a wall's are
}
pub enum Anchor { SegStart(u16), SegEnd(u16), Point(EntityId), Face(EntityId) }
```

- **The cut.** `follow_walls` turns each opening into two **stations** on its wall's segment:
  `s₀ = station(at)` and `s₁ = s₀ + width`, measured along the centreline (along the arc for a
  curved wall). Both faces are split at the stations, giving more `FaceKey` pieces, keyed
  `Station::Jamb`. The body between them is left out of the outline, and two **jamb** edges
  (`Part::Jamb(id, 0 | 1)`) close the outline on either side. So the poché goes around the
  opening without any hole-punching. An opening that would cross a node corner or another
  opening is refused when placed or dragged, and clamped while dragging.
- **The symbol** is generated at the host's thickness by the library family's generator, as
  the opening's own keyed entities (`Owner::Opening`): `swing_door(width, wall, swing)`,
  `casement_window(width, wall, sashes)` and the rest, already in `library/doors.rs` and
  `library/windows.rs` with the wall thickness as a parameter. It uses their documented base
  point ("the hinge side of the opening, on the room-side face of the wall"), is placed at the
  jamb on `facing`, and is mirrored for `hand`. It is generated rather than an `Instance` of a
  `Block` because the symbol depends on the wall's thickness (jambs, sills, frames across the
  wall), and blocks are fixed geometry. This is the "towards parametric blocks" step PLAN.md
  describes, done for hosted families first. Placing a user's own block as an opening symbol
  is W3c.
- **Opening width vs leaf width.** The type tables must keep the two apart. The **opening
  width** is the cut in the wall; the **leaf width** is what the swing draws; and the family's
  generator knows the frame between them. Today's US tables are by nominal door size; each
  type gets both numbers.
- **Swing.** While placing, **Space** cycles the four orientations (right-in, left-in,
  left-out, right-out), consistent with Space flipping walls. A picked opening shows two flip
  arrows on the canvas (hand and facing), and the Inspector has the same.
- **Size sets: US imperial is the default; metric and national sets are planned options**
  (Q8, decided: the user works in the USA, and wants options for a global audience).
- **Size sets are data, not code.** A *size set* is a table of door and window types (and the
  wall thickness presets, §5.3) for one market:

  ```rust
  pub struct SizeSet {
      pub id: &'static str,         // "us", "metric", "din-18101", "bs-4787"
      pub name: &'static str,       // "US (imperial)", "Germany (DIN 18101)"
      pub units: UnitSystem,        // which drawings it suits by default
      pub doors: Vec<DoorRow>,      // family, leaf width × height, opening width × height
      pub windows: Vec<WindowRow>,  // family, nominal width × height, opening width × height
      pub walls: Vec<WallPreset>,   // label, actual thickness
  }
  ```

  - **Where they live.** Each set is a JSON file in `crates/drift-app/src/library/sizes/`
    (`us.json`, `metric.json`, `din-18101.json`, `bs-4787.json`), compiled in with
    `include_str!` (wasm-safe, no file access), parsed once, and checked by a test: every row
    names a family that exists, and every opening is at least its leaf.
  - **The generators stay in code.** The families' generators (`swing_door`,
    `casement_window`, …) stay as they are; a set only says *which sizes* each family comes in.
  - **Adding a market** (Nordic, Australian, Canadian metric, …) means one JSON file and one
    line registering it. No code changes, and no format change: files store the opening's
    actual sizes, never a reference to a table row.
  - **Which set a drawing uses** is a drawing setting, `DrawingRules::size_set: Option<String>`.
    `None`, the default, means "by the drawing's units": imperial gets `us`, metric gets
    `metric`. A metric drawing in Germany picks `din-18101` once, and the opening tool bar, the
    Blocks panel and the thickness presets follow it. The field is an addition with no bump
    (§2.6). The Blocks panel also has a set picker for browsing other sets.
  - **Names** follow the existing "Family – Type" form, in the set's own terms:
    `Door – Single Swing – 3′0″ × 6′8″` or `Door – Single Swing – 860 × 1985`.
- **US imperial (the default, W3).** It extends the existing US library (`library/doors.rs`,
  `library/windows.rs`). Today that has single swing and pocket doors at 24, 28, 30, 32 and 36″
  only, and windows by width alone. Each type row holds both the **leaf** and the **rough
  opening**:
  - **Door leaves:** 2′0″ to 3′0″ in 2″ steps (24, 26, 28, 30, 32, 34, 36″), plus 3′6″ and
    4′0″, and pairs (double, French) from the same leaves. Heights 6′8″, 7′0″ and 8′0″.
  - **Rough opening** (the cut in the wall): the leaf **+ 2″ wide and + 2½″ high**, the
    usual allowance for a prehung frame (a 3′0″ × 6′8″ door gets a 38″ × 82½″ rough opening).
    The type table stores it per type, so a family or manufacturer that differs can say so.
  - **Windows:** common nominal sizes, width × height, named the US way (`3050` = 3′0″ ×
    5′0″). Widths 2′0″ to 6′0″ and heights 2′0″ to 6′0″, in the steps the existing families
    use. Each type stores its rough opening, which defaults to the nominal size, because
    window makers mostly quote the rough opening.
  - The width-only names the library has today stay as aliases, so drawings that already use
    them keep their blocks.
- **Generic metric (W3, with US).** The default for metric drawings: door leaves 700, 800, 900
  and 1000 mm (and 1200 and pairs), 2040 and 2100 mm high, with the opening stored per row
  (leaf plus the frame allowance); windows on a 300 mm module, 600 to 2400 mm wide. These are
  common international sizes, not a national standard.
- **National sets (W3b), planned, not optional extras.** Each is checked against its
  standard before it ships:
  - **Germany, DIN 18101:** leaves (e.g. 610, 735, 860 and 985 mm wide; 1985 and 2110 mm
    high), with structural openings on the 125 mm module (e.g. 750, 875, 1000 and 1125 mm). The
    older DIN 18100 for openings is reported withdrawn; the table follows current practice.
  - **UK, BS 4787:** leaves (e.g. 762, 838 and 926 mm wide; 1981 and 2040 mm high), with
    openings per the standard's frame sizes.
  - **More** (Nordic, Australian, Canadian), added as JSON files when users ask, without
    code changes.

  Whatever the set, the opening tool bar takes any width typed in, so a size missing from a
  table never blocks drawing it.
- **Placing.** The **Opening tool** (in the Block tool's group; a tool bar picks the family and
  type) is hovered along a wall. It shows the cut and the symbol live, with temporary
  dimensions to the nearest face corners on both sides (the `temp_dims.rs` machinery). A
  click places it. Typing a value sets the offset from the nearer corner, and **Tab** switches
  the field to the width.
- **Dragging along the wall.** A picked opening's grip slides it along its segment, clamped
  between the nodes' corners and its neighbours, and snapping to centred and to the
  nearest-corner offset. It shows its two `PropertyDim`s (offset and width), and typing into
  either writes `at` or `width`. Dragging onto another wall re-hosts it (W3c).
- **Rooms aren't opened by openings** (§7).

---

## 7. Room tags

```rust
pub struct Room {
    pub name: String,               // "Bedroom 1"
    pub number: Option<String>,     // "1.04"
    /// Where the tag is: also the point the room is found from.
    pub at: DVec2,
    pub layer: LayerId,
    pub tag: TagStyle,              // which lines show (name, number, area); text height (paper)
}
```

- **The area comes from the walls, not from the curves around it.** It is the face containing
  `at` among the **bounding shapes** of every room-bounding wall. A wall's bounding shapes are
  its **room-side face lines**, with every opening **closed at the face plane**. So a door
  doesn't make a room leak into the hall, and a deep window reveal isn't counted.
  **Room separation lines** add to them: curves on a layer marked "Bounds rooms", for
  open-plan splits.
- **Bounding doesn't depend on visibility.** Every wall bounds rooms unless its layer is marked
  **"Doesn't bound rooms"** (the inverse mark, for furniture-height partitions or demolition).
  Hiding a layer, or hiding it in a sheet view, never changes an area.
- **How it is computed.** The faces come from `drift_geom::regions::regions` over these
  derived shapes, not over document entities. So furniture, hatching and dimensions inside a
  room never change its area, and nothing depends on entity ids.
- **Why not `path_region`.** A room is usually bounded by several clusters' faces (concrete
  outer walls, stud partitions), so no single Path bounds it. `path_region` would also take the
  furniture inside as holes. The room query reuses the same engine (bounds pruning, the `Areas`
  index) on a different input. In the common case, a room enclosed by one cluster's inner loop,
  the answer is exactly that loop's region, and a test checks that the two agree.
- **Net area:** to the finished room-side faces. Free-standing wall clusters inside a room
  (columns, a stair core drawn as walls) are holes. Gross area (to centrelines) is a later
  option.
- **Units:** `Units::format_area`, so m² for metric and ft² for imperial. The tag's precision
  is a drawing setting, by default 2 decimals in m² and whole ft². The area is cached per
  revision, keyed by the walls' revision and `at`.
- **It follows the walls.** Move a wall and the area updates. A room whose point is no longer in
  a closed face shows "—" and a red tag, as a lost fill is flagged, and analyze reports it. A
  tag point inside a wall's poché says so ("This room's tag is inside a wall").
- **The tag** is a small generated text group (name, number, area) at `at`. Dragging it moves
  `at`, so dragging the tag into another room re-points it, as Revit does. The **Room tool**
  (in the Note tool's group) places a tag with a click inside a closed face, and offers the next
  number.
- **A room list** (name, number, area, a total) in a panel, with Copy as CSV, is an S-sized
  part of W4. It is not a BIM schedule.

---

## 8. Output, DXF and the script

- **Print, PDF and SVG** already follow the canvas stack (batch 2: `stack.rs` →
  `output.rs` `stacked()`). Walls are entities, paths and fills, so they print as they look:
  - pochés print under their outlines;
  - edges with Stroke: none (butt and against edges) are skipped in output, as they are today
    (`output.rs` `stroked()`);
  - centrelines never print (they are construction).

  No wall-specific output code is needed. One test renders a wall plan to PDF and SVG and reads
  the paint order back, as `ui_tests/stacking.rs` does.
- **DXF R12** (now). The faces are LINE and ARC entities on the wall's layer, written in stack
  order. Curves without a stroke aren't written (`dxf.rs`). Poché is written as SOLID
  triangles, as fills are now: check that the triangulation handles a whole-floor cluster
  (one outer loop with hundreds of holes), and the file size that makes. Openings are lines
  and arcs on their layer. This is correct, but other CAD programs see plain geometry, as with
  any R12 export.
- **DXF AC1015** (later, the audit's batch 6):
  - each cluster outline loop as a closed `LWPOLYLINE` (bulges for arcs) on the wall's layer;
  - poché as a `HATCH` whose boundary is that polyline (SOLID or the pattern), not associative;
  - openings as `INSERT`s of a per-type `BLOCK`.

  Walls need no XDATA.
- **The script already has walls, rooms and openings by name** (`agent/script`: `room bed W x
  D at 0,0 walls 200 outside`, `wall w1 from … thick 200 left`, `door d1 900 in bed.south
  centred`, `bed.south[2]`, `.walls`, `hatch bed.walls`). It lowers them to offset lines with
  linked `PointLineDistance` dimensions, and to faces split at jambs: the prototype's model.
  Reconcile by **keeping the language and changing what it lowers to**, in two steps so the
  script and the tool don't make two wall models for long:
  - **W5a (with or right after W1):**
    - `wall` → a `Wall`: the centreline from `from`/`to`, `thick` → thickness or expression,
      `left|right|centre` → `Justify`. `length` → a dimension on the centreline, and `fixed` →
      Fix on its start point.
    - `room … walls t side` → a closed `Wall` ring around the inside rectangle (`inside` puts
      the centreline t/2 out, `outside` and `centre` likewise). `right of bed` → the new room's
      ring T-joins and shares the existing wall instead of drawing a second one.
    - Names resolve through `FaceKey`: `w1.left` → `Part::Left` of segment 0. `.walls` → the
      cluster's poché fill, and `hatch bed.walls ANSI31` → sets the walls' `FillStyle`.
  - **Until W3 lands,** a W1 wall can't host a cut, and the owner guard refuses splitting
    its faces. So a script that has `door`, `window` or `opening` statements keeps today's
    lowering (plain lines) for its walls, and the result says so. Scripts without openings
    get Wall records. Nothing a script does today stops working.
  - **W5b (after W3 and W4):**
    - `door|window|opening d 900 in bed.south centred|offset 600 from bed.sw` → an `Opening`,
      with `OpeningAt` taken straight from the clause and the family from the statement word.
      `bed.south[2]` → the piece of that face after the opening's jamb station.
    - `label bed "Bedroom 1"` → the room's name.
    - `Runner::rooms()` (`run.rs`) reports `Room` areas from §7 (correct with openings and
      non-rectangles) instead of the corner shoelace.
  - **MCP:** `list_entities` and `analyze` report walls, openings and rooms (ids `w`, `o`,
    `r`) with their thickness, length, area and lost rooms, and `delete` accepts their ids. A
    small `walls` tool (set thickness, justify, poché, join) can wait. The schema stays a Labs
    schema (the user's decision: MCP continues as Labs), so changing what statements make is
    allowed. The script's tests are updated to the new entities.
- **Blocks and copies.** **Make block** from a selection with walls explodes them into the
  block (§2.1 checklist). Copy and paste copies the records and their centrelines, and
  regenerates on paste with new ids.

---

## 9. Tests and performance

### 9.1 Unit tests (`drift-geom/src/walls.rs`)

Port the prototype's five (`an_l_wall_is_mitred_at_its_corner`,
`a_closed_room_has_inner_and_outer_rectangles`, `justification_puts_the_path_on_a_face`,
`a_curved_wall_has_concentric_faces`, `a_wall_can_end_on_another`, the last now for `against`).
Add tests for:
- the node rule for T, X, Y and a 5-way star;
- straight on with equal and unequal thickness (the step), at the 1e-6 rad threshold;
- a tangent arc into a line, and an arc into an arc (tangent and not);
- the mitre limit (bevel), and a 1° fork;
- a segment shorter than its thickness;
- an arc too tight, which gives an error, not a panic;
- butt joins between different materials;
- an n-way node with a wall running straight through it.

### 9.2 Properties (`crates/drift-app/src/walls_props.rs`, proptest, beside `drag_props.rs`)

Generators: random wall networks on a jittered grid (L, T and X joins, arcs, mixed
thicknesses, 1–60 walls), random openings, and random edit sequences (move a node, re-thicken,
flip, add or delete a wall, add an opening, split a wall).

| Property | Batch |
|---|---|
| **Idempotent:** running `follow_walls` twice leaves the document unchanged (`same_sketch`, and equal ids). | W1 |
| **Order independent:** the same network built in any order gives the same outlines (compared by geometry, not ids). | W1 |
| **Join symmetric:** L(a, b) and L(b, a) give identical corners. | W1 |
| **Closed and simple:** every cluster outline loop closes and doesn't cross itself, and its shoelace area equals `path_region`'s area for that loop (both models agree). | W1 |
| **Area:** for straight, centre-justified walls with joins inside the mitre limit and no openings, the poché area equals Σ(segment length × thickness), less the overlaps at T and X nodes (computed independently). With openings, it is less by Σ(width × thickness). | W1 / W3 |
| **Parallel at thickness:** every straight face piece lies at exactly the signed offset from its centreline (1e-9 relative). | W1 |
| **Stable keys:** an edit that keeps the topology (moving a node, re-thickening, adding a T or an opening elsewhere on the face) keeps every other generated entity id. | W1 |
| **Unchanged keys aren't journalled:** a commit that moves one wall touches only the entities whose geometry changed. | W1 |
| **Undo:** any edit sequence, fully undone, restores the document as `Document::same_sketch` sees it. Redo restores the same ids. (Id counters aren't restored, so later edits may allocate different ids.) | W1 |
| **Free straight walls skip the solver:** with no relations, `needs_solve` is false and `drag_solves` is 0 through any edit of straight walls. | W1 |
| **Relations on faces survive** key-preserving edits, and are dropped with a notice when their key goes. | W2 |
| **Lowering is local:** a dimension between two faces solves only the centrelines it touches (scope size check), and the drag properties hold with walls. | W2 |
| **Rooms:** a room's area equals the analytic net area for rectangular rooms; it is unchanged by an opening in its walls, by furniture inside, or by hiding a layer; and it agrees with the inner loop's region when one cluster bounds it. | W4 |
| **Round trip:** save, load, then `follow_walls` is a no-op. | W1 |

### 9.3 Tool and UI tests

The prototype's four tool tests, adapted. "An L is 6 lines" becomes "an L is one cluster with
one loop", and the T test asserts that the host's entities kept their ids. Plus UI tests in
`ui_tests/walls.rs`:
- the keys: W, W again, Shift+Alt+W for thin lines, Space when idle, and Space inside `6 1/2"`;
- click-drag bending;
- Backspace removing the last corner;
- clicking inside the poché picks the wall, and the nearest wall at a node;
- the pick filters (Edges, Fills);
- the owner guard (Trim on a face is refused, dragging a face offsets the wall);
- Explode;
- a PDF and SVG read-back of an L with a door.

### 9.4 Performance budgets (release, `perf_walls_*`, `--ignored`)

The fixture is generated: a floor of 40 × 25 rooms (about 2,000 wall segments in 50 runs,
1,000 openings, 1,000 rooms), all one cluster (the worst case for re-chaining), plus 5,000
furniture curves inside.

| Operation | Budget |
|---|---|
| Commit of one wall edit: regenerating the dirty walls and their nodes, X detection (bounds-indexed), re-chaining the cluster, diffing keys | ≤ 5 ms, with only changed keys journalled |
| Drag frame moving a 4-way node (preview faces only) | ≤ 2 ms on top of today's drag frame |
| The cluster's poché in `fill_shapes` (the wall fast path, §2.5) | ≤ 2 ms, and independent of the furniture count |
| The rest of `fill_shapes` when area fills exist | measured and budgeted (see below) |
| All room areas after one wall edit | ≤ 20 ms (rooms re-query only if their bounds touch the dirty walls) |
| Opening a file with the fixture (load and the verification regen) | ≤ 150 ms more than the same geometry without wall records |
| Canvas frame | unchanged from the same entities without walls (the `perf_` zoom and pan tests) |

**The global arrangement.** `fill_shapes` runs the whole-drawing `Editor::regions()`
arrangement whenever any area fill exists, and wall faces are solid curves. A 2,000-segment
plan adds about 5,000 face pieces to it on every commit. They have to stay in: an area fill
clicked inside a room is bounded by wall faces, which is the reason to have them. So W1
measures this cost on the fixture with one area fill and budgets it. If it is too high, the
fix belongs to the regions work (incremental arrangement, plan §3), not to walls.
`regions_among`'s per-shape intersection filter for *drawn* shapes inside the building is
today's cost, unchanged by walls.

Baselines are recorded in the batch's PR, and the existing `perf_` suite (drag 1,000 curves,
solve, frames) must not regress.

---

## 10. Phasing

| Batch | Contents | Size | Depends on |
|---|---|---|---|
| (done) **Command registry**, #167 | One registry for commands, keys and the palette. W freed (thin lines → Shift+Alt+W). | — | merged |
| (done) **Tolerant reader**, #166 | Format 42 with `reader`: additions bump nothing, so wall tables need only fixtures (§2.6). | — | merged |
| **W1: walls v1** | The drift-core `Wall` record, the `walls` and `property_dims` tables (additions, no bump; fixture `v42.1`), journal, the §2.1 checklist, the Load repair (§2.6). drift-geom `walls.rs` (ported offsets, node rule with every §3.2 case, caps, against). `follow_walls` with station keys, cluster outlines, poché fills with the region fast path, the Load check. `follow_paths`/`follow_fills` skip owned records. The owner guard table and Explode. The Wall tool (registry row `draw.wall` on W, W again and A/L mid-run, drag to bend, Space, typed, locks, gaptol closing, undo per segment, Backspace, L/T/X/against by snapping), tool bar prefs and presets. Picking, Inspector, grips, Walls from curves. Relations on centrelines; on faces only *driven* dimensions; thickness `PropertyDim`. Output tests. W1's unit, property and perf tests from §9. | **L** | #164, #166, #167 (all merged) |
| **W1.5: Make walls from lines** | Pair parallel lines at a wall-like spacing (near-parallel and near-equal spacing within tolerance), matched by overlapping extent rather than equal length, across gaps at openings. Build centrelines and walls, and weld ends (§3.3 rule 6). For imported DWG and DXF plans and old script walls. | **M** | W1 |
| **W2: constraints on faces** | `walls::lower`, the `OffsetFrom` residual, `connected`/`needs_solve`/`build_in` reading through the lowering, driving dimensions on faces, constrained tee and against ends, drag property tests over walls. **Same release as W1.** | **M** | W1 |
| **W3: openings** | `Opening` record and `openings` table (addition, fixture `v42.2`), stations, cuts and jambs, generated library symbols at host thickness, data-driven size sets (§6) with **US imperial** (the default) and **generic metric**, leaf and opening per type, `DrawingRules::size_set`, Opening tool, swing cycling and flip arrows, drag along the wall, offset and width `PropertyDim`s. | **M–L** | W1 |
| **W3b: national size sets** (not scheduled: walls are frozen) | DIN 18101 (Germany) and BS 4787 (UK) as JSON size sets, each checked against its standard, with their thickness presets. Planned, part of the walls work. More markets are added the same way. | **S** | W3 |
| **W3c: openings, more** (not scheduled: walls are frozen) | Re-host by dragging onto another wall; user blocks as opening symbols. | **S–M** | W3 |
| ~~**W4: rooms**~~ **Cut 2026-10-27**, not deferred: it's the path to BIM. | `Room` record and `rooms` table (addition, fixture `v42.3`), bounding shapes with openings closed at the face plane, "Bounds rooms" and "Doesn't bound rooms" layer marks, tag, Room tool, area units and precision, lost rooms in analyze, room list panel. | **M** | W1 (W3 for closing openings) |
| **W5a: script walls** (not scheduled: walls are frozen) | `wall` and `room … walls` → Wall records, names through `FaceKey`, `.walls` / `hatch`, script tests. | **S–M** | W1 |
| ~~**W5b: script openings and rooms**~~ **Cut 2026-10-27**, not deferred: it's the path to BIM. | `door/window/opening` → Opening, `label` → Room, `rooms()` from §7, list/analyze/delete for `w`, `o` and `r` ids. | **S–M** | W3, W4 |
| ~~**W6: wall types**~~ **Cut 2026-10-27**, not deferred: it's the path to BIM. | Named types in the drawing (thickness, poché, join priority, hatch origin), type picker on the tool bar, priority replacing "thickest runs through", angled end caps, columns merging into walls by priority. An addition (its own fixture), no bump. | **M** | W1 |
| ~~**W7: DXF AC1015 for walls**~~ **Cut 2026-10-27**, not deferred: it's the path to BIM. | LWPOLYLINE outlines, HATCH poché, INSERT openings. Part of the audit's DXF 2000+ batch. | **S** on top of that batch | audit batch 6 |
| Later (not scheduled: walls are frozen) | Compound walls (bands), curtain walls, columns as a type, gross areas, wall tags, free arcs skipping the solver (solver plan §1). | — | — |

W1 is the first batch and can ship alone: architects get walls that draw, join, fill, print
and stay editable. W2 should follow it into the same release. W2, W3 and W1.5 can run in
parallel sessions after W1, because they touch different files (the solver, openings, import).

---

## 11. Open questions

The review's answers to the first version's questions were adopted where they agree with
mine; where I differ, the reason is given. Q8 was answered by the user. None are open.

1. **Faces as real entities?** *Decided: real, owned, keyed entities.* This holds under the two
   conditions the review set: keys are station-based (§2.3), and generated entities never
   enter the solver (§4.2).
2. **One record per run, or per segment?** *Decided: per run.* A click picks the run, the
   Inspector lists per-segment lengths, grips edit segments, and Split and Join convert.
3. **Tool settings?** *Decided: tool bar memory in v1, with presets; wall types in the drawing
   in W6 (§5.3).* The review suggested editor fields rather than app prefs. In the code, the
   Fill switch is both (an `Editor` field carried by `keep_prefs_from` and the `newfill`
   pref), and walls do the same.
4. **Keys?** *Decided:* W (thin lines moves to Shift+Alt+W, rebound by the command registry), W
   again for arc mode, **and** click-drag to bend, both in W1. Space flips.
5. **Auto X joins of the same material?** *Decided: yes,* with a per-wall "Joins walls it
   crosses" (`Wall.cross`), because a mid-segment crossing has no end to hang "Don't join"
   on.
6. **Different materials: the thickest runs through?** *Decided: yes for v1*, with ties going
   to the wall drawn first. W6's type priority replaces the rule.
7. **Generated opening symbols rather than blocks?** *Decided: generated.* User blocks as
   symbols come in W3c.
8. **Which door and window size sets?** *Decided by the user (who works in the USA): US
   imperial first* (§6). Door leaves 2′0″ to 3′0″ in 2″ steps plus 3′6″, 4′0″ and pairs,
   6′8″, 7′0″ and 8′0″ high; rough openings leaf + 2″ wide and + 2½″ high; common nominal
   window sizes. Leaf and rough opening are separate columns. For a global audience, size
   sets are data (JSON tables), chosen per drawing or by its units: **generic metric** ships
   with US in W3, and **DIN 18101** and **BS 4787** follow in W3b as planned work. Other
   markets are one file each. Thickness presets come from the same sets: US walls at their
   actual sizes, a metric list for metric drawings (§5.3). Defaults follow the drawing's
   units.
9. **Room area?** *Decided: net, to the room-side finished faces, openings closed at the face
   plane, columns subtracted.* 2 decimals in m², whole ft². Gross area comes later.
10. **Thickness as a solver unknown?** *Decided: no.* It is a property with a property
    dimension (§4.3), and lowered relations see it as an offset.
11. **Default poché?** *Decided: on, solid, the layer's colour, opacity 1.* On a black layer
    this reads like Revit's coarse view, which is what most plans want.
12. **The script makes records?** *Decided: yes, and sooner:* W5a with W1, W5b after W3 and W4.
13. **Centreline display?** *Decided: hidden unless the wall is picked or hovered, faint while
    the Wall or Dimension tool is active, and a View toggle.* The Dimension tool snaps to it
    even when it is hidden.
