# Design note: one kind of filled shape (#202)

Status: **decided by the user on 2026-10-27 and built** in the same PR as the #201 fix
(fills re-found after a trim), after two independent reviews. The options considered and
the decisions are below.

## The problem

Two filled things could look the same on the canvas and behave differently:

| | **Shape** (a Path record with its fill) | **Area fill** (a fill bound to an area) |
| --- | --- | --- |
| Made by | Rectangle, Polygon, Circle, Ellipse, Slot, a pen path closed on its start; or a Fill-tool click in an area whose outline is exactly one closed shape | a Fill-tool click in an area bounded by separate lines and arcs |
| Click inside | picks the fill **and** its outline | picks the fill **alone** |
| Move, Shift+D, copy, Delete | act on the whole shape | act on the fill alone (the lines stay) |
| Inspector | Stroke, Fill, Layer, Arrange, Area | the fill's style; no stroke |
| Curves crossing it | never change it (it fills the inside of its own outline) | divide it (kept whole and flagged, or follows its seed, per the drawing rule) |
| Shared edges | a curve is in one path only | many fills can share a line (rooms in a plan) |

Nothing on screen said which one you had, and there was no way to turn one into the other.
A tester who drew a rectangle from four lines and filled it found that it picked, moved and
deleted unlike the rectangle drawn with R next to it.

The model is right to have both: area fills are what a drafter wants for hatching among
shared lines (rooms, wall pochés), and shapes are what an illustrator wants. The problem was
that the difference was hidden and couldn't be crossed.

## Options that were considered

- **A. Make shapes when it is safe.** A fill whose outline is a closed chain of free curves
  becomes a shape by itself. Good: the common case just works, with nothing to learn. Bad:
  two kinds still exist, by a rule the user can't see unless it is shown.
- **B. Make shape / Release shape commands, and show the kind.** Honest and covers every
  case, but two more commands to find, and a shape released by hand would have to stay
  released: a hidden flag the file and every edit must respect.
- **C. Area fills act like shapes.** Breaks among shared lines (moving one room's fill would
  drag the next room's walls). Not taken.

## Decisions (the user, 2026-10-27)

1. **One rule, A′, kept by every edit** (`follow_fills` in `crates/drift-app/src/fills.rs`).
   A fill is a **shape** exactly when its outline is one closed shape (path) or a closed
   chain of free curves: each meets the next at a shared end point, and none is a wall's,
   in another shape, or round another fill. Otherwise it is an **area fill** with no shape
   (`path = None`). There is no third state: no "shape fill covering an area", nothing that
   isn't in the file.
2. **Shapes are made by edits, never by opening a drawing.** A fill made, or touched by an
   edit (its outline or its points moved, reshaped or cut; restyling alone doesn't count),
   becomes a shape when its outline makes one. Opening a drawing changes nothing, and imports keep their fills as they were
   drawn (a CAD hatch is a boundary hatch). An area fill that could be a shape says so
   ("edit it to make one").
3. **No Make shape / Release shape commands.** They would need a sticky override, which is
   hidden state.
4. **Show the kind.** The Inspector's title is **Shape** (or the circle or ellipse it is) or
   **Area fill** / **Area hatch**, with "Not a shape: …" and the reason: "2 lines shared with
   other fills", "its lines are part of another shape", "lines run past it", "4 corners not
   joined", "bounded by walls" (a room's floor), "a wall's poché", "its shape's outline is
   cut open", or "opened or imported as an area: edit it to make a shape". Order matters and is shown, not undone: a rectangle
   filled first stays a shape when a neighbour drawn against it is filled; the neighbour is
   the area fill. The status line says the same when the pointer is over a fill
   (Select tool).
5. **Join corners** (a registry command: the Inspector's button, Modify menu, palette) is
   offered when the only thing between a picked area fill and a shape is corners that touch
   but aren't one point, as imports often leave them. It joins them (one undo step), and the
   edit makes the shape. Nothing is ever welded on its own.
6. **A shape cut open follows what still closes** (#201). When an edit cuts a shape's
   outline open, or the outline of one of its holes, its fill becomes the area fill of the
   area that what is left still closes around it, and a notice gives the new area. Only when
   nothing closes does it stay the shape's fill, shown as its outline closed straight across
   the gap, with a notice, until the outline closes again. That is found from the file alone,
   so it is the same after saving and opening again (for a side deleted, it is exactly the
   last shape). A hole merely moved, even across the outline, changes nothing.
7. **An outline closed again round an area fill makes it that shape's fill** (two filled
   shapes trimmed into each other: each becomes an area fill while open, the two come onto
   one area, and the joined outline is one shape with the top fill). A shape keeps one fill:
   the one drawn on top.

## What this means in practice

- A rectangle of four lines, filled: a shape. Click picks it with its outline; Shift+D,
  Delete and moves take it whole; a line drawn across it never changes it.
- Two rooms sharing a wall: area fills ("its lines also bound another fill"); a line across
  one divides it as before (kept whole and flagged).
- Lines that run past their corners: an area fill ("its lines cross or run on past it").
- A ring trimmed through between two lines: a C-shaped area fill, the slot empty.
- An old drawing: as it was until edited.

## Left open

- Lines shared between two fills are never copied to make both shapes; the rooms stay area
  fills. Revisit if the pilot asks.
- "Make hole / compound path" (trim-and-regions-plan T4) is still filed separately: a shape's
  holes are its islands, found each time, not recorded.
