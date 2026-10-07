# Drift

**Parametric 2D sketching that is precise, fast and runs everywhere.**

Drift is an open-source 2D CAD sketcher with SolidWorks-style geometric and dimensional
constraints, built for technical drawings and design exploration. It is written in Rust, renders
on the GPU with [wgpu](https://wgpu.rs), and runs natively on Windows, macOS and Linux and in the
browser (WebGPU, with a WebGL2 fallback).

> **Status: early development (milestone M5).** You can draw with typed values and automatic
> relations, constrain with the full set of SolidWorks-style relations, dimension with Smart
> Dimension, drive dimensions from variables and expressions, scrub variables with sliders,
> switch between variants, work in metric or imperial (including feet-inches with fractions),
> and save it all. Modify tools and layers come next. See [PLAN.md](PLAN.md) for the roadmap.

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
| `crates/drift-geom` | Exact geometry: nearest points, intersections, R-tree picking and snapping |
| `crates/drift-solver` | Constraint solver: substitution, sparse weighted least-norm Newton, DOF analysis |
| `crates/drift-io` | The `.drift` file format (versioned JSON) |
| `crates/drift-platform` | File dialogs and saving on desktop and web |
| `crates/drift-render` | wgpu canvas renderer: camera, frame builder, SDF shaders (WebGL2-compatible) |
| `crates/drift-app` | eframe/egui application: editor, tools, panels; headless UI tests |
| `apps/native` | Desktop binary |
| `apps/web` | Browser entry point and Trunk config |

The drafting crate described in the plan is added in the milestone where it is needed.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
