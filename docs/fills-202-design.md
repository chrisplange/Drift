# Design note: one kind of filled shape (#202)

Status: **for the user's decisions**. No code yet. Written with the #201 fix (fills re-found
after a trim), which touches the same model.

## The problem

Two filled things can look the same on the canvas and behave differently:

| | **Shape** (a Path record with its fill) | **Area fill** (a fill bound to an area) |
| --- | --- | --- |
| Made by | Rectangle, Polygon, Circle, Ellipse, Slot, a pen path closed on its start; or a Fill-tool click in an area whose outline is exactly one closed shape | a Fill-tool click in an area bounded by separate lines and arcs |
| Click inside | picks the fill **and** its outline | picks the fill **alone** |
| Move, Shift+D, copy, Delete | act on the whole shape | act on the fill alone (the lines stay) |
| Inspector | Stroke, Fill, Layer, Arrange, Area | the fill's style; no stroke |
| Curves crossing it | never change it (it fills the inside of its own outline) | divide it (kept whole and flagged, or follows its seed, per the drawing rule) |
| Shared edges | a curve is in one path only | many fills can share a line (rooms in a plan) |

Nothing on screen says which one you have, and there is no way to turn one into the other.
A tester who draws a rectangle from four lines and fills it will find that it picks, moves
and deletes unlike the rectangle drawn with R next to it.

The model is right to have both: area fills are what a drafter wants for hatching among
shared lines (rooms, wall pochés, a plate with bolt holes drawn as loose circles), and shapes
are what an illustrator wants. The problem is only that the difference is hidden and
can't be crossed.

## What #201 changed (this PR)

A shape whose outline, or a hole's outline, is cut open by an edit now takes the area that
what is left still closes around its fill (and the notice says so), instead of freezing on
its last shape. While its fill covers such an area it is not a shape (it picks, moves and
copies as an area fill does); it becomes the shape's own fill again when the outline closes,
or when the area it took is gone. A hole merely moved, even across the outline, changes
nothing: crossing curves still never change a shape's fill. So trimming a filled
ring through between two lines gives a cut ring that behaves like an area fill. Whether
that cut ring should become a shape again is open question 5.

## Options

### A. Make shapes when it is safe

When the Fill tool fills an area whose outline is a **closed chain of free curves** (each
meets the next at a shared end point, and none is in another path or bounds another fill),
record that chain as a path and bind the fill to it. The four-lines rectangle becomes a
shape the moment it is filled. Outlines that share lines with other fills, or whose curves
cross instead of meeting at their ends, stay area fills.

- Good: the common case (a closed outline drawn with Line, then filled) just works, with no
  new concept to learn. No format change: paths already exist.
- Bad: two kinds still exist (rooms among shared walls stay area fills), and which one you
  get depends on a rule the user can't see. Lines that later get shared (a neighbour drawn
  against the shape) don't change it back.

### B. Explicit Make shape / Release shape, and show it

Keep both kinds and give the user the switch:

- **Make shape** (Modify menu, palette, context bar) on a picked area fill or a picked closed
  loop: records its outline as a path. Lines that cross instead of meeting are split at the
  crossings, as Divide's pieces are (trim-and-regions-plan §6). Lines shared with another
  fill are copied (a curve is in one path only, and snapping never welds).
- **Release shape** on a shape: drops the path. The fill stays as an area fill.
- **Show it:** the Inspector header says **Shape** or **Area fill** (with "bounded by 4 lines"
  under it), so does the status line on hover, and hovering a shape lights its whole
  outline.

- Good: honest and predictable, and it covers every case (shared lines, crossings).
- Bad: one more concept and two more commands for a pilot user to find. On its own, the
  four-lines rectangle still surprises them until they learn about Make shape.

### C. One behaviour, two records

Leave the records alone but make area fills *act* like shapes: a click inside picks the fill
and its outline; Move, Shift+D and Delete take the outline too; the Inspector shows Stroke.

- Good: no new commands, and nothing to learn.
- Bad: it breaks among shared lines. Moving one room's fill would drag the walls of the room
  next door, and deleting it would delete them. Every action then needs an "unless shared"
  exception, which is the hidden rule again, in more places.

## Recommendation

**A, plus the "show it" part of B now; Make shape / Release shape after the pilot if
testers ask.**

A fixes the case #202 was filed for without teaching anything new. Showing the kind in the
Inspector header and on hover makes the rest explainable when it comes up: an area fill
among shared lines can say *why* it isn't a shape ("its lines bound 2 other fills"). The
explicit commands are the right long-term tool but are size M on their own (splitting at
crossings, copying shared edges), and the pilot will tell us whether drafters miss them.
Option C is not recommended.

Size: A plus the display is **S–M**. Smoke tests: a four-line rectangle plus Fill acts like a
drawn rectangle for click-pick, Delete, Shift+D, move and the Inspector; a room among shared
walls stays an area fill and says why.

## Open questions for you

1. **Auto-make shapes (A):** yes? And only for fills made from now on, or also for area
   fills in drawings that are opened (which would change how old drawings pick)?
2. **The rule's limits:** an outline that shares a line with another fill stays an area fill.
   Is that right for your work, or would you rather have the shared line copied so it
   becomes a shape anyway?
3. **Names in the UI:** "Shape" and "Area fill"? Or "Shape" and "Fill (lines)" or "Hatch
   area"?
4. **Make shape / Release shape:** now, after the pilot, or not at all?
5. **After a trim (#201):** when a shape is trimmed open and its fill takes the area left
   (the cut ring), should that area become a shape again automatically when its outline is
   a closed chain of free curves (A's rule), or stay an area fill until you ask?
6. **Holes:** a shape's islands are holes (decided earlier). With A, should an island drawn
   from lines inside a shape also become a shape of its own, or stay lines?
