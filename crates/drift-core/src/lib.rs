//! Drift document model and geometry primitives.
//!
//! All model geometry is stored in `f64` world units (millimetres). World space is y-up,
//! following the usual CAD convention.

pub mod bounds;
pub mod constraint;
pub mod demo;
pub mod document;
pub mod expr;
pub mod params;
pub mod shape;
pub mod units;

pub use bounds::Bounds;
pub use constraint::{Constraint, ConstraintId, Slot};
pub use document::{
    Document, DocumentError, Entity, EntityId, EntityKind, History, MeasurementId, Transaction,
    Variable, VariableId, Variant, VariantId, arc_shape, dimension_kind,
};
pub use glam::DVec2;
pub use shape::{DofState, Emphasis, LineKind, Shape, SketchItem};
pub use units::{Kind, LengthFormat, Quantity, Units};
