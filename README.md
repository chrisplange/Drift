# Drift

**Parametric 2D sketching that is precise, fast and runs everywhere.**

**[Website](https://drift-website-c9y.pages.dev)** ·
**[Download](https://drift-website-c9y.pages.dev/download/)** ·
**[Try it in your browser](https://drift-website-c9y.pages.dev/app/)** ·
**[User guide](https://drift-website-c9y.pages.dev/guide/)** ·
**[Release notes](https://drift-website-c9y.pages.dev/releases/)**

Drift is a 2D CAD sketcher built around geometric relations and driving dimensions, for
technical drawings, floor plans and design exploration. It is written in Rust, renders on the
GPU with [wgpu](https://wgpu.rs), and runs natively on Windows, macOS and Linux and in the
browser (WebGPU, with a WebGL2 fallback).

> **Status: beta (0.1.0-beta.1.7).** Expect rough edges, and save often. Files saved by a
> newer build don't open in an older one.

New to Drift? Start with the [user guide](docs/guide/README.md). The complete, code-referenced
list of what Drift does is [FEATURES.md](FEATURES.md).

## Highlights

### Sketch and constrain
- **Drawing tools:** line and polyline, rectangle, regular polygon, circle, arcs (centre,
  three-point and tangent), ellipse, slot, point, and a pen tool for smooth curves.
- **Type while you draw:** lengths, angles, sizes and positions, in any unit or as an
  expression (`2in`, `1'-6 1/2"`, `width/2`). Typed sizes become dimensions.
- **Relations:** coincident, horizontal, vertical, parallel, perpendicular, collinear, equal,
  tangent, smooth, concentric, coradial, midpoint, point on curve, symmetric, equal distance
  and fix. Many are added automatically while drawing; the relations pie (Q) adds the rest.
- **Dimensions (D):** one smart tool for length, distance, horizontal/vertical, point-to-line,
  radius, diameter, angle and concentric gap. Driven (reference) dimensions follow the geometry.
- **Solver:** a sparse incremental solver keeps every relation satisfied while you drag.
  Geometry is coloured free, fully defined or conflicting; conflicts are explained and can be
  picked from the status bar.
- **Parameters:** variables and unit-aware expressions with live sliders, dimensions linked to
  expressions, and saved options (values only or the whole sketch).
- **Units:** metric or imperial, including architectural feet-inches with fractions.

### Modify
- Move, rotate, scale, copy and mirror, with axis locks, typed offsets and base points.
- Trim, extend and split; fillet and chamfer that stay editable by dragging.
- Linear and circular arrays, previewed before they apply.
- Live offsets that follow their source curve.
- Sketch checks for gaps, short ends and overlapping curves.
- Undo and redo for every edit.

### Architecture
- **Walls** drawn by centreline, with thickness presets, clean joins and poché fills; walls
  from existing curves, and explode back to lines.
- **Doors, windows and openings** placed in walls and edited in the Inspector.
- **Blocks** for furniture and symbols, with a built-in library; edit one copy and every copy
  updates.
- **Underlays:** trace over a PDF or DXF drawing at a calibrated scale.

### Drafting and sheets
- Fills and hatches with built-in and imported `.pat` patterns that follow the geometry.
- Aligned, linear, angular, radial, chain, baseline and ordinate dimensions in ISO or ASME
  styles, with tolerances and dual units.
- Text and notes with leaders, edited in place; centre marks; annotation scale.
- Layers with colours, line weights and line types; groups and draw order.
- Sheets with title blocks, multiple scaled and cropped views, and paper-space notes and
  dimensions tied to the model.

### Files
- Native `.drift` format: versioned JSON, one entity per line, exact round-trips.
- Import DXF, DWG, vector PDF, SVG and PDF-compatible `.ai` files.
- Export DXF, DWG, PDF and SVG; print sheets.
- Crash recovery of unsaved work.

### Interface
- Light, dark, Midnight and Obsidian themes, or follow the system.
- Dockable Inspector, Layers, Blocks, Sheets and Parameters panels.
- A command palette (Ctrl/⌘+K) with CAD aliases, and a pie menu on right-click.
- A precise GPU canvas: crisp at any zoom, exact far from the origin, idle when nothing moves.

### Automation and AI
- `drift-mcp`, a standalone MCP server, lets AI assistants draw, constrain, dimension, render
  and analyse Drift drawings; see [Drawing with AI assistants](docs/guide/ai/assistants.md).
- Drift script builds rooms, walls, openings, grids, arrays, relations and dimensions in one
  undo step.
- The AI link (Labs) lets an assistant work in the open Drift window.
- A Blender add-on imports Drift profiles.

See **Help → Keyboard shortcuts** in the app for every key.

## Building

You need the Rust toolchain from [rustup.rs](https://rustup.rs).

Desktop:

```bash
cargo run -p drift --release
```

Browser (install the WASM target and [trunk](https://trunkrs.dev) first):

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
cd apps/web && trunk serve
```

Then open <http://127.0.0.1:8080>.

Tests, including headless UI tests that drive the real app (GPU tests skip themselves when no
graphics adapter is available):

```bash
cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full pre-PR check list.

Installers: `scripts/package-windows.ps1` builds a Windows installer and
`scripts/package-macos.sh` a universal macOS disk image. The `Package` workflow runs both for a
version tag and attaches them, with checksums, to a draft release.

## Repository layout

| Path | Purpose |
|---|---|
| `crates/drift-core` | Document model, constraints, units, expressions, transactions and undo |
| `crates/drift-geom` | Exact geometry: intersections, regions, offsets, walls, picking and snapping |
| `crates/drift-solver` | Constraint solver and degree-of-freedom analysis |
| `crates/drift-edit` | Editing tools and modify operations |
| `crates/drift-draft` | Drafting: hatches, text outlines, title blocks, the page display list |
| `crates/drift-io` | `.drift` format; DXF, DWG, PDF and SVG import and export; underlays |
| `crates/drift-platform` | File dialogs, saving, system fonts and the memory guard on desktop and web |
| `crates/drift-render` | wgpu canvas renderer |
| `crates/drift-app` | The egui application: panels, commands, agent tools; headless UI tests |
| `apps/native` | Desktop binary |
| `apps/web` | Browser entry point |
| `apps/mcp` | `drift-mcp` server |
| `integrations/blender` | Blender add-on |
| `installer`, `scripts` | Packaging and project scripts |

## Licence

Drift is proprietary software, licensed to its users under the [Drift End-User Licence
Agreement](LICENSE.md). This repository is private: its source code is not licensed to anyone
else. The open-source fonts and crates Drift is built with are listed, with their licences,
in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

Versions up to and including 0.1.0-beta.1.5 were released under the Functional Source
License, Version 1.1, ALv2 Future License (FSL-1.1-ALv2), and those versions stay under it.

Copyright 2026 Christopher Lange. All rights reserved.
