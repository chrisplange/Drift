# Walls

**What you'll make:** a small room with an interior wall teed into it, a curved wall, a
wall made from lines you had already drawn, and walls found in an imported plan. Then you'll
change a wall's thickness, from a preset and from a variable, and dimension the room's clear
width between finished faces.

Time: about 20 minutes.

## How walls work

A wall is drawn along its **centreline**: a chain of lines and arcs you click, like a
polyline. The wall's faces, its end caps and its corners are made from the centreline, its
**thickness** and its **justification** (the centreline in the middle, or along its left or
right face). Change the centreline (drag a corner, fillet it, type a length) and the faces
follow. You never edit a face on its own: click a face, the fill inside, or the centreline,
and you pick the whole wall.

Walls **join** where they meet, worked out from where their ends are, so you don't have to
hit the exact spot:

- **L and corners:** walls that share a centreline point are mitred, and so are walls whose
  ends land in each other's corner (a little short, a little past, or off to one side). Three
  or more walls at one place clean up together.
- **T:** a wall whose end lands anywhere inside another wall (on its face, its fill or its
  centreline) runs into it, and that wall's face opens where they meet. An end that lands
  near another wall's free end makes an L instead: a stub shorter than the wall is thick is
  taken back to the corner.
- **X:** walls that cross clean up as an X.
- **Against:** a wall ended on another curve (a column, a slab edge, a grid line) stops
  square on it, with no cap.
- A run that comes back to its own start, or into its own side, joins itself (a room, a P).

Joined walls of the same **poché** (fill) on the same layer are one area, with no line between
them. Walls of another poché or layer butt against the thickest wall there instead.

Centrelines are construction geometry: they never print. They show while the **Wall** or
**Dimension** tool is in hand, on a picked or hovered wall, or everywhere with **View → Wall
centrelines**.

## 1. Draw a room

1. Press W for the **Wall** tool (or pick it from **Build** in the tool palette, or the
   **Build** menu).
2. In the top bar, set **Thickness**: the **▾** list has US walls at their actual finished
   thickness (a 2×4 stud wall with ½″ gypsum both sides is 4½″) or, in a metric drawing,
   100 mm to 365 mm. You can type any value too.
3. Leave the justification at **Centre** and **Poché** on.
4. Click four corners of a room, then click the first corner again (anywhere in its corner
   will do). The room closes and the tool is ready for the next wall.

While you draw, the next segment's preview shows how it will join: its faces stop on the wall
it runs into, or mitre round the corner.

Each segment you click is one undo step. While drawing, **Backspace** takes the last segment
back, typing a number gives a length (Tab: an angle), and X / Y lock the segment to an axis.

## 2. A wall teed into it

1. With the Wall tool still in hand, click on the inside face of one side of the room.
2. Click a point inside the room, then press Enter.

The new wall runs into the room's wall, and the room's face opens where it meets it. Both are
one poché. Move the room's wall later and the T follows.

## 3. Arcs and bends

- **W again** switches the next segments between **Straight** and **Arc** (also in the top
  bar). For an arc, click its end, then a point it curves through.
- Mid-run, **A** makes the next segment an arc and **L** a straight one, as in a polyline.
- **Click and drag** at the next corner instead of clicking: the segment bends, arriving in the
  direction you pull, as the pen tool pulls a handle.
- **Space** flips the wall to its other face (Left face ↔ Right face) while you draw.

## 4. Walls from lines you already drew

