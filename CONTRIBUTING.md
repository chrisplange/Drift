# Contributing to Drift

Thanks for your interest! Drift is early, so the most useful thing right now is to read
[PLAN.md](PLAN.md) and open an issue before starting anything large.

## Ground rules

- **Precision first.** Model geometry is `f64`. Only convert to `f32` after making coordinates
  camera-relative (see `drift-render/src/frame.rs`).
- **The web is a first-class target.** Everything except `apps/native` must build for
  `wasm32-unknown-unknown`. Don't use `std::fs`, `std::thread` or `std::time::Instant` in
  library crates. Shaders must stay WebGL2-compatible (the `shader` test checks this).
- **Tests come with features.** Geometry and solver code needs unit tests. Rendering changes
  should keep the offscreen render tests passing.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo clippy --target wasm32-unknown-unknown -p drift-web
cargo test --workspace
```

If you add or update a dependency, also regenerate `THIRD-PARTY-NOTICES.txt` with
`python scripts/notices.py` (it needs `cargo install cargo-about --locked --features cli`).
CI checks that it matches `Cargo.lock`.

## Licence

Drift is released under the [FSL-1.1-ALv2](LICENSE.md), as described in the
[README](README.md#licence). By contributing, you license your contribution to Christopher
Lange, Drift's licensor, under the Apache License, Version 2.0, so that it can be released as
part of Drift under the FSL now and under Apache-2.0 when each version converts.
