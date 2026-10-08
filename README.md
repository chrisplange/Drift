# Drift

**Parametric 2D sketching that is precise, fast and runs everywhere.**

Drift is an open-source 2D CAD sketcher with SolidWorks-style geometric and dimensional
constraints, built for technical drawings and design exploration. It is written in Rust, renders
on the GPU with [wgpu](https://wgpu.rs), and runs natively on Windows, macOS and Linux and in the
browser (WebGPU, with a WebGL2 fallback).

> **Status: early development (milestone M6).** You can draw and modify (trim, extend, split,
> fillet, chamfer, offset, mirror, patterns), constrain with SolidWorks-style relations and
> dimensions, drive them from variables and expressions, measure closed regions, organise
> geometry on layers with colours and line weights, work in metric or imperial units, and save
> it all. Drafting (dimension styles, notes, hatching) comes next. See [PLAN.md](PLAN.md) for
> the roadmap.

## What works today

- **Drawing tools:** polyline (L), rectangle (R), circle (C), arcs (A: centre-point; press A
  again for 3-point, then tangent arc) and point (P). While drawing a polyline, A continues with
  a tangent arc from the last point and L switches back to lines.
  Click point by point, or press, drag and release. Or type exact values while drawing:
  X/Y for the first point, then length/angle, width/height, diameter, radius or sweep (Tab
  switches field, Enter applies). Typed sizes become dimensions.
- **Automatic relations and inference:** nearly horizontal or vertical lines snap straight and
  get the relation; rectangles come with their H/V relations. Points placed on (or dropped onto)
  a curve or a midpoint stay there. Dashed inference lines align new points with points you've
  just hovered.
- **Smart Dimension (D):** click a line, circle, arc, two points, two lines, or a point and a
  line, then place the label. Its position chooses aligned, horizontal or vertical. Type the
  value straight away.
- **Dimensions and relations on the canvas:** drag dimension labels to move them, double-click
  to edit the value, and click relation symbols to pick them (Delete removes them).
- **Modify tools:**
  - Trim (T): click a piece, or drag across pieces. Extend (E) and Split.
  - Fillet and Chamfer at two-line corners. Fillets are tangent and dimensioned.
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
  - Hatch lines are clipped exactly and stay crisp at any zoom.
- **Sheets and output:**
  - Lay the drawing out on A4–A0 or ANSI A–E paper at a scale, with an ISO or ASME title block
    and a revision table, and preview it exactly as it prints.
  - Export true-scale vector PDF (every sheet) and SVG, or DXF (R12, with layers and line
    types).
  - Import DXF drawings (lines, arcs, circles, polylines with bulges).
  - Print (Ctrl/⌘+P).
- **Drafting:**
  - ISO 129 or ASME Y14.5 dimension styles: aligned or horizontal text, arrowheads, zeros
    and dual units.
  - Prefixes and suffixes (`4× Ø6 THRU`) and tolerances (±, stacked deviations, limits, fits
    like `H7`).
  - Chain and baseline dimensions (Shift+click after a horizontal or vertical dimension) and
    ordinate dimensions.
  - Notes with leaders (N), hole and chamfer callouts, centre marks and centre lines.
- **Layers:**
  - Colours, ISO line weights and line types (continuous, dashed, hidden, centre, phantom),
    by layer or per entity.
  - Show / lock / print flags. Locked geometry stays fixed.
  - A Layers panel (Ctrl/⌘+L) that floats over the canvas or docks left or right, for
    switching layers and showing / locking them while drawing.
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
- **Snapping:** end points, centres, midpoints, quadrants and nearest-on-curve, plus optional grid
  snapping (Shift+G). Hold Alt to place freely. Snapping onto an existing point connects to it.
- **Blender-style keys:**
  - **Axis locks while drawing lines:** X / Y lock the current segment to an axis. The lock
    lasts for that segment only and adds the H/V relation.
  - **Moving with G:** G moves the selection.
    - X / Y lock an axis.
    - Type an exact offset (dX, Tab, dY), or a distance when locked.
    - B picks the base point (snaps to points, e.g. a corner of the selection).
    - Click or Enter places it; Esc or right-click cancels.
    - Snapped and typed moves land exactly.
- **Selection and editing:** click, Shift-click and Ctrl-click; drag right for a window selection
  and left for a crossing selection. Drag to move. Drop an end point on another to join them.
  Delete, construction toggle (X), and solid / construction / centre line styles.
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

- **Native-feeling interface:** system fonts, light and dark themes, an icon toolbar with
  descriptive tooltips, and a segmented Inspector / Parameters / Layers sidebar. The status bar
  carries grid, axes, snap and thin-line toggles; the grid can also be hidden with Ctrl/⌘+'.

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
| `crates/drift-draft` | Drafting: `.pat` hatch patterns and exact hatch clipping, the page display list shared by all outputs, sheet borders, title blocks and revision tables |
| `crates/drift-solver` | Constraint solver: substitution, sparse weighted least-norm Newton, DOF analysis |
| `crates/drift-io` | The `.drift` file format (versioned JSON); PDF, SVG and DXF output; DXF import |
| `crates/drift-platform` | File dialogs and saving on desktop and web |
| `crates/drift-render` | wgpu canvas renderer: camera, frame builder, SDF shaders (WebGL2-compatible) |
| `crates/drift-app` | eframe/egui application: editor, tools, panels; headless UI tests |
| `apps/native` | Desktop binary |
| `apps/web` | Browser entry point and Trunk config |


## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
