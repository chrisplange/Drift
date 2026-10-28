//! Drift document model and geometry primitives.
//!
//! All model geometry is stored in `f64` world units (millimetres). World space is y-up,
//! following the usual CAD convention.

pub mod affine;
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
pub mod openings;
pub mod params;
pub mod paths;
pub mod polygon;
pub mod rules;
pub mod shape;
pub mod shared;
pub mod sheets;
pub mod texts;
pub mod tolerance;
pub mod underlays;
pub mod units;
pub mod walls;

pub use affine::Affine2;
pub use blocks::{Block, BlockId, BlockItem, Instance, InstanceId, Placement};
pub use bounds::{Bounds, FAR_MM};
pub use constraint::{Constraint, ConstraintId, Slot};
pub use document::{
    Changes, Document, DocumentError, Entity, EntityId, EntityKind, History, MeasurementId,
    Transaction, Variable, VariableId, Variant, VariantId, arc_shape, dimension_kind,
};
pub use drafting::{
    Anchor, Annotation, AnnotationId, AnnotationRef, ArrowStyle, DimNote, DraftStyle, Fill, FillId,
    FillKind, FillStyle, Hatch, Leader, LeaderEnd, LeaderShape, NoteFrame, NoteStyle, OrdinateAxis,
    Shown, Standard, StyleOverride, Tolerance,
};
pub use fonts::{EmbeddedFont, FontFile, FontSet};
pub use glam::DVec2;
pub use groups::{Group, GroupId};
pub use layers::{Appearance, Layer, LayerId, LineType, LineWeight, Rgb};
pub use openings::{
    Hand, Opening, OpeningAt, OpeningId, OpeningMade, SegEnd, SizeSetId, Symbol, WallFace,
};
pub use paths::{Path, PathId};
pub use rules::DrawingRules;
pub use shape::{DofState, Emphasis, LineKind, Shape, SketchItem};
pub use shared::Shared;
pub use sheets::{
    FieldSet, PaperSize, PlacedView, Project, Revision, Sheet, SheetColours, SheetId, TitleBlock,
    TitleFields, View, ViewFrame, ViewId, Viewport,
};
pub use texts::{SketchText, TextAlign, TextFont, TextId};
pub use underlays::{RefPrim, Underlay, UnderlayId, UnderlayImage};
pub use units::{Kind, LengthFormat, Quantity, Units};
pub use walls::{
    CornerKey, Dir, EndKey, FaceKey, Justify, LoopKey, Made, Owner, Owners, Part, PropertyDim,
    PropertyDimId, PropertyRef, Station, Turn, Wall, WallEnd, WallId,
};