1. Draw a few connected lines (or pick a rectangle's sides with Tab).
2. Pick them, then choose **Walls from curves** (in the **Build** menu, in the **Wall** tool's
   flyout: right-click it in the tool palette, or in the command palette).

The lines become the walls' construction centrelines, at the Wall tool's settings.

## 5. Walls from an imported plan's lines

A plan from AutoCAD or another CAD program (DWG, DXF or PDF) draws each wall as its two face
lines, broken at every door and window. **Walls from lines** finds those pairs and makes them
walls.

1. Import the plan (**File → Import…**).
2. Drag a box round the part of the plan you want (or press Ctrl+A for all of it). Picking
   furniture, text or dimensions along with the walls does no harm: only lines and arcs are
   looked at.
3. Choose **Walls from lines** (in the **Build** menu, the **Wall** tool's flyout, or the
   command palette).

It pairs lines that are parallel, a wall's thickness apart (2″ to 18″, 50 mm to 460 mm) and on
the same layer, wherever they run side by side: they needn't be the same length, and a face
broken where another wall tees in is matched by its unbroken partner. Each wall is made at
the thickness it measures, snapped to the nearest of the Wall tool's presets when it's
within ⅛″ (3 mm), and otherwise rounded to 1⁄16″ (1 mm in a metric drawing). Walls meet where their centrelines
cross, so L, T and X joins come out clean, and walls that meet two at a corner become one run.
Concentric arcs become curved walls.

At a door or window, both faces stop and the wall stops with them: the walls end at the
jambs, and the gap is left open (the door's swing and the window's lines stay as they were
drawn). Walls from lines doesn't make hosted [doors and windows](15-openings.md) yet.

The status bar and the top of the **Inspector** say what it did: "Made 24 walls from 61
lines; 2 openings left open; 3 line pairs skipped: spacing varies". It skips:

- **Spacing varies:** two lines that taper apart, so they aren't a wall's faces.
- **Evenly spaced lines:** lines with partners on both sides at the same distance, as in a
  hatch or a grid of tiles.
- **Too short to stand alone:** a short piece joined to nothing, such as a shelf, a counter
  or a column.

Click **Pick** beside any of these to pick its lines and look at them. Lines the walls cover
only in part (a face that carries on as something else) are made into walls and kept.

The lines that became walls are **Removed** by default: the walls' faces lie where they were,
and **Undo** brings them back (the whole command is one step). In the Inspector's **Lines
used** list you can choose **Kept** (left over the walls) or **Moved to a hidden layer**
("Wall lines") instead; changing it does the command again that way, and Drift remembers the
choice. It also tidies walls an older Drift script drew as lines, centrelines included.

## 6. Change a wall

Click a wall (a face, its fill or its centreline). The **Inspector** shows:

- **Thickness:** type a value, pick a preset, or type a variable's name (for example
  `t_ext`). A thickness from a variable follows it: change the variable in the **Parameters**
  tab and every wall that uses it is re-thickened.
- **Justify**, **Poché** (on or off, solid or a hatch) and **Layer**.
- **Joins:** whether it joins walls it crosses, and at each end whether it joins at all.
  With two walls picked, **Join** meets their nearest ends (an L where their centrelines
  cross, or a T where one points at the other); **Unjoin** caps the ends where the picked
  walls meet, so they overlap. Both are in the **Build** menu too (**Join walls**, **Unjoin
  walls**), with **Explode walls**.
- **Segments:** each segment's length, with **Split after** to make two walls there.
- **Show thickness** draws a dimension of its thickness on the canvas. Double-click its value
  to type a new thickness (or a variable's name); it prints like any other dimension.
- **Explode** turns the wall into plain lines, arcs and a fill.

Dragging a picked wall's centreline points moves its corners (and the walls joined there).
Drag a free end into another wall and it joins as you go; drag it out and it lets go.
Fillet a centreline corner to round the wall. Trim, Extend, Split, Fillet and Chamfer refuse a
wall's faces: work on its centreline, or Explode it.

## 7. Dimension to the faces

Architects dimension finished faces: "3.60 m clear". In Drift a dimension on a wall's face
drives the wall.

1. Press D for the **Dimension** tool.
2. Click the inside face of one wall of the room, then the inside face of the wall opposite.
   A click on a wall gives its **face** (the centreline shows faintly while the tool is in
   hand; click on it to dimension the centreline instead).
3. Place the label and type the clear width, for example `3600` or `11'10"`.

The walls move so the faces are that far apart, and each keeps its thickness: the dimension
holds the face, and the centreline moves. The same works from a point or a line to a face
(a column's centre to a wall's face, a grid line to a finished face), and relations work on
faces too: a face **Horizontal**, **Vertical**, **Parallel**, **Perpendicular** or at an
**Angle** to a line, **Collinear** with a line, a point **On** a face, the **Radius** of a
curved wall's face, or **Concentric** with it.

Things to know:

- **Change the thickness and the dimensioned face stays put.** Make the wall thicker and its
  other face (or, for a Centre wall, both its centreline and other face) moves away from the
  dimension. If both faces of a wall are held by dimensions, its thickness can't change until
  you make one of them a reference (or delete it): Drift says so rather than moving
  something else.
- **Flip the justification and the same face stays held.** A dimension on the left face is on
  the left face whichever line the centreline is.
- **Corners follow the joins.** A dimension to a wall's corner (where two faces meet, or a
  cap's end) can't drive the wall, because the corner moves whenever the joins change. It is
  added as a reference dimension instead, with a note. Dimension the face, or the centreline.
- **Ends that run into a wall stay on it.** A wall teed into a dimensioned wall keeps its end
  on that wall as the dimension moves it.
- When a wall later tees into a dimensioned face, the face is split there and the dimension
  stays on the piece under it.
- While you drag, walls keep the joins they had when the drag began (only an end you drag
  yourself joins or lets go as it goes), so a dimension never flips an L into a T mid-drag.
  Joins are worked out again when you let go.

## What's next

[Doors and windows](15-openings.md) cut into walls. Room tags with live areas are coming in
the next batches.
