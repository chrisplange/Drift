# Drift

**Parametric 2D sketching that is precise, fast and runs everywhere.**

**[Website](https://drift-website-c9y.pages.dev)** ·
**[Download](https://drift-website-c9y.pages.dev/download/)** ·
**[Try it in your browser](https://drift-website-c9y.pages.dev/app/)** ·
**[User guide](https://drift-website-c9y.pages.dev/guide/)** ·
**[Release notes](https://drift-website-c9y.pages.dev/releases/)**

Drift is a source-available 2D CAD sketcher with SolidWorks-style geometric and dimensional
constraints, built for technical drawings and design exploration. It is written in Rust, renders
on the GPU with [wgpu](https://wgpu.rs), and runs natively on Windows, macOS and Linux and in the
browser (WebGPU, with a WebGL2 fallback).

> **Status: beta (0.1.0-beta.1.5).** Drawing, modifying, constraints, parameters, drafting,
> fills and hatches, blocks, underlays, sheets and PDF / SVG / DXF output all work, and four
> rounds of alpha tester feedback are in. Expect rough edges, and save often. Files saved by
> a newer build don't open in an older one. See [PLAN.md](PLAN.md) for what was built in each
> milestone, what was left out, and what comes next.

**New to Drift?** Read the [user guide](docs/guide/README.md): installing, a tour of the
window, step-by-step tutorials, and reference pages for tools, panels, units and keys.

**Drawing with AI.** `drift-mcp` lets Claude and other MCP assistants sketch, constrain,
dimension, render and analyse Drift drawings: see [Drawing with AI assistants](docs/ai.md).

## What works today

- **Drawing tools:** polyline (L), rectangle (R), circle (C), arcs (A: centre-point; press A
  again for 3-point, then tangent arc) and point (V). While drawing a polyline, A continues with
  a tangent arc from the last point and L switches back to lines.
  Click point by point, or press, drag and release. Or type exact values while drawing:
  X/Y for the first point, then length/angle, width/height, diameter, radius or sweep (Tab
  switches field, Enter applies). Typed sizes become dimensions (with Auto relations off
  they only set the size).
- **Automatic relations and inference:** nearly horizontal or vertical lines snap straight and
  get the relation; rectangles come with their H/V relations. Points placed on (or dropped onto)
  a curve or a midpoint stay there. Dashed inference lines align new points with points you've
  just hovered. With **Auto relations** off (status bar, Shift+Alt+R), nothing adds a relation on its own:
  not drawing, not the modify tools, not an axis lock.
- **Relations pie (Q):** with something selected, hold Q, flick towards a relation and let
  go (or tap Q and click). Each relation has a fixed place; one already there is lit and
  comes off; one that would over-define the sketch says so first.
- **Dimension (D), a smart dimension tool:** click a line, circle, arc, two points, two lines (their angle, or
  the distance between them if they run alongside), or a point and a line, then place the
  label. Its position chooses aligned, horizontal or vertical. Type the
  value straight away. The number sits in the middle of its line, or just outside a line too
  short for it.
- **Dimensions and relations on the canvas:** drag dimension labels to move them, double-click
  to edit the value, and click relation symbols to pick them (Delete removes them).
- **Modify tools:**
  - Trim (T): click a piece, or drag across pieces (the drag leaves a trail). Extend (E) and
    Split. They work on lines, arcs, circles, ellipses and pen curves.
  - Fillet and Chamfer (F, F again): click a corner, move to size it in round steps, click to
    apply; or type the size. One that is already there is re-sized in place (click it with
    the tool, or drag a fillet's arc), its lines keeping their directions. Right-click one
    for Remove fillet / Remove chamfer.
  - Offset (O) on connected chains; the corners rejoin.
  - Mirror (M) with symmetric relations.
  - Linear and circular patterns.
  - Each is one undo step, and the results stay constrained to the original.
- **Regions:** click inside any closed area to select it and see its exact area, perimeter and
  centroid (holes included).
- **Fills and hatches (H):** click inside a closed area to fill it solid or hatch it.
  - Holes are left out, and fills follow the geometry as it changes.
  - Built-in ANSI and material patterns (concrete, earth, insulation, brick, block…) plus
    imported AutoCAD / Revit `.pat` files.
  - Scale, angle, drafting or model size, background colour and draw order.
  - A picked hatch has handles on the canvas to move the pattern's origin (it snaps) and to
    turn it.
  - Hatch lines are clipped exactly and stay crisp at any zoom.
- **Sheets and output:**
  - Lay the drawing out on A4–A0 or ANSI A–E paper, with a modern, ISO or ASME title block
    and a revision table, and preview it exactly as it prints.
  - A sheet holds several views of the sketch, each with its own scale, title and frame:
    drag a view to move it, its handles to crop it, Alt-drag to slide the drawing inside it.
    Each view chooses whether to show dimensions and notes.
  - A logo for the title block from an SVG file, kept in the drawing.
  - Export true-scale vector PDF (every sheet) and SVG, or DXF (R12, with layers and line
    types).
  - Import DXF drawings from other CAD programs: lines, arcs, circles, ellipses, polylines,
    splines, text and paragraph text, hatches (as live fills), blocks (nested, arrayed,
    stretched, with attributes), dimensions (as drawn), layers with colours, weights and
    line types, and paper-space layouts as sheets with their viewports as views. A report
    lists anything left out or read approximately.
  - Print (Ctrl/⌘+P).
- **Drafting:**
  - ISO 129 or ASME Y14.5 dimension styles: aligned or horizontal text, arrowheads, zeros
    and dual units.
  - Prefixes and suffixes (`4× Ø6 THRU`) and tolerances (±, stacked deviations, limits, fits
    like `H7`).
  - Chain and baseline dimensions (Shift+click after a horizontal or vertical dimension) and
    ordinate dimensions.
  - Notes with leaders (N), hole and chamfer callouts, centre marks and centre lines.
  - Section marks (a cutting line with a lettered flag at each end) and elevation marks, each
    able to name the sheet it is drawn on.
- **Layers:**
  - Colours, ISO line weights and line types (continuous, dashed, hidden, centre, phantom),
    by layer or per entity.
  - Show / lock / print flags. Locked geometry stays fixed.
  - A Layers panel (Ctrl/⌘+L) for switching layers and showing / locking them while drawing.
  - The top bar shows the selection's layer, stroke and fill colour, stroke weight and line
    type, each for the selection itself or its layer.
  - A Revit-style **thin lines** toggle (W), and a choice between constraint-status colours
    and layer colours.
- **Parameters (Parameters tab):**
  - Variables such as `width = 120` and `hole = width / 10`, with sliders that re-solve the
    sketch live.
  - Dimensions linked to expressions (type `=width/2` into a dimension; it shows `Σ`).
  - Named variants you can switch between, or show as ghosts.
  - Snapshots to return to, and pinned measurements that update as you edit.
- **Units:**
  - Metric (mm, cm, m) or imperial: decimal inches or feet, or architectural feet-inches with
    fractions (`1'-6 1/2"`).
  - Any value can be typed in any unit (`2in`, `25mm`, `1'-6"`, `30deg`) or as an expression.
  - The grid follows the units (1/16", 1", 1').
  - Changing units never moves geometry, and rounded labels are marked `~`.
- **Free directions:** View → Show free directions draws arrows showing how each
  under-defined point can still move.
- **Snapping:** end points, centres, midpoints, quadrants, nearest-on-curve and the origin, plus
  optional grid snapping (Shift+G). Hold Alt to place freely. Snapping onto an existing point
  connects to it. **Angle snap** (status bar, beside grid snap) draws lines at every 1°, 5°, 15° or 45°.
- **Blender-style keys:**
  - **Axis locks while drawing lines:** X / Y lock the current segment to an axis. The lock
    lasts for that segment only, and adds the H/V relation when Auto relations is on.
  - **Moving with G:** G moves the selection.
    - X / Y lock an axis.
    - Type an exact offset (dX, Tab, dY), or a distance when locked.
    - B picks the base point (snaps to points, e.g. a corner of the selection).
    - Click or Enter places it; Esc or right-click cancels.
    - Snapped and typed moves land exactly.
- **Pen curves (P):** draw like Illustrator or Affinity: click for corners, drag for smooth
  points, Alt to break a handle, click the first point to close. Handles show on picked curves,
  anchors carry their handles, smooth points keep their tangent, and curvature combs show
  how the curve bends.
- **Text:** text drawn as true outlines at a real size (for engraving or signage), in one
  line or several (Shift+Enter), aligned left, centre or right, optionally wrapped to a width:
  click, type and watch it update; set height, angle and font (two built in, any font
  installed on the computer, or a font file you add); convert it to curves.
- **Fonts travel with the drawing:** a font the drawing uses is kept inside its file (where
  the font's licence allows), so it looks the same on another computer and in the browser.
  A drawing can also have its own font for dimensions, notes and title blocks (Parameters →
  Drafting → Font); PDFs embed it, so the text stays selectable and searchable.
- **Polygons, slots and ellipses:** regular polygons (K; type the sides, inscribed or
  circumscribed), straight / centre / arc slots (U, again to switch) and ellipses and partial
  ellipses (C again from the circle), all built to stay true when dimensioned or dragged.
- **Blocks (furniture and symbols):** make a block from the selection or pick one from the
  library of US doors, windows, kitchen and bath fixtures, furniture and utility items; place copies (I; R turns, X mirrors); double-click a copy to edit the
  block and every copy updates; explode, replace and count copies. DXF blocks and inserts
  are kept both ways.
- **Underlays:** trace over a PDF or DXF drawing. Import a page, calibrate its scale from
  two points and a known distance (or pick 1 : 50, 1/4" = 1'-0" …), snap to its corners,
  midpoints and curves while you draw, and convert it to geometry when you want to edit it.
- **Rotate and groups:** R rotates the selection (type an angle, 15° steps stick, B picks
  the pivot). Ctrl/⌘+G groups things so they pick, move and rotate together; double-click a
  group to edit inside it.
- **Selection and editing:** click, Shift-click and Ctrl-click; drag right for a window selection
  and left for a crossing selection. Drag to move. Drop an end point on another to join them.
  Delete, construction toggle (Alt+C; with a drawing tool, it switches to drawing
  construction geometry), and solid / construction / centre line styles.
- **Constraints:** coincident, horizontal, vertical, parallel, perpendicular, collinear, equal,
  tangent, concentric, coradial, midpoint, on curve, symmetric and fix relations. Dimensions
  cover length, distance (aligned, horizontal, vertical, point-to-line), radius, diameter and
  angle.
  - Pick them in the properties panel for the current selection. Dimension values can be typed
    and edited.
  - Conflicting, redundant or collapsing constraints are refused with an explanation.
  - A dimension that would over-define the sketch can be added as a **driven** (reference)
    dimension, shown grey in parentheses, which follows the geometry.
- **Constraint solver:** the geometry moves as little as possible to satisfy the constraints.
  Dragging keeps every constraint satisfied while the grabbed geometry follows the pointer.
  Under-defined geometry is blue, fully defined is white (or black in the light theme), and
  conflicting is red, with a remaining-DOF count in the status bar. Sketches of thousands of
  entities solve in milliseconds (Help → Run solver benchmark measures it in your browser).
- **Undo and redo** for every edit, with drags recorded as a single step.
- **Files:** native `.drift` JSON with one entity per line, so files diff cleanly in git. Saving
  and loading round-trips numbers exactly. Open/save dialogs on desktop; open and download in
  the browser. The session is restored automatically after a restart or a closed tab.
- **Precise GPU canvas:** analytic lines, arcs and circles that stay crisp at any zoom, exact far
  from the origin, with an adaptive grid. It uses no CPU when idle.
- **Live readouts:** cursor coordinates, the length, angle or radius of what you're drawing, and
  exact values of the selection in the properties panel.

- **Interface:** light and dark themes, an icon toolbar with descriptive tooltips, and panels
  (Inspector, Parameters, Layers, Sheets, Blocks) that dock left or right, float, or collapse
  to a rail. A command palette (Ctrl/⌘+K) finds every command. The status bar carries grid,
  axes, snap and thin-line toggles; the grid can also be hidden with Ctrl/⌘+'.
- **Mouse and trackpad:** a wheel zooms at the cursor; two fingers on a trackpad pan and a
  pinch zooms; the middle or right button drags the view. Preferences can force scrolling to
  zoom or to pan.
- **Safety nets:** the session is autosaved and restored; closing with unsaved changes asks
  first; and Drift stops itself before a fault could exhaust the computer's memory.

See **Help → Keyboard shortcuts** in the app for the full list.

## Running

You need the Rust toolchain from [rustup.rs](https://rustup.rs).

Desktop:

```bash
cargo run -p drift --release
```

Browser (first install the WASM target and [trunk](https://trunkrs.dev)):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd apps/web && trunk serve
```

Then open <http://127.0.0.1:8080>.

Installers (see [`scripts/`](scripts) and [`installer/`](installer)): `package-windows.ps1`
builds an Inno Setup installer and `package-macos.sh` a universal `.app` in a disk image. The
`Package` workflow runs both on GitHub, and for a version tag (`v0.1.0-alpha.5`) puts them
on a draft release with their checksums. They are unsigned, so Windows SmartScreen and macOS
Gatekeeper ask for confirmation the first time each is opened.

Tests, including headless UI tests that drive the real app (GPU tests are skipped automatically
when no graphics adapter is available):

```bash
cargo test --workspace
```

## Repository layout

| Path | Purpose |
|---|---|
| `crates/drift-core` | Document model, transactions and undo history (`f64`, mm, y-up) |
| `crates/drift-geom` | Exact geometry: nearest points, intersections, regions, R-tree picking and snapping |
| `crates/drift-draft` | Drafting: `.pat` hatch patterns and exact hatch clipping, text outlines, the SVG logo reader, the page display list shared by all outputs, sheet borders, title blocks and revision tables |
| `crates/drift-solver` | Constraint solver: substitution, sparse weighted least-norm Newton, DOF analysis |
| `crates/drift-io` | The `.drift` file format (versioned JSON); PDF, SVG and DXF output; DXF import |
| `crates/drift-platform` | File dialogs and saving on desktop and web, system fonts, the memory guard |
| `crates/drift-render` | wgpu canvas renderer: camera, frame builder, SDF shaders (WebGL2-compatible) |
| `crates/drift-app` | eframe/egui application: editor, tools, panels; headless UI tests |
| `apps/native` | Desktop binary |
| `apps/web` | Browser entry point and Trunk config |
| `installer`, `scripts` | Windows and macOS packaging |


## Licence

Drift is licensed under the [Functional Source License, Version 1.1, ALv2 Future
License](LICENSE.md) (FSL-1.1-ALv2). You may use, copy, change and share it for any purpose
except making a product or service that competes with Drift. Two years after each version
is released, that version is also available under the Apache License, Version 2.0.

Copyright 2026 Christopher Lange.
