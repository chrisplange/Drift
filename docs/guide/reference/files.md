# Files and compatibility

## The `.drift` file

A Drift sketch is a `.drift` file: plain UTF-8 JSON, one entity per line, in a stable
order. So:

- It diffs cleanly in git, and a merge conflict is readable.
- Numbers are written so that saving and opening gives back exactly the same values.
- Dimension expressions are kept as you typed them (`width / 2`), alongside their values.
- Solved positions are stored, so a file opens without re-solving.

Everything the drawing needs travels inside the file: layers, views, sheets, blocks, fills,
imported hatch patterns, underlays, title-block logos, and the fonts the drawing uses
(where the font's licence allows embedding, and under 12 MB). A font that can't be kept is
used from the computer it's on, and Drift says so.

Snapshots aren't saved in the file.

## Versions

The file records a format version. This build writes version 39, which keeps the drawing's
own rules for closed areas and fills (see
[Drawing settings](preferences.md#closed-areas-and-fills)); version 38 recorded shapes drawn
as one (paths). A file from before version 39 opens with the rules new drawings start with. A file from before version 27, when views came to belong to the
drawing and sheets to place them, opens with each of its sheets' views made a view of its
own.

- **Older files always open** in newer builds. They're upgraded as they load.
- **Newer files don't open in older builds.** The older build says the file's version is
  newer than it supports. During the beta the format changes often, so everyone sharing
  files should use the same build.

Check your build in **Help → About Drift**.

## Session and autosave

Drift keeps your session (the open sketch, whether or not it's saved) and restores it on the
next start: every few seconds, and when you quit. In the browser it's kept in the browser's
storage for the site.

This is a safety net. Save your work to a file as well.

When you close Drift, start a new sketch or open another with unsaved changes, Drift asks:
**Save**, **Don't save** or **Cancel**. **Don't save** when quitting also clears the stored
session, so the next start is fresh.

## Opening files

- **File → Open…** (Ctrl+O): a `.drift` file.
- **File → Open recent**: the last ten.
- Pass a `.drift` file on the command line, or drop it on the window, to open it. Dropping
  a DWG, DXF, SVG, PDF or `.ai` file asks how it should come in: **Open as new drawing**,
  **Import as geometry** or **Underlay to trace** (the last two through the import dialog).
  See [Dropping a file on the window](../tutorials/11-underlays-and-dxf.md#dropping-a-file-on-the-window).
- On Windows, the installer can make `.drift` files open in Drift. On macOS, use
  **File → Open…** or drop the file on the window for now.

## Imports and exports

| | Format | Notes |
|---|---|---|
| Import | DXF (ASCII), DWG | As geometry or an underlay, at a drawing scale or calibrated: **File → Import…**. DWG is read for AutoCAD R14 to 2018 and comes in the same way as DXF. See [Underlays and DXF import](../tutorials/11-underlays-and-dxf.md). Binary DXF isn't read, and Drift doesn't save DWG: export DXF. |
| Import | SVG, PDF, Illustrator (`.ai`) | As geometry or an underlay, at a drawing scale (found from its dimensions where it can be) or calibrated: **File → Import…**. See [Part 3](../tutorials/11-underlays-and-dxf.md#part-3-import-svg-pdf-and-illustrator-artwork). Vector PDFs only. |
| Import | `.pat` | Hatch patterns: **Parameters → Hatch patterns → Import .pat…**. |
| Import | `.ttf`, `.otf` | Fonts: **Add a font file…** in any font menu. |
| Import | SVG | A title-block logo: **Sheets → Logo → Choose SVG…**. |
| Export | PDF | Every sheet, true scale. |
| Export | SVG | The current sheet, true scale: a named group per layer (Inkscape layers; Illustrator groups, which **Release to Layers** turns into layers) and joined paths. |
| Export | DXF (R12) | The model at full size, with layers, line types and blocks. |
| Export | SVG, PDF (model) | The model, or the selection, at 1 : 1 on a page fitted to it with a 5 mm margin: no sheet, title block or border. A named group per layer and joined paths (SVG). Points, construction and hidden or non-printing layers are left out; **Dimensions and notes** includes them. **File → Export → Model as SVG… / Model as PDF…**. |

| Export | Blender profiles (`.drift-profiles.json`) | The blocks marked as **Blender profile**, for the Drift Profiles add-on: **File → Export → Blender profiles…**. Saving writes them beside the drawing too. See below. |

Exports never replace the `.drift` file: Ctrl+S always saves the sketch. See
[Sheets and output](../tutorials/10-sheets-and-output.md#5-output).

## The Blender profiles file

When any block is marked as a **Blender profile**, saving `Trim.drift` also writes
`Trim.drift-profiles.json` beside it. When no block is marked, saving deletes that file.
While a profile block is open for editing, Drift also rewrites the file as the block
changes, up to ten times a second, without saving the drawing. Blender's Auto-reload uses
this to follow your edit as you work.
Blender's Drift Profiles add-on reads it; see
[Profiles in Blender](../tutorials/13-profiles-in-blender.md).

The file has its own format, versioned separately from `.drift`
(`"format": "drift-profiles"`, `"version": 1`). It holds:

- One profile per marked block: its name, and its id, which stays the same when the block
  is renamed.
- The block's solid curves, joined end to end into loops. Construction and centre lines,
  points, dimensions, text and fills are left out.
- Coordinates in metres, y up, with the block's base point as the origin.
- Each loop as cubic Bézier knots, each knot a point and its two handles. Arcs, circles and
  ellipses become Béziers.
- A `closed` flag on each loop. An open loop is half of a turned shape; a closed loop
  inside another is a hole.

Profiles are listed in block order, so the file changes only where the drawing did. Drift
writes the whole file under a temporary name and then renames it, so the add-on never reads
half a file.

## The clipboard

Ctrl+C copies the selection as a small Drift document on the system clipboard (curves with
their relations and dimensions, fills, block copies with their blocks, text), so you can
paste between Drift windows. Layers and blocks are matched by name. Plain text pasted into
Drift becomes sketch text.
