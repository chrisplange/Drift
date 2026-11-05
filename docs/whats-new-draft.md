# What's new since Beta 1.5 (draft)

A running draft for the next release's notes and the website's What's new. It lives outside
`docs/guide`, so the site doesn't publish it. Grouped by what you notice; the PR list for the
release notes is in the comment at the end.

Drawings saved by this version are file format 44 (Beta 1.5 wrote 33). Older drawings open
as before, but **Beta 1.5 can't open a drawing saved by this version**: it says the file is
newer than it supports, and opens nothing. Keep a copy saved from Beta 1.5 if you need to go
back.

## What you draw is what you print

- **Print, PDF and SVG match the screen.** Fills, curves, pictures and text stack the same
  way on paper as on the canvas.
- **Arrange.** Bring a fill or shape forward or back with Ctrl+] and Ctrl+[ (add Shift to
  send it all the way), from the right-click Arrange menu, or in the Inspector.
- **Shapes are objects.** A click inside a filled shape picks the whole shape. The Inspector
  shows it as one thing with a Stroke and a Fill, like a drawing program. Set Stroke to None
  for a fill with no outline: the outline still bounds it but doesn't draw or print.
- **Fill as you draw.** Rectangle, Polygon, Circle, Ellipse, Slot and Pen have a Fill switch
  that fills each closed shape as you draw it.
- **Fills keep to their own shape.** A fill stays inside the shape it was put in, whatever
  else crosses it, and follows that shape through splits, fillets and trims. When an edit
  divides an area fill, it's marked "Covers N areas" so you can fit, split or keep it.
- **Hatch alignment.** A picked hatch shows handles to move its origin and turn it. The
  Inspector's Pattern alignment sets the origin by value or on a corner or the centre, and
  **Align to edge…** lines the pattern up with any edge. Moving or rotating a shape now
  carries its hatch with it.
- **One rule for shapes.** A fill is a shape when its outline is one closed shape or a closed
  chain of curves; otherwise it's an area fill. The Inspector's title says which, and when it
  isn't a shape, why ("4 corners not joined", "bounded by walls", …).
- **Join corners** (Modify menu, Inspector) joins corners that touch but aren't one point, as
  imports leave them, so the outline makes a shape.
- **Edit fill** (right-click a filled shape) picks its fill on its own, to restyle or move it
  apart from its outline. A double-click inside the shape does the same.
- A trimmed ring keeps its fill as a cut ring. Fills on sheets have smooth edges.

## Picking and moving

- **Selection filters.** Toggles in the top bar choose what Select picks: points, edges,
  fills, areas, blocks, text and annotations, in any combination. Alt-click or double-click
  one to pick only that kind. Ctrl+A follows the filter.
- **Moves you can type.** Dragging a selection is a proper move: X or Y locks it to an axis,
  and typing a distance and pressing Enter moves it exactly.
- **Predictable drags.** Dragging a point gives the same result however you get there, and
  geometry with nothing holding it moves without waiting for the solver.
- **Layers.** Alt-click a layer's lock or snap toggle to lock (or make snappable) that layer
  alone; Alt-click again to put the others back.
- **The last thing you pick is the active one.** When you add a relation, the active element
  stays where it is and the others conform. Pick the line to turn, then the one to match:
  Perpendicular turns the first, Equal gives it the second's size. Shift-click a picked element
  to make it the active one. It's drawn heavier in a colour of its own, and the Inspector says which
  one stays.
- **Drags that hold together.** A drag no longer crushes an arc to a point, closes it into a
  full circle or leaves its end off its circle: it stops at the last shape that works. When the
  solver can't finish a step, the drag tries a smaller one, or carries along the curve the
  dragged points are held on.
- **A typed distance in a mouse drag.** Dragging a selection shows how far it has moved; type a
  distance and press Enter to move it exactly.
- **Big drawings draw, drag and edit much faster.**
- **Picking you can predict.** Hovering lights only what a click picks: geometry on a locked
  layer no longer lights up, and a click on a locked line goes through to what's beneath.
  Groups pick whole whatever the pick filter says. An arc's or circle's centre shows, and can
  be picked, only while the curve is hovered or picked.
- **Move to layer moves the whole thing:** a shape with its fill, block copies, texts, walls
  and openings. **Layer** is the first row of the Inspector for anything that has one, and shows
  **Mixed** for a mixed pick.
- **Tab** picks a chain only along curves on the same layer. **Zoom to selection** (Shift+Z) is
  in the right-click menu.
- **A transform box on one closed curve.** One picked circle, ellipse or closed pen curve gets
  the box. A circle scales evenly from every handle.
- **Trim against picked curves.** Pick curves before Trim or Extend, and only they cut or are
  reached.

## Walls, doors and windows

- **Walls.** Build → **Wall** (W) draws walls along a centreline, with real faces, corners and
  poché that are made again whenever the wall changes. Ends that meet join by themselves, as an
  L, a T or a cross. **Join walls** and **Unjoin walls** decide by hand; **Explode walls** turns
  them into plain lines and fills.
- **Dimension a wall's face.** A dimension or relation on a wall's face drives the wall.
- **Doors and windows.** Build → **Door / window** (Shift+O) places an opening in a wall. Space
  turns through the four swings; type the distance from the nearer corner, and Tab to the width.
  A picked opening slides along its wall and flips with the arrows on the canvas, and the
  Inspector sets its offset, width, swing and size set (US or metric). Openings follow their
  wall when it moves, splits or joins.
- **Walls from lines.** A plan drawn as pairs of face lines, as imported DWG, DXF and PDF plans
  are, becomes walls in one step: Build → **Walls from lines**. The Inspector lists what it
  skipped, with a **Pick** button for each. **Walls from curves** makes walls along picked
  centrelines.
- Moving, pasting, mirroring and patterning walls keeps their faces and room fills with them.

## Offsets that follow

- **An offset is one object.** Edit the curves it came from and it follows them, for every kind
  of curve: lines, arcs, circles, ellipses, pen curves and chains of them, open or closed.
- **One distance.** Type a value into its dimension, or drag any side, and the whole offset
  changes.
- **Detach offset** (Modify menu) turns it into plain curves that no longer follow.

## Fillets, relations and expressions

- **Fillet or chamfer several corners at once.** Shift-click corners, or pick a shape before
  pressing F to take all its corners, then drag or type a size: every corner gets it, in one
  undo step. If a corner hasn't room, nothing changes; the corner is marked and the status bar
  says what size fits them all.
- **Fillets keep what you measured.** A dimension or relation on a sharp corner carries on after
  you round or chamfer it: the corner is kept as a construction point. A midpoint or an equal
  length on a side that was cut short still counts from corner to corner. **Remove fillet**
  gives the sharp corner back.
- **Closing a shape keeps its direction.** Ending a line on a snapped point keeps the Horizontal
  or Vertical it was drawn with. **Fix** on a partly defined shape pins only what's still free.
- Trim, Extend and Split no longer drop a dimension on the end they cut away.
- **Expressions:** a plain number added to a length is in the drawing's unit, so `W + 2` in a
  feet-and-inches drawing adds 2 inches. It's stored as `W + 2 in`, so changing units later
  never changes the value.

## Menus, keys and commands

- **Draw, Modify and Annotate menus** list every tool and command, with their keys.
- **Repeat the last command** with Enter or Space while Select is idle.
- The command palette, menus, right-click menus and the shortcuts list now all come from one
  list, so they always agree.
- The thin-lines toggle moved to **Shift+Alt+W**.
- **The tool rail groups related tools.** Rectangle holds Polygon and Slot, the arcs share a
  place, Fillet holds Chamfer, and so on; a small triangle marks a family. Right-click it, or
  hold it down, for the rest. The rail goes **Draw, Build, Modify, Annotate**, like the menus,
  and fits a 1280×800 window. Move, Rotate and Pattern are on it.
- **A new Build menu** holds Wall, Door / window, Walls from curves, Walls from lines, Place
  block, and Join, Unjoin and Explode walls.
- **3-point arc leads the arcs.** A starts it, and A again steps through Tangent and Centre arc.
- **Help → Show logs folder** opens Drift's log and crash log, to attach to a report.
- Notices that aren't errors (such as "Copy removed") no longer show as errors.

## Options, groups and blocks

- **Options replace variants and snapshots.** In the Parameters panel, save the current
  state as an option: its values only, or the whole sketch. Apply one to switch to it, and
  turn on its eye to see it ghosted over the drawing.
- **Groups and blocks convert either way:** Make block from group, and Explode to group.
  A click on a fill in a grouped shape picks the group.
- **F2 renames** the picked block or group, and renaming from the Blocks panel works again.
- **Block base points.** Make block asks for the base point; Set base point moves it later
  without moving any copy.
- **Block editing stays in the block.** While a block or group is open for editing, clicks,
  Ctrl+A, box select, Fill and Trim see only what's inside it, and in a block, snapping too.
  Only Esc or **Finish editing** leaves.
- **What you make in a block edit goes into the block.** Offsets, walls, openings and pasted
  copies become the block's own curves when you finish. Undo and redo stay in the edit. Saving,
  exporting or printing mid-edit gives the drawing as if you'd finished.
- **Mirror and Pattern take block copies**, and Pattern takes texts and notes picked with the
  geometry too.
- **Esc after Shift+D or Paste** removes the copy instead of leaving it stacked on the original.
  Copies stacked exactly on one another count as overlapping, so Select overlapping finds them.
- **Copying a whole group gives a new group** ("Panel" becomes "Panel 2"). A block copy no
  longer snaps to itself while you move it. The block **Name** fields take spaces as you type.

## Bringing drawings in

- **One way in.** File → Import… takes SVG, PDF, Illustrator, DXF and DWG, with the same scale
  controls for each, as editable geometry or as an underlay to trace.
- **Hatches from DWG and DXF come in right.** Patterns come in at their real size; they came
  in at a size that depended on Drift's drawing scale. Each brings its own pattern, so
  AutoCAD's and your office's patterns come in exactly instead of as diagonal lines. Hatches
  inside blocks come in. Solid fills come in opaque (or as transparent as the file says),
  SOLID shapes such as dimension arrowheads are filled, and gradients take their first colour.
- **Blocks from DWG and DXF come in right.** Mirrored blocks, such as doors swung the other way,
  were turned 180° instead. Blocks stretched in one direction were stretched in both. Parts
  drawn "by block" now take each copy's colour. A dynamic block is named after itself
  instead of `*U12`. A block of the same name already in the drawing is only reused when it's
  the same. External references are named in the import report instead of vanishing.
- **Dropping a file** on the window asks whether to open it as a new drawing or import it into
  the one you have open.
- **Underlays show their text.** A PDF underlay's text is drawn as the outlines of its letters,
  at the right place, size and angle; DXF, DWG and SVG underlays show their text too.
- **PDF and Illustrator files** keep their layer order, and words with ligatures ("first",
  "office") come in whole.
- **Imported plans close properly.** Ends that stop a hair short of another curve join it,
  lines drawn twice become one, lines overlapping along each other are trimmed to meet, and
  hatches in blocks find their edges, so more rooms make areas you can fill.
- **Line weights stay as imported** when you reopen the drawing: a 0.09 mm line stays 0.09 mm.
- A damaged DWG that crashed Drift's DWG reader is refused with a message instead of
  crashing Drift.

## Settings

- **Preferences are simpler:** three pages (General, Drawing aids, Updates & Labs).
- **Rules that change the drawing travel with it.** How closed areas and fills behave (areas
  per layer, fills covering what's beneath, what happens when an edit divides a fill, the gap
  tolerance) moved to Drawing settings and is saved in the file. Older drawings keep the
  values they had on your computer.
- **View switches** live in the View menu and the status bar, and say what they did. Hidden
  dimensions or relations show a chip to bring them back, and come back at each start.

## Output

- **Export the model at 1 : 1** as SVG or PDF (File → Export), with or without dimensions and
  notes.
- **Export DWG.** File → Export → DWG / DXF… writes DWG or DXF for AutoCAD 2018 and later,
  and it comes out as a drawing, not a picture of one: true ellipses and curves, layers with
  their names, true colours, line types and weights, blocks with their fills, solid fills and
  hatches as real hatches (with their patterns, scale and angle), and text in any script.
  DXF R12 stays in the same dialog for laser cutters, CNC and older software. A block placed
  on several layers shows each copy in its own layer's colour, weight and line type.
- **Dashes and weights come in alike from every file.** A dashed line imports as the same
  line type from DXF, DWG, PDF and SVG, and weights keep AutoCAD's values (0.30 mm stays
  0.30 mm). R12 layers whose names differ only in case or punctuation stay apart.
