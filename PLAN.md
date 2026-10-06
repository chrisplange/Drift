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
- **Flags per entity:** `construction`, `locked`, layer, style.
- **Constraints:** an enum, each variant referencing entity IDs and (for dimensions) a
  `Value = Literal(f64) | Expr(ExprId)` plus a `driving: bool`.
- **Global variables and equations:** named parameters (`width = 120 mm`,
  `hole_d = width / 10`) evaluated by a small expression engine with units. This is what makes
  sketches truly *parametric*.
- **Units:** internal base unit is mm. Display and input support mm, cm, m, in, ft, and deg/rad,
  with mixed input like `1in + 3mm`.

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
  - `variables`, `entities`, `constraints`, `layers`, `styles`, `sheets`, and `variants`.
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
- Optional fills and hatch patterns for drawings.

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
- **Hatching:** ANSI31, ISO patterns and solid fill, with angle and scale per region. Hatches are
  associative, so they follow region changes.
- **Line types and weights:** ISO 128 line types (continuous, dashed, chain, phantom) and weights
  (0.25 / 0.35 / 0.5 / 0.7 mm), assignable per layer or entity, and output at true weight in PDF.
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
- Layers (visibility, lock, color), line styles (solid, dashed, center), and stroke weights.
- Global variables/equations table.
- Native file plus autosave and crash recovery.
- Import DXF (lines, arcs, circles, polylines with bulges, splines, and text). Import SVG paths
  (converting Béziers to splines/arcs).
- Export DXF, SVG, PDF (true vector, 1:1 scale), and PNG.

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

**M1 — Geometry without constraints (2–3 wks)**
- Document model, line/circle/arc/point/rectangle tools, selection, move, undo/redo.
- Save/load through `drift-platform` (native dialogs; web File System Access API or download), plus
  IndexedDB autosave.
- R-tree picking and basic snapping (endpoint, midpoint, center).

**M2 — Solver core (4–6 wks, highest risk)**
- Parameter/equation framework, Newton–LM, substitution of coincident/horizontal/vertical, and
  component decomposition.
- Constraints: coincident, H/V, parallel, perpendicular, equal, fix, distance, radius, and angle.
- Constraint-preserving drag, plus DOF coloring.
- Jacobian finite-difference tests and a property-test harness.
- Benchmarks run **in the browser** too: the WASM single-thread numbers are the ones that count.

**M3 — Sketching UX (3–4 wks)**
- Inference lines and auto-relations, typed input during drawing, and the Smart Dimension tool
  with an on-canvas editor.
- Relation glyphs, the property panel, and the status bar DOF count.

**M4 — Full relations and diagnostics (3–4 wks)**
- Tangent, concentric, midpoint, symmetric, collinear, point-on-curve, and coradial.
- Redundancy/conflict detection and the driven-dimension prompt.

**M5 — Parametrics and exploration (3 wks)**
- Global variables, the expression engine with units, and the equations panel.
- Variable sliders with live re-solve, variants/configurations, snapshots, free-DOF arrows, and
  pinned measurements.

**M6 — Modify tools (4–5 wks)**
- Robust intersections in drift-geom, then trim/extend/split, fillet/chamfer, offset, mirror, and
  linear/circular patterns.
- Region detection with area properties.

**M7 — Drafting (4–5 wks)**
- ISO/ASME dimension styles, tolerances, baseline/chain/ordinate dimensions, and hole and chamfer
  callouts.
- Notes, leaders, centre marks, centrelines, and hatching.
- ISO 128 line types and weights, plus layers.

**M8 — Sheets and output (3 wks)**
- Sheets with a scale, title-block templates, and a revision table.
- True-scale vector PDF, SVG, and DXF import/export (layers and line types preserved). Printing.

**M9 — Advanced geometry (3–4 wks)**
- Ellipses, slots, polygons, and text.
- Splines (control-point and fit) with curvature combs and G1/G2 constraints.

**M10 — Polish and release (ongoing)**
- Themes, command palette, keybinding config, crash recovery, PWA offline install, and
  accessibility pass.
- Performance profiling against targets (WASM first), plus native packaging (MSI, dmg, AppImage /
  Flatpak).
- Docs site, example gallery, and a 1.0 release.

**Rough total to a solid v1:** about 8–10 months for one experienced developer. Open-source
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
2. M1: document model with parameter-backed entities (replacing `drift_core::Shape` as the source
   of truth), drawing tools, selection, undo/redo, and `.drift` save/load through `drift-platform`.
3. Spike the solver early, in parallel with M1: a standalone `drift-solver` test that solves a
   constrained rectangle + tangent arc, benchmarked natively and in WASM. This de-risks the
   hardest part first.
