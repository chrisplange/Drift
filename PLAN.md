# Drift — Project Plan

A lightweight, GPU-accelerated, precise 2D parametric sketching app with SolidWorks-style
geometric and dimensional constraints.

Project name: **Drift**. Crates use the `drift-` prefix, and documents use the `.drift` extension.

---

## 1. Goals and non-goals

**Goals**
- **Precision first.** All model geometry is stored in `f64`. Dimensions are exact to the
  displayed precision, and what you type is what you get. No drift after repeated edits.
- **SolidWorks-class constraints.** Geometric relations and driving dimensions, live DOF
  feedback (under-defined, fully defined, over-defined), and constraint-preserving drag.
- **Lightweight.** Single native binary under about 20 MB, WASM bundle under about 6 MB
  compressed, cold start under 300 ms native and under 2 s on the web, idle CPU
  near 0% (redraw only on change).
- **GPU-accelerated.** 60+ fps pan and zoom with 50k+ entities. Lines, arcs and circles are
  rendered analytically, so they stay crisp at any zoom.
- **User-friendly.** Inference snapping, auto-relations while drawing, on-canvas dimension
  entry, discoverable tools, and forgiving undo.
- **Full 2D toolset.** Covers everything you'd expect in a CAD sketch environment (see §5).

- **Cross-platform, browser-first parity.** One codebase runs natively on Windows, macOS and Linux
  and in the browser (WASM + WebGPU, with a WebGL2 fallback). The web build is a first-class
  target from M0, not a later port.
- **Open source.** Developed in the open with a permissive licence (see §2.1).

**Primary use cases** (these drive prioritisation)
1. **Technical drawings:** standards-aware dimensioning (ISO/ANSI), annotations, line types,
   hatching, sheets and title blocks, and true-scale PDF output.
2. **Design exploration:** fluid constraint-preserving drag, variables and equations, splines,
   and quick variants for comparing alternatives.

**Non-goals (v1)**
- 3D, extrusions and assemblies.
- Real-time collaboration (the document model should not preclude it later).
- Raster or paint tools.
- Full GD&T authoring (basic feature-control frames are post-v1).

---

## 2. Technology choices

| Concern | Choice | Rationale |
|---|---|---|
| Language | **Rust** (stable) | Performance, memory safety, great wgpu ecosystem, single binary |
| GPU | **wgpu** | Vulkan / Metal / DX12 / WebGPU from one codebase; WASM build possible later |
| App shell | **eframe** (egui + winit + wgpu) | One shell for native and web; handles the canvas lifecycle in the browser |
| UI chrome | **egui** | Immediate mode, tiny, fast iteration, runs identically on web; canvas is our own renderer via an `egui_wgpu` paint callback |
| Web build | **trunk** + `wasm-bindgen` | Simple WASM bundling, hot reload, static output deployable to GitHub Pages |
| Curve math | **kurbo** (+ own code) | Robust Bézier/arc primitives, nearest-point, arclength |
| Fill tessellation | **lyon** | Only for filled regions/hatches; strokes are analytic |
| Linear algebra | **faer** (sparse + dense) | Fast pure-Rust sparse QR/LU, needed by the solver |
| Text | **cosmic-text** / `swash` | Font shaping → outlines for sketch text entities |
| Serialization | **serde** + JSON (native `.drift`), versioned schema | Diffable, debuggable, forward-migratable |
| DXF I/O | **dxf** crate | Interop with CAD/CAM/laser tools |
| SVG/PDF export | own SVG writer, **pdf-writer** / `krilla` | Vector-exact output |
| IDs/arenas | `slotmap` | Stable handles, generational safety |
| Testing | `proptest`, `insta`, `criterion` | Property tests for the solver, snapshot tests, benchmarks |

**Why not Vello?** Vello is excellent for general vector art, but a CAD canvas is mostly a few
primitive types (segments, arcs, circles, points, splines) that need pixel-exact,
zoom-independent stroke widths and fast hit-testing. A custom instanced SDF pipeline is simpler,
faster for this workload, and fully under our control. Vello can be revisited for
fills or text if needed.

**Why a custom solver?** SolveSpace's `libslvs` (C++, GPLv3) and FreeCAD's PlaneGCS (C++, LGPL,
Eigen) both exist, but linking C++ into the WASM build adds an Emscripten toolchain, a larger
bundle, and awkward debugging. A pure-Rust solver compiles to WASM with zero friction. The solver
is also the heart of the product, and owning it gives us control over drag behaviour,
diagnostics, and performance. PlaneGCS and SolveSpace remain **reference implementations** to
study and benchmark against.

### 2.1 Licence

Chosen: **MIT OR Apache-2.0** (dual licence, the Rust ecosystem convention, and the most flexible option).
- It is compatible with every dependency above, and others can embed `drift-solver` or `drift-geom` in
  their own projects. A good standalone Rust 2D constraint solver would be valuable to the wider
  ecosystem.
- Trade-off: we cannot copy or port GPL code (SolveSpace). We can still implement the same
  *algorithms* from papers and documentation, which are not copyrightable.

Repo hygiene from day one: `LICENSE-MIT`, `LICENSE-APACHE`, `CONTRIBUTING.md`, a code of conduct,
issue templates, `cargo-deny` in CI for licence and advisory checks, and DCO or a simple
"contributions under the project licence" policy.

---

## 3. Architecture

Cargo workspace:

```
crates/
  drift-core/     Document model: entities, constraints, parameters, IDs, units, expressions
  drift-geom/     Exact geometry: intersections, projections, offsets, trimming, loops/regions
  drift-solver/   Constraint system → equations → decomposition → Newton/LM solve, DOF analysis
  drift-render/   wgpu renderer: camera, SDF primitive pipelines, picking, grid, overlays
  drift-io/       Native format, DXF import/export, SVG/PDF export (pure Rust, byte-buffer based)
  drift-draft/    Drafting layer: dimension styles, annotations, hatches, sheets, title blocks
  drift-app/      eframe/egui shell, tools (state machines), inference engine, commands, undo
  drift-platform/ Trait-based platform services: file open/save, clipboard, storage, fonts, timers
apps/
  native/         Desktop binary (thin main.rs)
  web/            WASM entry point, index.html, trunk config, service worker (PWA)
```

All crates except `drift-platform`'s backends and `apps/*` must compile for
`wasm32-unknown-unknown` with no `std::fs`, `std::thread` or `std::time::Instant` use. CI
enforces this.

Data flow:

```
 input ─► Tool state machine ─► Command ─► Document (drift-core)
                                              │
                                   constraint graph changed
                                              ▼
                                  Solver (drift-solver) ─► new parameter values
                                              │
                                              ▼
                     Derived geometry cache (drift-geom) ─► GPU buffers (drift-render)
```

### 3.1 Document model (drift-core)
- **Parameters** are the solver's unknowns: `f64` values in a flat arena. Every entity is
  defined *only* by parameters, for example `Point { x: ParamId, y: ParamId }`,
  `Line { p0: PointId, p1: PointId }`, and
  `Arc { center, start, end: PointId }` (radius implied, with an internal equal-distance equation).
- **Entities:** Point, Line, Circle, Arc, Ellipse, EllipticalArc, Spline (B-spline via control
  points; "fit" splines through points), Text (outline-driven, positioned by an anchor point
  and angle).
- **Flags per entity:** `construction`, `locked`, layer, and appearance overrides (colour,
  line weight, line type; "by layer" by default). See §5.11.
- **Constraints:** an enum, each variant referencing entity IDs and (for dimensions) a
  `Value = Literal(f64) | Expr(ExprId)` plus a `driving: bool`.
- **Global variables and equations:** named parameters (`width = 120 mm`,
  `hole_d = width / 10`) evaluated by a small expression engine with units. This is what makes
  sketches truly *parametric*.
- **Units:** internal base unit is mm. Display and input support mm, cm, m, in, ft, and deg/rad,
  with mixed input like `1in + 3mm`. Each document has a unit system (metric or imperial) and a
  display style; see M5 for the imperial engineering vs architectural formats.

### 3.2 Undo/redo
- Command pattern: every edit is a reversible `Command`. Drags coalesce into one command.
- The document is small (parameters plus topology), so commands may store before/after
  snapshots of touched parameters rather than inverse logic. This keeps the code simple and
  correct.

### 3.3 Precision strategy
- The model uses `f64` everywhere. The renderer uses **camera-relative `f32`**: positions are
  computed relative to the view origin on the CPU in `f64` before upload, so there is no jitter
  at large coordinates or deep zoom.
- Solver convergence tolerance is `1e-10` (model units), well below the displayed precision.
- Dimension values are stored as entered (literal or expression). The solver never rewrites a
  user value.
- Snap and inference results are applied as *constraints* (for example coincident or
  horizontal), not as approximate coordinates.

### 3.4 Cross-platform and web strategy
- **Single-threaded by design.** The solver and geometry must meet performance targets on one
  thread, because WASM threads need `SharedArrayBuffer`, which requires COOP/COEP headers that
  many hosts (including GitHub Pages) don't set. Optional parallelism (`rayon`) sits behind a
  feature flag for native builds only.
- **Long solves never block a frame.** Large re-solves are time-sliced across frames with a
  progress indicator, rather than relying on a worker thread.
- **Platform services** (`drift-platform` traits):
  - *Files:* native uses `rfd` dialogs. Web uses the File System Access API where available
    (Chromium) and falls back to `<input type=file>` and download links elsewhere.
  - *Autosave/recovery:* native uses the app data directory. Web uses IndexedDB.
  - *Clipboard:* the system clipboard, with sketch data on a custom MIME type and SVG as a
    fallback.
  - *Fonts:* a bundled open font set (for example Inter for the UI, and an ISO 3098-style
    technical font for drawings), so drawings render identically everywhere.
- **Time:** use `web-time` instead of `std::time::Instant`.
- **Keyboard:** the browser reserves some shortcuts (Ctrl+W, Ctrl+T, Ctrl+N). Defaults avoid
  them, and Ctrl+S/Ctrl+O are intercepted with `preventDefault`.
- **Bundle budget:** the WASM bundle stays under about 6 MB compressed. Use `wasm-opt -Oz`,
  `opt-level = "z"` for the web profile, and lazy-load fonts. CI tracks the size.
- **Distribution:**
  - Web: static hosting on GitHub Pages, installable as a PWA, and works offline.
  - Native: GitHub Releases with Windows MSI, macOS dmg (universal), and Linux AppImage/Flatpak.
- **Browser support:** WebGPU on Chromium, Safari 26+ and Firefox (where enabled), with wgpu's
  WebGL2 backend as a fallback. The renderer is designed to WebGL2 limits from the start
  (see §6).

### 3.5 File format (`.drift`)
- UTF-8 JSON, pretty-printed with stable key order and stable entity ordering, so files diff
  cleanly in git and merge conflicts stay readable.
- The top-level object holds:
  - `"format": "drift"` and `"version": <int>`.
  - Document metadata (title, author, units, created/modified).
  - `variables`, `entities`, `constraints`, `layers`, `fills`, `styles`, `sheets`, and
    `variants`.
- **IDs:** short stable string IDs (`"e12"`, `"c40"`) rather than array indices, so edits don't
  renumber everything.
- **Numbers:** `f64` values are written with shortest round-trip formatting (Rust's default
  `Display` / `ryu`), so save-load is bit-exact. Dimension values keep the user's literal or
  expression text alongside the evaluated number.
- **Solver state:** solved parameter values are stored so that files open instantly without
  re-solving. They are treated as the starting point on load, and the stored constraints remain
  the source of truth.
- **Versioning:** each schema version has a migration function (`vN → vN+1`) in `drift-io`, with
  golden test files for every past version. Unknown fields are preserved on a best-effort basis
  for forward compatibility.
- **Schema:** a JSON Schema is published from the Rust types (`schemars`), so third-party tools
  and scripts can read and write `.drift` files.
- **Embedding:** fonts and title-block images are referenced by name by default. An optional
  "pack" mode embeds them as base64 for fully self-contained sharing.

---

## 4. Constraint solver (drift-solver) — the critical path

### 4.1 Formulation
- Each constraint becomes 1 or more residual equations `f_i(params) = 0`, plus analytic partial
  derivatives. Examples:
  - Distance(p, q) = d → `|p − q| − d`
  - Tangent(line, arc) → signed distance from the center to the line `− r` (sign chosen from the
    current configuration)
- Equations are formulated to be well-conditioned. For example, use
  `cross(u, v) / (|u||v|)` for parallel instead of slopes, and avoid `atan2` discontinuities.

### 4.2 Pipeline
1. **Substitution pass:** coincident points, horizontal/vertical, and fixed values are eliminated
   by parameter *aliasing* before numeric solve. This shrinks the system dramatically.
2. **Graph decomposition:** split into connected components of the equation–parameter bipartite
   graph. A finer pass then uses Dulmage–Mendelsohn / block-triangular ordering, so
   rigid sub-clusters solve independently and incrementally.
3. **Numeric solve:** Newton–Raphson with **Levenberg–Marquardt** damping fallback, using a
   sparse Jacobian (faer).
   - **Under-constrained systems:** minimum-norm step, meaning the smallest change from the
     current state. This keeps geometry from jumping.
4. **Drag mode:** dragged parameters get soft target equations (weighted least squares).
   Everything else stays as close to its previous position as possible, and the solve runs at
   frame rate.
