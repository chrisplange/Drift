# Preferences

Open **Edit → Preferences…** (Ctrl+,). Most of these are also in the **View** menu or the
status bar; this window gathers them in one place. Preferences are remembered between
sessions and carry over when you open another drawing.

What is saved with a drawing (its units and drafting style) is in **File → Drawing
settings…** instead; see [Drawing settings](#drawing-settings) below.

The window has a page for each group, listed down its left side:

| Page | Holds |
|---|---|
| **General** | [Appearance](#appearance), [Window](#window) and [Memory](#memory). |
| **Drawing aids** | [Grid and snapping](#grid-and-snapping), [Dimensions](#dimensions) and [Closed areas](#closed-areas). |
| **Display** | [Display](#display) and [Crosshair](#crosshair). |
| **Mouse and trackpad** | [Scrolling](#mouse-and-trackpad). |
| **Updates** | Checking for and installing updates (desktop only). |

## Appearance

The light or dark theme, or follow the system. (Also **View → Appearance**.)

## Grid and snapping

| Setting | What it does |
|---|---|
| **Show the grid** | Also Ctrl+' and the status bar. |
| **Show the origin axes** | The red and green lines through the origin. |
| **Snap to the grid** | Also Shift+G and the status bar. The arrow keys nudge the selection one grid step. |
| **Add relations automatically while drawing** | The same as the **Auto relations** switch in the status bar (Shift+Alt+R). |
| **Round sizes when scaling with dimensions (Alt)** | With a transform handle and Alt, the width and height step through round sizes. Off: any size. |

## Dimensions

| Setting | What it does |
|---|---|
| **Show temporary dimensions on the picked geometry** | Revit-style temporary dimensions when one line, circle or arc is picked. See [Temporary dimensions](../tutorials/05-temporary-dimensions.md). |

## Closed areas

| Setting | What it does |
|---|---|
| **Find closed areas on each layer separately** | An area is closed only by curves on one layer, so furniture or notes on other layers don't cut it up. An outline must then be all on one layer. |
| **Pick an area only where it is filled or hatched** | On (the default): clicking inside an empty outline picks nothing, so a box selection can start there. Off: it picks the area, to measure it. |

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

## Display

| Setting | What it does |
|---|---|
| **Colour curves by constraint status** | Blue, white or black, and red by how well defined. Off: each curve's own or its layer's colour. Also Shift+C. |
| **Thin lines (every line one pixel wide)** | For editing dense drawings. It never changes the drawing or its output. Also W. |
| **Always show end points** | Off: end points show only on hovered or picked curves. Also the **End points** switch in the status bar (Shift+Alt+V). |
| **Show dimensions and notes** | Also Alt+D. |
| **Show relations** | The relation symbols. Also Alt+R. |
| **Curvature combs on picked curves** | Combs on pen curves showing how they bend. |

## Window

- **Reset panel layout** puts every panel back in the right dock.
- **Show the start panel** brings the start panel back over an empty, unsaved sketch.

## Memory

Shows how much memory Drift is using. Drift warns at 3 GB and closes itself at 8 GB, so a
fault can't exhaust the computer; the sketch is restored next time.

## Drawing settings

**File → Drawing settings…** (also in the command palette) holds what is saved in the open
drawing rather than as a preference:

- **Units:** **Metric** or **Imperial**, the **Format**, **Decimals** or the fraction to
  **Round to**, and **Angles** decimals. See [Units and length formats](units.md).
- **Drafting:** **ISO 129** or **ASME Y14.5**, then scale, text height, **Arrows**,
  **Arrow size**, **Ext. gap**, **Ext. past**, text placement, **Zeros**, **Dual units**,
  **Colour** and the drawing's **Font**. A picked dimension or annotation can override
  these for itself; see [The drafting style](../tutorials/09-annotation.md#the-drafting-style).

## Elsewhere

Some settings live outside this window:

- **Gaps up to**, the sketch checker's tolerance, is in the **Check sketch** window and
  remembered.
- **Angle snap** (off, 1°, 5°, 15° or 45°) is in the status bar.
- **View → Free directions**, **Variant ghosts** and **Hidden annotations** (Alt+H) are in
  the **View** menu.
