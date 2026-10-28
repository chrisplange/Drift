# Coming from another program

Drift borrows from several programs. If you know one of them, this page tells you where
your habits carry over and where they don't.

Everywhere: the command palette (Ctrl+K) finds any command by name, and ? lists every key.

## From Revit

| In Revit | In Drift |
|---|---|
| Temporary dimensions on a picked wall | The same: pick one line and dashed dimensions appear. Click a value to type a new one; the lock makes it permanent; drag the far end's square to measure to something else. [Temporary dimensions](tutorials/05-temporary-dimensions.md). |
| Tab to cycle and pick chains | Tab highlights the chain of connected curves under the pointer; click picks it. |
| Thin lines (TL) | Shift+Alt+W, or the status bar. |
| Families | Blocks, with a library of US doors, windows, cabinets, fixtures and furniture, named "Family – Type". [Blocks](tutorials/07-blocks.md). No parametric blocks yet: each size is its own block. |
| Sheets, views, view titles, scales | Views are drawn on the canvas with the **View** tool (a named crop at a scale) and dragged onto sheets from the **Views** list. Reframe a view's crop and every sheet it's on follows. Unlike Revit, a view can go on several sheets. Each placed view has a title and a number. Architectural and engineering scales are there. [Sheets and output](tutorials/10-sheets-and-output.md). |
| Crop region, Crop Region Visible | A view's crop shows on the canvas with its name, with grips to reframe it. **View → View crops** (Shift+Alt+B) shows or hides them all, a view's **Crop** switch just that one. Crops are rectangles, not rotated, and never print. No separate annotation crop yet: dimensions and notes are cut at the crop. |
| Hide in view / hide on sheet | Each dimension and note can be off the canvas and / or off sheets. Not per view yet. |
| Drafting vs model patterns | The same distinction for hatches. |
| Dimension text: below, prefix / suffix, replace with text (EQ) | **Under line**, **Prefix** / **Suffix**, **Show instead**, and chips for TYP, EQ, VIF and others. |
| Section and elevation marks | **Section mark** and **Elevation mark**, with the sheet number in the flag. They don't create views. |
| Constraints (padlocks, EQ) | Full sketch relations and driving dimensions, as in SolidWorks (below). |

What's different: Drift is 2D only. Walls are drawn as 2D walls with joins, doors and windows
([Walls](tutorials/14-walls.md)), but there are no rooms, levels, schedules or wall types.

## From AutoCAD

| In AutoCAD | In Drift |
|---|---|
| Command line | The command palette (Ctrl+K), plus one-key tools: L line, C circle, A arc, R rectangle, T trim, E extend, O offset, M mirror, F fillet. |
| Typing a distance after picking a direction | The same: start drawing, point the way, type the length, Enter. Tab moves to the angle. |
| Ortho (F8) / polar tracking | X and Y lock the segment to an axis; **Angle snap** in the status bar steps the angle (1°, 5°, 15°, 45°). |
| Object snaps | End, mid, centre, quadrant, nearest and the origin, always on; perpendicular and tangent while drawing lines (**Perpendicular & tangent** in the status bar). Hold Alt to place without snapping. Intersection isn't a snap yet. |
| Object snap tracking | Inference lines: hover a point, move away, and a dashed guide shows when you're level with it or above it. |
| Layers, ByLayer | The same model: colour, line weight and line type by layer unless set on the object. Layer **0** always exists. |
| Blocks, INSERT, BEDIT, BASE, EXPLODE | **Make** block (then click its base point), **Place block** (I), double-click a copy to edit in place, **Set base point**, **Explode**. **Make block from group** and **Explode to group** turn groups and blocks into each other. Blocks go to and from DXF. |
| BHATCH, pick points | **Fill / hatch** (H): click inside an area. Hatches are associative. `.pat` files import. |
| Paper space layouts and viewports | Sheets and views. A DXF's layouts come in as sheets. |
| MOVE, ROTATE with base point | G and R with the selection, B to pick the base point or pivot; type values. |
| COPY | Shift+D, or Ctrl+C / Ctrl+V. |
| PEDIT join / OVERKILL | **Edit → Check sketch…** joins gaps; the status bar finds overlapping curves. |
| DWG | **File → Import…** reads DWG (R14 to 2018) as DXF is read; **File → Open…** opens one as a new drawing. **File → Export → DWG / DXF…** writes DWG or DXF for AutoCAD 2018 and later (or DXF R12). |

