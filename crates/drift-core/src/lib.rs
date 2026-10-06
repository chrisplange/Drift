//! Drift document model and geometry primitives.
//!
//! All model geometry is stored in `f64` world units (millimetres). World space is y-up,
//! following the usual CAD convention.

pub mod bounds;
pub mod demo;
pub mod shape;

pub use bounds::Bounds;
pub use glam::DVec2;
pub use shape::{LineKind, Shape, SketchItem};
