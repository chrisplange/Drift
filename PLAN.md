# Drift — Project Plan

> Drift's goals, technology choices and architecture. What Drift does today is
> [FEATURES.md](FEATURES.md), the backlog is GitHub issues, and the way work runs is
> [WORKFLOW.md](WORKFLOW.md). The old roadmap, risks and next steps (former §4–11) were cut on
> 2026-11-24 and are in git history; finished design notes are in [docs/archive](docs/archive/README.md).

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
- **Lightweight.** Single native binary under 50 MB, WASM bundle under about 6 MB
  compressed, cold start under 300 ms native and under 2 s on the web, idle CPU
  near 0% (redraw only on change).
- **GPU-accelerated.** 60+ fps pan and zoom with 50k+ entities. Lines, arcs and circles are
  rendered analytically, so they stay crisp at any zoom.
- **User-friendly.** Inference snapping, auto-relations while drawing, on-canvas dimension
  entry, discoverable tools, and forgiving undo.
- **Full 2D toolset.** Covers everything you'd expect in a CAD sketch environment (see [FEATURES.md](FEATURES.md)).

- **Cross-platform.** One codebase runs natively on Windows, macOS and Linux and in the
  browser (WASM + WebGPU). The web build is a demo and trial.
- **A paid product.** Closed source under an end-user licence (see §2.1).

**What Drift is for** (this drives prioritisation)
Rapid, precise 2D drawing of all kinds: details, profiles, plans, elevations and diagrams.
Technical illustration is the frame. Every feature gets one test: does it make some kind of
drawing faster without slowing the others?

**Non-goals (v1)**
- 3D, extrusions and assemblies.
- Real-time collaboration (the document model should not preclude it later).
- Raster or paint tools.
- Full GD&T authoring (basic feature-control frames are post-v1).
- BIM: no rooms, levels, schedules or wall types.
- Illustration-art features: no gradients or other Adobe Illustrator-style features.
- A growing content catalog: blocks, parametric blocks and importing your own stay; no more
  library items are added.

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

Drift is **closed source and proprietary** (copyright Christopher Lange), to become a paid app
(decided 2026-10-19). `LICENSE.md` is an end-user licence agreement, still a DRAFT awaiting
legal review; packaging refuses to release while it says DRAFT. Betas up to 1.5 shipped under
FSL-1.1-ALv2. Releases, public issues and the roadmap live in `chrisplange/drift-releases`.
- Every dependency must stay compatible with closed distribution (all permissive or MPL with
  its file-level duties; `cargo-deny` checks).
- We cannot copy or port GPL code (SolveSpace). We can still implement the same *algorithms*
  from papers and documentation, which are not copyrightable.

Repo hygiene: `LICENSE.md`, `CONTRIBUTING.md`, issue templates, and `cargo-deny` in CI for
licence and advisory checks.

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
  drift-edit/     Editing without a window: the Editor (session, undo, caches), edits, drawing tools
  drift-app/      eframe/egui shell: panels, input, inference, the AI link and agent tools
  drift-platform/ Trait-based platform services: file open/save, clipboard, storage, fonts, timers
apps/
  native/         Desktop binary (thin main.rs)
  mcp/            drift-mcp: MCP server over stdio so AI assistants can create and edit drawings
  web/            WASM entry point, index.html, trunk config, service worker (PWA)
```

All crates except `drift-platform`'s backends and `apps/*` must compile for
`wasm32-unknown-unknown` with no `std::fs`, `std::thread` or `std::time::Instant` use. CI
enforces this.

**Integrations.** The Blender profile add-on (`integrations/blender`) stays for now. It needs
drift-solver and drift-geom to stay free of app and UI dependencies.

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
  line weight, line type; "by layer" by default).
- **Constraints:** an enum, each variant referencing entity IDs and (for dimensions) a
  `Value = Literal(f64) | Expr(ExprId)` plus a `driving: bool`.
- **Global variables and equations:** named parameters (`width = 120 mm`,
  `hole_d = width / 10`) evaluated by a small expression engine with units. This is what makes
  sketches truly *parametric*.
- **Units:** internal base unit is mm. Display and input support mm, cm, m, in, ft, and deg/rad,
  with mixed input like `1in + 3mm`. Each document has a unit system (metric or imperial) and a
  display style (imperial has engineering and architectural formats).

### 3.2 Undo/redo
- Command pattern: every edit is a reversible `Command`. Drags coalesce into one command.
- The document is small (parameters plus topology), so commands may store before/after
  snapshots of touched parameters rather than inverse logic. This keeps the code simple and
  correct.

### 3.3 Precision strategy
- The model uses `f64` everywhere. The renderer uses **camera-relative `f32`**: positions are
  computed relative to the view origin on the CPU in `f64` before upload, so there is no jitter
  at large coordinates or deep zoom.
- Every solve uses one tolerance: 1e-9 × a quarter of the part's size, in the part's own frame
  (solver audit decision 2, [docs/solver-audit.md](docs/solver-audit.md)).
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
- **Browser support:** WebGPU only (Chromium, Safari 26+, Firefox where enabled). Where WebGPU
  is missing, the page says to use a current browser or download Drift.

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
