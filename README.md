# Drift

**Parametric 2D sketching that is precise, fast and runs everywhere.**

Drift is an open-source 2D CAD sketcher with SolidWorks-style geometric and dimensional
constraints, built for technical drawings and design exploration. It is written in Rust, renders
on the GPU with [wgpu](https://wgpu.rs), and runs natively on Windows, macOS and Linux and in the
browser (WebGPU, with a WebGL2 fallback).

> **Status: early development (milestone M0).** The canvas renders and navigates. Drawing tools,
> constraints and files come next. See [PLAN.md](PLAN.md) for the full roadmap.

## What works today

- GPU canvas with analytic (signed-distance) lines, arcs, circles and points: perfectly crisp at
  any zoom, with no tessellation.
- Exact rendering far from the origin: geometry is stored in `f64` and made camera-relative
  before it reaches the GPU. Try *View → Go to precision test*, a 10 µm feature 1 km away.
- Adaptive metric grid, ISO-style dashed construction lines and dash-dot centre lines.
- Pan (drag), zoom to cursor (scroll / pinch), zoom to fit (`F` or double middle-click).
- Dark and light themes. Redraws only on input, so the app uses no CPU when idle.

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

Tests (the GPU render tests are skipped automatically when no graphics adapter is available):

```bash
cargo test --workspace
```

## Repository layout

| Path | Purpose |
|---|---|
| `crates/drift-core` | Document model and geometry primitives (`f64`, world space in mm, y-up) |
| `crates/drift-render` | wgpu canvas renderer: camera, frame builder, SDF shaders (WebGL2-compatible) |
| `crates/drift-app` | eframe/egui application shell shared by desktop and web |
| `apps/native` | Desktop binary |
| `apps/web` | Browser entry point and Trunk config |

The solver, geometry, I/O, drafting and platform crates described in the plan are added in
the milestones where they are needed.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
