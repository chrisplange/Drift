# Drift user guide

Drift is a parametric 2D CAD sketcher. You draw lines, arcs and curves, then hold them in
shape with relations (horizontal, tangent, equal…) and dimensions, as in a SolidWorks sketch.
Change a dimension and the drawing follows. The same drawing then goes onto sheets with a
title block and out as PDF, SVG or DXF.

It runs on Windows, macOS and Linux, and in the browser.

This guide is for **0.1.0-alpha.7**. It is written for architects, designers and engineers
who already know a CAD or drawing program (Revit, AutoCAD, SolidWorks, Illustrator,
Affinity) but have never used Drift.

> **Alpha software.** Save often. A file saved by a newer Drift won't open in an older one,
> so everyone testing together should use the same build.

## How the guide is organised

**Start here**

- [Getting started](getting-started.md): install Drift, find your way round the window,
  open the example, navigate, and save.
- [Coming from another program](coming-from.md): what maps to what if you know Revit,
  AutoCAD, SolidWorks or Illustrator.

**Tutorials.** Short projects, each with numbered steps. Do the first three in order; the
rest stand alone.

1. [Your first sketch](tutorials/01-first-sketch.md): lines, rectangles, circles, snapping,
   typed sizes and axis locks.
2. [Constraints and dimensions](tutorials/02-constraints-and-dimensions.md): relations, the
   Dimension tool, degrees of freedom and driven dimensions.
3. [Parameters](tutorials/03-parameters.md): variables, expressions and variants.
4. [Editing](tutorials/04-editing.md): selecting, moving, rotating, scaling, trim, fillet,
   offset, mirror and groups.
5. [Temporary dimensions](tutorials/05-temporary-dimensions.md): Revit-style sizing of a
   picked line.
6. [Fills and hatches](tutorials/06-fills-and-hatches.md).
7. [Blocks](tutorials/07-blocks.md): furniture and symbols drawn once, placed many times.
8. [Construction geometry](tutorials/08-construction-geometry.md).
9. [Annotation](tutorials/09-annotation.md): notes, callouts, marks and dimension text.
10. [Sheets and output](tutorials/10-sheets-and-output.md): views drawn on the canvas,
    sheets, scales, title blocks, PDF, SVG, DXF and printing.
11. [Underlays and DXF import](tutorials/11-underlays-and-dxf.md).
12. [Checking a sketch](tutorials/12-sketch-checker.md): gaps, short ends and curves of no
    length.

**Reference.** One page per area, to look things up.

- [Tools](reference/tools.md)
- [Panels](reference/panels.md)
- [Preferences](reference/preferences.md)
- [Units and length formats](reference/units.md)
- [Files and compatibility](reference/files.md)
- [Keyboard shortcuts](reference/keyboard-shortcuts.md)

## Conventions

- Names of buttons, menus and fields are in **bold**, as they appear in Drift.
- Keys are written as Ctrl+K, Shift+F, Alt+C. On a Mac, use Cmd for Ctrl and Option for Alt.
- Menu paths are written **File → Open example**.

## About the screenshots

The screenshots are made by Drift's own headless UI tests, so they can be brought up to
date whenever the interface changes. From the repository root:

```bash
cargo test -p drift-app docs_screenshots -- --ignored
```

The scenes are staged in
[`crates/drift-app/src/ui_tests/docs.rs`](../../crates/drift-app/src/ui_tests/docs.rs) and
written to [`docs/guide/images/`](images/). The test is skipped when the computer has no
graphics adapter.

## Reporting problems

Use [GitHub issues](https://github.com/chrisplange/Drift/issues). **Help → About Drift**
has a **Copy details** button with the version and system information to paste into a
report.
