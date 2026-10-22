# Preferences

Open **Edit → Preferences…** (Ctrl+,). Preferences are how Drift works on this computer:
they are remembered between sessions and are the same for every drawing.

Two other kinds of setting live elsewhere:

- **What the canvas shows** (grid, origin axes, dimensions, relations, thin lines, status
  colours and so on) is switched in the **View** menu and the status bar; see
  [View switches](#view-switches) below.
- **What a drawing is** (its units, how its closed areas and fills behave, its drafting
  style) is saved with it, in **File → Drawing settings…**; see
  [Drawing settings](#drawing-settings) below. So the same file fills, picks and prints the
  same on every computer.

The window has three pages, listed down its left side:

| Page | Holds |
|---|---|
| **General** | [Appearance](#appearance), [Crosshair](#crosshair) and [Mouse and trackpad](#mouse-and-trackpad). |
| **Drawing aids** | [While drawing](#while-drawing) and [Picked geometry](#picked-geometry). |
| **Updates & Labs** | [Updates](#updates) and [Labs](#labs) (desktop only). |

## Appearance

The light or dark theme, or follow the system. (Also **View → Appearance**.)

## Crosshair

While a drawing or editing tool is over the canvas, Drift draws its own crosshair in place
of the system cursor (whose size and weight can't be set).

| Setting | What it does |
|---|---|
| **Size** | How far each arm reaches from the centre, from 6 to 200 pt. |
| **Full screen** | Lines across the whole canvas, as in AutoCAD (**Size** is then unused). |
| **Weight** | The lines' thickness, from 0.5 to 3 pt. |

## Mouse and trackpad

**Scrolling**: **Automatic** (a mouse wheel zooms, two fingers on a trackpad pan, a pinch
zooms), **Zoom** (scrolling always zooms) or **Pan** (scrolling always pans; Ctrl+scroll
zooms). The middle or right mouse button drags the view either way.

## While drawing

| Setting | What it does |
|---|---|
| **Add relations automatically while drawing** | Horizontal, vertical, coincident, tangent and so on, as you snap. Also the **Auto relations** switch in the status bar (Shift+Alt+R). |
| **Snap to the grid** | Also Shift+G and the status bar. The arrow keys nudge the selection one grid step. |
| **Snap perpendicular and tangent** | Lines snap square onto lines, circles and arcs, or tangent to circles and arcs, and follow guides. Also the status bar and the canvas's right-click menu. |
| **Angle snap** | Off, 1°, 5°, 15° or 45°: lines are drawn at those steps (Alt places freely). Also the status bar. |
| **Round sizes when scaling with dimensions (Alt)** | With a transform handle and Alt, the width and height step through round sizes. Off: any size. |

## Picked geometry

| Setting | What it does |
|---|---|
| **Show temporary dimensions on the picked geometry** | Revit-style temporary dimensions when one line, circle or arc is picked. See [Temporary dimensions](../tutorials/05-temporary-dimensions.md). |
| **Always show end points** | Off: end points show only on hovered or picked curves. Also the **End points** switch in the status bar (Shift+Alt+V). |

## Updates

**Check for updates when Drift starts**, **Check now**, and a skipped version to offer
again. See [Getting started](../getting-started.md).

## Labs

Features still taking shape, which may change between versions.

**Let AI assistants edit the open drawing** turns on the live link (desktop only). Claude,
or another assistant connected through `drift-mcp`, then works on the drawing in this
window, and you see each change as it is made. It is off until you turn it on. See
[Drawing with AI assistants](../ai/assistants.md).

**Server** shows where the installed `drift-mcp` is. **Copy Claude Desktop config** copies
the entry to paste into Claude Desktop's `claude_desktop_config.json`, with `--root` set to
your Documents folder.

## View switches

These are in the **View** menu (and most in the status bar), not in Preferences. Each one
says in the status bar what it just did.

| Switch | Keys | Kept for the next start? |
|---|---|---|
| **Grid** | Ctrl+' | Yes |
| **Origin axes** | | Yes |
| **Snap to grid** | Shift+G | Yes |
| **Thin lines** (every line one pixel wide; never changes the output) | W | Yes |
| **Constraint status colours** (off: each curve's own or its layer's colour) | Shift+C | Yes |
| **Always show end points** | Shift+Alt+V | Yes |
| **Dimensions and notes** | Alt+D | No: shown again at each start |
| **Hidden annotations (faded)** | Alt+H | Yes |
| **Relations** | Alt+R | No: shown again at each start |
| **Curvature combs** | | Yes |
| **View crops** | Shift+Alt+B | Yes |

While dimensions or relations are hidden, the status bar shows **Dimensions hidden ×** or
**Relations hidden ×**; click it to show them again.

Also in the menus:

- **View → Panels → Reset panel layout** puts every panel back in the right dock.
- **File → Show the start panel** brings the start panel back over an empty, unsaved
  sketch.
- **Help → About Drift** shows how much memory Drift is using. Drift warns at 3 GB and
  closes itself at 8 GB, so a fault can't exhaust the computer; the sketch is restored
  next time.

## Drawing settings

**File → Drawing settings…** (also in the command palette) holds what is saved in the open
drawing rather than as a preference:

- **Project:** the **Name**, **Number**, **Client**, **Address** and **Company** every
  sheet's title block shares, and the **Title block fields** (**Mechanical** or
  **Architectural**). See [The project](../tutorials/10-sheets-and-output.md#the-project).
- **Units:** **Metric** or **Imperial**, the **Format**, **Decimals** or the fraction to
  **Round to**, and **Angles** decimals. See [Units and length formats](units.md).
- **Closed areas and fills:** see [below](#closed-areas-and-fills).
- **Drafting:** **ISO 129** or **ASME Y14.5**, then scale, text height, **Arrows**,
  **Arrow size**, **Ext. gap**, **Ext. past**, text placement, **Zeros**, **Dual units**,
  **Colour** and the drawing's **Font**. A picked dimension or annotation can override
  these for itself; see [The drafting style](../tutorials/09-annotation.md#the-drafting-style).

### Closed areas and fills

| Setting | What it does |
|---|---|
| **Find closed areas on each layer separately** | An area is closed only by curves on one layer, so furniture or notes on other layers don't cut it up. An outline must then be all on one layer. |
| **Pick an area only where it is filled or hatched** | On (the default): clicking inside an empty outline picks nothing, so a box selection can start there. Off: it picks the area, to measure it. |
| **Fills cover what is beneath them** | On (the default): each solid fill or hatch is drawn over everything below it, with its outline on top, as a drawing program stacks shapes; fills higher in the draw order (**Bring to front**, higher layers) cover lower ones and their outlines. Off: every fill is drawn under every curve. |
| **When an edit divides a filled area** | What happens when an edit cuts a filled area's outline so that it bounds several areas (a side split where a line crosses it, a fillet that makes a junction). **Keep whole and flag** (the default): the fill or hatch keeps covering the whole area and is marked *Covers 2 areas*; pick it to fit it to one area, split it into one per area, or keep it whole. **Follow its seed**: it takes the area where it was first clicked, and a notice says so. A curve merely drawn across a fill never changes it: the fill keeps its whole area, marked *Covers 2 areas*, whichever is chosen. |
| **Gaps up to** | Check sketch takes ends closer than this to be meant to meet. Also in the **Check sketch** window. |

Changing one is an undo step. **Use for new drawings** makes the open drawing's settings
the ones new drawings start with on this computer. A drawing saved before these were kept
in the file (file version 38 or older) opens with the ones new drawings start with, which
at first are the values you had set in Preferences.

The tool bars' **Fill** switch (fill closed shapes as they are drawn) is remembered as you
last left it.
