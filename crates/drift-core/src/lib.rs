//! Drift document model and geometry primitives.
//!
//! All model geometry is stored in `f64` world units (millimetres). World space is y-up,
//! following the usual CAD convention.

pub mod bounds;
pub mod demo;
pub mod document;
pub mod shape;

pub use bounds::Bounds;
pub use document::{
    Document, DocumentError, Entity, EntityId, EntityKind, History, Transaction, arc_shape,
};
pub use glam::DVec2;
pub use shape::{Emphasis, LineKind, Shape, SketchItem};
