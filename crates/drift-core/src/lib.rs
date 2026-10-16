//! Drift document model and geometry primitives.
//!
//! All model geometry is stored in `f64` world units (millimetres). World space is y-up,
//! following the usual CAD convention.

pub mod blocks;
pub mod bounds;
pub mod constraint;
pub mod demo;
pub mod document;
pub mod drafting;
pub mod expr;
pub mod fonts;
pub mod groups;
pub mod layers;
pub mod params;
pub mod shape;
pub mod sheets;
pub mod texts;
pub mod underlays;
pub mod units;

pub use blocks::{Block, BlockId, BlockItem, Instance, InstanceId};
pub use bounds::Bounds;
pub use constraint::{Constraint, ConstraintId, Slot};
pub use document::{
    Document, DocumentError, Entity, EntityId, EntityKind, History, MeasurementId, Transaction,
    Variable, VariableId, Variant, VariantId, arc_shape, dimension_kind,
};
pub use drafting::{
    Anchor, Annotation, AnnotationId, AnnotationRef, ArrowStyle, DimNote, DraftStyle, Fill, FillId,
    FillKind, Hatch, Leader, LeaderEnd, LeaderShape, NoteFrame, NoteStyle, OrdinateAxis, Shown,
    Standard, Tolerance,
};
pub use fonts::{EmbeddedFont, FontFile, FontSet};
pub use glam::DVec2;
pub use groups::{Group, GroupId};
pub use layers::{Appearance, Layer, LayerId, LineType, LineWeight, Rgb};
pub use shape::{DofState, Emphasis, LineKind, Shape, SketchItem};
pub use sheets::{
    PaperSize, Revision, Sheet, SheetId, SheetView, TitleBlock, TitleFields, ViewFrame,
};
pub use texts::{SketchText, TextAlign, TextFont, TextId};
pub use underlays::{RefPrim, Underlay, UnderlayId};
pub use units::{Kind, LengthFormat, Quantity, Units};