5. **Branch stability:** solve from the current configuration with step-size limits. Store
   orientation hints (arc direction, tangent side, angle quadrant) with the constraint so that
   solutions don't flip.

### 4.3 Diagnostics (SolidWorks-style UX)
- **DOF analysis:** rank of the Jacobian via rank-revealing sparse QR gives the remaining DOF per
  entity. Entities are color-coded:
  - Blue = under-defined
  - Black = fully defined
  - Red = over-defined / conflicting
  - Yellow = dangling
- **Redundancy and conflict detection:** when a new constraint is added, test whether its row is
  linearly dependent.
  - If it's consistent but redundant, offer to make it a **driven (reference)** dimension.
  - If it's inconsistent, refuse it, highlight the conflicting set, and offer "SketchXpert"-style
    fixes (delete one of N).
- **Status bar:** "Under defined: 3 DOF remaining" (or "Fully defined" / "Over defined").

### 4.4 Performance targets
- 1,000 entities / 2,000 constraints: full solve < 5 ms, drag solve < 2 ms (incremental).
- 10,000 entities: full solve < 50 ms.
- Benchmarks run in CI with `criterion`, plus a regression corpus of tricky sketches.

### 4.5 Testing
- Every constraint type has a finite-difference Jacobian check.
- Property tests generate random valid sketches, perturb them, re-solve, and assert that all
  residuals are below tolerance and the topology is unchanged.
- Golden sketches: known-hard cases (tangent chains, slot patterns, symmetric linkages,
  near-degenerate angles).

---

## 5. Feature set

### 5.1 Drawing tools
| Category | Tools |
|---|---|
| Lines | Line, polyline (chain), centerline, construction line, infinite line |
| Rectangles | Corner, center, 3-point corner, 3-point center, parallelogram |
| Circles | Center-radius, perimeter (3-point), 2-point, tangent-tangent-tangent |
| Arcs | Center-point, tangent arc (auto from line end), 3-point |
| Ellipses | Ellipse, partial ellipse |
| Slots | Straight, center straight, 3-point arc slot, center arc slot |
| Polygons | Inscribed / circumscribed, N sides (a constrained construction circle plus equal segments) |
| Splines | Control-point B-spline, fit spline through points, with tangency/curvature handles |
| Points | Point, midpoint helper |
| Text | Single-line text with font, height, and angle, convertible to geometry |
| Construction | Toggle any entity to construction (dashed, excluded from profiles) |

### 5.2 Modify tools
- Trim (power trim: drag across segments), extend, split at point, and join.
- Fillet and chamfer: these create geometry *plus* the tangent/coincident constraints and keep
  the parametric radius/distance.
- Offset (single or chain, both directions, with cap options): offsets stay linked through an
  offset-distance constraint group.
- Mirror about a line, which adds symmetric relations.
- Linear and circular patterns, kept parametric (count, spacing, and angle are editable).
- Move, copy, rotate, and scale, either keeping or breaking relations (user's choice).
- Convert to construction, and explode text/patterns.

### 5.3 Relations (geometric constraints)
Coincident, merge, midpoint, point-on-curve, horizontal, vertical, collinear, parallel,
perpendicular, tangent, concentric, coradial, equal (length/radius), symmetric (about a line),
fix/lock, equal curvature (G2, spline ↔ curve), horizontal/vertical alignment between points.

### 5.4 Dimensions
- Smart Dimension (a single tool that infers the type from the selection): linear, horizontal,
  vertical, aligned, angular, radius, diameter, arc length, and point-to-curve distance.
- Driving vs driven (reference) dimensions.
- On-canvas edit box that accepts expressions, units, and variable names. Dimensions can link to
  global variables.
- Placement is draggable. Text, arrowhead, and leader styles follow a dimension style.

### 5.5 Inference and snapping
- Snaps to endpoint, midpoint, center, quadrant, intersection, nearest-on-curve, tangent,
  perpendicular foot, and grid (optional).
- Inference lines: horizontal/vertical alignment with existing points, extension lines, and
  parallel/perpendicular hints, shown as dotted guides with relation glyphs.
- Auto-relations: while drawing, an inferred snap creates the matching constraint, which can be
  toggled per-drag with a modifier key or globally.
- Typed input during drawing: Tab cycles length/angle fields, and pressing Enter commits exact
  values as dimensions.

### 5.6 Selection and inspection
- Click, box (window vs crossing, depending on drag direction), lasso, and chain-select.
- Property panel shows coordinates, lengths, radii, constraints on the selection, and DOF.
- Relations are displayed as glyphs on the canvas (toggleable). Click a glyph to select or delete
  the relation.
- Measure tool and area/centroid/perimeter of closed profiles.

### 5.7 Profiles and regions
- Automatic closed-loop detection (planar graph faces) for shading, area properties, and export.
- Regions are the boundaries for fills and hatches (§5.12).

### 5.8 Technical drawing (drift-draft) — primary use case
- **Dimension styles:** ISO 129 and ASME Y14.5 presets, covering:
  - Arrowheads, text height and font, gaps and extensions, and units/precision.
  - Trailing-zero rules, dual units, and tolerances (symmetric, deviation, limits, fits like
    `H7`).
- **Dimension types beyond sketch dimensions:** baseline, chain/continuous, ordinate, chamfer
  (`2 × 45°`), and hole callouts (`⌀6 THRU`, `4× ⌀6`). Driven dimensions can be added freely as
  annotation, so they don't over-constrain the sketch.
- **Annotations:** notes (multi-line text), leaders with arrow or dot ends, balloons, centre
  marks and centrelines (auto-generated for circles and arcs), surface-finish symbols, and
  datum labels.
- **Hatching and fills:** see §5.12.
- **Line types and weights:** ISO 128 line types and weights, by layer or per entity, output at
  true weight in PDF. See §5.11.
- **Sheets:** A-series and ANSI sheet sizes, a drawing scale per sheet, editable title-block
  templates with fields (author, date, scale, revision) bound to document properties, and a
  revision table.
- **Output:** true-scale vector PDF, printing, and DXF export with layers and line types
  preserved.

### 5.9 Design exploration — primary use case
- **Variables panel** with sliders: drag a slider to scrub a variable and watch the sketch
  re-solve live. This is the core exploration loop.
- **Variants (configurations):** named sets of variable values. Switch between them instantly,
  or show them side by side or overlaid as ghosts.
- **Snapshots:** quick, named states for "try something" branching, with one-click revert. These
  build on the undo history.
- **Under-defined is fine:** dragging under-defined geometry is a deliberate exploration tool,
  not an error state. The "remaining DOF" display shows *which* directions are free (small
  arrows on draggable entities).
- **Splines with direct manipulation:** tangent and curvature handles, a curvature comb
  overlay, and G1/G2 continuity constraints.
- **Measurements on live geometry:** pinned measurements (area, distance, angle) that update as
  you drag.

### 5.10 Document and organization
- Layers with colours, line weights and line types (§5.11).
- Global variables/equations table.
- Native file plus autosave and crash recovery.
- Import DXF (lines, arcs, circles, polylines with bulges, splines, and text). Import SVG paths
  (converting Béziers to splines/arcs).
- Export DXF, SVG, PDF (true vector, 1:1 scale), and PNG.

### 5.11 Layers, line weights, colours and thin lines
*Added at the user's request (2026-10-07).*

- **Layers** are a document table. Each layer has:
  - A name, colour, line weight and line type.
  - Visible, locked and printable flags.
  - A draw order.

  Layer "0" always exists and can't be deleted or renamed. The current layer is chosen in the
  layers panel; new geometry goes on it.
- **By layer, with overrides:** entities take colour, weight and line type from their layer
  unless overridden per entity (the AutoCAD/Revit "ByLayer" model). Overrides are set in the
  properties panel, which has a layer dropdown and an "x by layer" reset.
- **Construction is a role, not a style:**
  - Construction geometry keeps its own thin dashed look whatever the layer.
  - It is excluded from regions, never printed, and has no weight.
  - The current Solid / Construction / Centre choice splits into this construction flag and
    the line type (centre becomes a line type).
- **Line weights:** the ISO 128 series 0.13 / 0.18 / 0.25 / 0.35 / 0.5 / 0.7 / 1.0 / 1.4 / 2.0 mm,
  plus a thin "hairline" default.
  - A weight is a *paper* width. On screen it is drawn at the width it would print at the
    current zoom/scale (constant screen width in model view at 1:1, scaled by the sheet scale
    once sheets exist in M8), with a minimum of 1 device pixel.
  - PDF output is at true weight.
- **Thin lines toggle (as in Revit):**
  - A view setting that draws every curve at 1 device pixel regardless of weight, for precise
    editing of dense or heavy drawings.
  - It is per window and remembered as a user preference. It never changes the document or
    output.
  - Hatch lines follow it; dimension and annotation lines are already thin. It lives in the
    View menu and as a status-bar toggle, with a shortcut (proposed `W`, remappable).
- **Colours:**
  - Per layer and per entity, as RGB.
  - A "foreground" colour flips between black and white with the theme (like AutoCAD colour
    7), and other colours too close to the background are nudged for contrast on screen only.
  - Selection, hover, preselection and conflict highlights always override.
  - View → "Constraint status colours" (on by default while sketching) shows the SolidWorks
    under/fully-defined colouring. Turned off, it shows layer colours for a drawing view.
- **Visibility and locking:**
  - Hidden layers are not drawn, picked or snapped to, but their geometry and constraints still
    solve.
  - Locked layers are drawn, can be snapped to and dimensioned against, but can't be selected for
    editing. The solver treats their parameters as fixed, so dragging connected geometry can't
    move them.
  - Isolate (show only this layer), "select all on layer" and "move selection to layer" are in
    the layers panel and context menu.
- **Undo and files:**
  - Layer edits and assignments are ordinary journalled changes.
  - The file gains a `layers` table and optional per-entity `layer` / `color` / `weight` /
    `linetype` (absent = by layer). This is format v6; older files load onto layer "0".
- **Rendering:** a width and colour per instance (the SDF line/arc pipelines already take
  per-instance attributes). Dash patterns follow arclength and scale with the weight.
- **Interop:** DXF layers map one-to-one, with weights rounded to the nearest DXF lineweight and
  colours written as true colour plus the nearest ACI. SVG and PDF use stroke width and colour.

### 5.12 Fills and hatches
*Added at the user's request (2026-10-07).*

- **Prerequisite:** region detection (M6): faces of the planar graph of non-construction curves,
  with islands (holes) found by containment. Arcs and circles stay exact; later splines and
  ellipses join in M9.
- **Fill objects are associative.** A fill stores its boundary as curve ids, orientation, and a
  seed point inside, and is recomputed when the geometry changes.
  - If the boundary breaks (a curve deleted, or a gap opened), the fill keeps its last valid
    shape, is flagged "boundary lost" in the panel, and can be re-picked.
- **Creating:**
  - A Fill / Hatch tool: click inside a closed area to use its smallest enclosing face, with
    islands excluded (like AutoCAD BHATCH "pick points").
  - Or select closed boundary curves and choose Fill.
  - The preview highlights the region before you click.
- **Kinds:**
  - **Solid fill:** colour and opacity.
  - **Hatch patterns:** ANSI31–38, common ISO / architectural patterns (concrete, brick,
    insulation, earth, steel, wood, glass) and user patterns imported from AutoCAD `.pat`
    definitions.
  - **Background colour:** a hatch can have one behind its lines.
  - Gradients are a later extra.
- **Pattern scaling (the Revit distinction):**
  - *Drafting* patterns keep a constant size on paper (scaled by the sheet scale).
  - *Model* patterns have real-world size (e.g. brick courses of 75 mm) and scale with the
    geometry.
  - Each hatch has an angle, scale and origin, and its own colour, line weight and layer
    (by layer by default).
- **Draw order:** fills draw beneath curves, layered by layer order, with "bring to front / send
  to back" within a layer.
- **Selection and area:**
  - Clicking inside a fill selects it at a lower priority than curves.
  - Hatch lines aren't snap targets.
  - A selected fill shows its area, perimeter and centroid in the properties panel.
- **Rendering:**
  - Solid fills are tessellated with lyon from the region (arcs flattened to a tolerance tied
    to zoom), cached per region revision and level of detail.
  - Hatch lines are clipped to the exact boundary on the CPU in `f64` (pattern line families
    against lines and arcs), then drawn through the existing SDF line pipeline, so they stay
    crisp and exact far from the origin.
  - Hatches too dense for the current zoom fade to a tinted fill to avoid moiré.
  - Counts are capped per frame with progressive refinement.
- **Files and interop:**
  - The file gains a `fills` table: boundary, seed, kind, pattern, scale, angle, origin and
    appearance.
  - DXF HATCH is written with boundary paths, associative where possible. PDF writes true
    filled paths and clipped hatch lines; SVG writes `<pattern>` or clipped paths.

---

## 6. Rendering (drift-render)

- **WebGL2-compatible baseline:** no storage buffers or compute shaders in the core path. Per-primitive
  data goes through instanced vertex buffers, and uniform buffers stay within WebGL2 limits.
  WebGPU-only optimisations, if any, must be optional.
- **Pipelines** (instanced, one draw call per primitive type):
  - Line segments: quads expanded in the vertex shader with an SDF fragment. They have constant
    screen-space width, anti-aliasing, and dash patterns computed from arclength in the shader.
  - Circles and arcs: an analytic SDF on a bounding quad, so they are perfectly round at any zoom
    with no tessellation.
  - Ellipses and splines: adaptive flattening on the CPU (zoom-aware, cached per LOD) fed into
    the segment pipeline.
  - Points, handles, glyphs, and dimension text: an SDF/MSDF atlas.
  - Fills/hatches: lyon tessellation, cached.
- **Grid:** a procedural shader with adaptive major/minor spacing.
- **Picking:** an R-tree (`rstar`) on the CPU over the `f64` geometry, with a screen-space
  tolerance. It is exact and needs no GPU readback.
- **Redraw policy:** event-driven (no frames are drawn while idle). Each redraw rebuilds instances
  on the CPU in `f64`, camera-relative, culled and clipped, then uploads them as `f32` pixel
  coordinates. This is what makes rendering exact far from the origin. Measured in M0: 50k visible
  entities build in about 1.4 ms per frame (release, native). If that becomes a bottleneck, the
  fallback is an `f32` world-space upload with a periodically re-centred anchor.
- **Color states:** selection, hover, preselection, DOF colors, construction, and errors, all
  driven by per-instance flags.

---

## 7. UX principles

- **Canvas-first:** minimal chrome, with a left toolbar of tool groups, a right property panel,
  and a bottom status/DOF bar.
- **Command palette** (Ctrl+K / S-key shortcut wheel) for fast tool access.
- **Esc always cancels,** and right-click opens context-sensitive actions. Every tool shows an
  inline hint ("Click start point · Tab to type length").
- **Feedback over dialogs:** conflicts are shown inline on the canvas rather than in modal popups.
- **Shortcuts:** remappable and stored in a config file.
- **Themes:** dark and light. Line weights scale with HiDPI.

---

## 8. Roadmap

Each milestone ends with something usable and demoable **in both the browser and native builds**.
Every merge to `main` deploys the web build to GitHub Pages, so progress is always live.

**M0 — Foundations, native and web (2 wks)** — *done locally on 2026-10-06. The CI workflow is
written but hasn't run yet (no GitHub repo).*
- Rust toolchain, workspace, licences, contributor docs, and CI:
  - fmt, clippy, tests, `cargo-deny`.
  - A `wasm32` build check, a WASM size report, and Pages deploy.
- An eframe app running natively and in the browser (WebGPU and WebGL2 verified).
- Camera pan/zoom, procedural grid, and the instanced line/circle/arc SDF pipelines.

**M1 — Geometry without constraints (2–3 wks)** — *done locally on 2026-10-06.*
- Document model, line/circle/arc/point/rectangle tools, selection, move, undo/redo.
- Save/load through `drift-platform` (native dialogs; web File System Access API or download), plus
  IndexedDB autosave.
- R-tree picking and basic snapping (endpoint, midpoint, center).

*M1 as built, and where it differs from the above:*
- **Model:** curves reference shared point entities (points own `x`/`y`, circles own `radius`).
  These are the solver unknowns; M2 maps them to a parameter vector instead of storing
  `ParamId`s in the document. Arcs are centre/start/end points, with the end kept on the circle.
- **Connectivity before constraints:** snapping onto an existing point reuses it, and dropping
  a dragged end point on another merges them. This is the "coincident by aliasing" the solver's
  substitution pass would produce anyway.
- **Undo:** transactions record the before/after of each touched entity. Drags are one step.
- **Snapping** also covers quadrants, nearest-on-curve and an optional grid, with Alt to
  suppress it.
- **Web save is a download**, and open uses a file picker. The File System Access API (save in
  place on Chromium) is deferred.
- **Autosave / session restore** uses eframe's storage (a file natively, `localStorage` on the
  web) every 10 s and on exit, instead of IndexedDB. This is fine up to roughly 30k entities.
  Revisit IndexedDB if documents outgrow `localStorage`.
