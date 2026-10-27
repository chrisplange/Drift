# Keyboard shortcuts

Press ? in Drift (or **Help → Keyboard shortcuts**) for this list, with a filter. On a Mac,
use Cmd for Ctrl and Option for Alt.

Keys aren't configurable yet.

The command palette (Ctrl+K) finds every command by name, and takes AutoCAD's command names
and aliases too: MOVE or M, ROTATE or RO, SCALE or SC, TRIM or TR, EXTEND or EX, OFFSET or
O, MIRROR or MI, ARRAY or AR, FILLET or F, CHAMFER or CHA, LINE or L, CIRCLE or C,
RECTANGLE or REC, DIMENSION or DIM, HATCH or H. Where an alias is also one of Drift's own
keys, Drift's key comes first: M in the palette is **Mirror**, as the M key is; type MOVE
for **Move**.

With nothing in hand (the **Select** tool, nothing being moved or drawn), Enter or Space
does the last command or tool again, as in AutoCAD.

<!-- BEGIN commands: made from crates/drift-app/src/commands.rs -->

## File

| Key | Command | Does |
|---|---|---|
| Ctrl+N | **New sketch** | Start a new, empty drawing |
| Ctrl+O | **Open…** | Open a Drift sketch, or a drawing to import |
| Ctrl+S | **Save** | Save the drawing |
| Ctrl+Shift+S | **Save as…** | Save the drawing under a new name |
| Ctrl+P | **Print…** | Print the sheets (as a PDF) |

## Edit

| Key | Command | Does |
|---|---|---|
| Ctrl+Z | **Undo** | Undo the last change |
| Ctrl+Shift+Z / Ctrl+Y | **Redo** | Redo what was undone |
| Ctrl+X | **Cut** | Copy the selection and delete it |
| Ctrl+C | **Copy** | Copy the selection; works between windows |
| Ctrl+V | **Paste** | Under the pointer, or in the middle of the view |
| Ctrl+Shift+V | **Paste in place** | Where it was copied from |
| Del / Backspace | **Delete selection** | Delete the selection, or the picked relation or dimension |
| Alt+C | **Toggle construction** | Make the picked curves construction geometry, or geometry again (with a drawing tool: draw construction geometry) |
| Ctrl+G | **Group** | Group the picked curves; double-click a group to enter it |
| Ctrl+Shift+G | **Ungroup** | Ungroup the picked groups |
| Shift+D | **Duplicate** | Copy the selection in place, then move the copy |
| F2 | **Rename block or group…** | Rename the picked group, or the block of the picked copies |
| Ctrl+A | **Select all** | Pick everything the selection filter lets through |
| S | **Select** | Pick, drag and box-select geometry; G moves the selection |
| Ctrl+, | **Preferences…** | Theme, pointer, drawing aids and updates |

## View

| Key | Command | Does |
|---|---|---|
| Ctrl+K / Ctrl+Shift+P | **Command palette…** | Any tool, setting, panel or command, by name |
| Shift+F | **Zoom to fit** | Fit the view to the drawing (or double middle-click) |
| Shift+Z | **Zoom to selection** | Fit the view to what is picked |
| Ctrl+' | **Grid** | Show or hide the grid |
| Shift+G | **Snap to grid** | Snap to the grid |
| Shift+Alt+W | **Thin lines** | Thin lines: every line one pixel wide |
| Shift+C | **Constraint status colours** | Colour curves by constraint status, or by their own and their layer's colours |
| Shift+Alt+V | **Always show end points** | Show every end point, or only those of hovered and picked curves |
| Alt+D | **Dimensions and notes** | Show or hide dimensions and notes (shown again at each start) |
| Alt+H | **Hidden annotations (faded)** | Show hidden dimensions and notes, faded, to pick and show them again |
| Alt+R | **Relations** | Show or hide relation symbols |
| Shift+Alt+B | **View crops** | Show or hide views' crops and names on the canvas |
| Shift+Alt+R | **Automatic relations** | Add relations automatically while drawing, or not |
| Ctrl+\ | **Hide all panels** | Hide every panel for a clear view, and bring them back |

## Draw

| Key | Command | Does |
|---|---|---|
| L | **Line** | Draw connected lines; type a length, X / Y lock an axis |
| P | **Pen** | Curves: click for corners, drag for smooth points (Alt breaks the handle) |
| R | **Rectangle** | Two opposite corners, or type the width and height |
| K | **Polygon** | Regular polygon: centre, then a corner. Tab types sides; X: inscribed / circumscribed |
| U | **Slot** | Straight, centre or arc slot (press U again to switch), then width |
| C | **Circle** | Centre, then a point on the circle (or type the diameter). C again switches through: circle, ellipse, partial ellipse |
| A | **3-point arc** | Start, end, then a point on the arc. Press A again for other arc types |
| V | **Point** | A sketch point, for reference or construction |
| Shift+T | **Text** | A line of text drawn as outlines, convertible to curves |
| H | **Fill / hatch** | Fill or hatch a closed area: click inside it |

## Build

| Key | Command | Does |
|---|---|---|
| W | **Wall** | Walls along a centreline: they join the walls they meet. W again: arcs; Space: the other face |
| Shift+O | **Door / window** | Doors, windows and openings in walls: hover a wall, click to place. Space: the way it opens; type the offset, Tab: width |
| I | **Place block** | Place copies of a block from the Blocks tab; R turns, X mirrors |

