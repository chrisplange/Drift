# Files and compatibility

## The `.drift` file

A Drift sketch is a `.drift` file: plain UTF-8 JSON, one entity per line, in a stable
order. So:

- It diffs cleanly in git, and a merge conflict is readable.
- Numbers are written so that saving and opening gives back exactly the same values.
- Dimension expressions are kept as you typed them (`width / 2`), alongside their values.
- Solved positions are stored, so a file opens without re-solving.

Everything the drawing needs travels inside the file: layers, sheets, blocks, fills,
imported hatch patterns, underlays, title-block logos, and the fonts the drawing uses
(where the font's licence allows embedding, and under 12 MB). A font that can't be kept is
used from the computer it's on, and Drift says so.

Snapshots aren't saved in the file.

## Versions

The file records a format version. This build writes version 23.

- **Older files always open** in newer builds. They're upgraded as they load.
- **Newer files don't open in older builds.** The older build says the file's version is
  newer than it supports. During the alpha the format changes often, so everyone sharing
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
  a DXF imports it into the open sketch; dropping a PDF adds it as an underlay.
- On Windows, the installer can make `.drift` files open in Drift. On macOS, use
  **File → Open…** or drop the file on the window for now.

## Imports and exports

| | Format | Notes |
|---|---|---|
| Import | DXF (ASCII) | As geometry: **File → Import DXF…**. See [Underlays and DXF import](../tutorials/11-underlays-and-dxf.md). Binary DXF and DWG aren't read. |
| Import | PDF, DXF | As an underlay to trace: **File → Import underlay…**. Vector PDFs only. |
| Import | `.pat` | Hatch patterns: **Parameters → Hatch patterns → Import .pat…**. |
| Import | `.ttf`, `.otf` | Fonts: **Add a font file…** in any font menu. |
| Import | SVG | A title-block logo: **Sheets → Logo → Choose SVG…**. |
| Export | PDF | Every sheet, true scale. |
| Export | SVG | The current sheet, true scale. |
| Export | DXF (R12) | The model at full size, with layers, line types and blocks. |

Exports never replace the `.drift` file: Ctrl+S always saves the sketch. See
[Sheets and output](../tutorials/10-sheets-and-output.md#5-output).

## The clipboard

Ctrl+C copies the selection as a small Drift document on the system clipboard (curves with
their relations and dimensions, fills, block copies with their blocks, text), so you can
paste between Drift windows. Layers and blocks are matched by name. Plain text pasted into
Drift becomes sketch text.