What's different: geometry in Drift can carry constraints. With **Auto relations** on, a
line drawn nearly level becomes exactly horizontal and stays that way. Turn **Auto
relations** off (its switch in the status bar, or Shift+Alt+R) to draw plain, unconstrained
geometry, closer to AutoCAD.

## From SolidWorks

Drift's sketcher is modelled on the SolidWorks sketch.

| In SolidWorks | In Drift |
|---|---|
| Smart Dimension | **Dimension** (D): click geometry, place the label; the kind follows. Type the value straight away. |
| Add Relations | The **Inspector**'s **Add relation** buttons, the right-click menu, or the relations pie (Q). |
| Inferred relations | Lines snap straight and get Horizontal / Vertical. Drawing on from a line, arc, partial ellipse or pen curve, guides show square to the line or along the curve's tangent, and add Perpendicular or Tangent. |
| Blue / black / red status | The same colours (white instead of black on the dark theme), and the DOF count in the status bar. |
| Driven dimensions | Offered automatically when a dimension would over-define the sketch; switch with **Driven (reference)**. |
| Equations / global variables | **Parameters → Variables**, with sliders. Link a dimension by typing a variable's name into it. |
| Configurations | **Options** (values or the whole sketch), with ghosts. |
| Construction geometry (For construction) | Alt+C. |
| Sketch fillet / chamfer | F (F again for chamfer). Size on the canvas by moving the pointer, or type it. |
| Convert entities, offset, mirror, patterns | Offset (O), Mirror (M), linear and circular patterns in the **Inspector**. |
| Repair sketch | **Edit → Check sketch…** |
| Drawings | Sheets with views, title blocks (modern, ISO 7200, ASME Y14.1, border only, or blank paper) and ISO or ASME dimension styles. |
| Splines | The pen tool (P) draws Bézier curves the Illustrator way. |

What's different: there's one sketch, no features or 3D. Drift adds Blender-style keys for
moving and rotating (G, R, X / Y, B), and Revit-style temporary dimensions.

## From Illustrator or Affinity

| In Illustrator / Affinity | In Drift |
|---|---|
| Pen tool | The pen (P) works the same: click for corners, drag for smooth points, Alt breaks a handle, click the first point to close, Enter or Esc ends an open path. Alt-click a point to make it smooth or a corner. |
| Direct selection of anchors and handles | Handles show on picked or hovered curves; drag anchors and handles directly. |
| Stroke weight and colour | The top bar's **Stroke** colour, weight and line type; **Fill** colour. A filled shape is one object in the **Inspector**, with a **Stroke** (or none) and a **Fill**. |
| Arrange (bring to front, send backward) | **Modify → Arrange**: Ctrl+] and Ctrl+[ move a fill or shape one step, with Shift all the way. |
| Artboards | Sheets, at real paper sizes and true scale. |
| Align and distribute | Not yet. Use relations (horizontal, vertical, symmetric, equal) instead; they keep things aligned as you edit. |
| Transform box | Corner and side handles scale and stretch; just outside a corner turns. Shift keeps the shape, Ctrl scales about the middle. |
| Smart guides | Inference lines and snapping. |
| Space to pan | Middle or right drag, or two fingers on a trackpad. |

What's different: everything has a real size, in millimetres or inches, and is placed by
typed values and constraints rather than by eye. Dimensions you add hold the geometry.

## Next

Start with [Your first sketch](tutorials/01-first-sketch.md).
