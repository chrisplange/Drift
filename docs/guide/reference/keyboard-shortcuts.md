# Keyboard shortcuts

Press ? in Drift (or **Help → Keyboard shortcuts**) for this list, with a filter. On a Mac,
use Cmd for Ctrl and Option for Alt.

Keys aren't configurable yet.

## Tools

| Key | Tool |
|---|---|
| S or Esc | **Select** |
| L | **Line** (polyline) |
| P | **Pen**: click for corners, drag for smooth points |
| R | **Rectangle** (with a selection and the **Select** tool: rotate) |
| K | **Polygon**; X switches inscribed / circumscribed |
| C | **Circle**; again for **Ellipse**, then **Partial ellipse** |
| A | **Arc** (centre point); again for **3-pt arc**, then **Tangent arc** |
| U | **Slot**; again for centre and arc slots |
| V | **Point** (a vertex) |
| Shift+T | **Text** |
| I | **Place block**: R turns, X mirrors |
| D | **Dimension** |
| N | **Note** |
| H | **Fill / hatch** |
| T | **Trim** |
| E | **Extend** |
| O | **Offset** |
| M | **Mirror** |
| F | **Fillet**; again for **Chamfer** |

**Split**, **Ordinate**, **Callout**, **Centre mark**, **Section mark** and **Elevation
mark** have no key: use the tool palette or the command palette.

## While drawing

| Key | Does |
|---|---|
| X / Y | Lock the segment to the X or Y axis (press again to unlock) |
| 0–9 . - ( | Start typing a value |
| = | Start typing with a variable name or expression, such as `=width/2` |
| Tab | Next field (length → angle, width → height…) |
| Enter | Apply the typed values; with nothing typed, finish a polyline |
| Right-click | Finish a polyline |
| A / L | In a polyline: switch to a tangent arc, or back to lines, from the last point |
| Alt (hold) | Place without snapping |
| Alt+C | Draw construction geometry, or geometry again |
| Esc | Stop typing, cancel the step, or (pressed again) go back to **Select** |

Units work in typed values: `25mm`, `2in`, `3"`, `1'-6 1/2"`, `30deg`. Bare numbers use the
display unit.

## Dimensions and annotation

| Key or action | Does |
|---|---|
| D | **Dimension** tool |
| Shift+click (after a horizontal or vertical dimension) | Chain dimensions along the row (in **Single** mode) |
| B (in a row) | Switch between chain and baseline |
| Double-click a dimension | Edit its value |
| Drag a dimension | Move its label |
| N | **Note**; Enter finishes, Shift+Enter adds a line |
| Alt+D | Show or hide dimensions and notes |
| Alt+R | Show or hide relation symbols |
| Alt+H | Show hidden annotations, faded, to pick them again |

## Selection and editing

| Key or action | Does |
|---|---|
| Click / drag | Select, or box-select (right: window, left: crossing) |
| Shift+click | Add to the selection |
| Ctrl+click | Toggle in or out of the selection |
| Ctrl+A | Select all |
| Tab | Highlight the chain under the pointer (click to pick it); with nothing under the pointer, widen the selection to whole chains |
| Click near a line's middle | Pick its midpoint |
| G | Move: X / Y lock an axis, type dX Tab dY (or a distance when locked), B picks the base point |
| R (with a selection) | Rotate: type an angle, B picks the pivot |
| Shift+D | Duplicate in place and move the copy |
| Arrow keys | Nudge one grid step; Shift+arrow, ten |
| Q | Relations pie: hold and flick, or tap and click |
| Alt+C | Toggle construction on the picked curves |
| Ctrl+G / Ctrl+Shift+G | Group / ungroup; double-click a group to enter it |
| Double-click text | Type into it in place |
| Ctrl+C / Ctrl+X / Ctrl+V | Copy, cut, paste under the pointer |
| Ctrl+Shift+V | Paste in place |
| Delete or Backspace | Delete the selection, or the picked relation or dimension |
| Esc or right-click | Cancel the operation, or clear the selection |
| Right-click (Select tool) | The canvas menu |

In a move or rotate (G, R): click or Enter places, Esc or right-click cancels.

## View

| Key or action | Does |
|---|---|
| Shift+F or double middle-click | Zoom to fit |
| Shift+Z | Zoom to the selection |
| Middle or right drag | Pan |
| Scroll / pinch | Zoom at the pointer (two fingers on a trackpad pan) |
| Shift+G | Snap to the grid |
| Ctrl+' | Show or hide the grid |
| Shift+C | Constraint status colours, or curves' own and their layers' colours |
| W | Thin lines: every line one pixel wide |
| Shift+Alt+B | Show or hide views' crops and names (**View → View crops**) |

In the sheet preview: Shift+F fits the sheet, Delete takes the picked view off the sheet
(it stays in the drawing), Esc returns to the model.

On the canvas, with a view's crop picked: Delete deletes the view (asking first if it's on a
sheet), Esc lets it go.

## Panels and files

| Key | Does |
|---|---|
| Ctrl+K or Ctrl+Shift+P | Command palette |
| Ctrl+L | Show or hide the **Layers** panel |
| Ctrl+\ | Hide all panels, and bring them back |
| Ctrl+Z | Undo |
| Ctrl+Y or Ctrl+Shift+Z | Redo |
| Ctrl+S / Ctrl+Shift+S | Save / save as |
| Ctrl+N / Ctrl+O | New / open |
| Ctrl+, | Preferences |
| Ctrl+P | Print (the sheets as a PDF) |
| ? | The shortcuts list |