- **Testing:** headless end-to-end UI tests (`egui_kittest` + wgpu) drive the real app, in
  addition to unit tests for the model, geometry, file format and each tool.

**M2 — Solver core (4–6 wks, highest risk)** — *done locally on 2026-10-06.*
- Parameter/equation framework, Newton–LM, substitution of coincident/horizontal/vertical, and
  component decomposition.
- Constraints: coincident, H/V, parallel, perpendicular, equal, fix, distance, radius, and angle.
- Constraint-preserving drag, plus DOF coloring.
- Jacobian finite-difference tests and a property-test harness.
- Benchmarks run **in the browser** too: the WASM single-thread numbers are the ones that count.

*M2 as built (2026-10-06), and what changed from the design in §4:*
- **Step formulation:** each Newton step solves `(JᵀJ + μ·M⁻¹) Δ = −JᵀF` in parameter space.
  As μ → 0 this is the weighted minimum-norm step from §4.2. The first version used the
  equation-space form `J M Jᵀ`, but substitution merges parameters into widely shared unknowns
  (e.g. one y for a long run of horizontal edges), and each of those turned `J M Jᵀ` into a dense
  clique. In parameter space they are single hub nodes instead.
- **Linear algebra:** pure Rust (no faer). It uses a Reverse Cuthill–McKee ordering with hubs
  ordered last, envelope (skyline) Cholesky for the steps, and row-wise Givens QR of the
  equilibrated Jacobian for rank/DOF analysis. Rank decisions on `JᵀJ` proved unreliable for
  nearly-parallel constraints (cond² error growth), which a property test caught.
- **Measured** (native, release; one connected component unless noted):

  | Sketch | Solve | DOF analysis |
  |---|---|---|
  | 1,000 entities, 750 constraints (125 independent rectangles) | 0.8 ms | 0.5 ms |
  | 1,600 entities, 1,598 constraints | 3.6 ms | 4.8 ms |
  | 4,000 entities, 3,998 constraints | 6.9 ms | 22 ms |

  Before the sparse rewrite, the 4,000-entity case took 11.8 s to solve and 1.5 s to analyse.

  In the browser (release WASM in Chromium/WebGPU, after warm-up, via `?bench` or
  Help → Run solver benchmark), chained rectangles in one component:

  | Entities | Solve | DOF analysis |
  |---|---|---|
  | 200 | 0.9 ms | 4.4 ms |
  | 800 | 5.8 ms | 6.9 ms |
  | 2,000 | 9.6 ms | 24 ms |

  That is about 2.5–3.5× native. The solver crate is built with `opt-level = 3` inside the
  size-tuned web profile. The 2,000-entity row misses the "full solve < 5 ms" target in the
  browser (it is met natively). The DOF analysis only runs after an edit, never per drag frame.
- **Editing rules:**
  - Adding constraints is refused when it conflicts, is redundant (adds a dependency or removes
    no freedom), or collapses geometry (e.g. horizontal plus vertical on one line).
  - Dimensions are created at the current measured value, so adding one never moves anything.
    The value is then typed into the focused field.
  - Driven/reference dimensions are M4.
- **Not yet:** tangent and the other M4 relations, so the example plate's fillets can't be
  fully constrained yet. Constraints have no on-canvas glyphs (M3); they are listed in the
  properties panel.

**M3 — Sketching UX (3–4 wks)** — *done locally on 2026-10-07.*
- Inference lines and auto-relations, typed input during drawing, and the Smart Dimension tool
  with an on-canvas editor.
- Relation glyphs, the property panel, and the status bar DOF count.

*M3 as built:*
- **New dimension kinds:** diameter, horizontal distance, vertical distance, and point-to-line
  distance.
  - Horizontal/vertical and point–line distances record which side the geometry is on, so
    solutions can't flip over.
  - Dimension labels store an offset from an anchor on the measured geometry, so they move
    with it.
  - File format v3 (v1/v2 still load).
- **Smart Dimension (D):** click a line, circle, arc, two points, two lines, or a point and a
  line. Where you place the label chooses aligned vs horizontal vs vertical, as in SolidWorks.
  The value field opens straight away. Redundant dimensions are refused with a message (driven
  dimensions are M4).
- **Dimensions on the canvas:** extension lines, arrows, and text with a background. Drag the
  text to move it (one undo step); double-click it to edit the value in place.
- **Relation glyphs:** small vector icons for H, V, ∥, ⊥, =, fix and coincident, stacked per
  entity. Hover or click one to highlight its geometry; Delete removes the picked one.
- **Automatic relations:**
  - Lines within the pick tolerance of horizontal/vertical are snapped straight and get the
    relation.
  - Rectangles get their four H/V relations.
  - Typed angles that are multiples of 90° add H/V relations, and use exact unit directions
    so already-placed points don't shift by round-off.
  - On/off in the View menu.
- **Inference lines:** new points align horizontally or vertically with the last few points the
  pointer snapped to (SolidWorks-style "wake up" points), shown as dashed guides. They add no
  relation.
- **Typed input in every drawing tool:**
  - First point: X/Y.
  - Line: length/angle (Tab switches field). Rectangle: width/height. Circle: diameter.
    Arc: radius, then sweep.
  - Typed sizes become dimensions.
  - Keys are routed to the tool before shortcuts. Keyboard focus that Tab moved onto a button
    is released while a tool takes typed values.
- **Midpoints (added after the user asked):**
  - Line midpoints can be hovered and picked in Select and Smart Dimension, within 60% of the
    normal pick radius so clicking elsewhere on the line still picks the line.
  - A **Midpoint** relation keeps a point at a line's midpoint. It is offered for a point plus
    a midpoint or line, and added automatically when a new point snaps to a midpoint or a
    dragged end point is dropped on one.
  - Distance dimensions (aligned / horizontal / vertical, and the point side of point–line)
    can reference a midpoint by putting the *line's* id in a point slot
    (`Slot::PointOrMidpoint`). The solver adds a hidden midpoint (two unknowns plus two
    equations, so no extra DOF), and a small triangle marks it on the canvas.
- **Not yet:** dimension text uses egui's font in the overlay (fine on screen; PDF output in M8
  will draw dimensions itself). There are no witness-line gaps or arrow flipping for very short
  dimensions beyond a simple "arrows inside/outside" rule. Angles are always measured
  counter-clockwise from the first line picked.

**M4 — Full relations and diagnostics (3–4 wks)**
- Tangent, concentric, symmetric, collinear, point-on-curve, and coradial (midpoint was done in
  M3).
- Redundancy/conflict detection and the driven-dimension prompt.

