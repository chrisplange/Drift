# Underlays and DXF import

There are two ways to bring another program's drawing into Drift:

- **As an underlay:** a PDF or DXF placed behind your sketch to trace over. You can snap to
  it, but not edit it, and it never prints.
- **As geometry:** a DXF whose lines, arcs, text, hatches and blocks become ordinary Drift
  geometry you can edit.

**What you'll do:** trace a room from a 1:50 PDF plan, then look at what a DXF import gives
you.

Time: about 10 minutes.

## Part 1: trace over a PDF

### 1. Import the PDF

1. Choose **File → Import underlay…** (or **Import PDF or DXF…** in the **Layers** panel)
   and pick a PDF. You can also drop a PDF on Drift's window.
2. For a PDF of several pages, choose the page.

The page comes in at its paper size, with its lower-left corner on the origin, drawn faded
grey behind the sketch. The **Layers** panel opens with the underlay picked.

PDF underlays read the vector drawing: lines, curves, rectangles and filled shapes (as
their outlines). Text and images are skipped. Scanned (raster) PDFs have nothing to read.

### 2. Set its scale

A plan drawn at 1:50 comes in 50 times too small. Either:

- **Pick the scale:** in the underlay's **Scale** menu, choose the scale it was drawn at,
  such as **1 : 50** or 1/4" = 1'-0". Or
- **Calibrate it:**
  1. Click **Calibrate…**.
  2. Click a point on the underlay whose distance you know, such as a wall end. It snaps.
  3. Click the other end.
  4. Type the real distance (`5000`, or `16'-5"`) and press Enter.

  Drift rescales the underlay about the first point and reports the scale it found.

### 3. Trace

Draw over it with any tool. Tools snap to the underlay's end points, midpoints, centres,
quadrants and to points along its curves. Your sketch's own points win when both are
close.

![A PDF plan calibrated to 1:50 in the Layers panel, its outline traced with the Line tool](../images/underlay.png)

Points snapped on an underlay are new points: they aren't tied to it.

### Underlay settings

Each underlay in the **Layers** panel has:

- Switches for visible, snap and locked.
- **Opacity** and **Colour** (grey by default).
- **Scale**, **Position** (X, Y) and **Angle** (with 0, 90, 180 and 270 buttons).
- **Move…**: click a point on the underlay, then where it should go.
- **Zoom to**, and **×** to delete it.
- **Convert to geometry**: turn the underlay into lines, arcs and circles on the current
  layer, with touching ends joined, so you can edit them.

Underlays are kept inside the `.drift` file. They never print or export.

## Part 2: import a DXF as geometry

1. Choose **File → Import DXF…** and pick the file. Or drop a DXF on the window.
2. The drawing is added to the open sketch, in its own units (read from the file; a file
   that doesn't say is read in your display unit).
3. A report window lists what came in, anything left out, and anything read with changes.
   **Copy this report** copies it, and **Check sketch…** looks for gaps (see
   [Checking a sketch](12-sketch-checker.md)).

What comes in:

- Lines, arcs, circles, ellipses, polylines (with arcs) and splines, as Drift curves.
  Touching ends share points, so outlines stay connected and can be constrained.
- Layers, with their colours, line weights, line types and on / off, locked and plot
  settings.
- Text and paragraph text, as sketch text.
- Hatches, as live fills.
- Blocks and their inserts, as Drift blocks and copies (nested, arrayed and unevenly scaled
  ones are expanded into curves).
- Dimensions, as the lines and text they're drawn with. They look the same but no longer
  measure.
- Paper-space layouts, as sheets, with their viewports as views.

What doesn't (yet): associative dimensions, what's drawn directly on a layout (title blocks,
notes), MLEADER, TOLERANCE, images, 3D solids, tables, and binary DXF. DWG files aren't
read: save them as DXF first.

Drift's DXF export is described in [Sheets and output](10-sheets-and-output.md#5-output).

## What you learned

- **File → Import underlay…** puts a PDF or DXF behind the sketch to trace; calibrate it
  from two points and a known distance.
- **File → Import DXF…** brings a DXF in as editable geometry, with a report of what changed.

## Next

[Checking a sketch](12-sketch-checker.md).
