# Tools

Every tool in the tool palette, with its key. Pressing a tool's key again cycles through
related tools where the table says so. The active tool's options appear in the top bar, and
its next step is always in the status bar's hint.

Esc cancels the step in progress; pressed again, it returns to **Select**.

## Select

| Tool | Key | What it does |
|---|---|---|
| **Select** | S | Pick, drag and box-select. Its top bar shows the pick filters (toggles for **Points**, **Edges**, **Fills**, **Areas**, **Blocks**, **Text** and **Annotations**, which combine; Alt-click one for that kind alone; **All**) or, with something picked, the selection's layer, stroke, fill, **Construction**, **Actions** and **Delete**. G moves the selection, R rotates it. See [Editing](../tutorials/04-editing.md). |

## Draw

Every drawing tool's top bar starts with **Geometry** / **Construction** (Alt+C). Each
takes typed values: the first point as X, Tab, Y; then the sizes listed below. Typed sizes
become dimensions when **Auto relations** is on.

| Tool | Key | Steps | Typed values |
|---|---|---|---|
| **Line** | L | Click point after point; Enter, Esc or right-click ends the chain. X / Y lock the segment to an axis. A switches to a tangent arc from the last point, L back to lines. | Length, Tab, angle. |
| **Pen** | P | Click for a corner point, press and drag for a smooth one (Alt while dragging breaks the handle). Click the first point to close; Enter or Esc ends an open path. Alt-click a point between two curves to make it smooth or a corner. Top bar: **Curvature combs**. | — |
| **Rectangle** | R | Two opposite corners. (With a selection and the **Select** tool, R rotates instead.) | Width, Tab, height. |
| **Polygon** | K | Centre, then a corner (or the middle of a side, circumscribed). X switches inscribed / circumscribed. Top bar: **Sides** (3 to 64) and **Inscribed** / **Circumscribed**. | Radius (centre to corner; centre to the middle of a side when circumscribed), Tab, sides. It becomes the construction circle's diameter dimension, or a centre-to-side distance. |
| **Circle** | C | Centre, then a point on the circle. | Diameter. |
| **Ellipse** | C, C | Centre, the end of one axis, then the other axis. | Half-lengths. |
| **Partial ellipse** | C, C, C | As the ellipse, then where it starts and ends (anticlockwise). | Half-lengths. |
| **Arc** | A | Centre, start (sets the radius), end. Move round the centre to choose the direction. | Radius, then sweep. |
| **3-pt arc** | A, A | Start, end, then a point on the arc. | Chord length, Tab, angle; then radius. |
| **Tangent arc** | A, A, A | Starts from the end of a line, arc, partial ellipse or pen curve, tangent to it. Chains like a polyline; L switches to lines. | Radius, Tab, sweep. |
| **Slot** | U | **Straight** (two end centres, then the width), **Centre** (the middle, then an end centre) or **Arc** (the arc's centre, the two end centres, then the width). U again cycles; also in the top bar. | Length, then width. |
| **Point** | V | Click to place a point. | X, Tab, Y. |
| **Text** | Shift+T | Click where the text starts (its baseline), or in an existing text, and type. Enter finishes; Shift+Enter starts a new line. Top bar: **Height**. | — |
| **Place block** | I | Copies of the block picked in the **Blocks** panel follow the pointer; click to place. R turns the next copy 90°, X mirrors it. | — |

## Annotate

| Tool | Key | What it does |
|---|---|---|
| **Dimension** | D | Click geometry, then place the label; the kind follows what you clicked and where the label goes. Top bar: **Single** / **Chain** / **Baseline**, **Overall**, **Show dimensions**, **Show relations**. See [Constraints and dimensions](../tutorials/02-constraints-and-dimensions.md). |
| **Ordinate** | — | Click the origin point, then points; each gets its X distance in a row. Y / X switch the coordinate. |
| **Note** | N | Click for a note, or click geometry first for a leader to it. Type; Enter finishes, Shift+Enter adds a line. |
| **Callout** | — | Click a hole for `4× Ø6 THRU`, or a chamfer line for `2 × 45°`, then where the text goes. |
| **Centre mark** | — | Click a circle or arc for a centre mark; then another circle, arc or point for a centre line between them. Two lines give a centre line midway. |
| **Section mark** | — | Two clicks for the cutting line, with a lettered flag at each end. X / Y keep it to an axis. |
| **Elevation mark** | — | Click where it goes, then towards what it looks at. X / Y keep it to an axis; Alt allows any angle. |
| **View** | — | Drag a rectangle, or click two corners: a view of the model with that crop, at the last view's scale, to place on sheets. It's picked, with its grips, to name and reframe. See [Sheets and output](../tutorials/10-sheets-and-output.md#3-draw-a-view-on-the-canvas). |
| **Fill / hatch** | H | Click inside a closed area to fill or hatch it with the style in the top bar. Alt+click a fill to pick up its style. The Rectangle, Polygon, Circle, Ellipse, Slot and Pen tools' bars have a **Fill** switch that fills closed shapes as they are drawn, in the same style. See [Fills and hatches](../tutorials/06-fills-and-hatches.md). |

## Modify

| Tool | Key | What it does |
|---|---|---|
| **Trim** | T | Click a piece to remove it, up to the curves that cut it; or drag across pieces to trim several. |
| **Extend** | E | Click near the end of a line or arc to lengthen it to the next curve. |
| **Split** | — | Click a curve where it should be cut in two (a circle or ellipse into halves). |
| **Fillet** | F | Click a corner where two lines meet (or an existing fillet), move to size it, click to apply; or type the radius. F again switches to **Chamfer**. |
| **Chamfer** | F, F | As the fillet; type the distance, Tab for a second distance. |
| **Offset** | O | Click a curve (its connected chain is taken too), then the side; or type the distance. |
| **Mirror** | M | Pick geometry first; then click the line to mirror it about. The copies are held symmetric. |

## Tools started from panels

| Tool | Started from | What it does |
|---|---|---|
| **Calibrate underlay** | **Layers** panel, **Calibrate…** | Two points on the underlay, then the real distance between them. |
| **Move underlay** | **Layers** panel, **Move…** | A point on the underlay, then where it goes. |

## Commands without a tool

Some actions work on the selection instead of being tools: move (G), rotate (R), duplicate
(Shift+D), group (Ctrl+G), make block, explode, linear and circular patterns (the
**Inspector**'s **Pattern** section), clear relations and dimensions, and check sketch
(**Edit → Check sketch…**). All are in the command palette (Ctrl+K).