*M4 as built:*
- **New relations:** tangent (line–circle/arc, circle/arc–circle/arc), concentric, coradial,
  collinear, point-on-curve (lines extended infinitely, and arcs' full circles) and symmetric
  (two points about a line).
  - **Solver:** concentric and coradial merge centres by substitution; coradial also equates
    the radii.
  - **Tangency without a shared point:** it records which side the geometry is on (line–circle
    side; outside vs inside for two circles) when added, so it can't flip.
  - **Tangency at a shared end point (fillets):** it uses perpendicular-radius /
    collinear-centre equations instead of distance = radius. Distance = radius has a double
    root there and converged only to about 1e-5.
  - Validation refuses a point on its own curve and a symmetry axis through either point.
- **Driven (reference) dimensions:**
  - The document keeps a journalled `driven` set. The solver skips driven dimensions, and
    each commit refreshes their stored values to the measurement.
  - They are drawn grey with the value in parentheses.
  - A dimension refused as redundant or conflicting (from Smart Dimension or the panel) opens
    a "Make this dimension driven?" prompt.
  - A Driven checkbox in the constraint list switches either way. Switching back to driving is
    checked for over-definition like a new constraint.
- **Automatic point-on-curve:** a new point placed on a curve (nearest-on-curve or quadrant
  snap) gets an On curve relation, as does a point dropped on a curve while dragging. This
  respects the automatic-relations toggle. Typed coordinates never add one.
- **Glyphs** for all six new relations. Symmetric sits on its axis, and on-curve on its point.
- **File format v4:** the new relation records plus `"driven": true` on dimensions. v1–v3
  still load.
- **Not yet:**
  - Tangent glyphs sit beside the curve, not at the tangency point.
  - Relations between more than two curves (e.g. "all tangent") are added pairwise by hand.
  - The constrained example plate is still unconstrained.
- **Added after M4 at the user's request: Blender-style modal keys.**
  - **Tool keys:** tools can claim X / Y / G / B (`Tool::key`). Unclaimed X still toggles
    construction.
  - **Line tool:** X / Y lock the current segment to an axis (per segment, always adds H/V).
  - **Select tool, G (grab):** a modal move of the selection.
    - Live preview uses incremental solver drags, like a mouse drag.
    - Confirming rolls back and re-applies the final offset in one step from the original
      geometry, so typed and snapped moves are exact. It falls back to 16 steps if one solve
      can't reach.
    - Typed fields: dX/dY, or a Distance when an axis is locked.
    - B re-picks the base point; snapping there may hit the selection itself.
    - Right-click cancels; other shortcuts are suspended while it runs.
  - **Grid snapping** moved from G to Shift+G.
- **Added after M4 at the user's request: 3-point and tangent arc tools.**
  - **3-point arc:** start, end, then a point on the arc. You can type the chord length/angle
    and the radius. A third point picked on an existing point gets On curve.
  - **Tangent arc:** starts on an existing line/arc end. Where several curves meet, it uses
    the one whose outgoing direction best matches the pointer. It gets a Tangent relation
    (the exact shared-endpoint formulation) and chains like a polyline. Radius and/or sweep
    can be typed.
  - **Switching:** A cycles centre → 3-point → tangent. Mid-polyline, A switches to tangent
    arc and L back to lines, continuing from the chain end (`Tool::chain_end` /
    `continue_from`).
  - Rotate (R) / scale modal ops would follow the same pattern. R and S are currently tool
    shortcuts.

**M5 — Parametrics and exploration (3–4 wks)**
- Global variables, the expression engine with units, and the equations panel.
- Variable sliders with live re-solve, variants/configurations, snapshots, free-DOF arrows, and
  pinned measurements.
- **Unit selection** (added at the user's request): a per-document unit system, metric or
  imperial, saved in the file's `meta.units`. Geometry stays in mm internally, so switching
  units never moves anything.
  - **Metric:** mm, cm or m, with configurable decimal places.
  - **Imperial engineering:** decimal inches (`12.375"`) or decimal feet (`1.031'`), with
    configurable decimal places.
  - **Imperial architectural:** feet-inches with fractions (`1'-0 3/8"`).
    - Fractions are rounded to a chosen denominator (1/2 … 1/64) and reduced (`4/8` → `1/2`).
    - Zero feet is omitted or shown, by option (`0'-6"` vs `6"`).
  - **Everywhere it applies:**
    - Dimension labels, typed input fields, the properties panel, cursor readouts and the
      status bar.
    - The grid's minor/major spacing follows the unit system: e.g. 1/16", 1", 1' steps for
      imperial instead of 1/10/100 mm.
  - **Input accepts any unit regardless of the display setting:**
    - Unit suffixes: `25.4mm`, `1in`, `3"`, `2'`.
    - Architectural forms: `1'6"`, `1'-6 1/2"`, `6-1/2"`, `3/8"`.
    - Bare numbers are read in the document's current unit (inches for architectural).
  - **Rounding vs precision:** display rounding never changes stored values. Converted values
    round-trip losslessly (the inch is exactly 25.4 mm). A dimension shows a `~` marker when its
    displayed value is rounded (e.g. a metric dimension shown in fractional inches).
  - **UI:** a status-bar/View-menu selector, with a document setting in File → Document
    properties. A user preference sets the default for new documents.

*M5 as built (2026-10-07):*
- **Expressions (`drift-core::expr`):**
  - A recursive-descent parser with `+ - * / ^`, the functions `sqrt abs min max hypot round
    floor ceil sin cos tan asin acos atan atan2`, and the constant `pi`.
  - Quantities carry dimensions (length / angle / number exponents), so mixing them is an error
    with a plain message ("can't add a number and a length").
  - Base units are mm and **degrees**. Degrees rather than radians, so a typed 30° stays exactly
    30 through evaluation.
  - A unit after a term applies to the whole product before it (`100/2 mm` = 50 mm, `pi/4
    rad`), except right before `^` (`3mm^2`).
  - Architectural forms (`1'-6 1/2"`, `6-1/2"`, `3/8"`) are lexed as single literals, so `1'-6"`
    isn't a subtraction; with spaces (`2' - 6"`) it is.
  - **Bare numbers** take the context's unit: the display unit for lengths (inches for
    architectural), degrees for angles. Stored text gets the unit made explicit (`5` → `5 in`),
    so changing the display unit never changes a value.
- **Variables and linked dimensions (`drift-core::params`):**
  - A journalled variables table (name, expression, kind: length / angle / number). Dimensions
    can link to an expression; their stored value is its last result.
  - Evaluation runs in dependency order with cycle detection ("circular reference: a → b →
    a"). Invalid results, such as a negative length, are refused.
  - Renaming rewrites every expression token-wise; deleting refuses while the variable is
    used.
- **Units (`drift-core::units`):** mm / cm / m, decimal inches or feet, and feet-inches with
  fractions (1/2–1/64, reduced, optional `0'-`). Readouts use fixed decimals with the unit.
  Dimension labels trim zeros and get a `~` when the display rounds the value (float
  round-off doesn't count).
  - **Imperial grid:** 1/64"–1/2", 1", 6", 1', then tens of feet.
- **Editor:**
  - Every parameter edit re-evaluates, pushes linked values into the dimensions, solves, and
    commits as one undo step, or rolls back with the reason.
  - Sliders scrub plain-valued variables live in one open transaction. A value the sketch
    can't take is skipped, keeping the last good one, and release commits.
  - Loading re-applies expressions.
- **Typed input:**
  - Every tool field accepts expressions. `=` starts typing with a variable name, and once
    typing has started letters go to the field rather than shortcuts.
  - The cursor label shows `= result` or the error.
  - Dimensions created from typed expressions that use variables stay linked
    (`Editor::add_typed_dimension`).
  - Linked dimensions show `Σ` and edit as their expression.
- **Variants:** named sets of variable expressions, applied as one step; the matching one is
  shown as active. Other variants can be drawn as ghosts, solved on a copy and cached per
  revision.
- **Snapshots:** named document copies for the session. Restoring is one undoable step via
  `Document::restore_from`, which journals every differing key.
- **Free directions (View → Show free directions):**
  - `drift_solver::free_directions` takes the null space of each component's equilibrated
    Jacobian by dense RREF (components up to 400 unknowns). Each point's 2×k motion block then
    gives fixed / one direction / plane, and a rotation shows as the tangent direction.
  - Drawn as small arrows.
- **Pinned measurements:** measured-only dimension definitions in the document, listed with
  live values in the Parameters tab. They're removed with their geometry.
- **File format v5:**
  - `display` (only non-default settings), `variables`, a dimension `expr`, `measurements` and
    `variants`. Sections are written only when used.
  - `meta.units` still names the storage unit (always mm). v1–v4 load unchanged.
- **UI:**
  - The right panel has Properties / Parameters tabs.
  - The Parameters tab has: units, variables with sliders, variants, snapshots, and pinned
    measurements.
  - A units picker sits in the status bar; Pin measurement (collapsed) is in Properties.
- **Not yet:**
  - Variables can't reference measurements. Snapshots aren't saved in the file.
  - There's no user preference for the default units of new documents (they start metric).
  - There's no dual-unit display.

**M6 — Modify tools, layers and line weights (6–7 wks)**
- Robust intersections in drift-geom, then trim/extend/split, fillet/chamfer, offset, mirror, and
  linear/circular patterns.
- Region detection with area properties (the basis for fills in M7).
- Layers, colours, line weights and line types, and the thin-lines toggle (§5.11). File
  format v6.

*M6 as built (2026-10-07):*
- **Intersections (`drift-geom::intersect`):**
  - Hits are reported for lines, arcs and circles, with each hit's parameter along both curves.
  - **Touching cases follow fixed rules:**
    - Tolerance is 1e-9 relative to the coordinates.
    - A near-touch gives a single hit flagged as tangent.
    - Hits just past an end snap exactly onto it.
    - Parallel and concentric overlaps give no hits.
  - **Extended variants:** a line extended or an arc taken as its full circle, used by extend
    and by offset corners.
  - **Tests:** a randomised check that all of 3,000 pairs' hits lie on both curves.
- **Modify operations (`drift-app::modify`):** each is one undo step, solved, or refused with
  a reason.
  - **Trim:**
    - Removes the clicked piece between the nearest cuts. The curve keeps its id, so its
      relations stay.
    - A removed middle piece leaves two parts joined by Collinear (lines) or Equal (arcs).
    - A circle becomes an arc in place.
    - New ends get On curve to the cutter, or join its existing end.
  - **Extend:** the nearest end moves to the first curve along the extension; a connected end
    is refused.
  - **Split:** a new shared point, with Collinear or Equal between the parts.
  - **Fillet:** at a two-line corner. It adds an arc tangent to both lines (2× Tangent) and a
    Radius dimension, and the sharp corner point goes.
  - **Chamfer:** two distances along the lines.
  - **Offset:**
    - Applies to the connected chain, with corners rejoined (line–line by intersection,
      tangent joins shared).
    - Lines get Parallel and a point-to-line distance; arcs and circles share their centre.
    - Offsets that invert a segment are refused.
  - **Mirror:** Symmetric relations about the axis. Points on the axis are shared, and arcs
    swap ends to stay counter-clockwise.
  - **Linear and circular patterns:**
    - They copy geometry and the relations within the selection (with expressions, label
      placements and driven flags).
    - Rotation drops orientation-dependent relations (H/V, H/V distances, Fix).
  - Copies keep their source's layer, appearance and line role (`Document::copy_look`).
- **Tools:**
  - Trim (T) previews the piece in red dashes and supports drag-across power trim.
  - Extend (E) previews the extension.
  - Split, Fillet and Chamfer (typed sizes are remembered).
  - Offset (O): pick a chain, then set the side and distance by pointer or typing.
  - Mirror (M): uses the selection; click the axis.
  - Patterns are in a collapsed Pattern section in Properties; their fields take expressions.
  - The toolbar is grouped into Draw / Annotate / Modify and scrolls; shortcuts are optional.
- **Regions (`drift-geom::regions`):**
  - Curves are split at their intersections, ends merged into vertices, and dangling edges
    pruned.
  - Faces are traced on a half-edge graph by taking the edge just clockwise at each vertex.
    Outgoing edges are ordered by angle, then curvature, so tangent fillets work.
  - **Exact area:** chord shoelace plus each arc's circular segment. Perimeter is exact; the
    centroid comes from fine flattening.
  - Clockwise boundaries become holes of the smallest larger containing region.
  - Only solid curves count: construction and centre lines are references.
  - **In the app:**
    - Click inside a closed area to select its region.
    - It shows a translucent fill, triangulated by lyon with even-odd holes.
    - Properties shows area (in display units²), perimeter, centroid and hole count.
- **Layers (§5.11):**
  - A journalled layers table with layer `l0` ("0") always present, a current layer, and
    per-entity layer plus appearance overrides.
  - **Locked layers:** pinned by the solver (marked constraint id 0, never reported as a
    conflict) and not selectable.
  - **Hidden layers:** not indexed, so not drawn, picked or snapped.
  - Deleting a layer moves its geometry to "0".
  - **Rendering:**
    - Per-item colour, weight and line type.
    - Weights show at about 2× print size on a 96 dpi screen (minimum the old default width).
    - Dash patterns scale with weight. The shader supports phantom lines: a negative dot means
      two dots.
  - **Thin lines** (W / status bar / View menu): every line is one device pixel.
  - **View → Constraint status colours:** on by default. Off shows layer colours, with low
    contrast colours (WCAG ratio under 2:1 against the theme background) swapped for the
    foreground.
  - **UI:** a Layers tab (current, show / lock / print, auto colour or picker, weight, line
    type, select all, move selection here, add / delete), plus layer, colour, weight and line
    type overrides for the selection in Properties.
  - **File format v6:**
    - `layers` and `current_layer` are written only when not default.
    - Entity `layer` / `color` (#rrggbb) / `weight` (mm, snapped to ISO) / `linetype`.
    - An entity on a missing layer is rejected. v1–v5 load unchanged.
- **Example plate:**
  - It is now fully defined (DOF 0, no redundancy, nothing moves when solved):
    - Tangent fillets.
    - The first bolt hole on the bolt circle at an exact 45°; the other three Symmetric about
      the centre lines.
    - A slot of vertical sides tangent to both ends.
  - Its dimension labels are laid out like a drawing.
  - Zoom-to-fit and opening a file now include placed dimension labels.
- **Not yet:**
  - Fillets between a line and an arc, or two arcs.
  - Trim against points.
  - Offset relations for arcs (only the shared centre).
  - Pattern copies aren't linked to the pattern spacing.
  - Dual units and line-weight scale settings.

**M6.5 — UI cleanup pass**

*As built (2026-10-07):* make Drift feel like a native Apple app without hiding any capability.
- **Theme (`theme.rs`):**
  - System fonts on desktop (SF Pro / Segoe UI / Noto, with SF Mono / Consolas / Menlo for
    numbers); egui's fonts stay as fallbacks, and the web build keeps them.
  - macOS-style light and dark palettes, the system accent blue, soft rounded controls, hairline
    separators and quiet shadows.
  - Shared helpers: section headers, label / value rows, segmented controls and status pills.
- **Toolbar:** vector icons for every tool (`icons.rs`), grouped by draw, dimension and modify.
  Tooltips give the name, shortcut and a one-line description.
- **Inspector:** a segmented Inspector / Parameters / Layers switch.
  - An empty selection shows document counts.
  - A single entity shows its geometry in aligned rows.
  - The sketch-wide constraint list folds away behind "Show all" unless something conflicts.
- **Parameters:** a Metric / Imperial segmented control, with aligned rows for format and
  precision.
- **Layers:** each layer is a card with eye, lock and print icon toggles; the current layer is
  outlined in the accent colour.
- **Status bar:**
  - Coordinates on the left; the hint, truncated, in the middle.
  - On the right: icon toggles for grid, axes, snap and thin lines, the units menu, a DOF pill
    (Fully defined / N DOF free / conflicting) and the zoom percentage.
- **Messages:** shown as a toast at the bottom of the canvas that fades out.
- **Empty canvas:** a short "Start a sketch" guide.
- **Grid and axes:** both can be hidden from the View menu or the status bar, and Ctrl/⌘+' toggles
  the grid. View settings (grid, axes, snap, thin lines, overlays) persist between sessions.
- **Menus:** use platform shortcut glyphs (⌘ / ⇧ on macOS). Ctrl/⌘+N starts a new sketch.
- **Accessibility:** icon buttons and segments carry accessible labels.
- **Tests:** UI tests render the light theme, the hidden grid and the Parameters and Layers tabs.

**M7 — Drafting, fills and hatches (5–6 wks)**
- ISO/ASME dimension styles, tolerances, baseline/chain/ordinate dimensions, and hole and chamfer
  callouts.
- Notes, leaders, centre marks and centrelines.
- Fills and hatches (§5.12): associative boundaries, solid fills, built-in and `.pat` patterns,
  drafting vs model scaling, and draw order.

*M7 as built (2026-10-08):*
- **Fills and hatches:**
  - **Fill / hatch tool (H):** click inside a closed area. The area under the pointer is
    highlighted first, and islands (holes) are left out.
  - **From a selected region:** Fill and Hatch buttons fill the region already picked.
  - **Associative:** a fill stores its region's outer-boundary curve ids and a seed point.
    - Regions now report the curve each loop edge came from (`Loop::sources`).
    - A fill is re-found by its exact curve set (nearest its seed if several match), or by the
      region around its seed if they still share a boundary curve.
    - Otherwise it is flagged "boundary lost": it keeps its last shape and can be re-picked.
  - **Kinds:**
    - Solid fills: colour (by layer or own) and opacity.
    - Hatches: pattern, scale, angle, origin, drafting or model size, line colour, weight and
      an optional background colour.
  - **Patterns:** the new `drift-draft` crate parses AutoCAD / Revit `.pat` files
    (`;%UNITS=INCH`, `;%TYPE=MODEL`). Built-in patterns, swatched in the pattern menu:
    - ANSI31–38, STEEL, CONCRETE, EARTH, INSULATION, GLASS, WOOD, GRID, DOTS, SAND.
    - BRICK and BLOCK as model patterns.
  - **Imported patterns:** Parameters → Hatch patterns → Import .pat… stores them in the
    document, so they travel with the file.
  - **Hatch lines:** clipped exactly in `f64` against line and arc edges.
    - Arcs are split where they turn back across the scan direction.
    - The even–odd rule is decided just off each scan line, so vertices count once.
    - Lines are generated only for the visible area, then drawn by the existing SDF line
      pipeline.
    - Hatches denser than 2.5 px on screen, or past 40,000 segments, become a tint (no moiré).
  - **Solid fills:** tessellated with lyon (relative to the region's centroid, for f32) and
    cached per document revision. They are drawn by a new triangle pipeline under all curves
    (WebGL2-compatible).
  - **Draw order:** by layer, then each fill's order (Bring to front / Send to back).
  - **Picking and the inspector:** clicking inside a fill picks it, below curves in priority.
    The inspector shows its style, layer, draw order, area, perimeter and centroid.
- **Dimension styles (Parameters → Drafting):**
  - ISO 129 and ASME Y14.5 presets: text height, arrowheads (closed, open, tick, dot) and their
    size, extension gap and overshoot.
  - Text: ISO places it aligned above the line, readable from the bottom or the right; ASME
    uses horizontal text in a break.
  - Trailing and leading zeros, dual units in brackets, and the drawing scale (1:N).
  - Sizes are paper millimetres at 96 dpi on screen.
- **Dimension text (picked dimension → inspector):**
  - Prefix and suffix, with ×, Ø, THRU and TYP shortcuts.
  - Tolerances: symmetric (±), deviation (stacked + / −), limits (stacked) and fits (H7).
- **Chain and baseline:** after a horizontal or vertical dimension, Shift+click more points to
  chain dimensions along the row. B switches to baseline: all from the same base, each row a
  step further out. Dimensions that would over-define the sketch become driven automatically.
- **Annotations (new `annotations` table):**
  - **Notes (N):** click for a free note, or click geometry first for a leader with an arrow or
    dot.
    - The text is typed on the canvas: Return finishes, Shift+Return adds a line, Esc cancels.
    - Leader targets are points or spots on curves, so they follow the geometry.
  - **Centre marks and centre lines:**
    - Click a circle or arc for a centre mark. Then click another circle, arc or point for a
      centre line through both centres, or two lines for one midway between them.
    - "Add centre marks" marks every selected circle at once.
    - They are drawn by the GPU as centre lines.
  - **Callouts:** `4× Ø6.4 THRU` for holes (counting same-size holes; THRU, TAP, CBORE or CSK as
    the suffix). `2 × 45°` or `2 × 3` for chamfers, with legs measured to the corner the
    chamfer cuts.
  - **Ordinates:** click the origin point (it reads 0), then points.
    - X / Y choose the coordinate.
    - Labels line up in a row clear of the geometry and can be dragged.
  - **Editing:** every annotation's label can be dragged, and double-click edits a note.
  - **Deleting geometry:** marks and callouts on deleted geometry go; a note keeps its place
    without the leader.
- **Format v7:** adds `drafting`, `fills`, `annotations` and `patterns`, and dimension
  `prefix` / `suffix` / `tol`. Defaults are left out of files, and v1–v6 load unchanged.
- **Not yet:**
  - Sheets: the drawing scale stands in for the sheet scale until M8.
  - Gradients and hatch origin picking on the canvas.
  - Surface finish symbols, datum labels and balloons.
  - Dimension text editing on paper: annotations are screen-sized, like dimensions.
  - Annotations don't move with a G grab of their geometry's neighbours, only through their
    anchors.

**M8 — Sheets and output (3 wks)**
- Sheets with a scale, title-block templates, and a revision table.
- True-scale vector PDF, SVG, and DXF import/export (layers and line types preserved). Printing.

*M8 as built (2026-10-08):*
- **Sheets (Sheets tab; `sheets` in format v8):**
  - Paper: A4–A0 or ANSI A–E, landscape or portrait.
  - A drawing scale per sheet, picked from the preferred scales (`fit_scale`, 10:1 to 1:10000).
    "Fit" centres the drawing, its dimensions included, at the largest scale that clears the
    title block.
  - An ISO 7200 or ASME Y14.1 title block, or none. Its fields:
    - Typed: title, drawing number, drawn by, checked, date (with "Today"), company and
      material.
    - Automatic: scale, sheet "n / N", and the revision (the latest row of the table). ANSI
      sheets also show the size letter.
  - A revision table in the top-right corner: "Add revision" takes the next letter, today's
    date and the author.
  - Typing into the fields merges into one undo step.
- **Sheet preview (View → Sheet, or Show sheet):**
  - The canvas shows the page exactly as it prints.
  - Scroll zooms and middle / right drag pans. Dragging moves the drawing within the sheet,
    as one undo step.
  - F refits and Esc returns to the model.
- **Output pipeline:**
  - `drift-draft::page` is a display list in millimetres (y up): lines, polylines, true arcs
    and circles, fills (rings and triangles, plus masks behind text) and Helvetica-metric text.
    Every item has a pen (colour, width, ISO line type) and a layer.
  - `drift-draft::sheet` draws the border, centring marks, title blocks and the revision
    table. Their backgrounds mask the view.
  - In the app, `output.rs` builds a sheet (clipped to the drawing area) or model space.
    - Exact: printable geometry (no points, construction or hidden / non-printing layers), by
      layer or own colour, weight and line type; centre-style curves as centre lines.
    - Fills are faded to paper by their opacity. Hatches are clipped exactly, with drafting
      patterns sized by the sheet's scale.
    - Centre marks are drawn too.
    - Dimensions and notes go through a new `Pen` trait (egui painter or `Recorder`), so
      output matches the screen. Text is placed by its glyph baselines.
- **PDF (`drift-io::pdf`):**
  - Hand-written, no dependencies: one true-scale page per sheet, Bézier arcs, even–odd fills,
    clipping, and dash patterns.
  - Text is Helvetica in WinAnsi (Ø, ±, °, ×), positioned with Helvetica widths.
  - Checked in Chromium's viewer and Inkscape.
- **SVG (`drift-io::svg`):** the current sheet, in mm units, with a group per layer run, a
  clip path, true arcs and rotated text.
- **DXF export (`drift-io::dxf`):**
  - R12 (AC1009) ASCII of model space at full size, so every CAD program opens it.
  - Written in the document's display units, so lengths read the same elsewhere: inches for
    decimal-inch and architectural documents (with `$LUNITS` architectural), feet, mm, cm or
    m otherwise, plus `$INSUNITS`, `$LUPREC` and `$MEASUREMENT`.
  - LTYPE table (Continuous, Dashed, Hidden, Center, Phantom; `$LTSCALE` = drawing scale) and
    a LAYER table with nearest ACI colours and line types.
  - LINE / ARC / CIRCLE / POLYLINE / SOLID (fill triangles) / TEXT (`%%c`, `%%d`, `%%p`).
  - Dimensions and notes are exploded onto an ANNOTATIONS layer.
- **DXF import (File → Import DXF…, or open a .dxf):**
  - Reads LINE, ARC, CIRCLE, LWPOLYLINE and POLYLINE (with bulge arcs), POINT and the LAYER
    table, scaled by `$INSUNITS`. Files that don't give their units are read in the
    document's display unit.
  - Curves land on layers of the same names, created with their colour and line type.
  - Coincident ends share points, so outlines stay connected and can be constrained.
  - Text, splines, blocks and so on are counted and reported.
- **Layers HUD (View → Layers panel, Ctrl/⌘+L):**
  - A compact layer list to keep open while drawing. It floats over the canvas as a
    translucent panel, or docks to the canvas's left or right edge.
  - Each row: the current layer (click a name to switch), a colour swatch, the curve count,
    and show / lock toggles. Right-click a row to select its curves or move the selection
    there.
  - Its placement is saved with the view settings. The full editor stays in the Layers tab.
- **Printing (File → Print…, Ctrl/⌘+P):** writes the PDF to a temporary folder and opens it
  in the system viewer (a new tab on the web) to print.
- **Not yet:**
  - Several views per sheet, and title-block template editing or logos.
  - DXF line weights (R12 has none; R2000 output later), DXF HATCH and DIMENSION entities, and
    importing text, splines and blocks.
  - PDF font embedding (Helvetica is a standard PDF font).
  - Direct printing without a viewer.

**M8.5 — Floor-plan tools: rotate, groups, blocks, underlays**

*Part 1 as built (2026-10-08): rotate and groups.*
- **Rotate (R with a selection; Blender-style):**
  - Turns about the selection's centre. The angle follows the pointer, with multiples of 15°
    sticking within 2° and snaps giving exact angles.
  - Type an angle for an exact turn. B picks another pivot; click or Enter places; Esc or
    right-click cancels. The guide shows the pivot, the reference and current rays and the
    arc.
  - Quarter turns are exact. Horizontal / vertical relations and dimensions inside the turned
    set are swapped on a quarter turn, kept on a half turn, and removed (with a notice) at
    other angles. Fixed points are re-fixed where they land. Constrained geometry that can't
    turn is refused.
  - Without a selection, R is still the rectangle tool.
- **Groups (format v9 `groups`):**
  - Ctrl/⌘+G groups the selection (curves, plus standalone points); Ctrl/⌘+Shift+G
    ungroups. Both are also in the Edit menu.
  - Picking any member (click, Shift / Ctrl-click, box, or drag) picks the whole group.
    Moves (G, drag) and rotates take it together.
  - Double-click a group to edit inside it (members pick one by one); Esc or a click outside
    leaves it.
  - While a group is open, anything drawn or created (lines, arcs, points, the pieces a
    trim or split leaves) joins it, in the same undo step. Blocks being edited will work the
    same way.
  - Canvas: picked groups get a dashed frame with their name, hovered groups a faint frame,
    and the open group a solid "Editing …" frame.
  - The inspector shows the group: rename it, see its member count, Edit group, or Ungroup.
  - An entity is in at most one group. Members leave a group when deleted, and empty groups
    go.
*Part 2 as built (2026-10-08): blocks.*
- **Model (format v10 `blocks`, `instances`):**
  - A block is rigid geometry (lines, arcs, circles, points with their style and look) in
    its own coordinates, with its base point at the origin.
  - A copy places it with a position, angle, scale, mirror (x → −x before turning) and layer.
  - Copies aren't solved, so many copies cost nothing in the solver. They're drawn, picked
    and exported through their transform.
  - Deleting a block deletes its copies; layer removal moves copies to layer 0.
- **Making and placing:**
  - Make a block from the selection (Blocks tab or Edit → Make block). It's based at the
    selection's centre, and a copy replaces the geometry.
  - The Place block tool (I) previews the copy at the pointer, snapping by its base point. R
    turns the next copy 90° and X mirrors it.
  - A starter furniture library: chair, desk, double bed, sofa, WC, basin and a 900 door
    (mm).
- **Copies on the canvas:**
  - Pick a copy on its curves or inside its outline; Shift / Ctrl and box selection work.
  - Drag it, G (base = insertion point) and R (about its insertion point; several about
    their middle) move it, with other geometry in the selection too. Delete removes it.
  - The inspector edits position, angle (quick 0 / 90 / 180 / 270), scale, mirror and layer.
    It can replace the block, Explode (back to connected geometry), or Select all copies.
- **Edit in place:**
  - Double-click a copy (or Edit block). The copy becomes ordinary geometry in an open group,
    so anything drawn joins the block.
  - Esc, a click outside or "Finish editing" writes it back, and every copy updates.
- **Blocks tab:** thumbnails, copy counts, click to place, and a context menu to rename,
  select copies or delete. Sidebar segments are now sized to their labels.
- **Output:**
  - Sheets (PDF, SVG, print, preview) draw copies as their curves.
  - DXF export writes BLOCK definitions and INSERTs (a mirrored copy is a negative X scale).
  - DXF import reads BLOCKS / INSERT: blocks are created by name (reused if the drawing has
    one) and copies placed on their layers.
- **Not yet:** fills, hatches and text inside blocks; nested blocks; snapping to copies' own
  points; non-uniform DXF insert scales (X scale is used).
- **Since:** Shift+D duplicates the selection in place and grabs the copy (curves, their
  relations and dimensions except Fix, fills and hatches of copied areas, block copies).
  Shift+C or the status bar toggles constraint status colours. Layers have a separate fill
  and hatch colour (format v11 `fill`; absent uses the stroke colour).

*Part 3 as built (2026-10-08): underlays.*
- **Model (format v12 `underlays`):**
  - An underlay is a reference drawing kept in the file: lines, polylines, circles and arcs
    in its own units (PDF points, or mm for DXF), shared behind an `Arc` so undo is cheap.
  - It has a position, a scale (sketch mm per unit), an angle, a colour (grey by default),
    an opacity, and Visible / Snap / Locked switches.
  - Underlays can't be selected or edited. They print and export nowhere.
- **Import:**
  - File → Import underlay… (or opening any PDF) takes a PDF or DXF file.
  - PDF: a pure-Rust reader (`lopdf` plus our own content interpreter) follows the
    transforms (`cm`, `q`/`Q`), lines, curves (flattened), rectangles and closed paths
    through nested form XObjects. Filled shapes come in as their outlines. Text and images
    are skipped and counted, and the page's crop box and rotation are honoured.
  - A multi-page PDF asks which page. It's placed at paper size with the page's lower-left
    corner at the origin.
  - DXF: its curves (copies of blocks expanded) in the file's units.
- **Calibrate and place (Layers tab → Underlays):**
  - Calibrate…: click two points on the underlay (snapped), type the real distance, Enter.
    It rescales about the first point and reports the drawing scale (e.g. 1 : 50).
  - Scale presets for PDFs (metric 1 : n and architectural / engineering imperial), or a
    factor for DXF.
  - Move… by two points, X / Y, angle (0 / 90 / 180 / 270 turn about its middle), opacity
    (one undo step per drag), colour, Zoom to, Delete.
  - Convert to geometry: the underlay becomes lines, arcs and circles on the current layer,
    with coincident ends shared.
- **Snapping and drawing:**
  - Tools snap to underlay end points, midpoints, centres, quadrants and nearest-on-curve.
    The sketch wins ties, and grid snapping only applies when nothing is near.
  - Snaps on an underlay create new, unconstrained points.
  - Underlays draw beneath everything, faded towards the background and culled to the view
    via a per-underlay R-tree. That tree is rebuilt only when the underlay moves or changes.
  - Zoom to fit includes visible underlays.
- **Not yet:** raster (scanned) PDFs; PDF text as outlines; clipping paths; snapping to
  intersections between underlay curves.

**M9 — Advanced geometry (3–4 wks)**
- Ellipses, slots, polygons, and text.
- Splines (control-point and fit) with curvature combs and G1/G2 constraints.
- **Planned in four parts:**
  1. Polygons and slots.
  2. Ellipses and elliptical arcs (a new entity kind).
  3. Curves: a pen tool.
  4. Text that converts to geometry.
- **Curves must feel like the Illustrator / Affinity pen tool** (the user was emphatic; the
  SolidWorks spline tool is the counter-example):
  - Click places a corner; click-drag pulls out mirrored handles.
  - A rubber-band preview of the next segment always follows the pointer.
  - Alt breaks a handle; clicking the first point closes the path; Enter / Esc ends it.
  - Anchors and handles are edited directly afterwards.
  - Curvature combs and G1 / G2 are extras on top.

*Part 1 as built (2026-10-08): polygons and slots.*
- **Polygon (K):**
  - Click the centre, then a corner. Tab types the sides (3–64, remembered; 6 by default);
    X switches to circumscribed, where the pointer is the middle of a side.
  - Built as equal sides with every corner on a construction circle, so it stays regular:
    four freedoms (centre, size, turn).
  - Rotation sticks to 15° steps within 2°, and a level or plumb side gets Horizontal /
    Vertical.
  - A typed size is the circle's diameter (inscribed) or the centre-to-side distance
    (circumscribed, i.e. half across flats).
  - Tangent-sided polygons were rejected: an even-sided one can collapse into a rhombus.
- **Slot (U; U again cycles straight → centre → arc):**
  - Straight: two end centres, then the width. Centre: the middle, then an end centre.
    Arc: the arc's centre, the start and end centres (either direction), then the width.
  - Straight slots are two sides tangent to two equal end arcs, with a construction axis
    line (a Midpoint relation for centre slots): five freedoms.
  - Arc slots are concentric inner and outer arcs with end caps tangent to both (the caps'
    equal size follows, so no Equal): six freedoms.
  - Typed length and width become dimensions (centre-to-centre distance, side-to-side
    distance, or the cap's diameter for arc slots).
- **Not yet:** 3-point arc slots; a drawing-scale readout of a circumscribed size.

*Part 2 as built (2026-10-09): ellipses.*
- **Model (format v13 `ellipse` entities):**
  - An ellipse is three points: centre, the end of one semi-axis (`major`) and the end of
    the other (`minor`), the last kept on the perpendicular through the centre by one
    equation (5 freedoms).
  - A partial ellipse adds start and end points held on it (7 freedoms), counter-clockwise.
  - Being points, the axis ends snap, drag, rotate, mirror and take Distance dimensions with
    no special cases.
  - `Shape::Ellipse` uses parametric angles (a whole one sweeps 2π). Exact bounds come from
    where x and y peak; a shared flattening helper serves output and regions.
- **Solver:**
  - A point-on-ellipse equation (stretch across the major axis to a circle of radius a,
    then its distance from it) with an analytic gradient, checked by the finite-difference
    test.
  - Point on curve takes ellipses (a new `Curve` slot). Tangent, Equal and other circular
    relations stay circles-and-arcs only.
- **Tools:**
  - Ellipse: centre, one axis end, then the other axis (or type half-lengths).
  - Partial ellipse: the same, then start and end, picked on the ellipse.
  - C cycles Circle → Ellipse → Partial ellipse. Live previews at every step; a level or
    plumb axis gets Horizontal / Vertical points; typed sizes become Distance dimensions.
- **Geometry:**
  - Nearest point (sampling plus golden-section refinement), box selection, quadrant snaps
    at all four axis ends, underlay snaps.
  - Fills work inside ellipses (they take part in regions as fine straight edges).
- **Output:**
  - SVG: exact elliptical arc commands. PDF and the sheet preview: flattened.
  - DXF: ELLIPSE both ways, with the axes swapped when the second is the longer (DXF wants
    a ratio ≤ 1). A sweep within 1e-5 of a full turn reads back as a whole ellipse.
  - Blocks, Shift+D, mirror and patterns copy ellipses. The inspector shows the semi-axes
    and rotation, and the dimension tool on an ellipse measures its first semi-axis.
- **Toolbar:** rows are 24 px so all 26 tools fit an 800 px window.
- **Not yet:**
  - Trim, extend, split, fillet and offset with ellipses: ellipses don't cut or get cut.
  - Tangency to ellipses.
  - Splitting a fill region where an ellipse crosses another curve.

*Part 3 as built (2026-10-09): curves and the pen.*
- **Model (format v14 `bezier` entities, `smooth` relations):**
  - A curve is a cubic Bézier between two anchor points with two handle points (8
    freedoms); a path is a chain sharing anchors, so curves and lines join like lines do.
  - Smooth (G1) keeps the handles either side of an anchor on one line through it (the
    angle equation at 180°); handle lengths stay independent, as in Illustrator.
  - `Shape::Bezier` with exact bounds (derivative roots), evaluation, curvature, splitting
    (de Casteljau) and flattening.
- **The pen (B):**
  - Click: corner point. Press and drag: smooth point, handles mirrored; Alt while dragging
    breaks them (only the outgoing one moves; Alt before moving gives no incoming handle).
  - A live rubber band shows the next segment to the pointer; while dragging, both handles
    and the curve update.
  - Clicking or dragging on the first point closes the path (ringed when hovered), smooth
    if it was. Enter, Esc or a right-click ends an open path.
  - Segments with no handles at either end are lines; a missing handle sits a third of the
    way along, like a retracted one.
  - Alt-click a point between two curves to make it smooth or a corner.
- **Editing:**
  - Handles show only around picked or hovered curves (or a picked anchor or handle), with
    grey handle lines. Hidden ones can't be picked or box-selected, and handles are never
    snap targets.
  - Dragging or grabbing an anchor carries its handles. Dragging a smooth point's handle
    swings the other one round.
  - Curvature combs on picked curves (View → Curvature combs), scaled to a fixed screen
    length.
- **Output:** SVG `C` and PDF `c` exactly; DXF a degree-3 SPLINE per segment; blocks,
  Shift+D, mirror and patterns copy curves; the inspector shows ends and length.
- **Not yet:**
  - DXF SPLINE import.
  - Trim, extend, split, fillet and offset with curves.
  - G2 (equal curvature) relations.
  - Continuing a path from an existing end with its handle.
  - Smooth / corner buttons in the inspector.

*Part 4 as built (2026-10-09): text.*
- **Model (format v15 `texts`):**
  - A sketch text is one line at a real size: words, baseline start, cap height, angle,
    font (built-in Sans = Ubuntu Light, Mono = Hack, so files look the same everywhere),
    layer and colour.
  - Notes stay the drafting text sized on paper.
- **Outlines:** `drift_draft::outline` reads glyphs with skrifa (already in the tree via
  egui) into closed contours of lines and cubic Béziers (TrueType quadratics raised
  exactly), cached per text change.
- **Text tool (toolbar, DRAW):**
  - Click where the text starts. It's picked, and the inspector's field is focused, so you
    type straight away and see the outlines update live. Typing is one undo step.
  - Text left empty is removed once you move on.
- **Editing:**
  - Hover highlights, a click picks (anywhere in its box), and drag moves it (one undo
    step). Delete removes it; Esc clears it.
  - Inspector: words, height, angle (0 / 90 / 180 / 270), font, Convert to curves (curves
    and lines with each contour's ends shared, grouped as "Text: …"), Delete.
- **Output:** outlines on sheets (PDF, SVG, preview) and in DXF, as curves.
- **Not yet:**
  - System fonts.
  - Multi-line text and alignment.
  - Snapping to text outlines (convert first).
  - Text in blocks and Shift+D.
  - A keyboard shortcut for the Text tool.

*Hotkeys added (2026-10-09):* Alt+D / Alt+R show or hide dimensions / relations; Shift+F
zooms to fit; F is Fillet (again for Chamfer).

*Performance torture test (2026-10-09):*
- **What it runs** (`cargo test --release -p drift-app perf_ -- --ignored --nocapture`): 10,000
  curves (28,822 entities, 6,472 relations) of lines, arcs, circles, curves and ellipses,
  through every per-frame and per-edit path, and the real app at 1920 × 1080 on the GPU.
- **Fixed:**
  - Region finding: 130 s → 0.24 s. Vertex merging was a linear search over ~110k edge
    ends; now a hash grid. Also: hole hosts prefiltered by bounds, a queue for pruning,
    sort-and-sweep for crossing pairs, and region bounds for quick `contains`.
  - Dragging 1,000 curves: 32 → 4.5 ms a move. The solver builds only the connected part
    (`drift_solver::connected`, `SolveOptions::scope`), and the index refreshes only those
    shapes during a drag, rebuilding its tree on the next commit.
  - Rotating 1,000 curves: 85 → 7.8 ms a move (the same scoping).
  - GPU frame at "fit all": 310k → 30k instances, 9 → 0.9 ms. Curves and ellipses are
    flattened by their size on screen (about one segment per 4 px), not to a fixed minimum.
  - The relation list shows at most 100 rows (a selection of 1,000 curves laid out
    thousands of rows every frame).
  - The index compares shapes in place and works out point roles only when the structure
    changes. rstar 0.13 panicked on removes mixed with inserts, so the tree is bulk-loaded.
- **Now:**
  - Full app frames: 5–6 ms whatever the zoom, and 12 ms with 1,000 curves picked.
  - Snapping under 0.02 ms, picking 0.3 ms.
- **Still slow (once per edit, not per frame):**
  - A commit re-solves, re-analyses (23 ms) and re-indexes (20 ms) the whole sketch: about
    60 ms. Undo and redo cost about 38 ms.
  - Regions take 0.24 s per change, when fills or region picking need them.
  - Next: scope analysis and indexing to what an edit touched.

**M10 — Polish and release (ongoing)**
- **Part 1 (done): panels and the command palette.**
  - Dockable panels: Inspector, Parameters, Layers, Sheets and Blocks dock as tabs on either
    side of the canvas, float over it, or hide.
  - Drag a tab onto the canvas to float it, or onto a side to dock it (drop zones light up);
    drag a floating panel's header onto a side to dock it.
  - Right-click a tab, or use its ⋯ menu, to dock, float or close it. Collapse a dock to an
    icon rail; narrow windows collapse both docks.
  - View → Panels shows each panel, hides all (Ctrl+\) or resets the layout. The layout is
    saved with the view preferences.
  - One Layers panel replaces the old Layers HUD and Layers tab. Compact rows (Ctrl+L) open to
    show colours, line settings and output; double-click a name to rename it.
  - The command palette (Ctrl+K or Ctrl+Shift+P) searches every tool, view setting, panel and
    command, with recent commands first.
  - Polish: inspector titles with icons, an empty-state summary, fold-out sections, section
    headers with hairlines, floating scroll bars and a shorter tooltip delay.
  - Not yet: keybinding config, drag-to-reorder for floating panels, and tear-off for more
    than one panel per window.
- **Part 2 (done): context bar, canvas menu, shortcuts sheet.**
  - A context bar under the menu shows the active tool and its options: polygon sides and
    inscribed / circumscribed; slot kind; circle / ellipse / partial ellipse; arc kind;
    fillet / chamfer; text height; curvature combs for the pen; annotation visibility.
  - With the select tool the bar holds selection actions (duplicate, move, rotate, mirror,
    group, construction, make block, delete). At the right, a search field opens the
    command palette.
  - Right-click on the canvas (select tool) picks what's under the pointer and opens a menu:
    edit actions, Add relation / Add dimension submenus, Move to layer, select chain,
    explode and delete. With nothing picked: select all, zoom to fit, grid, snap and the
    palette.
  - Help → Keyboard shortcuts (or ?) is grouped into sections, shows keycaps, and filters as
    you type.
- **Part 3 (done): memory safety.** Prompted by a system-wide out-of-memory freeze while
  Drift was open (the logs did not show which program was responsible).
  - A memory guard (`drift_platform::memory`): a background thread ends the app at 8 GB
    (`DRIFT_MEMORY_LIMIT_MB` overrides) and leaves a note, so the next start says what
    happened and restores the autosave. The UI warns at 3 GB. Help shows current use.
  - Bounds on everything found that could grow without limit: the rotate tool's angle arc
    (points grew with the typed angle), arc and ellipse flattening counts, sketch text length
    (500 typed, 2,000 outlined), undo history (500 steps), the text outline cache, and GPU
    instance buffers (now shrink after a peak).
  - Exports no longer become the document's file: exporting a PDF, SVG or DXF used to rename
    the sketch to the export, so Ctrl+S would overwrite it. Save only ever writes `.drift`.
  - Ctrl+P opens the PDF viewer once per press, not once per key repeat.
  - Memory torture tests (`mem_tests.rs`, opt-in): zoom extremes, huge and tiny text, dense
    hatches, 10k–200k entities, 300k-segment underlays, geometry out to 1e300, imperial
    units, every panel, the sheet preview and random input. Worst peak: +0.8 GB at 115k
    entities; everything else under +0.2 GB, with nothing retained.
    `cargo test --release -p drift-app mem_ -- --ignored --nocapture --test-threads=1`
  - A soak mode for the real window: `DRIFT_SOAK=<seconds>` makes the app drive itself
    (drawing, zooming, resizing, minimising) and log memory to `<temp>/drift/soak.log`.
    170 s / 7,900 frames: flat at 435 MB.
  - Not yet: the idle baseline is about 400 MB plus 250–300 MB of GPU memory, most of it
    the graphics backend; worth a look.
- **Part 4 (done): alpha readiness.** Version is now 0.1.0-alpha.1.
  - Clipboard: Ctrl+C / X / V copy the selection as a small Drift document on the system
    clipboard (curves, relations and dimensions among them, fills, block copies with their
    blocks, sketch text), so paste works between windows. Paste lands under the pointer;
    Ctrl+Shift+V pastes in place. Layers and blocks are matched by name. Plain text pastes
    as sketch text. Duplicate shares the same copy routine (`clipboard.rs`).
  - Closing with unsaved changes asks: Save, Don't save, Cancel (also for New and Open).
    "Don't save" on quitting drops the session, so the next start is fresh.
  - Recent files (File → Open recent, ten kept) and a start panel over an empty sketch:
    New, Open, the example, recent files and three tips.
  - App icon (drawn by `apps/native/assets/make_icon.py`; `.ico` ready for the installer),
    Help → About with version and system details to copy into a bug report.
  - A sketch passed on the command line or dropped on the window opens (PDF and DXF files
    dropped are imported into the open sketch).
  - Inspector sizes are fields: a line's length, a circle's or arc's radius and diameter,
    a point's coordinates. Typing one sets the dimension that drives it or adds one (units
    and variables work).
  - Arrow keys nudge the selection one grid step, ten with Shift.
  - Edit → Preferences (Ctrl+,): appearance, units, grid and snapping, display, window and
    memory in one window.
  - Text: Shift+T picks the tool; text is typed in place on the canvas (double-click a text
    to type into it again).
  - The status bar's zoom is a menu: fit, selection (Shift+Z), and 25–400%.
  - Not yet: the icon embedded in the executable and the `.drift` file association (both
    installer work); align and distribute; configurable keys.
- **Part 5 (done): inspector editing; trim with ellipses and pen curves.**
  - Inspector fields apply when left by Return, Tab or a click elsewhere (they needed
    Return before, so a typed value was easily lost); Esc restores the old value.
  - A line's Angle, Start and End, a circle's, arc's and ellipse's Centre and a curve's ends
    are fields too. Sizes (length, radius, diameter) set or add a dimension; positions and
    angles move the geometry as far as its relations allow.
  - Esc in any text field no longer also clears the selection (egui drops the field's focus
    before the app's key handling ran).
  - Intersections with ellipses and Béziers (`intersect.rs`, numeric: sampled chords give
    starting guesses that Newton's method polishes; tangent touches found; overlapping
    curves give none). Lines extend to ellipses and curves.
  - Trim cuts lines, arcs and circles where ellipses and pen curves cross them, and cuts
    ellipses (into elliptical arcs sharing their axes), elliptical arcs and pen curves
    (into shorter curves on the same path, keeping smooth joins at the ends that remain).
  - Closed areas bounded partly by an ellipse or pen curve crossing other curves are now
    found, so they can be filled.
  - "Point on curve" works for pen curves (`Equation::OnBezier`); the document accepted it
    before but the solver had no case for it and would have stopped the app. Relations that
    need a line or circle no longer accept a pen curve.
  - The unsaved-changes prompt now also catches a close request while the window is
    covered or minimised (eframe runs only `App::logic` then, so the handling moved there),
    and brings the window forward.
  - Split works on every curve: a pen curve becomes two on the same path, joined smoothly;
    an elliptical arc becomes two arcs of the same ellipse; a whole ellipse or a circle
    (which used to refuse) becomes two halves, cut at the point and opposite it.
  - Offset works on ellipses and pen curves, alone or in a path with lines and arcs
    (chains now run through every kind of open curve). Their offsets aren't curves of the
    same kind, so they come out as smooth runs of pen curves fitted to the true offset
    (`drift_geom::offset`, within 0.01% of the shape's size); they're copies, not linked
    to the original as offset lines and arcs are. Too large an inward offset says so. A
    corner that opens between a curve and its neighbour is bridged with a straight line.
- **Part 6 (done): the modern sheet.** A new default title block, "Modern" (ISO 7200 and
  ASME Y14.1 remain as choices; format v16).
  - In the Swiss manner: an even 10 mm margin, a hairline frame, and a full-width strip
    under one strong rule. No boxes round the cells.
  - The title is the largest type on the sheet, under the company in small tracked
    capitals. Drawn, checked, date and material sit as grey labels over plain values;
    the drawing number is bold; the sheet number is reversed out of a black square.
  - The revision table follows suit: labels, a rule, rows parted by hairlines.
  - It adapts to the paper (A4 upright to A0); long values shrink a little, then are cut.
  - The drawing model gained bold text and letter-spacing (`TextStyle`), carried through
    PDF (Helvetica Bold, character spacing), SVG and the preview, and filled rectangles.
  - Preview arcs are drawn finely enough to look round at any zoom.
  - Not yet: other typefaces (PDF would need to embed them), a logo, and user-defined
    title blocks.
- **Part 7 (in progress): packaging.**
  - `drift.exe` carries its icon and version details (`apps/native/build.rs`, winresource).
  - `scripts/package-windows.ps1` builds `dist/`: a portable zip, and with Inno Setup 6 the
    installer from `installer/windows/drift.iss` (per-user by default, Start menu entry,
    optional desktop icon, `.drift` association, clean upgrade and uninstall).
  - `scripts/package-macos.sh` builds a universal `Drift.app` in a `.dmg` (Apple silicon
    and Intel, icon from `icon-mac.png`, ad-hoc signed unless `CODESIGN_IDENTITY` is set).
  - `.github/workflows/package.yml` runs both on GitHub runners (by hand or on a `v*` tag).
  - Not yet: the installer compiled and tried (no Inno Setup on the development machine);
    the macOS script run at all (it needs a Mac); code signing and notarisation; opening a
    double-clicked sketch on macOS (so no file association there).
- **Alpha feedback, phase 1 (done): fixes and quick wins.**
  - "Automatic relations" off now means none are added, anywhere: trim, extend, split,
    fillet, offset and mirror go through `Editor::relate`, and polygons, slots and tangent
    and three-point arcs come out as plain geometry. Trimmed ends that meet still share a
    point, so an outline closed by trimming stays closed. An X / Y lock still draws the
    segment along its axis, without the horizontal / vertical relation. (Relations the user
    asks for, the pen's smooth points and typed dimensions are not "automatic".)
  - Panels: the blue drop zones show only while a panel is dragged by its tab or header;
    closing a floating panel puts it back in the dock it came from.
  - Paste goes straight into a move. A fill is deleted with its boundary (one undo step).
  - P is the pen and V the point. Trim's drag leaves a trail and cuts everything it crosses.
  - The top bar is a control bar: the selection's layer, stroke and fill (each for the
    selection or its layer), Construction and an Actions menu; Auto relations and End
    points toggles at the right.
  - Angle dimensions sit between the two lines as drawn, on the side the label is on, with
    extension lines where needed; new ones measure the corner rather than its supplement.
  - Regions: curves drawn along each other (the touching walls of two rectangles, a line
    drawn twice) no longer merge or leak areas. A status-bar warning counts overlapping
    curves and selects them when clicked.
- **Alpha feedback, phase 2 (done): interaction polish.**
  - Scrolling: a mouse wheel zooms and two fingers on a trackpad pan (a pinch, or Ctrl +
    scroll, zooms). The two are told apart per gesture: points on macOS, and small,
    fractional or sideways amounts elsewhere, are a trackpad. Preferences has Automatic /
    Zoom / Pan for when the guess is wrong (a free-spinning wheel, a touch mouse). The sheet
    preview follows the same setting.
  - Fillet and chamfer are sized on the canvas, as Blender's bevel is: click (or press on) a
    corner, move to size it in round steps a few pixels apart (Alt: freely), click to apply.
    The arc or cut passes under the pointer. A second click without moving reuses the last
    size; a typed value applies to the corner being sized; Esc or right-click cancels.
    A fillet that is already there (an arc between two lines, tangent to both) is re-sized
    in place: clicked with the fillet tool, or dragged by its arc with the select tool. Its
    ends slide along their lines and its centre along the bisector (`Editor::drag_fillet`),
    so the lines keep their directions and nothing else moves; a radius dimension on it
    takes the new value. The same holds wherever the radius is set: typed in the inspector
    or on its dimension, the fillet is put where that radius has it before the solve
    (`Editor::settle_fillet`). Over a fillet the select tool shows a resize cursor.
  - Relations pie (Q, or Relations… in the canvas menu): the relations that fit the
    selection in a ring at the pointer, each at a fixed place (Horizontal right, Vertical up,
    Parallel / Tangent upper right, Perpendicular / Concentric upper left, Equal left, Fix
    lower left, Coincident / Collinear / On curve / Coradial down, Midpoint / Symmetric
    lower right). Hold Q, flick and let go; or tap Q and click. One already on the selection
    is lit and choosing it takes it off; ones that don't fit are dimmed; one that would
    over-define the sketch says so before it is chosen. Relations only: D is for dimensions.
- **Alpha feedback, phase 3 (done): sheets, second version.**
  - A sheet holds several views (`Sheet::views`, each a `SheetView`): a window onto the
    model with its own scale, centre, title, and whether it shows dimensions and notes. A
    view either fills the sheet's drawing area (what a new sheet's one view does, and it
    follows the paper size) or has a frame: a place and size on the paper. A framed view is
    opaque, like a piece of paper laid on the sheet. Format v18: `views` on a sheet; a sheet
    from before (its one view written as the sheet's own scale and centre) reads as it was.
  - The sheet editor (the canvas in sheet mode): click a view to pick it; drag it to move it
    on the paper; drag its handles to crop it (the drawing stays put on the paper); Alt-drag
    slides the drawing inside it; Delete removes it (a sheet keeps one). The Sheets panel
    lists the views and has the picked one's title, scale, Fit, Fills sheet / Framed, and
    what it shows. "Add view" makes a framed view of the selection in the model, or of the
    whole drawing, placed beside the views already there.
  - A view's title is its name in bold over a rule with its scale beneath, numbered in a
    ring when a sheet has several. The title block's scale reads "As shown" when the views'
    scales differ.
  - Output: every item carries the window it is seen through (`Item::clip`), so PDF, SVG and
    the preview clip each view to its own frame.
  - Section and elevation marks (Annotate): a section is a cutting line with a lettered flag
    at each end, pointing the way it looks; an elevation is a numbered flag. Each can name
    the sheet it is drawn on (in the lower half of the ring). They take the next free letter
    or number, are dragged whole, print with the notes, and are edited in the inspector
    (label, sheet, flip / direction).
  - A logo for the title blocks, from an SVG file (Sheets panel, Title block, Logo). It is
    kept in the drawing's file. `drift_draft::logo` reads paths and the basic shapes, flat
    fills and strokes, groups and transforms; not gradients (drawn black), text (convert it
    to outlines), images or style sheets. In the modern strip it leads, beside the title; in
    the ISO and ASME blocks it sits in the company's cell.
  - System fonts for sketch text (`TextFont::System(family)`): the font menu lists the
    computer's families, with a field to find one. Text is drawn and exported as outlines,
    so nothing has to be embedded in a PDF. Where a font isn't installed the text is shown
    in the built-in sans, says so in the inspector, and keeps the font's name. Desktop
    only: a browser gives no access to installed fonts.
  - Corners: the chamfer tool re-sizes a chamfer that is already there (a line between two
    lines that would meet beyond it), as the fillet tool does a fillet. "Remove fillet" /
    "Remove chamfer" on the canvas menu puts the sharp corner back.
  - Not done: titles and number rings can't be dragged off their views; views don't snap to
    one another when moved; a section mark's ends are moved together (re-draw it to change
    its length); marks don't look up a view's number or sheet by themselves.
- **Open after phase 3 (as of 2026-10-13).** Not features: things built but not yet proven.
  - The repository is on GitHub (`chrisplange/Drift`); phases 1 to 3 arrived as its first
    pull request, from the branch `drift-alpha`.
  - Packaging has never been through a release. The Windows installer script needs Inno
    Setup 6 and has not been compiled; the macOS script and the `Package` workflow have not
    run. Builds are unsigned.
  - Checked only through the headless test harness, not by hand in the window: trackpad
    panning on real hardware (and on a Mac), the fillet resize cursor, the relations pie's
    flick distances, the top bar's colour pop-ups, the logo file picker, the font menu, the
    Windows clipboard path, and the unsaved-changes prompt on closing.
  - The tool list is taller than the window at about 800 px of height (it scrolls). With
    two more annotation tools it may want grouping or fly-outs.
  - Files are format v18. Each bump (v17, v18) locks out older builds, which matters once
    testers hold different versions.
- **Workspace notes, round 1 (done).**
  - Two lines that run alongside each other (within 1 degree of parallel) dimension by the
    distance between them; before, any two lines gave an angle, which for parallel lines
    was a dimension of 0 degrees.
  - With automatic relations off, a typed size only sizes: a length typed in the inspector
    resizes the geometry and keeps no dimension (`Editor::resize_once`), and values typed
    while drawing (length, radius, width and height) leave none either. A dimension that is
    already there is still edited by the inspector's field.
  - The origin is a snap target (`SnapKind::Origin`), shown as a ring. A sketch point lying
    on it comes first, so lines still join to that point.
  - Angle snapping for the line tool (status bar, beside grid snap: off, 5, 15 or 45 degrees): the
    segment turns in steps and is as long as the pointer is far along that direction. A
    step on an axis gets its horizontal or vertical relation as usual; an exact snap to a
    point still wins; Alt places freely. Kept with the view preferences.
- **DXF import, second version (done).** The reader (`drift-io/src/dxf_import.rs`) was
  rewritten so a drawing from a live project comes in whole.
  - Model space and paper space are kept apart. Before, entities drawn on a layout were
    mixed into the model. Each layout becomes a sheet (named from its LAYOUT object, with
    its paper size) and each viewport a framed view at its scale and centre.
  - Splines: B-splines of degree 1 to 3 convert to lines and cubic Béziers exactly (by knot
    insertion); weighted and higher-degree spans are fitted in quarters (a quarter circle is
    within a few millionths of its radius); splines stored as fit points get a curve
    through them.
  - TEXT, MTEXT, and block attributes become sketch text: alignment and attachment points
    are worked out from the text's measured width, `%%c` and `\U+XXXX` codes are decoded
    and MTEXT formatting is stripped. Files that aren't UTF-8 are read as Windows Latin.
  - Hatches become live fills: boundaries from polyline, line, arc, ellipse and spline
    edges; a boundary that coincides with a curve already in the drawing shares it rather
    than doubling it; the fill is seeded from the file's seed point or a point found inside.
  - Blocks: nested inserts are expanded into their parent; an insert scaled unevenly, an
    array (MINSERT) or one drawn mirrored is expanded into curves (circles become
    ellipses); constant attributes and attribute values come in as text.
  - Dimensions come in as the geometry in their anonymous block: they look the same but no
    longer measure.
  - Layers: off / frozen, locked, not plotted, line weight, true colour, and the whole
    colour index. Custom line types take the nearest of Drift's five by their dash pattern.
  - Entities drawn "from behind" (extrusion 0, 0, -1) are un-mirrored, a common cause of
    arcs and blocks landing on the wrong side.
  - Binary DXF and DWG files are named as such in the error. Everything left out is counted
    by kind, and everything read approximately is explained, in an import report that stays
    up until closed.
  - Not done: associative dimensions (re-creating Drift dimensions from DXF ones); what is
    drawn on a layout itself (title blocks, notes); text inside blocks moving with the
    block; wrapping MTEXT to its width; text styles and fonts; MLEADER, TOLERANCE, images,
    3D solids, tables; solid fills of SOLID / TRACE (read as outlines); binary DXF. Export
    is still R12: text, layers, blocks and curves go out, but hatches as solid triangles
    and no dimensions, layouts or splines as such.
  - To try a real file without the app: `DRIFT_DXF=path cargo test -p drift-io probe --
    --ignored --nocapture` prints what came in, what was skipped and the notes.
- **Fonts, second version (done): fonts kept in drawings, and one font for a drawing's
  annotations.**
  - A drawing keeps the fonts it uses (`Document::fonts`, a `FontSet`): each family's
    regular face and its bold one where it has one. Picking an installed family for text
    keeps it automatically; "Add a font file…" in any font menu keeps a `.ttf` / `.otf`
    from disk, which is also how the browser build gets fonts. Format v19: `fonts`, with the
    files as base64; only families something is set in are written.
  - A font isn't kept where its licence flags forbid embedding, or where its file is over
    12 MB (full CJK fonts). It is then used from this computer only, and the app says so.
  - The drawing's font (Parameters, Drafting, Font) is what dimensions, notes, callouts,
    marks, view titles and title blocks are set in. `None` is the standard look, unchanged:
    Helvetica in print.
  - One measure everywhere: `drift_draft::font` holds the drawing's font for the thread;
    `styled_width` and the cap-height ratio come from it, so sheet layout, the screen and
    each output agree. On screen the font is given to egui under its own family, with the
    interface's fonts behind it for characters it lacks.
  - PDF: the font is embedded whole (compressed) as a composite font, text is written by
    glyph, and a glyph-to-Unicode map keeps it selectable and searchable in any script the
    font covers. Without a bold face, bold is the regular face filled and stroked. A font
    that can't be embedded (licence, or a face from a font collection) has its text drawn
    as outlines. SVG names the family, with the standard ones behind it.
  - Checked by writing a PDF, parsing it with another library (the font file comes back
    byte for byte and the text extracts as typed), and rendering it with Inkscape.
  - Not done: shaping (kerning, ligatures, right-to-left, complex scripts); a fallback
    font inside PDFs for characters the drawing's font lacks (they show as its "missing"
    box in print, though the screen falls back); multi-line sketch text and alignment;
    italic and other weights; SHX fonts and DXF text styles; fonts inside SVG; subsetting
    (a kept font adds its whole size to the file and to each PDF); the font scan still
    blocks the interface once; the title block's sheet number can crowd in a wide font.
- **Multi-line sketch text (done).** A text can run to several lines.
  - `SketchText` gains `align` (left, centre, right), `spacing` (baseline to baseline in
    cap heights, 1.6 by default) and `wrap` (a width lines are broken at spaces to stay
    within). Its anchor is on the first line's baseline, at the start, middle or end of
    each line as the alignment says, so centred and right-aligned text grows about its
    anchor as it is typed. Written to the file only where they differ from a plain line.
  - Typing: Enter finishes as before; Shift+Enter starts a new line, on the canvas and in
    the inspector. The inspector has Align, Spacing and Wrap; changing the alignment moves
    the anchor rather than the text.
  - Layout is `drift_draft::outline::Paragraph`: the text's own breaks, then greedy
    wrapping by measured advance widths; each line is outlined as before.
  - DXF import: MTEXT is one text with its line breaks, its attachment as alignment, its
    line spacing and its width as the wrap; TEXT and attributes keep centre and right
    alignment instead of being converted to a left-hand position.
  - Not done: justified text; vertical anchoring (middle, bottom) as a property rather
    than worked out on import; wrapping uses the font Drift shows, so a DXF paragraph set
    in a font we don't have may break at different words; no hyphenation.
- **Workspace notes, next: tabs for separate drawings (design, not built).** A tab bar
  along the top, one drawing per tab, as Revit and Illustrator have.
  - What a tab owns: the editor (document, selection, undo history), its file (name, path,
    saved revision), the camera and fit target, the current sheet and sheet preview state,
    and the picked relation or annotation. What stays shared: preferences, the panel
    layout, the clipboard (so copy and paste work across drawings), fonts, the tool.
  - How: `DriftApp` keeps those per-drawing fields as it does now, for the tab in front,
    and a list of the others parked in a `Workspace` struct; switching tabs swaps them. This
    keeps the hundreds of `self.editor` uses as they are.
  - New and Open make a tab instead of replacing the drawing (an untouched, empty Untitled
    tab is reused). Closing a tab with unsaved changes asks, as closing the window does;
    closing the window asks once per unsaved tab.
  - Autosave and session restore keep every open drawing, not one: the stored session
    becomes a list, with which tab was in front.
  - Things to settle first: whether a block library or layers should be copyable between
    tabs beyond the clipboard; whether the memory guard needs a per-tab count; the browser
    build's storage limit with several drawings in one session.
- **Alpha feedback, phase 4: content and parametric blocks.** A library of common
  imperial items; blocks with their own parameters (doors and windows at custom sizes),
  which needs its own design.
- Themes, keybinding config, crash recovery, PWA offline install, and accessibility pass.
- Performance profiling against targets (WASM first), plus native packaging (MSI, dmg, AppImage /
  Flatpak).
- Docs site, example gallery, and a 1.0 release.

**Rough total to a solid v1:** about 9–11 months for one experienced developer. Open-source
contributors can parallelise M6–M9 once the M2–M4 core is stable.

---

## 9. Key risks and mitigations

| Risk | Mitigation |
|---|---|
| Solver flips to an alternate solution while dragging | Minimum-norm steps, step limiting, orientation hints stored on constraints, solving from the current state |
| Poor convergence on near-degenerate sketches | LM damping, well-conditioned residual formulations, golden regression corpus |
| Solver perf on large sketches, single-threaded WASM | Substitution, graph decomposition, incremental re-solve of affected clusters only, time-slicing across frames, browser benchmarks in CI |
| WebGPU availability and driver quirks across browsers | WebGL2-compatible renderer baseline; test matrix of Chrome, Firefox and Safari in CI (Playwright screenshot tests) |
| Browser file-system limitations (Firefox, Safari) | `drift-platform` abstraction with download/upload fallback, plus IndexedDB autosave so work is never lost |
| WASM bundle bloat | Size budget enforced in CI, `-Oz`, lazy font loading, and dependency audits |
| Trim/offset robustness with arcs and splines | Exact line/arc intersection math, tolerance policy defined once in drift-geom, heavy property testing |
| Drafting standards are large and detailed | Implement the commonly used ISO 129 / ASME Y14.5 subset first, with style presets data-driven (not hard-coded) so contributors can extend them |
| Scope creep (it's a CAD app) | Strict milestone gates; a public roadmap with "good first issue" labels to channel contributions |
| egui limits for polished UI | The canvas and on-canvas widgets are our own renderer, and egui is only used for panels. Swappable later |

---

## 10. Decisions

All major decisions are settled:
- **Name:** Drift.
- **Licence:** MIT OR Apache-2.0 (see §2.1).
- **Distribution:** open source, cross-platform, with a first-class browser build.
- **Primary use cases:** technical drawings and design exploration.
- **File format:** JSON-based `.drift` files (see §3.5).

Minor choices will be made during implementation, for example the exact fonts and the
title-block template syntax.

---

## 11. Immediate next steps

1. Create the `drift` GitHub repo and push, so CI runs and Pages deploys. Enable Pages with
   "GitHub Actions" as the source.
2. M9: advanced geometry (ellipses, slots, polygons, text, splines with curvature combs and
   G1/G2 constraints).