With no key (use the Build menu or the command palette): **Walls from curves**, **Walls from lines**, **Join walls**, **Unjoin walls**, **Explode walls**.

## Modify

| Key | Command | Does |
|---|---|---|
| G | **Move** | Move the selection: X / Y lock an axis, type dX Tab dY, B picks the base point |
| R (with a selection) | **Rotate** | Turn the selection: type an angle, B picks the pivot, R again rotates by reference |
| M | **Mirror** | Mirror the selection about a line, kept symmetric |
| O | **Offset** | A parallel copy of a curve and the curves connected to it |
| T | **Trim** | Remove a piece up to the curves that cut it; drag to trim several |
| E | **Extend** | Lengthen a line or arc to the next curve |
| F | **Fillet** | Round the corner where two lines meet. F again switches through: fillet, chamfer |

With no key (use the Modify menu or the command palette): **Scale**, **Join corners**, **Split**, **Linear pattern**, **Circular pattern**.

## Annotate

| Key | Command | Does |
|---|---|---|
| D | **Dimension** | Click geometry, then place the label: the type follows |
| N | **Note** | Text, with a leader if you start on geometry |

With no key (use the Annotate menu or the command palette): **Ordinate**, **Callout**, **Section mark**, **Elevation mark**, **Centre mark**, **View**, **Align picked dimensions**, **Space picked dimensions evenly**, **Hide picked dimensions and notes on the canvas**, **Leave picked dimensions and notes off sheets**.

## Arrange

| Key | Command | Does |
|---|---|---|
| Ctrl+Shift+] | **Bring to front** | Bring the picked fills and shapes to the front of their layer |
| Ctrl+] | **Bring forward** | Bring the picked fills and shapes forward one step on their layer |
| Ctrl+[ | **Send backward** | Send the picked fills and shapes backward one step on their layer |
| Ctrl+Shift+[ | **Send to back** | Send the picked fills and shapes to the back of their layer |

## Layers

| Key | Command | Does |
|---|---|---|
| Ctrl+L | **Layers panel** | Layers and underlays |

## Help

| Key | Command | Does |
|---|---|---|
| ? | **Keyboard shortcuts** | This list, with a filter |

## While drawing

| Key | Does |
|---|---|
| X / Y | Lock the segment to the X or Y axis (press again to unlock) |
| 0–9 . - ( | Start typing a value |
| = | Start typing with a variable name or expression, such as `=width/2` |
| Tab | Next field (length → angle, width → height…) |
| Enter | Apply the typed values; with nothing typed, finish a polyline |
| Right-click | Finish a polyline |
| A / L | In a polyline: switch to a tangent arc, or back to lines, from the last point; drawing walls: an arc or a straight segment next |
| W | Drawing walls: straight or arc segments |
| Space | Drawing walls: the other face (Left ↔ Right) |
| Backspace | Drawing walls: take back the last segment |
| Alt (hold) | Place without snapping (also no perpendicular or tangent snaps) |
| Esc | Stop typing, cancel the step, or (pressed again) go back to Select |

## Selection

| Key | Does |
|---|---|
| Click / drag | Select, or box-select (right: window, left: crossing); Shift adds, Ctrl toggles |
| Tab | Highlight the chain under the pointer (click to pick it); with nothing under the pointer, widen the selection to whole chains |
| Click near a line's middle | Pick its midpoint |
| Arrow keys | Nudge one grid step; Shift+arrow, ten |
| Q | Relations pie: hold and flick, or tap and click |
| Double-click text | Type into it in place |
| Enter or Space | With nothing in hand: the last command or tool again |
| Esc | Cancel the operation, or clear the selection; with a dialog or window open, close it (and nothing else) |
| Right-click | In a drawing tool, finish (as Enter does); in Select, the canvas menu; in a move or rotate, cancel |

## Dimensions

| Key | Does |
|---|---|
| Shift+click (after a horizontal or vertical dimension) | Chain dimensions along the row; B switches to baseline |
| Double-click a dimension | Edit its value |
| Drag a dimension | Move its label |

## Mouse

| Key | Does |
|---|---|
| Double middle-click | Zoom to fit |
| Middle or right drag | Pan |
| Scroll / pinch | Zoom at the pointer (two fingers on a trackpad pan) |
| Drag a panel's tab | Onto the canvas to float it, or onto either side to dock it |

## Sheet preview

| Key | Does |
|---|---|
| V | Add a view, by its region in the model |
| Shift+F | Fit the sheet |
| Delete or Backspace | Take the picked view off the sheet (it stays in the drawing) |
| Esc | Back to the model |

<!-- END commands -->

Units work in typed values: `25mm`, `2in`, `3"`, `1'-6 1/2"`, `30deg`. Bare numbers use the
display unit.

In a move or rotate (G, R): click or Enter places, Esc or right-click cancels. While a
dialog or window is open (Preferences, Drawing settings, Check sketch, an import…), the
canvas takes no keys: Delete, Ctrl+Z and the tool keys wait until it is closed.

The view settings (Shift+G, Shift+C, Shift+Alt+W, Alt+D…) each say in the status bar what they just
did.

On the canvas, with a view's crop picked: Delete deletes the view (asking first if it's on a
sheet), Esc lets it go.