- **Blocks from DWG and DXF keep their layers' colours.** Lines inside a block that sit on a
  layer of their own show that layer's colour, weight and line type, as in AutoCAD; they took
  the colour of the layer the copy was on.
- **DXF R12 opens in AutoCAD.** A drawing with an ellipse or a pen curve made a DXF that
  AutoCAD refused outright; those curves now go out as polylines within 0.01 mm of the curve.
  Accents and symbols in text (é, ½, m²) no longer turn into question marks.
- **Title block None** gives blank paper; the old None is now **Border only**.
- **Sheet views export cropped.** In PDF and SVG, each view on a sheet holds only what shows
  through its window; releasing the clipping mask in Illustrator no longer reveals the whole
  drawing.
- **Several sheet views** can be picked at once (Shift-click, Ctrl-click, a box, or Ctrl+A), and
  views have their own right-click menu with **Delete view…** and **Remove from sheet**.
- **Dimensions** start their extension lines past the geometry they run along, with grips to
  set a start by hand. Radius dimensions on arcs always point at the arc.

## Blender

- **Blender profiles:** blocks drawn in Drift, swept or lathed in Blender, with a live link
  while a profile block is open for editing.

## Safer files

- **Drawings from newer versions** open, and anything this version doesn't understand is kept
  when you save. If saving would lose something, Drift asks first.
