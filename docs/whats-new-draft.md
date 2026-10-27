# What's new since Beta 1.5 (draft)

A running draft for the next release's notes and the website's What's new. It lives outside
`docs/guide`, so the site doesn't publish it. Grouped by what you notice; the PR list for the
release notes is in the comment at the end.

Drawings saved by this version are file format 42 (Beta 1.5 wrote 33). Older drawings open
as before.

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

## Menus, keys and commands

- **Draw, Modify and Annotate menus** list every tool and command, with their keys.
- **Repeat the last command** with Enter or Space while Select is idle.
- The command palette, menus, right-click menus and the shortcuts list now all come from one
  list, so they always agree.
- The thin-lines toggle moved to **Shift+Alt+W**.

## Options, groups and blocks

- **Options replace variants and snapshots.** In the Parameters panel, save the current
  state as an option: its values only, or the whole sketch. Apply one to switch to it, and
  turn on its eye to see it ghosted over the drawing.
- **Groups and blocks convert either way:** Make block from group, and Explode to group.
  A click on a fill in a grouped shape picks the group.
- **F2 renames** the picked block or group, and renaming from the Blocks panel works again.
- **Block base points.** Make block asks for the base point; Set base point moves it later
  without moving any copy.

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

Internal / not for the notes
- #121 #122 packaging (Inno Setup 7, test build versions)
- #123 #125 #127 #141 #157 #162 website and licence pages
- #131 #161 dependency advisories
- #136 fill close-out tests; #139 #142 solver and region determinism tests
-->