- **A session that can't be restored** after a crash is kept as a file, and Drift says where.
- **Damaged or unusual files** to import are refused with a message instead of closing Drift.
- **Saves can't half-write a file.** Every save writes beside the file and swaps it in, and
  keeps the previous version as `<name>.drift.bak`. If a save fails, the file on disk is left as
  it was.
- **Each window keeps its own autosave.** After a crash, the next window offers what every other
  window left open: **Open them**, **Later** or **Delete them**.
- **An error in one command** no longer closes Drift: the edit is undone and Drift says so.
- **A drawing can't get a broken point.** Drift refuses a position that isn't a number and won't
  save one. While a file dialog is open, the drawing waits for it.
- Dialogs take the keyboard while open, and Esc closes them.
- **Smooth deep zoom:** zooming far in no longer slows drawing down.
- The AI server (drift-mcp) now ships with Drift; Preferences shows where it is and copies a
  Claude Desktop config for you.
- **Drift script** reads plain numbers in the drawing's unit, as typing into Drift does
  (inches in a feet-and-inches drawing), and every result says which unit that was.

<!--
PRs since v0.1.0-beta.1.5 (for the release notes' What's Changed):

User-facing
- #116 Radius dimensions on arcs always point at the arc (#115)
- #118 Dimension extension lines start past the geometry they run along; draggable starts
- #119 Model export at 1:1, block base points, blank-paper title block
- #124 Blender profiles: blocks drawn in Drift, swept or lathed in Blender
- #126 Ship drift-mcp with Drift's downloads
- #128 Fills follow their own outlines and the area around their seed (#120, #101)
- #132 F2 renames the picked block or group; fix renaming in the block menu (#130)
- #137 Fills behave like a drawing program's: stacked, drawn with shapes, kept with them
- #138 Shapes drawn as one are recorded as paths, and their fills follow them alone
- #140 Blender live link while a profile block is open for editing; add-on file filter fix
- #144 Simplify Preferences: drawing rules saved in the file, View toggles apart
- #145 One way in for drawings: Import… for every format, drop dialog
- #146 Drags solve from their start; free geometry skips the solver (Fixes #143, #103)
- #149 Solver: one "keep shape" preference ranked below the relations
- #152 Batch 1 hardening: dialogs own the keys, new-drawing rules, one import path, drag budget, fill fixes (Fixes #134, #148, #151)
- #156 Batch 2: print matches the screen, shapes as one object, smooth deep zoom (Fixes #150)
- #160 Batch 3: cheap document copies, design options, groups and blocks (Fixes #159)
- #163 Format fixtures and stability policy; importers never panic on bad input
- #164 Selection & move batch: hatch alignment, one move path, pick filters, layer Alt-click (Fixes #153, #154, #155, #158; drift-releases#9, #10)
- #166 Tolerant .drift reader: carry unknown data through, "reader" version (format 42) (Fixes #165)
- #167 Command registry: palette, menus, keys and shortcut docs from one table
- #169 Drift script: say which unit plain numbers were read in
- #172 PDF import: keep the file's layer order, read ligatures
- #173 Trim and Extend against picked curves; trim by a shape (script + MCP)
- #174 DXF export opens in AutoCAD
- #175 DXF/DWG import: blocks come in as AutoCAD shows them
- #176 Walls W1: wall runs, generated faces, joins from geometry
- #177 #178 DXF/DWG import: hatches come in as AutoCAD draws them
- #179 Tool rail: families with flyouts, a Build section, one order with the menus
- #180 Walls W2: relations on wall faces
- #181 Walls W3: doors and windows hosted in walls
- #189 Walls W1.5: Walls from lines
- #190 DWG and DXF R2018 export
- #196 One pick for the select tool; hover skips locked layers
- #197 Drag speed: index, fills, items and copy hover
- #199 CAD styles: block copies, R12 names, dashes and weights, block layers
- #209 Closing relations, partial Fix, snap markers (Fixes #204, #203, #200; format 43)
- #210 Fills: one rule for shapes, labels and Join corners; a trimmed ring keeps a cut ring (Fixes #201, #202)
- #211 Arc order, rail tooltips, view right-click menu (Fixes #206, #207, #208)
- #212 PDF underlays show their text; DXF/DWG/SVG underlay text too (Fixes #205)
- #220 Safer files: atomic saves, picker guard, no NaN, errors caught, a session per window, drift.log (Fixes #214–#219)
- #225 Box on one closed curve, trim wording, Edit fill (Fixes #221–#223)
- #229 Big drawings: faster frames and edits
- #233 Edit speed on big drawings
- #246 Snapping, group copies, offsets, sheet views, macOS opening (Fixes #232, #227, #235, #228)
- #252 Walls: faces follow a move; copies keep floors and mirror truly (Fixes #249, #250, #248)
- #254 Drag big drawings part by part
- #256 Keep imported line weights on reopen (Fixes #255)
- #268 Join imported ends onto curves, merge curves drawn twice (Fixes #184)
- #273 Walls: a room whose corner touches the outer loop stays a hole
- #274 Move to layer, Tab chains, groups, Zoom to selection, Views list (Fixes #266, #267, #261, #262, #263)
- #275 Fillet and chamfer keep what was measured to the corner (Fixes #238; format 44)
- #288 DWG import turns a crash in the reader into an error (Fixes #287)
- #309 Expressions: a bare number added to a length follows the drawing unit (Fixes #241)
- #310 Mirror mirrors block copies; Esc after Shift+D or Paste removes the copy; stacked copies overlap (Fixes #260)
- #315 Offsets as objects (Fixes #247)
- #316 Keep tools inside a block or group being edited (Fixes #265, #298, #299)
- #323 Block edits keep what they make, undo into edit mode, save and print finished (Fixes #318, #319, #322)
- #324 Fillet and Chamfer on several corners; typed distance in a mouse drag (Fixes #293, drift-releases#25)
- #325 Layer first in the Inspector, smooth sheet fills, Pattern takes block copies (Fixes #300, #301, #311, #320)
- #328 Import: block hatch edges, corners onto curves, overlapping strokes trimmed (Fixes #270, #271, #269)
- #329 Drag: an unfinished step isn't kept; a failing step is tried smaller (Fixes #281)
- #330 Export: each sheet view cropped to its window in PDF and SVG (Fixes #258)
- #331 Drag: a move carries the curve its dragged points are held on (Fixes #283)
- #332 Solver: fold steps let a stalled curve move (#282)
- #334 Relations: the active (last-picked) element stays, the others conform (Fixes #333)
- #335 Solver: a row that holds with no gradient left is left out of the step (#277)
- #339 Block name spaces, notice levels, Pattern takes texts and notes, centres only with their curve (Fixes #336, #312, #327, #259)
- #340 Drag: an arc crushed or closed by a step counts as turned (Refs #280)
- #341 Big drawings: Group, undo frames, area moves and the Inspector's offers stay quick (#317, #286, #321)

Internal / not for the notes
- #121 #122 packaging (Inno Setup 7, test build versions)
- #123 #125 #127 #141 #157 #162 website and licence pages
- #131 #161 dependency advisories
- #136 fill close-out tests; #139 #142 solver and region determinism tests
- #168 #230 #234 #236 design and plan documents; #170 guide pass; #171 smoke suite
- #213 cull and module split; #251 #257 #292 #302 #304 #308 core refactors R1–R3
- #226 GPU gate for UI tests; #276 green baseline and perf budgets
- #237 #272 CI and website workflow
-->
