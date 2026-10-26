//! The sketch document: entities, their connectivity, and transactional edits for undo/redo.
//!
//! Curves reference [`EntityKind::Point`] entities for their defining points, so connected
//! geometry shares points. Each point's `x`/`y` and each circle's radius are the unknowns the
//! constraint solver solves for; [`Constraint`]s relate them.

use crate::shared::Shared;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use glam::DVec2;

use crate::blocks::{Block, BlockId, BlockItem, Instance, InstanceId};
use crate::constraint::Slot;
use crate::drafting::{
    Anchor, Annotation, AnnotationId, DimNote, DraftStyle, Fill, FillId, Leader,
};
use crate::groups::{Group, GroupId};
use crate::layers::{Appearance, Layer, LayerId};
use crate::openings::{Opening, OpeningId};
use crate::paths::{Path, PathId};
use crate::sheets::{PlacedView, Sheet, SheetId, View, ViewId};
use crate::texts::{SketchText, TextId};
use crate::underlays::{Underlay, UnderlayId};
use crate::units::{Kind, Units};
use crate::walls::{Owner, Owners, PropertyDim, PropertyDimId, Wall, WallId};
use crate::{Constraint, ConstraintId, LineKind, Shape};

/// Stable identifier of an entity within a document. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u32);

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "e{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntityKind {
    Point(DVec2),
    Line {
        a: EntityId,
        b: EntityId,
    },
    Circle {
        center: EntityId,
        radius: f64,
    },
    /// Counter-clockwise arc from `start` to `end` around `center`. The radius is
    /// `|start - center|`; `end` is kept on that circle.
    Arc {
        center: EntityId,
        start: EntityId,
        end: EntityId,
    },
    /// An ellipse: its centre and the ends of two semi-axes. `minor` is kept on the
    /// perpendicular to `center → major` through the centre.
    Ellipse {
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    },
    /// A cubic Bézier curve from `a` to `b`, shaped by the handle points `c1` (leaving `a`)
    /// and `c2` (arriving at `b`). Paths are chains of these sharing end points.
    Bezier {
        a: EntityId,
        c1: EntityId,
        c2: EntityId,
        b: EntityId,
    },
    /// The counter-clockwise part of an ellipse from `start` to `end` (both kept on it).
    EllipseArc {
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        start: EntityId,
        end: EntityId,
    },
}

impl EntityKind {
    /// The point entities this entity is defined by.
    pub fn points(&self) -> impl Iterator<Item = EntityId> {
        let ids: [Option<EntityId>; 5] = match *self {
            EntityKind::Point(_) => [None; 5],
            EntityKind::Line { a, b } => [Some(a), Some(b), None, None, None],
            EntityKind::Circle { center, .. } => [Some(center), None, None, None, None],
            EntityKind::Arc { center, start, end } => {
                [Some(center), Some(start), Some(end), None, None]
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => [Some(center), Some(major), Some(minor), None, None],
            EntityKind::Bezier { a, c1, c2, b } => [Some(a), Some(c1), Some(c2), Some(b), None],
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => [
                Some(center),
                Some(major),
                Some(minor),
                Some(start),
                Some(end),
            ],
        };
        ids.into_iter().flatten()
    }

    pub fn is_point(&self) -> bool {
        matches!(self, EntityKind::Point(_))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            EntityKind::Point(_) => "Point",
            EntityKind::Line { .. } => "Line",
            EntityKind::Circle { .. } => "Circle",
            EntityKind::Arc { .. } => "Arc",
            EntityKind::Ellipse { .. } => "Ellipse",
            EntityKind::EllipseArc { .. } => "Elliptical arc",
            EntityKind::Bezier { .. } => "Curve",
        }
    }

    /// A curve's end points (where it connects to others): a line's or arc's ends, a
    /// Bézier's anchors, an elliptical arc's ends.
    pub fn ends(&self) -> Option<[EntityId; 2]> {
        match *self {
            EntityKind::Line { a, b } | EntityKind::Bezier { a, b, .. } => Some([a, b]),
            EntityKind::Arc { start, end, .. } | EntityKind::EllipseArc { start, end, .. } => {
                Some([start, end])
            }
            _ => None,
        }
    }

    /// An end point this curve and `other` both have.
    pub fn shared_end(&self, other: &EntityKind) -> Option<EntityId> {
        let theirs = other.ends()?;
        self.ends()?.into_iter().find(|p| theirs.contains(p))
    }

    /// Lines, circles and arcs; ellipses and elliptical arcs are curves too, but most modify
    /// tools don't handle them yet.
    pub fn is_ellipse(&self) -> bool {
        matches!(
            self,
            EntityKind::Ellipse { .. } | EntityKind::EllipseArc { .. }
        )
    }

    /// Use point `to` wherever the entity uses `from`.
    pub fn replace_point(&mut self, from: EntityId, to: EntityId) {
        let swap = |id: &mut EntityId| {
            if *id == from {
                *id = to;
            }
        };
        match self {
            EntityKind::Point(_) => {}
            EntityKind::Line { a, b } => {
                swap(a);
                swap(b);
            }
            EntityKind::Circle { center, .. } => swap(center),
            EntityKind::Arc { center, start, end } => {
                swap(center);
                swap(start);
                swap(end);
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => {
                swap(center);
                swap(major);
                swap(minor);
            }
            EntityKind::Bezier { a, c1, c2, b } => {
                swap(a);
                swap(c1);
                swap(c2);
                swap(b);
            }
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => {
                swap(center);
                swap(major);
                swap(minor);
                swap(start);
                swap(end);
            }
        }
    }
}

/// Stable identifier of a variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariableId(pub u32);

impl std::fmt::Display for VariableId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// A named value defined by an expression (`width = 120 mm`, `hole = width / 10`).
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub name: String,
    pub expr: String,
    pub kind: Kind,
}

/// Stable identifier of a variant (configuration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VariantId(pub u32);

impl std::fmt::Display for VariantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "k{}", self.0)
    }
}

/// A design option: a named state of the drawing to return to in one step. Either a set of
/// variable expressions ("values only", applied over the current sketch) or a copy of the
/// whole sketch (`sketch`; `values` is then empty).
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: String,
    pub values: BTreeMap<VariableId, String>,
    /// The whole sketch, without options of its own (a copy shares the document's parts).
    pub sketch: Option<Shared<Document>>,
}

impl Variant {
    /// An option holding variable values.
    pub fn values(name: impl Into<String>, values: BTreeMap<VariableId, String>) -> Self {
        Self {
            name: name.into(),
            values,
            sketch: None,
        }
    }

    /// An option holding a copy of the whole sketch.
    pub fn sketch(name: impl Into<String>, doc: &Document) -> Self {
        Self {
            name: name.into(),
            values: BTreeMap::new(),
            sketch: Some(Shared::new(doc.sketch_copy())),
        }
    }

    pub fn is_sketch(&self) -> bool {
        self.sketch.is_some()
    }
}

/// Stable identifier of a pinned measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeasurementId(pub u32);

impl std::fmt::Display for MeasurementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "m{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entity {
    pub kind: EntityKind,
    /// Geometry, construction (reference) or centre line.
    pub style: LineKind,
    pub layer: LayerId,
    /// Colour / weight / line type overrides (by layer when unset).
    pub appearance: Appearance,
    /// Where a curve stacks among its layer's fills and curves: higher draws on top (as a
    /// fill's `order`); `None`, at the top of the layer, as curves always were. The curves
    /// outlining a fill draw with it instead.
    pub order: Option<i32>,
}

impl Entity {
    pub fn new(kind: EntityKind) -> Self {
        Self {
            kind,
            style: LineKind::Solid,
            layer: LayerId::DEFAULT,
            appearance: Appearance::default(),
            order: None,
        }
    }
}

/// Errors from document operations that reference entities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    Missing(EntityId),
    MissingConstraint(ConstraintId),
    NotAPoint(EntityId),
    /// A constraint refers to an entity of the wrong kind.
    WrongKind {
        id: EntityId,
        expected: &'static str,
    },
    Degenerate(&'static str),
    InvalidValue,
    MissingVariable(VariableId),
    /// A variable name that is invalid, reserved or already used.
    BadName(String),
    /// A variable still used by expressions (their owners are listed).
    InUse(String),
    MissingVariant(VariantId),
    MissingLayer(LayerId),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentError::Missing(id) => write!(f, "entity {id} does not exist"),
            DocumentError::MissingConstraint(id) => write!(f, "constraint {id} does not exist"),
            DocumentError::NotAPoint(id) => write!(f, "entity {id} is not a point"),
            DocumentError::WrongKind { id, expected } => {
                write!(f, "entity {id} is not {expected}")
            }
            DocumentError::Degenerate(what) => write!(f, "degenerate {what}"),
            DocumentError::InvalidValue => write!(f, "invalid value"),
            DocumentError::MissingVariable(id) => write!(f, "variable {id} does not exist"),
            DocumentError::BadName(why) => f.write_str(why),
            DocumentError::InUse(by) => write!(f, "still used by {by}"),
            DocumentError::MissingVariant(id) => write!(f, "variant {id} does not exist"),
            DocumentError::MissingLayer(id) => write!(f, "layer {id} does not exist"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// Something the journal tracks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Key {
    Entity(EntityId),
    Constraint(ConstraintId),
    Placement(ConstraintId),
    ExtensionStarts(ConstraintId),
    /// Present (with `Record::Driven`) when the dimension is driven.
    Driven(ConstraintId),
    Variable(VariableId),
    /// The expression a dimension's value is linked to.
    Expression(ConstraintId),
    Variant(VariantId),
    Measurement(MeasurementId),
    Units,
    Layer(LayerId),
    CurrentLayer,
    Fill(FillId),
    Annotation(AnnotationId),
    DimNote(ConstraintId),
    Shown(crate::AnnotationRef),
    OnlyIn(crate::AnnotationRef),
    Style(crate::AnnotationRef),
    Draft,
    Patterns,
    Logo,
    Rules,
    Project,
    Fonts,
    Sheet(SheetId),
    View(ViewId),
    Group(GroupId),
    Path(PathId),
    Block(BlockId),
    Instance(InstanceId),
    Underlay(UnderlayId),
    Text(TextId),
    Wall(WallId),
    PropertyDim(PropertyDimId),
    Opening(OpeningId),
}

#[derive(Clone, Debug, PartialEq)]
enum Record {
    Entity(Entity),
    Constraint(Constraint),
    Placement(DVec2),
    ExtensionStarts([Option<f64>; 2]),
    Driven,
    Variable(Variable),
    Expression(String),
    Variant(Variant),
    Measurement(Constraint),
    Units(Units),
    Layer(Layer),
    CurrentLayer(LayerId),
    Fill(Fill),
    Annotation(Annotation),
    DimNote(DimNote),
    Shown(crate::Shown),
    OnlyIn(BTreeSet<ViewId>),
    Style(crate::StyleOverride),
    Draft(DraftStyle),
    Patterns(Shared<String>),
    Logo(Shared<String>),
    Rules(crate::DrawingRules),
    Project(crate::Project),
    Fonts(Shared<crate::fonts::FontSet>),
    Sheet(Sheet),
    View(View),
    Group(Group),
    Path(Path),
    Block(Block),
    Instance(Instance),
    Underlay(Underlay),
    Text(SketchText),
    Wall(Wall),
    PropertyDim(PropertyDim),
    Opening(Opening),
}

/// One undoable step: the before and after state of everything it touched.
/// A constraint left out when opening a document, and why.
pub type Dropped = (ConstraintId, &'static str);

#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub label: String,
    changes: Vec<(Key, Option<Record>, Option<Record>)>,
}

impl Transaction {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The entities it added or changed (not those it removed).
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.changes
            .iter()
            .filter_map(|(key, _, after)| match (key, after) {
                (Key::Entity(id), Some(_)) => Some(*id),
                _ => None,
            })
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    entities: Shared<BTreeMap<EntityId, Entity>>,
    constraints: Shared<BTreeMap<ConstraintId, Constraint>>,
    /// Where dimension labels sit: an offset in world units from the dimension's anchor
    /// (e.g. the midpoint of a measured line). Missing means "default placement".
    placements: Shared<BTreeMap<ConstraintId, DVec2>>,
    /// Where a linear dimension's extension lines start, set by hand: per end, a distance
    /// from the measured point along the extension line toward the dimension line. A missing
    /// entry, or `None` for an end, means "automatic".
    extension_starts: Shared<BTreeMap<ConstraintId, [Option<f64>; 2]>>,
    /// Driven (reference) dimensions: they show a measurement but don't constrain.
    driven: Shared<BTreeSet<ConstraintId>>,
    variables: Shared<BTreeMap<VariableId, Variable>>,
    /// Dimensions whose value comes from an expression (e.g. `width / 2`).
    expressions: Shared<BTreeMap<ConstraintId, String>>,
    variants: Shared<BTreeMap<VariantId, Variant>>,
    /// Pinned measurements: dimension definitions that are only measured, never solved.
    measurements: Shared<BTreeMap<MeasurementId, Constraint>>,
    units: Units,
    layers: Shared<BTreeMap<LayerId, Layer>>,
    /// The layer new geometry goes on.
    current_layer: LayerId,
    fills: Shared<BTreeMap<FillId, Fill>>,
    annotations: Shared<BTreeMap<AnnotationId, Annotation>>,
    /// Prefixes, suffixes and tolerances of dimensions.
    dim_notes: Shared<BTreeMap<ConstraintId, DimNote>>,
    /// Dimensions and annotations not shown everywhere (the rest are).
    shown: Shared<BTreeMap<crate::AnnotationRef, crate::Shown>>,
    /// Dimensions and annotations drawn in some views only (the rest are in every view).
    only_in: Shared<BTreeMap<crate::AnnotationRef, BTreeSet<ViewId>>>,
    /// Dimensions and annotations with settings of their own over the drawing's style.
    styles: Shared<BTreeMap<crate::AnnotationRef, crate::StyleOverride>>,
    draft: DraftStyle,
    /// Hatch patterns imported from `.pat` files, as `.pat` text, so they travel with the file.
    patterns: Shared<String>,
    /// The title blocks' logo, as SVG text ("" for none), so it travels with the file.
    logo: Shared<String>,
    /// How the drawing's closed areas and fills behave.
    rules: crate::DrawingRules,
    /// The project every sheet's title block shares.
    project: crate::Project,
    /// Fonts kept in the drawing, and the one its annotations are set in.
    fonts: Shared<crate::fonts::FontSet>,
    sheets: Shared<BTreeMap<SheetId, Sheet>>,
    /// Views of the model, placed on sheets by their viewports.
    views: Shared<BTreeMap<ViewId, View>>,
    groups: Shared<BTreeMap<GroupId, Group>>,
    paths: Shared<BTreeMap<PathId, Path>>,
    blocks: Shared<BTreeMap<BlockId, Block>>,
    instances: Shared<BTreeMap<InstanceId, Instance>>,
    underlays: Shared<BTreeMap<UnderlayId, Underlay>>,
    texts: Shared<BTreeMap<TextId, SketchText>>,
    /// Runs of wall, and the entities, paths and fills they made.
    walls: Shared<BTreeMap<WallId, Wall>>,
    /// Dimensions that show a wall's property (its thickness).
    property_dims: Shared<BTreeMap<PropertyDimId, PropertyDim>>,
    /// Doors, windows and openings in walls, and what their symbols made.
    openings: Shared<BTreeMap<OpeningId, Opening>>,
    next_id: u32,
    next_constraint_id: u32,
    /// Next id for variables, variants and measurements (one counter, ids never reused).
    next_param_id: u32,
    /// Next id for fills and annotations (one counter, ids never reused).
    next_drafting_id: u32,
    /// Next id for walls, property dimensions and openings (one counter, ids never reused).
    next_wall_id: u32,
    /// Incremented on every change, so caches (spatial index, autosave) can tell when to refresh.
    revision: u64,
    /// Pre-images of everything touched by the open transaction.
    journal: Option<HashMap<Key, Option<Record>>>,
    /// Counts transactions begun, so work spanning calls (a drag) can tell it is still in the
    /// same one (not part of the content).
    transaction_serial: u64,
    /// Leader targets as of the last [`remember_leader_targets`](Self::remember_leader_targets)
    /// (not part of the content).
    last_anchor_pos: HashMap<AnnotationId, DVec2>,
}

impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.entities == other.entities
            && self.constraints == other.constraints
            && self.placements == other.placements
            && self.extension_starts == other.extension_starts
            && self.driven == other.driven
            && self.variables == other.variables
            && self.expressions == other.expressions
            && self.variants == other.variants
            && self.measurements == other.measurements
            && self.units == other.units
            && self.layers == other.layers
            && self.current_layer == other.current_layer
            && self.fills == other.fills
            && self.annotations == other.annotations
            && self.dim_notes == other.dim_notes
            && self.shown == other.shown
            && self.only_in == other.only_in
            && self.styles == other.styles
            && self.draft == other.draft
            && self.patterns == other.patterns
            && self.logo == other.logo
            && self.fonts == other.fonts
            && self.sheets == other.sheets
            && self.views == other.views
            && self.groups == other.groups
            && self.paths == other.paths
            && self.blocks == other.blocks
            && self.instances == other.instances
            && self.underlays == other.underlays
            && self.texts == other.texts
            && self.walls == other.walls
            && self.property_dims == other.property_dims
            && self.openings == other.openings
    }
}

impl Default for Document {
    fn default() -> Self {
        Self {
            entities: BTreeMap::new().into(),
            constraints: BTreeMap::new().into(),
            placements: BTreeMap::new().into(),
            extension_starts: BTreeMap::new().into(),
            driven: BTreeSet::new().into(),
            variables: BTreeMap::new().into(),
            expressions: BTreeMap::new().into(),
            variants: BTreeMap::new().into(),
            measurements: BTreeMap::new().into(),
            units: Units::default(),
            layers: BTreeMap::from([(LayerId::DEFAULT, Layer::new("0"))]).into(),
            current_layer: LayerId::DEFAULT,
            fills: BTreeMap::new().into(),
            annotations: BTreeMap::new().into(),
            dim_notes: BTreeMap::new().into(),
            shown: BTreeMap::new().into(),
            only_in: BTreeMap::new().into(),
            styles: BTreeMap::new().into(),
            draft: DraftStyle::default(),
            patterns: String::new().into(),
            logo: String::new().into(),
            rules: crate::DrawingRules::default(),
            project: crate::Project::default(),
            fonts: Shared::default(),
            sheets: BTreeMap::new().into(),
            views: BTreeMap::new().into(),
            groups: BTreeMap::new().into(),
            paths: BTreeMap::new().into(),
            blocks: BTreeMap::new().into(),
            instances: BTreeMap::new().into(),
            underlays: BTreeMap::new().into(),
            texts: BTreeMap::new().into(),
            walls: BTreeMap::new().into(),
            property_dims: BTreeMap::new().into(),
            openings: BTreeMap::new().into(),
            // Ids start at 1; `e0` and `c0` are not valid ids in `.drift` files.
            next_id: 1,
            next_constraint_id: 1,
            next_param_id: 1,
            next_drafting_id: 1,
            next_wall_id: 1,
            revision: 0,
            journal: None,
            transaction_serial: 0,
            last_anchor_pos: HashMap::new(),
        }
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild a document from stored entities (e.g. when loading a file). Validates references.
    pub fn from_entities(
        entities: impl IntoIterator<Item = (EntityId, Entity)>,
    ) -> Result<Self, DocumentError> {
        Self::from_parts(entities, [])
    }

    /// Rebuild a document from stored entities and constraints. Validates references.
    /// Like [`from_parts`](Self::from_parts), but a constraint that has become meaningless
    /// (a point held on its own curve, say, after its points were joined) is left out rather
    /// than refusing the whole document. Returns what was left out and why.
    pub fn from_parts_dropping_degenerate(
        entities: impl IntoIterator<Item = (EntityId, Entity)>,
        constraints: impl IntoIterator<Item = (ConstraintId, Constraint)>,
    ) -> Result<(Self, Vec<Dropped>), DocumentError> {
        let mut doc = Self::from_parts(entities, [])?;
        let mut dropped = Vec::new();
        let mut kept = BTreeMap::new();
        for (id, c) in constraints {
            match doc.validate(&c) {
                Ok(()) => {
                    kept.insert(id, c);
                }
                Err(DocumentError::Degenerate(why)) => dropped.push((id, why)),
                Err(e) => return Err(e),
            }
        }
        doc.next_constraint_id = kept
            .keys()
            .chain(dropped.iter().map(|(id, _)| id))
            .map(|id| id.0 + 1)
            .max()
            .unwrap_or(1);
        doc.constraints = kept.into();
        Ok((doc, dropped))
    }

    pub fn from_parts(
        entities: impl IntoIterator<Item = (EntityId, Entity)>,
        constraints: impl IntoIterator<Item = (ConstraintId, Constraint)>,
    ) -> Result<Self, DocumentError> {
        let entities: BTreeMap<_, _> = entities.into_iter().collect();
        let constraints: BTreeMap<_, _> = constraints.into_iter().collect();
        let next_id = entities.keys().last().map_or(1, |id| id.0 + 1);
        let next_constraint_id = constraints.keys().last().map_or(1, |id| id.0 + 1);
        let doc = Self {
            entities: entities.into(),
            constraints: constraints.into(),
            next_id,
            next_constraint_id,
            ..Self::default()
        };
        for entity in doc.entities.values() {
            for point in entity.kind.points() {
                doc.point(point)?;
            }
        }
        for constraint in doc.constraints.values() {
            doc.validate(constraint)?;
        }
        Ok(doc)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn get(&self, id: EntityId) -> Option<&Entity> {
        self.entities.get(&id)
    }

    pub fn contains(&self, id: EntityId) -> bool {
        self.entities.contains_key(&id)
    }

    /// All entities in id order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &Entity)> {
        self.entities.iter().map(|(id, e)| (*id, e))
    }

    /// The unit direction `curve` leaves its end `point` in, continuing past it (for a line,
    /// arc, elliptical arc or Bézier); `None` if it doesn't end there.
    pub fn end_direction(&self, curve: EntityId, point: EntityId) -> Option<DVec2> {
        let at = |id: EntityId| self.point(id).ok();
        let p = at(point)?;
        let d = match self.get(curve)?.kind {
            EntityKind::Line { a, b } if a == point || b == point => {
                Some(p - at(if a == point { b } else { a })?)
            }
            EntityKind::Arc { center, start, end } if start == point || end == point => {
                // Counter-clockwise travel direction at the end; reversed at the start.
                let along = (p - at(center)?).perp();
                Some(if end == point { along } else { -along })
            }
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } if start == point || end == point => {
                let c = at(center)?;
                let (m, b) = (at(major)? - c, at(minor)?.distance(c));
                // The derivative of c + M·cos t + M⊥·(b/a)·sin t, counter-clockwise like an arc.
                let t = crate::shape::ellipse_angle(c, m, b, p);
                let along = m.perp() * (b / m.length()) * t.cos() - m * t.sin();
                Some(if end == point { along } else { -along })
            }
            EntityKind::Bezier { a, c1, c2, b } if a == point || b == point => {
                // Out along the end's handle, or the next control point where it has no length.
                let ctrl = [at(a)?, at(c1)?, at(c2)?, at(b)?];
                Some(p - ctrl[crate::shape::bezier_end_handle(&ctrl, a != point)])
            }
            _ => None,
        }?;
        d.try_normalize()
    }

    pub fn point(&self, id: EntityId) -> Result<DVec2, DocumentError> {
        match self.entities.get(&id) {
            Some(Entity {
                kind: EntityKind::Point(p),
                ..
            }) => Ok(*p),
            Some(_) => Err(DocumentError::NotAPoint(id)),
            None => Err(DocumentError::Missing(id)),
        }
    }

    /// The location a dimension slot refers to: a point's position, or a line's midpoint.
    pub fn ref_point(&self, id: EntityId) -> Option<DVec2> {
        match self.entities.get(&id)?.kind {
            EntityKind::Point(p) => Some(p),
            EntityKind::Line { a, b } => Some((self.point(a).ok()? + self.point(b).ok()?) * 0.5),
            _ => None,
        }
    }

    /// Entities that reference `point`.
    pub fn users_of(&self, point: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        self.entities
            .iter()
            .filter(move |(_, e)| e.kind.points().any(|p| p == point))
            .map(|(id, _)| *id)
    }

    /// Every point referenced by a curve. Points not in this set are standalone sketch points.
    pub fn referenced_points(&self) -> std::collections::HashSet<EntityId> {
        self.entities
            .values()
            .flat_map(|e| e.kind.points())
            .collect()
    }

    /// The geometric shape of an entity, resolved from its points.
    pub fn shape(&self, id: EntityId) -> Option<Shape> {
        let entity = self.entities.get(&id)?;
        let p = |id| self.point(id).ok();
        Some(match entity.kind {
            EntityKind::Point(at) => Shape::Point(at),
            EntityKind::Line { a, b } => Shape::Line { a: p(a)?, b: p(b)? },
            EntityKind::Circle { center, radius } => Shape::Circle {
                center: p(center)?,
                radius,
            },
            EntityKind::Arc { center, start, end } => {
                let (c, s, e) = (p(center)?, p(start)?, p(end)?);
                arc_shape(c, s, e)
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => {
                let c = p(center)?;
                Shape::Ellipse {
                    center: c,
                    major: p(major)? - c,
                    minor: p(minor)?.distance(c),
                    start: 0.0,
                    sweep: TAU,
                }
            }
            EntityKind::Bezier { a, c1, c2, b } => Shape::Bezier {
                p: [p(a)?, p(c1)?, p(c2)?, p(b)?],
            },
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => {
                let c = p(center)?;
                let (m, b) = (p(major)? - c, p(minor)?.distance(c));
                let t0 = crate::shape::ellipse_angle(c, m, b, p(start)?);
                let t1 = crate::shape::ellipse_angle(c, m, b, p(end)?);
                let mut sweep = (t1 - t0).rem_euclid(TAU);
                if sweep == 0.0 {
                    sweep = TAU;
                }
                Shape::Ellipse {
                    center: c,
                    major: m,
                    minor: b,
                    start: t0,
                    sweep,
                }
            }
        })
    }

    // ----- Transactions --------------------------------------------------------------------

    /// Start recording changes. Nested calls are ignored; the outer transaction wins.
    pub fn begin(&mut self) {
        if self.journal.is_none() {
            self.journal = Some(HashMap::new());
            self.transaction_serial += 1;
        }
    }

    /// Which transaction is open (or was last): a new number for each [`begin`](Self::begin)
    /// that opens one.
    pub fn transaction_serial(&self) -> u64 {
        self.transaction_serial
    }

    pub fn in_transaction(&self) -> bool {
        self.journal.is_some()
    }

    /// True if curve `id` was deleted, or given other points or another kind (split, trimmed,
    /// extended), in the open transaction.
    pub fn reshaped_in_transaction(&self, id: EntityId) -> bool {
        let Some(journal) = &self.journal else {
            return false;
        };
        match journal.get(&Key::Entity(id)) {
            Some(Some(Record::Entity(before))) => self
                .entities
                .get(&id)
                .is_none_or(|now| now.kind != before.kind),
            _ => false,
        }
    }

    /// True if the open transaction touched entity `id` (moved, reshaped, restyled, added or
    /// deleted it).
    pub fn touched_in_transaction(&self, id: EntityId) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|j| j.contains_key(&Key::Entity(id)))
    }

    /// The entities the open transaction touched (sorted).
    pub fn entities_touched_in_transaction(&self) -> Vec<EntityId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut ids: Vec<EntityId> = journal
            .keys()
            .filter_map(|k| match k {
                Key::Entity(id) => Some(*id),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids
    }

    /// The walls the open transaction added, changed or removed, and the paths it touched
    /// (sorted).
    pub fn walls_and_paths_touched_in_transaction(&self) -> (Vec<WallId>, Vec<PathId>) {
        let Some(journal) = &self.journal else {
            return (Vec::new(), Vec::new());
        };
        let (mut walls, mut paths) = (Vec::new(), Vec::new());
        for k in journal.keys() {
            match k {
                Key::Wall(id) => walls.push(*id),
                Key::Path(id) => paths.push(*id),
                _ => {}
            }
        }
        walls.sort_unstable();
        paths.sort_unstable();
        (walls, paths)
    }

    /// The openings the open transaction added, changed or removed (sorted).
    pub fn openings_touched_in_transaction(&self) -> Vec<OpeningId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut out: Vec<OpeningId> = journal
            .keys()
            .filter_map(|k| match k {
                Key::Opening(id) => Some(*id),
                _ => None,
            })
            .collect();
        out.sort_unstable();
        out
    }

    /// Opening `id` as it was when the open transaction began.
    pub fn opening_before_transaction(&self, id: OpeningId) -> Option<Opening> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Opening(id))) {
            Some(Some(Record::Opening(o))) => Some(o.clone()),
            Some(_) => None,
            None => self.openings.get(&id).cloned(),
        }
    }

    /// Wall `id` as it was when the open transaction began (see
    /// [`entity_before_transaction`](Self::entity_before_transaction)).
    pub fn wall_before_transaction(&self, id: WallId) -> Option<Wall> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Wall(id))) {
            Some(Some(Record::Wall(w))) => Some(w.clone()),
            Some(_) => None,
            None => self.walls.get(&id).cloned(),
        }
    }

    /// Path `id` as it was when the open transaction began.
    pub fn path_before_transaction(&self, id: PathId) -> Option<Path> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Path(id))) {
            Some(Some(Record::Path(p))) => Some(p.clone()),
            Some(_) => None,
            None => self.paths.get(&id).cloned(),
        }
    }

    /// True if path `id` was added in the open transaction.
    pub fn path_created_in_transaction(&self, id: PathId) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|j| matches!(j.get(&Key::Path(id)), Some(None)))
    }

    /// True if fill `id` was added in the open transaction.
    pub fn fill_created_in_transaction(&self, id: FillId) -> bool {
        self.journal
            .as_ref()
            .is_some_and(|j| matches!(j.get(&Key::Fill(id)), Some(None)))
    }

    /// Entity `id` as it was when the open transaction began (as it is now outside one, or
    /// if the transaction hasn't touched it). None if it didn't exist then.
    pub fn entity_before_transaction(&self, id: EntityId) -> Option<Entity> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Entity(id))) {
            Some(Some(Record::Entity(e))) => Some(*e),
            Some(_) => None,
            None => self.entities.get(&id).copied(),
        }
    }

    /// Fill `id` as it was when the open transaction began (see
    /// [`entity_before_transaction`](Self::entity_before_transaction)).
    pub fn fill_before_transaction(&self, id: FillId) -> Option<Fill> {
        match self.journal.as_ref().and_then(|j| j.get(&Key::Fill(id))) {
            Some(Some(Record::Fill(f))) => Some(f.clone()),
            Some(_) => None,
            None => self.fills.get(&id).cloned(),
        }
    }

    /// Entities created in the open transaction (that still exist).
    pub fn created_in_transaction(&self) -> Vec<EntityId> {
        let Some(journal) = &self.journal else {
            return Vec::new();
        };
        let mut ids: Vec<EntityId> = journal
            .iter()
            .filter_map(|(key, before)| match (key, before) {
                (Key::Entity(id), None) if self.entities.contains_key(id) => Some(*id),
                _ => None,
            })
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Finish recording and return the changes made since [`begin`](Self::begin).
    pub fn commit(&mut self, label: impl Into<String>) -> Transaction {
        let journal = self.journal.take().unwrap_or_default();
        let mut changes: Vec<_> = journal
            .into_iter()
            .filter_map(|(key, before)| {
                let after = self.record(key);
                (before != after).then_some((key, before, after))
            })
            .collect();
        changes.sort_by_key(|(key, ..)| *key);
        Transaction {
            label: label.into(),
            changes,
        }
    }

    /// Abandon the open transaction, restoring every touched entity.
    pub fn rollback(&mut self) {
        if let Some(journal) = self.journal.take() {
            for (key, before) in journal {
                self.restore(key, before);
            }
            self.revision += 1;
        }
    }

    fn record(&self, key: Key) -> Option<Record> {
        match key {
            Key::Entity(id) => self.entities.get(&id).copied().map(Record::Entity),
            Key::Constraint(id) => self.constraints.get(&id).copied().map(Record::Constraint),
            Key::Placement(id) => self.placements.get(&id).copied().map(Record::Placement),
            Key::ExtensionStarts(id) => self
                .extension_starts
                .get(&id)
                .copied()
                .map(Record::ExtensionStarts),
            Key::Driven(id) => self.driven.contains(&id).then_some(Record::Driven),
            Key::Variable(id) => self.variables.get(&id).cloned().map(Record::Variable),
            Key::Expression(id) => self.expressions.get(&id).cloned().map(Record::Expression),
            Key::Variant(id) => self.variants.get(&id).cloned().map(Record::Variant),
            Key::Measurement(id) => self.measurements.get(&id).copied().map(Record::Measurement),
            Key::Units => Some(Record::Units(self.units)),
            Key::Layer(id) => self.layers.get(&id).cloned().map(Record::Layer),
            Key::CurrentLayer => Some(Record::CurrentLayer(self.current_layer)),
            Key::Fill(id) => self.fills.get(&id).cloned().map(Record::Fill),
            Key::Annotation(id) => self.annotations.get(&id).cloned().map(Record::Annotation),
            Key::DimNote(id) => self.dim_notes.get(&id).cloned().map(Record::DimNote),
            Key::Shown(r) => self.shown.get(&r).copied().map(Record::Shown),
            Key::OnlyIn(r) => self.only_in.get(&r).cloned().map(Record::OnlyIn),
            Key::Style(r) => self.styles.get(&r).copied().map(Record::Style),
            Key::Draft => Some(Record::Draft(self.draft)),
            Key::Patterns => Some(Record::Patterns(self.patterns.clone())),
            Key::Logo => Some(Record::Logo(self.logo.clone())),
            Key::Rules => Some(Record::Rules(self.rules)),
            Key::Project => Some(Record::Project(self.project.clone())),
            Key::Fonts => Some(Record::Fonts(self.fonts.clone())),
            Key::Sheet(id) => self.sheets.get(&id).cloned().map(Record::Sheet),
            Key::View(id) => self.views.get(&id).cloned().map(Record::View),
            Key::Group(id) => self.groups.get(&id).cloned().map(Record::Group),
            Key::Path(id) => self.paths.get(&id).cloned().map(Record::Path),
            Key::Block(id) => self.blocks.get(&id).cloned().map(Record::Block),
            Key::Instance(id) => self.instances.get(&id).copied().map(Record::Instance),
            Key::Underlay(id) => self.underlays.get(&id).cloned().map(Record::Underlay),
            Key::Text(id) => self.texts.get(&id).cloned().map(Record::Text),
            Key::Wall(id) => self.walls.get(&id).cloned().map(Record::Wall),
            Key::PropertyDim(id) => self
                .property_dims
                .get(&id)
                .copied()
                .map(Record::PropertyDim),
            Key::Opening(id) => self.openings.get(&id).cloned().map(Record::Opening),
        }
    }

    fn touch_key(&mut self, key: Key) {
        if self.journal.is_some() {
            let current = self.record(key);
            if let Some(journal) = &mut self.journal {
                journal.entry(key).or_insert(current);
            }
        }
        self.revision += 1;
    }

    fn touch(&mut self, id: EntityId) {
        self.touch_key(Key::Entity(id));
    }

    fn touch_constraint(&mut self, id: ConstraintId) {
        self.touch_key(Key::Constraint(id));
    }

    fn restore(&mut self, key: Key, record: Option<Record>) {
        match (key, record) {
            (Key::Entity(id), Some(Record::Entity(e))) => {
                self.entities.insert(id, e);
            }
            (Key::Entity(id), _) => {
                self.entities.remove(&id);
            }
            (Key::Constraint(id), Some(Record::Constraint(c))) => {
                self.constraints.insert(id, c);
            }
            (Key::Constraint(id), _) => {
                self.constraints.remove(&id);
            }
            (Key::Placement(id), Some(Record::Placement(p))) => {
                self.placements.insert(id, p);
            }
            (Key::Placement(id), _) => {
                self.placements.remove(&id);
            }
            (Key::ExtensionStarts(id), Some(Record::ExtensionStarts(s))) => {
                self.extension_starts.insert(id, s);
            }
            (Key::ExtensionStarts(id), _) => {
                self.extension_starts.remove(&id);
            }
            (Key::Driven(id), Some(_)) => {
                self.driven.insert(id);
            }
            (Key::Driven(id), None) => {
                self.driven.remove(&id);
            }
            (Key::Variable(id), Some(Record::Variable(v))) => {
                self.variables.insert(id, v);
            }
            (Key::Variable(id), _) => {
                self.variables.remove(&id);
            }
            (Key::Expression(id), Some(Record::Expression(e))) => {
                self.expressions.insert(id, e);
            }
            (Key::Expression(id), _) => {
                self.expressions.remove(&id);
            }
            (Key::Variant(id), Some(Record::Variant(v))) => {
                self.variants.insert(id, v);
            }
            (Key::Variant(id), _) => {
                self.variants.remove(&id);
            }
            (Key::Measurement(id), Some(Record::Measurement(m))) => {
                self.measurements.insert(id, m);
            }
            (Key::Measurement(id), _) => {
                self.measurements.remove(&id);
            }
            (Key::Units, Some(Record::Units(u))) => self.units = u,
            (Key::Units, _) => {}
            (Key::Layer(id), Some(Record::Layer(l))) => {
                self.layers.insert(id, l);
            }
            (Key::Layer(id), _) => {
                self.layers.remove(&id);
            }
            (Key::CurrentLayer, Some(Record::CurrentLayer(l))) => self.current_layer = l,
            (Key::CurrentLayer, _) => {}
            (Key::Fill(id), Some(Record::Fill(f))) => {
                self.fills.insert(id, f);
            }
            (Key::Fill(id), _) => {
                self.fills.remove(&id);
            }
            (Key::Annotation(id), Some(Record::Annotation(a))) => {
                self.annotations.insert(id, a);
            }
            (Key::Annotation(id), _) => {
                self.annotations.remove(&id);
            }
            (Key::DimNote(id), Some(Record::DimNote(n))) => {
                self.dim_notes.insert(id, n);
            }
            (Key::DimNote(id), _) => {
                self.dim_notes.remove(&id);
            }
            (Key::Shown(r), Some(Record::Shown(s))) => {
                self.shown.insert(r, s);
            }
            (Key::Shown(r), _) => {
                self.shown.remove(&r);
            }
            (Key::OnlyIn(r), Some(Record::OnlyIn(v))) => {
                self.only_in.insert(r, v);
            }
            (Key::OnlyIn(r), _) => {
                self.only_in.remove(&r);
            }
            (Key::Style(r), Some(Record::Style(s))) => {
                self.styles.insert(r, s);
            }
            (Key::Style(r), _) => {
                self.styles.remove(&r);
            }
            (Key::Draft, Some(Record::Draft(d))) => self.draft = d,
            (Key::Draft, _) => {}
            (Key::Patterns, Some(Record::Patterns(p))) => self.patterns = p,
            (Key::Patterns, _) => {}
            (Key::Logo, Some(Record::Logo(l))) => self.logo = l,
            (Key::Logo, _) => {}
            (Key::Rules, Some(Record::Rules(r))) => self.rules = r,
            (Key::Rules, _) => {}
            (Key::Project, Some(Record::Project(p))) => self.project = p,
            (Key::Project, _) => {}
            (Key::Fonts, Some(Record::Fonts(f))) => self.fonts = f,
            (Key::Fonts, _) => {}
            (Key::Text(id), Some(Record::Text(t))) => {
                self.texts.insert(id, t);
            }
            (Key::Text(id), _) => {
                self.texts.remove(&id);
            }
            (Key::Underlay(id), Some(Record::Underlay(u))) => {
                self.underlays.insert(id, u);
            }
            (Key::Underlay(id), _) => {
                self.underlays.remove(&id);
            }
            (Key::Sheet(id), Some(Record::Sheet(s))) => {
                self.sheets.insert(id, s);
            }
            (Key::Sheet(id), _) => {
                self.sheets.remove(&id);
            }
            (Key::View(id), Some(Record::View(v))) => {
                self.views.insert(id, v);
            }
            (Key::View(id), _) => {
                self.views.remove(&id);
            }
            (Key::Group(id), Some(Record::Group(g))) => {
                self.groups.insert(id, g);
            }
            (Key::Group(id), _) => {
                self.groups.remove(&id);
            }
            (Key::Path(id), Some(Record::Path(p))) => {
                self.paths.insert(id, p);
            }
            (Key::Path(id), _) => {
                self.paths.remove(&id);
            }
            (Key::Block(id), Some(Record::Block(b))) => {
                self.blocks.insert(id, b);
            }
            (Key::Block(id), _) => {
                self.blocks.remove(&id);
            }
            (Key::Instance(id), Some(Record::Instance(i))) => {
                self.instances.insert(id, i);
            }
            (Key::Instance(id), _) => {
                self.instances.remove(&id);
            }
            (Key::Wall(id), Some(Record::Wall(w))) => {
                self.walls.insert(id, w);
            }
            (Key::Wall(id), _) => {
                self.walls.remove(&id);
            }
            (Key::PropertyDim(id), Some(Record::PropertyDim(d))) => {
                self.property_dims.insert(id, d);
            }
            (Key::PropertyDim(id), _) => {
                self.property_dims.remove(&id);
            }
            (Key::Opening(id), Some(Record::Opening(o))) => {
                self.openings.insert(id, o);
            }
            (Key::Opening(id), _) => {
                self.openings.remove(&id);
            }
        }
    }

    fn apply(&mut self, tx: &Transaction, forward: bool) {
        for (key, before, after) in &tx.changes {
            self.restore(
                *key,
                if forward {
                    after.clone()
                } else {
                    before.clone()
                },
            );
        }
        self.revision += 1;
    }

    // ----- Edits -----------------------------------------------------------------------------

    fn insert(&mut self, mut entity: Entity) -> EntityId {
        entity.layer = self.current_layer;
        let id = EntityId(self.next_id);
        self.next_id += 1;
        self.touch(id);
        self.entities.insert(id, entity);
        id
    }

    pub fn add_point(&mut self, at: DVec2) -> EntityId {
        self.insert(Entity::new(EntityKind::Point(at)))
    }

    pub fn add_line(&mut self, a: EntityId, b: EntityId) -> Result<EntityId, DocumentError> {
        if a == b || self.point(a)? == self.point(b)? {
            return Err(DocumentError::Degenerate("line"));
        }
        Ok(self.insert(Entity::new(EntityKind::Line { a, b })))
    }

    pub fn add_circle(&mut self, center: EntityId, radius: f64) -> Result<EntityId, DocumentError> {
        self.point(center)?;
        if !(radius > 0.0 && radius.is_finite()) {
            return Err(DocumentError::Degenerate("circle"));
        }
        Ok(self.insert(Entity::new(EntityKind::Circle { center, radius })))
    }

    /// Add a cubic Bézier from `a` to `b` with handles `c1` and `c2`.
    pub fn add_bezier(
        &mut self,
        a: EntityId,
        c1: EntityId,
        c2: EntityId,
        b: EntityId,
    ) -> Result<EntityId, DocumentError> {
        self.check_bezier(a, c1, c2, b)?;
        Ok(self.insert(Entity::new(EntityKind::Bezier { a, c1, c2, b })))
    }

    fn check_bezier(
        &self,
        a: EntityId,
        c1: EntityId,
        c2: EntityId,
        b: EntityId,
    ) -> Result<(), DocumentError> {
        let ids = [a, c1, c2, b];
        for (i, x) in ids.iter().enumerate() {
            self.point(*x)?;
            if ids[i + 1..].contains(x) {
                return Err(DocumentError::Degenerate("curve"));
            }
        }
        if self.point(a)? == self.point(b)? && self.point(c1)? == self.point(c2)? {
            return Err(DocumentError::Degenerate("curve"));
        }
        Ok(())
    }

    /// Add an ellipse from its centre and the ends of its two semi-axes (`minor` is moved onto
    /// the perpendicular through the centre, keeping its distance).
    pub fn add_ellipse(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    ) -> Result<EntityId, DocumentError> {
        self.check_ellipse(center, major, minor)?;
        let id = self.insert(Entity::new(EntityKind::Ellipse {
            center,
            major,
            minor,
        }));
        self.normalize_arc(id);
        Ok(id)
    }

    /// Add a counter-clockwise part of an ellipse; `start` and `end` are moved onto it.
    pub fn add_ellipse_arc(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, DocumentError> {
        self.check_ellipse(center, major, minor)?;
        let (s, e) = (self.point(start)?, self.point(end)?);
        if start == end || s == e {
            return Err(DocumentError::Degenerate("elliptical arc"));
        }
        let id = self.insert(Entity::new(EntityKind::EllipseArc {
            center,
            major,
            minor,
            start,
            end,
        }));
        self.normalize_arc(id);
        Ok(id)
    }

    fn check_ellipse(
        &self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    ) -> Result<(), DocumentError> {
        let (c, m, n) = (self.point(center)?, self.point(major)?, self.point(minor)?);
        if center == major || center == minor || major == minor || c == m || c == n {
            return Err(DocumentError::Degenerate("ellipse"));
        }
        Ok(())
    }

    /// Add a counter-clockwise arc. `end` is projected onto the circle through `start`.
    pub fn add_arc(
        &mut self,
        center: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, DocumentError> {
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        if start == end || center == start || center == end || s == c || e == c || s == e {
            return Err(DocumentError::Degenerate("arc"));
        }
        let id = self.insert(Entity::new(EntityKind::Arc { center, start, end }));
        self.normalize_arc(id);
        Ok(id)
    }

    pub fn set_style(&mut self, id: EntityId, style: LineKind) {
        if self.entities.get(&id).is_some_and(|e| e.style != style) {
            self.touch(id);
            if let Some(e) = self.entities.get_mut(&id) {
                e.style = style;
            }
        }
    }

    pub fn set_point(&mut self, id: EntityId, at: DVec2) -> Result<(), DocumentError> {
        self.point(id)?;
        self.touch(id);
        let entity = Entity {
            kind: EntityKind::Point(at),
            ..self.entities[&id]
        };
        self.entities.insert(id, entity);
        Ok(())
    }

    /// Set a circle's radius.
    pub fn set_radius(&mut self, id: EntityId, radius: f64) -> Result<(), DocumentError> {
        match self.entities.get(&id).map(|e| e.kind) {
            Some(EntityKind::Circle { center, .. }) => {
                if !(radius > 0.0 && radius.is_finite()) {
                    return Err(DocumentError::Degenerate("circle"));
                }
                self.touch(id);
                if let Some(e) = self.entities.get_mut(&id) {
                    e.kind = EntityKind::Circle { center, radius };
                }
                Ok(())
            }
            Some(_) => Err(DocumentError::WrongKind {
                id,
                expected: "a circle",
            }),
            None => Err(DocumentError::Missing(id)),
        }
    }

    /// Change what an entity is (e.g. move a line's end to another point, or turn a circle
    /// into an arc), keeping its id so relations on it stay. Validated like adding it.
    pub fn set_entity_kind(&mut self, id: EntityId, kind: EntityKind) -> Result<(), DocumentError> {
        let current = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if current.kind.is_point() != kind.is_point() {
            return Err(DocumentError::WrongKind {
                id,
                expected: if kind.is_point() {
                    "a point"
                } else {
                    "a curve"
                },
            });
        }
        match kind {
            EntityKind::Point(p) if !p.is_finite() => return Err(DocumentError::InvalidValue),
            EntityKind::Line { a, b } => {
                if a == b || self.point(a)? == self.point(b)? {
                    return Err(DocumentError::Degenerate("line"));
                }
            }
            EntityKind::Circle { center, radius } => {
                self.point(center)?;
                if !(radius > 0.0 && radius.is_finite()) {
                    return Err(DocumentError::Degenerate("circle"));
                }
            }
            EntityKind::Arc { center, start, end } => {
                let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
                if start == end || center == start || center == end || s == c || e == c || s == e {
                    return Err(DocumentError::Degenerate("arc"));
                }
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => self.check_ellipse(center, major, minor)?,
            EntityKind::Bezier { a, c1, c2, b } => self.check_bezier(a, c1, c2, b)?,
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => {
                self.check_ellipse(center, major, minor)?;
                if start == end || self.point(start)? == self.point(end)? {
                    return Err(DocumentError::Degenerate("elliptical arc"));
                }
            }
            EntityKind::Point(_) => {}
        }
        if self.entities[&id].kind != kind {
            self.touch(id);
            if let Some(e) = self.entities.get_mut(&id) {
                e.kind = kind;
            }
            if matches!(kind, EntityKind::Arc { .. }) {
                self.normalize_arc(id);
            }
            // Relations the change made meaningless go with it: a point held on a curve
            // that now ends at it, a curve of a kind a relation can't take.
            let stale: Vec<ConstraintId> = self
                .constraints
                .iter()
                .filter(|(_, c)| c.entities().any(|e| e == id) && self.validate(c).is_err())
                .map(|(cid, _)| *cid)
                .collect();
            for cid in stale {
                self.remove_constraint(cid);
            }
        }
        Ok(())
    }

    /// [`set_entity_kind`](Self::set_entity_kind) without moving an arc's end onto the
    /// circle through its start: for geometry generated exactly (a wall's faces), whose
    /// corners other curves share.
    pub fn set_entity_kind_exact(
        &mut self,
        id: EntityId,
        kind: EntityKind,
    ) -> Result<(), DocumentError> {
        if !matches!(kind, EntityKind::Arc { .. }) {
            return self.set_entity_kind(id, kind);
        }
        let EntityKind::Arc { center, start, end } = kind else {
            unreachable!()
        };
        let current = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if current.kind.is_point() {
            return Err(DocumentError::WrongKind {
                id,
                expected: "a curve",
            });
        }
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        if start == end || center == start || center == end || s == c || e == c || s == e {
            return Err(DocumentError::Degenerate("arc"));
        }
        if self.entities[&id].kind != kind {
            self.touch(id);
            if let Some(e) = self.entities.get_mut(&id) {
                e.kind = kind;
            }
            let stale: Vec<ConstraintId> = self
                .constraints
                .iter()
                .filter(|(_, c)| c.entities().any(|e| e == id) && self.validate(c).is_err())
                .map(|(cid, _)| *cid)
                .collect();
            for cid in stale {
                self.remove_constraint(cid);
            }
        }
        Ok(())
    }

    /// [`add_arc`](Self::add_arc) without moving its end onto the circle through its start
    /// (see [`set_entity_kind_exact`](Self::set_entity_kind_exact)).
    pub fn add_arc_exact(
        &mut self,
        center: EntityId,
        start: EntityId,
        end: EntityId,
    ) -> Result<EntityId, DocumentError> {
        let (c, s, e) = (self.point(center)?, self.point(start)?, self.point(end)?);
        if start == end || center == start || center == end || s == c || e == c || s == e {
            return Err(DocumentError::Degenerate("arc"));
        }
        Ok(self.insert(Entity::new(EntityKind::Arc { center, start, end })))
    }

    /// Remove points no curve uses and no constraint refers to beyond themselves (cleanup
    /// after trimming moves a curve's end to a new point).
    pub fn remove_if_orphaned(&mut self, point: EntityId) {
        if self.point(point).is_ok() && self.users_of(point).next().is_none() {
            self.delete([point]);
        }
    }

    /// Every point that defines the given entities (points themselves included).
    pub fn defining_points(&self, ids: impl IntoIterator<Item = EntityId>) -> BTreeSet<EntityId> {
        let mut points = BTreeSet::new();
        for id in ids {
            if let Some(e) = self.entities.get(&id) {
                if e.kind.is_point() {
                    points.insert(id);
                }
                points.extend(e.kind.points());
            }
        }
        points
    }

    /// Translate the given entities. Shared points move once; connected curves stretch, and
    /// arcs whose points moved are kept circular.
    pub fn translate(&mut self, ids: impl IntoIterator<Item = EntityId>, delta: DVec2) {
        if delta == DVec2::ZERO {
            return;
        }
        let points = self.defining_points(ids);
        for &id in &points {
            if let Ok(p) = self.point(id) {
                let _ = self.set_point(id, p + delta);
            }
        }
        self.normalize_arcs_using(&points);
    }

    /// Re-project the end point of every arc that uses one of `points` onto its circle.
    pub fn normalize_arcs_using(&mut self, points: &BTreeSet<EntityId>) {
        let arcs: Vec<_> = self
            .entities
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e.kind,
                    EntityKind::Arc { .. }
                        | EntityKind::Ellipse { .. }
                        | EntityKind::EllipseArc { .. }
                ) && e.kind.points().any(|p| points.contains(&p))
            })
            .map(|(id, _)| *id)
            .collect();
        for arc in arcs {
            self.normalize_arc(arc);
        }
    }

    fn normalize_arc(&mut self, arc: EntityId) {
        let kind = self.entities.get(&arc).map(|e| e.kind);
        if let Some(
            EntityKind::Ellipse {
                center,
                major,
                minor,
            }
            | EntityKind::EllipseArc {
                center,
                major,
                minor,
                ..
            },
        ) = kind
        {
            self.normalize_ellipse(center, major, minor);
            if let Some(EntityKind::EllipseArc { start, end, .. }) = kind {
                for p in [start, end] {
                    self.project_onto_ellipse(center, major, minor, p);
                }
            }
            return;
        }
        let Some(EntityKind::Arc { center, start, end }) = kind else {
            return;
        };
        let (Ok(c), Ok(s), Ok(e)) = (self.point(center), self.point(start), self.point(end)) else {
            return;
        };
        let radius = (s - c).length();
        let dir = e - c;
        if radius > 0.0 && dir.length() > 0.0 {
            let projected = c + dir.normalize() * radius;
            if projected != e {
                let _ = self.set_point(end, projected);
            }
        }
    }

    /// Keep an ellipse's minor point on the perpendicular through its centre (at the same
    /// distance, on the side it's nearer).
    fn normalize_ellipse(&mut self, center: EntityId, major: EntityId, minor: EntityId) {
        let (Ok(c), Ok(m), Ok(n)) = (self.point(center), self.point(major), self.point(minor))
        else {
            return;
        };
        let axis = (m - c).normalize_or_zero().perp();
        let b = n.distance(c);
        if axis == DVec2::ZERO || b == 0.0 {
            return;
        }
        let side = if (n - c).dot(axis) < 0.0 { -1.0 } else { 1.0 };
        let target = c + axis * b * side;
        if target != n {
            let _ = self.set_point(minor, target);
        }
    }

    /// Move a point onto an ellipse, along the line from its centre.
    fn project_onto_ellipse(
        &mut self,
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        point: EntityId,
    ) {
        let (Ok(c), Ok(m), Ok(n), Ok(p)) = (
            self.point(center),
            self.point(major),
            self.point(minor),
            self.point(point),
        ) else {
            return;
        };
        let (a, b) = ((m - c).length(), (n - c).length());
        let u = (m - c).normalize_or_zero();
        let d = p - c;
        let q = ((d.dot(u) / a).powi(2) + (d.dot(u.perp()) / b).powi(2)).sqrt();
        if q > 0.0 && q.is_finite() {
            let target = c + d / q;
            if target != p {
                let _ = self.set_point(point, target);
            }
        }
    }

    /// Delete entities. Deleting a point also deletes the curves that use it; points left
    /// unused by deleting a curve are removed too (unless they were explicitly standalone).
    pub fn delete(&mut self, ids: impl IntoIterator<Item = EntityId>) {
        self.remember_leader_targets();
        let mut doomed: BTreeSet<EntityId> =
            ids.into_iter().filter(|id| self.contains(*id)).collect();
        // Curves that use a deleted point go too.
        let dependents: Vec<_> = self
            .entities
            .iter()
            .filter(|(_, e)| e.kind.points().any(|p| doomed.contains(&p)))
            .map(|(id, _)| *id)
            .collect();
        doomed.extend(dependents);

        let orphan_candidates = self.defining_points(doomed.iter().copied());
        for id in &doomed {
            self.touch(*id);
            self.entities.remove(id);
        }
        for point in orphan_candidates {
            if self.contains(point) && self.users_of(point).next().is_none() {
                self.touch(point);
                self.entities.remove(&point);
            }
        }
        // Constraints on anything that no longer exists go too.
        let dangling: Vec<_> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.entities().any(|e| !self.entities.contains_key(&e)))
            .map(|(id, _)| *id)
            .collect();
        for id in dangling {
            self.remove_constraint(id);
        }
        let dangling: Vec<_> = self
            .measurements
            .iter()
            .filter(|(_, c)| c.entities().any(|e| !self.entities.contains_key(&e)))
            .map(|(id, _)| *id)
            .collect();
        for id in dangling {
            self.remove_measurement(id);
        }
        // A fill whose whole boundary goes has nothing left to fill: it goes too (and comes
        // back with its curves on undo). One that keeps some of its curves keeps the deleted
        // ones' ids, so the editor can find the area around its seed now (or drop it when
        // there is none).
        let emptied: Vec<FillId> = self
            .fills
            .iter()
            .filter(|(_, f)| {
                f.boundary
                    .iter()
                    .all(|e| doomed.contains(e) || !self.entities.contains_key(e))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in emptied {
            self.remove_fill(id);
        }
        self.drop_dangling_annotations();
        self.prune_groups();
        self.prune_paths();
        self.prune_walls();
    }

    /// Walls whose centreline is gone go, with what they made (and come back with it on
    /// undo); tees on them let go, and ends against deleted curves cap.
    fn prune_walls(&mut self) {
        if self.walls.is_empty() && self.openings.is_empty() {
            return;
        }
        let gone: Vec<WallId> = self
            .walls
            .iter()
            .filter(|(_, w)| !self.paths.contains_key(&w.centre))
            .map(|(id, _)| *id)
            .collect();
        let mut made: Vec<EntityId> = Vec::new();
        // Openings in walls that go, go with them (their symbols too).
        let orphans: Vec<OpeningId> = self
            .openings
            .iter()
            .filter(|(_, o)| gone.contains(&o.wall) || !self.walls.contains_key(&o.wall))
            .map(|(id, _)| *id)
            .collect();
        for o in orphans {
            if let Some(op) = self.openings.get(&o) {
                made.extend(op.made.entities());
            }
            self.remove_opening(o);
        }
        for id in &gone {
            if let Some(w) = self.walls.get(id).cloned() {
                made.extend(w.made.entities());
                // The cluster's outlines and pochés it held go (they span the other walls
                // too, whose faces stay): regenerating makes them again under the next lead.
                for f in w.made.fills.values() {
                    self.remove_fill(*f);
                }
                for p in w.made.outlines.values() {
                    self.remove_path(*p);
                }
            }
            self.remove_wall(*id);
        }
        let stale: Vec<(WallId, Wall)> = self
            .walls
            .iter()
            .filter_map(|(id, w)| {
                let mut w2 = w.clone();
                for end in &mut w2.ends {
                    if end
                        .tee
                        .is_some_and(|(host, _)| !self.walls.contains_key(&host))
                    {
                        end.tee = None;
                    }
                    if end.against.is_some_and(|c| !self.entities.contains_key(&c)) {
                        end.against = None;
                    }
                }
                (w2 != *w).then_some((*id, w2))
            })
            .collect();
        for (id, w) in stale {
            self.touch_key(Key::Wall(id));
            self.walls.insert(id, w);
        }
        let made: Vec<EntityId> = made
            .into_iter()
            .filter(|e| self.entities.contains_key(e))
            .collect();
        if !made.is_empty() {
            self.delete(made);
        }
    }

    /// Drop deleted curves from paths (a path missing one is open), remove paths left with
    /// none, and unbind the fills of those.
    fn prune_paths(&mut self) {
        let stale: Vec<PathId> = self
            .paths
            .iter()
            .filter(|(_, p)| p.curves.iter().any(|e| !self.entities.contains_key(e)))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.touch_key(Key::Path(id));
            let entities = &self.entities;
            let empty = match self.paths.get_mut(&id) {
                Some(p) => {
                    p.curves.retain(|e| entities.contains_key(e));
                    p.closed = false;
                    p.curves.is_empty()
                }
                None => false,
            };
            if empty {
                self.paths.remove(&id);
                self.unbind_fills(id);
            }
        }
    }

    /// Fills of a path that is gone fill an area among the sketch's curves again.
    fn unbind_fills(&mut self, path: PathId) {
        let bound: Vec<FillId> = self
            .fills
            .iter()
            .filter(|(_, f)| f.path == Some(path))
            .map(|(id, _)| *id)
            .collect();
        for id in bound {
            self.touch_key(Key::Fill(id));
            if let Some(f) = self.fills.get_mut(&id) {
                f.path = None;
            }
        }
    }

    /// Drop deleted entities from groups, and groups left with fewer than one member.
    fn prune_groups(&mut self) {
        let stale: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, g)| g.members.iter().any(|e| !self.entities.contains_key(e)))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            self.touch_key(Key::Group(id));
            let entities = &self.entities;
            if let Some(g) = self.groups.get_mut(&id) {
                g.members.retain(|e| entities.contains_key(e));
                if g.members.is_empty() {
                    self.groups.remove(&id);
                }
            }
        }
    }

    /// A note's text position in the world (`at` is from its first leader's tip).
    pub fn note_text_pos(&self, at: DVec2, leaders: &[Leader]) -> Option<DVec2> {
        match leaders.first() {
            Some(l) => Some(self.anchor_pos(&l.to)? + at),
            None => Some(at),
        }
    }

    /// The `at` that puts a note's text at `pos` (world) with these leaders.
    pub fn note_at(&self, pos: DVec2, leaders: &[Leader]) -> Option<DVec2> {
        match leaders.first() {
            Some(l) => Some(pos - self.anchor_pos(&l.to)?),
            None => Some(pos),
        }
    }

    /// Annotations on deleted geometry go; a note keeps its text where it was, losing only the
    /// leaders whose targets went.
    fn drop_dangling_annotations(&mut self) {
        let alive = |doc: &Self, e: EntityId| doc.entities.contains_key(&e);
        let dangling: Vec<(AnnotationId, Annotation)> = self
            .annotations
            .iter()
            .filter(|(_, a)| a.entities().iter().any(|e| !alive(self, *e)))
            .map(|(id, a)| (*id, a.clone()))
            .collect();
        for (id, a) in dangling {
            self.touch_key(Key::Annotation(id));
            match a {
                Annotation::Note {
                    text,
                    at,
                    mut leaders,
                    style,
                } => {
                    // Where the text was, before its first leader's target went.
                    let first = leaders.first().and_then(|l| {
                        self.last_anchor_pos.get(&id).copied().or(match l.to {
                            Anchor::Free(p) => Some(p),
                            _ => None,
                        })
                    });
                    let pos = first.unwrap_or(DVec2::ZERO) + at;
                    leaders.retain(|l| l.to.entity().is_none_or(|e| alive(self, e)));
                    let at = self.note_at(pos, &leaders).unwrap_or(pos);
                    self.annotations.insert(
                        id,
                        Annotation::Note {
                            text,
                            at,
                            leaders,
                            style,
                        },
                    );
                }
                _ => {
                    self.annotations.remove(&id);
                }
            }
        }
    }

    /// Merge point `from` into point `into`: every reference to `from` is redirected and `from`
    /// is removed. Fails (changing nothing) if that would collapse a curve.
    pub fn merge_points(&mut self, from: EntityId, into: EntityId) -> Result<(), DocumentError> {
        self.point(from)?;
        self.point(into)?;
        if from == into {
            return Ok(());
        }
        let users: Vec<_> = self.users_of(from).collect();
        for &user in &users {
            let mut kind = self.entities[&user].kind;
            kind.replace_point(from, into);
            let collapses = match kind {
                EntityKind::Line { a, b } => a == b,
                EntityKind::Arc { center, start, end } => {
                    center == start || center == end || start == end
                }
                _ => false,
            };
            if collapses {
                return Err(DocumentError::Degenerate("merge"));
            }
        }
        for user in users {
            self.touch(user);
            if let Some(e) = self.entities.get_mut(&user) {
                e.kind.replace_point(from, into);
            }
        }
        let constrained: Vec<_> = self
            .constraints
            .iter()
            .filter(|(_, c)| c.entities().any(|e| e == from))
            .map(|(id, c)| (*id, *c))
            .collect();
        for (id, mut c) in constrained {
            self.touch_constraint(id);
            c.replace_point(from, into);
            if c.is_degenerate() || self.self_referencing(&c).is_some() {
                // e.g. a coincidence between the two merged points (now implied), or a point
                // held on a curve whose end it has become.
                self.constraints.remove(&id);
            } else {
                self.constraints.insert(id, c);
            }
        }
        let measured: Vec<_> = self
            .measurements
            .iter()
            .filter(|(_, c)| c.entities().any(|e| e == from))
            .map(|(id, c)| (*id, *c))
            .collect();
        for (id, mut c) in measured {
            self.touch_key(Key::Measurement(id));
            c.replace_point(from, into);
            if c.is_degenerate() || self.self_referencing(&c).is_some() {
                self.measurements.remove(&id);
            } else {
                self.measurements.insert(id, c);
            }
        }
        let grouped: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, g)| g.members.contains(&from))
            .map(|(id, _)| *id)
            .collect();
        for id in grouped {
            self.touch_key(Key::Group(id));
            if let Some(g) = self.groups.get_mut(&id) {
                g.members.remove(&from);
                g.members.insert(into);
            }
        }
        let annotated: Vec<_> = self
            .annotations
            .iter()
            .filter(|(_, a)| a.entities().contains(&from))
            .map(|(id, _)| *id)
            .collect();
        for id in annotated {
            self.touch_key(Key::Annotation(id));
            if let Some(a) = self.annotations.get_mut(&id) {
                a.replace_entity(from, into);
            }
        }
        self.touch(from);
        self.entities.remove(&from);
        self.normalize_arcs_using(&BTreeSet::from([into]));
        Ok(())
    }

    // ----- Constraints -----------------------------------------------------------------------

    pub fn constraint(&self, id: ConstraintId) -> Option<&Constraint> {
        self.constraints.get(&id)
    }

    /// All constraints in id order.
    pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &Constraint)> {
        self.constraints.iter().map(|(id, c)| (*id, c))
    }

    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    /// Constraints that refer to `entity`.
    pub fn constraints_on(&self, entity: EntityId) -> impl Iterator<Item = ConstraintId> + '_ {
        self.constraints
            .iter()
            .filter(move |(_, c)| c.entities().any(|e| e == entity))
            .map(|(id, _)| *id)
    }

    /// What makes a constraint refer to a curve's own points where that means nothing: a
    /// curve's point held on the curve, symmetry about a line through the points, a line's
    /// end at its own midpoint.
    fn self_referencing(&self, constraint: &Constraint) -> Option<&'static str> {
        let points_of = |id: &EntityId| self.entities.get(id).map(|e| e.kind.points());
        match constraint {
            Constraint::PointOnCurve { point, curve }
                if points_of(curve).is_some_and(|mut ps| ps.any(|p| p == *point)) =>
            {
                Some("point on its own curve")
            }
            Constraint::Symmetric { a, b, axis }
                if points_of(axis).is_some_and(|mut ps| ps.any(|p| p == *a || p == *b)) =>
            {
                Some("symmetric about a line through the points")
            }
            Constraint::Midpoint { point, line }
                if matches!(
                    self.entities.get(line).map(|e| e.kind),
                    Some(EntityKind::Line { a, b }) if *point == a || *point == b
                ) =>
            {
                Some("midpoint")
            }
            _ => None,
        }
    }

    /// Check that a constraint refers to existing entities of the right kinds.
    pub fn validate(&self, constraint: &Constraint) -> Result<(), DocumentError> {
        if constraint.is_degenerate() {
            return Err(DocumentError::Degenerate("constraint"));
        }
        if !constraint.value_is_valid() {
            return Err(DocumentError::InvalidValue);
        }
        let mut kinds = Vec::new();
        for (id, slot) in constraint.slots() {
            let kind = self
                .entities
                .get(&id)
                .ok_or(DocumentError::Missing(id))?
                .kind;
            let ok = match slot {
                Slot::Point => kind.is_point(),
                Slot::PointOrMidpoint => kind.is_point() || matches!(kind, EntityKind::Line { .. }),
                Slot::Line => matches!(kind, EntityKind::Line { .. }),
                Slot::Circular => {
                    matches!(kind, EntityKind::Circle { .. } | EntityKind::Arc { .. })
                }
                Slot::LineOrCircular => matches!(
                    kind,
                    EntityKind::Line { .. } | EntityKind::Circle { .. } | EntityKind::Arc { .. }
                ),
                Slot::Curve => !kind.is_point(),
            };
            if !ok {
                return Err(DocumentError::WrongKind {
                    id,
                    expected: slot.describe(),
                });
            }
            kinds.push(kind);
        }
        // Tangency is meaningless between a curve and itself (caught by `is_degenerate`).
        if let Some(why) = self.self_referencing(constraint) {
            return Err(DocumentError::Degenerate(why));
        }
        // Tangent to a circle or arc, or at a shared end to an elliptical arc or Bézier.
        if let Constraint::Tangent { b, .. } = constraint {
            let ok = match kinds[1] {
                EntityKind::Circle { .. } | EntityKind::Arc { .. } => true,
                EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. } => {
                    kinds[0].shared_end(&kinds[1]).is_some()
                }
                _ => false,
            };
            if !ok {
                return Err(DocumentError::WrongKind {
                    id: *b,
                    expected: "a circle or arc, or an elliptical arc or curve ending where it does",
                });
            }
        }
        // Equal needs two lines or two circular curves.
        if let Constraint::Equal { b, .. } = constraint {
            let first_is_line = matches!(kinds[0], EntityKind::Line { .. });
            if first_is_line != matches!(kinds[1], EntityKind::Line { .. }) {
                return Err(DocumentError::WrongKind {
                    id: *b,
                    expected: if first_is_line {
                        "a line"
                    } else {
                        "a circle or arc"
                    },
                });
            }
        }
        Ok(())
    }

    pub fn add_constraint(
        &mut self,
        constraint: Constraint,
    ) -> Result<ConstraintId, DocumentError> {
        self.validate(&constraint)?;
        let id = ConstraintId(self.next_constraint_id);
        self.next_constraint_id += 1;
        self.touch_constraint(id);
        self.constraints.insert(id, constraint);
        Ok(id)
    }

    /// Replace a constraint (e.g. to change a dimension's value).
    pub fn set_constraint(
        &mut self,
        id: ConstraintId,
        constraint: Constraint,
    ) -> Result<(), DocumentError> {
        if !self.constraints.contains_key(&id) {
            return Err(DocumentError::MissingConstraint(id));
        }
        self.validate(&constraint)?;
        self.touch_constraint(id);
        self.constraints.insert(id, constraint);
        Ok(())
    }

    pub fn remove_constraint(&mut self, id: ConstraintId) {
        if self.constraints.contains_key(&id) {
            self.touch_constraint(id);
            self.constraints.remove(&id);
        }
        if self.placements.contains_key(&id) {
            self.touch_key(Key::Placement(id));
            self.placements.remove(&id);
        }
        if self.extension_starts.contains_key(&id) {
            self.touch_key(Key::ExtensionStarts(id));
            self.extension_starts.remove(&id);
        }
        if self.driven.contains(&id) {
            self.touch_key(Key::Driven(id));
            self.driven.remove(&id);
        }
        self.set_expression(id, None);
        self.set_dim_note(id, None);
    }

    /// Whether a dimension is driven (a reference that reports but doesn't constrain).
    pub fn is_driven(&self, id: ConstraintId) -> bool {
        self.driven.contains(&id)
    }

    /// Make a dimension driven or driving. Only dimensions can be driven.
    pub fn set_driven(&mut self, id: ConstraintId, driven: bool) -> Result<(), DocumentError> {
        let constraint = self
            .constraints
            .get(&id)
            .ok_or(DocumentError::MissingConstraint(id))?;
        if driven && !constraint.is_dimension() {
            return Err(DocumentError::InvalidValue);
        }
        if self.driven.contains(&id) != driven {
            self.touch_key(Key::Driven(id));
            if driven {
                self.driven.insert(id);
            } else {
                self.driven.remove(&id);
            }
        }
        Ok(())
    }

    /// Where a dimension's label sits, as an offset from its anchor (see [`Document`]).
    pub fn placement(&self, id: ConstraintId) -> Option<DVec2> {
        self.placements.get(&id).copied()
    }

    pub fn set_placement(&mut self, id: ConstraintId, offset: DVec2) -> Result<(), DocumentError> {
        if !self.constraints.contains_key(&id) {
            return Err(DocumentError::MissingConstraint(id));
        }
        if !offset.is_finite() {
            return Err(DocumentError::InvalidValue);
        }
        if self.placements.get(&id) != Some(&offset) {
            self.touch_key(Key::Placement(id));
            self.placements.insert(id, offset);
        }
        Ok(())
    }

    /// Where a dimension's extension lines start, per end: a distance from the measured point
    /// along the extension line (toward the dimension line), or `None` for automatic.
    pub fn extension_starts(&self, id: ConstraintId) -> [Option<f64>; 2] {
        self.extension_starts.get(&id).copied().unwrap_or_default()
    }

    /// Set where one end's extension line starts (`end` 0 or 1), or `None` for automatic.
    pub fn set_extension_start(
        &mut self,
        id: ConstraintId,
        end: usize,
        start: Option<f64>,
    ) -> Result<(), DocumentError> {
        if !self.constraints.contains_key(&id) {
            return Err(DocumentError::MissingConstraint(id));
        }
        if end > 1 || start.is_some_and(|s| !s.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        let mut starts = self.extension_starts(id);
        if starts[end] != start {
            starts[end] = start;
            self.touch_key(Key::ExtensionStarts(id));
            if starts == [None, None] {
                self.extension_starts.remove(&id);
            } else {
                self.extension_starts.insert(id, starts);
            }
        }
        Ok(())
    }

    // ----- Layers ----------------------------------------------------------------------------

    /// All layers in id order (layer "0" first).
    /// The layers in draw order, bottom first.
    pub fn layers(&self) -> impl Iterator<Item = (LayerId, &Layer)> {
        let mut layers: Vec<(LayerId, &Layer)> =
            self.layers.iter().map(|(id, l)| (*id, l)).collect();
        layers.sort_by_key(|(id, l)| (l.order, *id));
        layers.into_iter()
    }

    /// Each layer's place in the draw order (0 at the bottom).
    pub fn layer_ranks(&self) -> std::collections::HashMap<LayerId, usize> {
        self.layers()
            .enumerate()
            .map(|(i, (id, _))| (id, i))
            .collect()
    }

    /// Move a layer to `index` in the draw order (0 at the bottom).
    pub fn reorder_layer(&mut self, id: LayerId, index: usize) {
        let mut order: Vec<LayerId> = self.layers().map(|(l, _)| l).collect();
        let Some(from) = order.iter().position(|l| *l == id) else {
            return;
        };
        order.remove(from);
        order.insert(index.min(order.len()), id);
        for (rank, l) in order.into_iter().enumerate() {
            if let Some(mut layer) = self.layers.get(&l).cloned() {
                layer.order = i32::try_from(rank).unwrap_or(i32::MAX);
                self.set_layer(l, layer);
            }
        }
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.get(&id)
    }

    /// The layer an entity is on (layer "0" if its layer is missing).
    pub fn layer_of(&self, entity: EntityId) -> &Layer {
        let id = self
            .entities
            .get(&entity)
            .map_or(LayerId::DEFAULT, |e| e.layer);
        self.layers
            .get(&id)
            .or_else(|| self.layers.get(&LayerId::DEFAULT))
            .expect("layer 0 exists")
    }

    /// Add a layer, on top of the others.
    pub fn add_layer(&mut self, mut layer: Layer) -> LayerId {
        layer.order = self.layers.values().map(|l| l.order + 1).max().unwrap_or(0);
        let id = LayerId(self.next_param());
        self.touch_key(Key::Layer(id));
        self.layers.insert(id, layer);
        id
    }

    /// Add or replace a layer with a given id (when loading, or editing its settings).
    pub fn set_layer(&mut self, id: LayerId, layer: Layer) {
        self.reserve_param(id.0);
        if self.layers.get(&id) != Some(&layer) {
            self.touch_key(Key::Layer(id));
            self.layers.insert(id, layer);
        }
    }

    /// Delete a layer, moving its entities to layer "0". Layer "0" can't be deleted.
    pub fn remove_layer(&mut self, id: LayerId) -> Result<(), DocumentError> {
        if id == LayerId::DEFAULT {
            return Err(DocumentError::BadName("layer 0 can't be deleted".into()));
        }
        if !self.layers.contains_key(&id) {
            return Err(DocumentError::MissingLayer(id));
        }
        let on: Vec<EntityId> = self
            .entities
            .iter()
            .filter(|(_, e)| e.layer == id)
            .map(|(eid, _)| *eid)
            .collect();
        for e in on {
            self.touch(e);
            if let Some(x) = self.entities.get_mut(&e) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let placed: Vec<InstanceId> = self
            .instances
            .iter()
            .filter(|(_, i)| i.layer == id)
            .map(|(iid, _)| *iid)
            .collect();
        for i in placed {
            self.touch_key(Key::Instance(i));
            if let Some(x) = self.instances.get_mut(&i) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let texts: Vec<TextId> = self
            .texts
            .iter()
            .filter(|(_, t)| t.layer == id)
            .map(|(tid, _)| *tid)
            .collect();
        for t in texts {
            self.touch_key(Key::Text(t));
            if let Some(x) = self.texts.get_mut(&t) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let fills: Vec<FillId> = self
            .fills
            .iter()
            .filter(|(_, f)| f.layer == id)
            .map(|(fid, _)| *fid)
            .collect();
        for f in fills {
            self.touch_key(Key::Fill(f));
            if let Some(x) = self.fills.get_mut(&f) {
                x.layer = LayerId::DEFAULT;
            }
        }
        let views: Vec<ViewId> = self
            .views
            .iter()
            .filter(|(_, v)| v.hidden_layers.contains(&id))
            .map(|(vid, _)| *vid)
            .collect();
        for v in views {
            self.touch_key(Key::View(v));
            if let Some(x) = self.views.get_mut(&v) {
                x.hidden_layers.remove(&id);
            }
        }
        if self.current_layer == id {
            self.set_current_layer(LayerId::DEFAULT)?;
        }
        self.touch_key(Key::Layer(id));
        self.layers.remove(&id);
        Ok(())
    }

    pub fn current_layer(&self) -> LayerId {
        self.current_layer
    }

    pub fn set_current_layer(&mut self, id: LayerId) -> Result<(), DocumentError> {
        if !self.layers.contains_key(&id) {
            return Err(DocumentError::MissingLayer(id));
        }
        if self.current_layer != id {
            self.touch_key(Key::CurrentLayer);
            self.current_layer = id;
        }
        Ok(())
    }

    /// Move an entity to a layer.
    pub fn set_entity_layer(&mut self, id: EntityId, layer: LayerId) -> Result<(), DocumentError> {
        if !self.layers.contains_key(&layer) {
            return Err(DocumentError::MissingLayer(layer));
        }
        let e = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if e.layer != layer {
            self.touch(id);
            if let Some(x) = self.entities.get_mut(&id) {
                x.layer = layer;
            }
        }
        Ok(())
    }

    /// Set where an entity stacks on its layer (see [`Entity::order`]).
    pub fn set_order(&mut self, id: EntityId, order: Option<i32>) -> Result<(), DocumentError> {
        let e = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if e.order != order {
            self.touch(id);
            if let Some(x) = self.entities.get_mut(&id) {
                x.order = order;
            }
        }
        Ok(())
    }

    /// Set an entity's appearance overrides.
    pub fn set_appearance(
        &mut self,
        id: EntityId,
        appearance: Appearance,
    ) -> Result<(), DocumentError> {
        let e = self.entities.get(&id).ok_or(DocumentError::Missing(id))?;
        if e.appearance != appearance {
            self.touch(id);
            if let Some(x) = self.entities.get_mut(&id) {
                x.appearance = appearance;
            }
        }
        Ok(())
    }

    /// Give `to` the same line role, layer and appearance as `from` (for copies made by modify
    /// tools).
    pub fn copy_look(&mut self, from: EntityId, to: EntityId) {
        let Some(src) = self.entities.get(&from).copied() else {
            return;
        };
        if let Some(dst) = self.entities.get(&to).copied()
            && (dst.style, dst.layer, dst.appearance, dst.order)
                != (src.style, src.layer, src.appearance, src.order)
        {
            self.touch(to);
            if let Some(x) = self.entities.get_mut(&to) {
                x.style = src.style;
                x.layer = src.layer;
                x.appearance = src.appearance;
                x.order = src.order;
            }
        }
    }

    /// Entities on a locked layer, and the points they use (for the solver to keep fixed).
    pub fn locked_entities(&self) -> BTreeSet<EntityId> {
        let locked: BTreeSet<LayerId> = self
            .layers
            .iter()
            .filter(|(_, l)| l.locked)
            .map(|(id, _)| *id)
            .collect();
        if locked.is_empty() {
            return BTreeSet::new();
        }
        let mut out = BTreeSet::new();
        for (id, e) in &self.entities {
            if locked.contains(&e.layer) {
                out.insert(*id);
                out.extend(e.kind.points());
            }
        }
        out
    }

    /// True if the entity's layer is visible.
    pub fn is_visible(&self, id: EntityId) -> bool {
        self.layer_of(id).visible
    }

    // ----- Parameters: units, variables, expressions, variants, measurements -----------------

    /// Every journal key with content in this document.
    fn keys(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = Vec::new();
        keys.extend(self.entities.keys().map(|id| Key::Entity(*id)));
        keys.extend(self.constraints.keys().map(|id| Key::Constraint(*id)));
        keys.extend(self.placements.keys().map(|id| Key::Placement(*id)));
        keys.extend(
            self.extension_starts
                .keys()
                .map(|id| Key::ExtensionStarts(*id)),
        );
        keys.extend(self.driven.iter().map(|id| Key::Driven(*id)));
        keys.extend(self.variables.keys().map(|id| Key::Variable(*id)));
        keys.extend(self.expressions.keys().map(|id| Key::Expression(*id)));
        keys.extend(self.variants.keys().map(|id| Key::Variant(*id)));
        keys.extend(self.measurements.keys().map(|id| Key::Measurement(*id)));
        keys.push(Key::Units);
        keys.extend(self.layers.keys().map(|id| Key::Layer(*id)));
        keys.push(Key::CurrentLayer);
        keys.extend(self.fills.keys().map(|id| Key::Fill(*id)));
        keys.extend(self.annotations.keys().map(|id| Key::Annotation(*id)));
        keys.extend(self.dim_notes.keys().map(|id| Key::DimNote(*id)));
        keys.extend(self.shown.keys().map(|r| Key::Shown(*r)));
        keys.extend(self.only_in.keys().map(|r| Key::OnlyIn(*r)));
        keys.extend(self.styles.keys().map(|r| Key::Style(*r)));
        keys.push(Key::Draft);
        keys.push(Key::Patterns);
        keys.push(Key::Logo);
        keys.push(Key::Rules);
        keys.push(Key::Project);
        keys.push(Key::Fonts);
        keys.extend(self.sheets.keys().map(|id| Key::Sheet(*id)));
        keys.extend(self.views.keys().map(|id| Key::View(*id)));
        keys.extend(self.groups.keys().map(|id| Key::Group(*id)));
        keys.extend(self.paths.keys().map(|id| Key::Path(*id)));
        keys.extend(self.blocks.keys().map(|id| Key::Block(*id)));
        keys.extend(self.instances.keys().map(|id| Key::Instance(*id)));
        keys.extend(self.underlays.keys().map(|id| Key::Underlay(*id)));
        keys.extend(self.texts.keys().map(|id| Key::Text(*id)));
        keys.extend(self.walls.keys().map(|id| Key::Wall(*id)));
        keys.extend(self.property_dims.keys().map(|id| Key::PropertyDim(*id)));
        keys.extend(self.openings.keys().map(|id| Key::Opening(*id)));
        keys
    }

    /// Make this document's content equal to `other`'s, as journalled changes (so it can be
    /// undone).
    pub fn restore_from(&mut self, other: &Document) {
        self.restore_parts(other, true);
    }

    /// [`restore_from`](Self::restore_from), keeping the options if not `options`.
    fn restore_parts(&mut self, other: &Document, options: bool) {
        let mut keys = self.keys();
        keys.extend(other.keys());
        if !options {
            keys.retain(|k| !matches!(k, Key::Variant(_)));
        }
        keys.sort();
        keys.dedup();
        for key in keys {
            let theirs = other.record(key);
            if self.record(key) != theirs {
                self.touch_key(key);
                self.restore(key, theirs);
            }
        }
        self.next_id = self.next_id.max(other.next_id);
        self.next_constraint_id = self.next_constraint_id.max(other.next_constraint_id);
        self.next_param_id = self.next_param_id.max(other.next_param_id);
        self.next_drafting_id = self.next_drafting_id.max(other.next_drafting_id);
        self.next_wall_id = self.next_wall_id.max(other.next_wall_id);
    }

    /// A copy of the drawing for a whole-sketch option: everything but its options (and no
    /// open transaction). Cheap: its parts are shared until changed.
    pub fn sketch_copy(&self) -> Document {
        let mut copy = self.clone();
        copy.variants = Shared::default();
        copy.journal = None;
        copy.last_anchor_pos.clear();
        copy
    }

    /// Make the drawing what `sketch` holds, keeping its options (inside a transaction, so it
    /// can be undone). Applies a whole-sketch option.
    pub fn restore_sketch(&mut self, sketch: &Document) {
        self.restore_parts(sketch, false);
    }

    /// True if the drawing is what `sketch` holds (its options aside).
    pub fn same_sketch(&self, sketch: &Document) -> bool {
        let mut me = self.clone();
        me.variants = Shared::default();
        me == *sketch
    }

    pub fn units(&self) -> Units {
        self.units
    }

    pub fn set_units(&mut self, units: Units) {
        if self.units != units {
            self.touch_key(Key::Units);
            self.units = units;
        }
    }

    fn next_param(&mut self) -> u32 {
        let id = self.next_param_id;
        self.next_param_id += 1;
        id
    }

    /// Make sure later ids don't collide with `id` (when loading stored ids).
    fn reserve_param(&mut self, id: u32) {
        self.next_param_id = self.next_param_id.max(id + 1);
    }

    /// All variables in id order.
    pub fn variables(&self) -> impl Iterator<Item = (VariableId, &Variable)> {
        self.variables.iter().map(|(id, v)| (*id, v))
    }

    pub fn variable(&self, id: VariableId) -> Option<&Variable> {
        self.variables.get(&id)
    }

    pub fn variable_named(&self, name: &str) -> Option<VariableId> {
        self.variables
            .iter()
            .find(|(_, v)| v.name == name)
            .map(|(id, _)| *id)
    }

    fn check_name(&self, name: &str, except: Option<VariableId>) -> Result<(), DocumentError> {
        crate::expr::valid_name(name).map_err(|e| DocumentError::BadName(e.0))?;
        match self.variable_named(name) {
            Some(other) if Some(other) != except => Err(DocumentError::BadName(format!(
                "there is already a variable called `{name}`"
            ))),
            _ => Ok(()),
        }
    }

    /// Add a variable. The expression isn't evaluated here (see [`crate::params`]).
    pub fn add_variable(&mut self, variable: Variable) -> Result<VariableId, DocumentError> {
        let id = VariableId(self.next_param_id);
        self.insert_variable(id, variable)?;
        Ok(id)
    }

    /// Add a variable with a given id (when loading a file).
    pub fn insert_variable(
        &mut self,
        id: VariableId,
        variable: Variable,
    ) -> Result<(), DocumentError> {
        self.check_name(&variable.name, None)?;
        self.reserve_param(id.0);
        self.touch_key(Key::Variable(id));
        self.variables.insert(id, variable);
        Ok(())
    }

    pub fn set_variable_expr(&mut self, id: VariableId, expr: &str) -> Result<(), DocumentError> {
        let v = self
            .variables
            .get(&id)
            .ok_or(DocumentError::MissingVariable(id))?;
        if v.expr != expr {
            self.touch_key(Key::Variable(id));
            if let Some(v) = self.variables.get_mut(&id) {
                v.expr = expr.to_owned();
            }
        }
        Ok(())
    }

    pub fn set_variable_kind(&mut self, id: VariableId, kind: Kind) -> Result<(), DocumentError> {
        let v = self
            .variables
            .get(&id)
            .ok_or(DocumentError::MissingVariable(id))?;
        if v.kind != kind {
            self.touch_key(Key::Variable(id));
            if let Some(v) = self.variables.get_mut(&id) {
                v.kind = kind;
            }
        }
        Ok(())
    }

    /// Rename a variable, updating every expression that uses it.
    pub fn rename_variable(&mut self, id: VariableId, name: &str) -> Result<(), DocumentError> {
        let old = self
            .variables
            .get(&id)
            .ok_or(DocumentError::MissingVariable(id))?
            .name
            .clone();
        if old == name {
            return Ok(());
        }
        self.check_name(name, Some(id))?;
        let rename = |text: &str| crate::expr::rename(text, &old, name);
        let vars: Vec<_> = self.variables.keys().copied().collect();
        for v in vars {
            let mut var = self.variables[&v].clone();
            let expr = rename(&var.expr);
            if v == id || expr != var.expr {
                if v == id {
                    var.name = name.to_owned();
                }
                var.expr = expr;
                self.touch_key(Key::Variable(v));
                self.variables.insert(v, var);
            }
        }
        let exprs: Vec<_> = self
            .expressions
            .iter()
            .map(|(c, e)| (*c, rename(e)))
            .filter(|(c, e)| self.expressions[c] != *e)
            .collect();
        for (c, e) in exprs {
            self.touch_key(Key::Expression(c));
            self.expressions.insert(c, e);
        }
        // Walls' thickness expressions.
        let walls: Vec<(WallId, Wall)> = self
            .walls
            .iter()
            .filter_map(|(id, w)| {
                let e = w.thickness_expr.as_deref()?;
                let renamed = rename(e);
                (renamed != e).then(|| {
                    let mut w = w.clone();
                    w.thickness_expr = Some(renamed);
                    (*id, w)
                })
            })
            .collect();
        for (id, w) in walls {
            self.touch_key(Key::Wall(id));
            self.walls.insert(id, w);
        }
        // Openings' offset and width expressions.
        let openings: Vec<(OpeningId, Opening)> = self
            .openings
            .iter()
            .filter_map(|(id, o)| {
                let mut o2 = o.clone();
                if let Some(e) = &mut o2.width_expr {
                    *e = rename(e);
                }
                if let crate::OpeningAt::From { expr: Some(e), .. } = &mut o2.at {
                    *e = rename(e);
                }
                (o2 != *o).then_some((*id, o2))
            })
            .collect();
        for (id, o) in openings {
            self.touch_key(Key::Opening(id));
            self.openings.insert(id, o);
        }
        let variants: Vec<_> = self.variants.keys().copied().collect();
        for k in variants {
            let mut variant = self.variants[&k].clone();
            for e in variant.values.values_mut() {
                *e = rename(e);
            }
            if variant != self.variants[&k] {
                self.touch_key(Key::Variant(k));
                self.variants.insert(k, variant);
            }
        }
        Ok(())
    }

    /// What uses variable `id`: names of other variables and ids of dimensions.
    pub fn users_of_variable(&self, id: VariableId) -> Vec<String> {
        let Some(name) = self.variables.get(&id).map(|v| v.name.as_str()) else {
            return Vec::new();
        };
        let uses = |text: &str| {
            crate::expr::parse(text).is_ok_and(|e| e.identifiers().iter().any(|i| i == name))
        };
        let mut users: Vec<String> = self
            .variables
            .iter()
            .filter(|(v, var)| **v != id && uses(&var.expr))
            .map(|(_, var)| var.name.clone())
            .collect();
        users.extend(
            self.expressions
                .iter()
                .filter(|(_, e)| uses(e))
                .map(|(c, _)| format!("dimension {c}")),
        );
        users.extend(
            self.walls
                .iter()
                .filter(|(_, w)| w.thickness_expr.as_deref().is_some_and(uses))
                .map(|(w, _)| format!("wall {w}")),
        );
        users.extend(
            self.openings
                .iter()
                .filter(|(_, o)| {
                    o.width_expr.as_deref().is_some_and(uses)
                        || matches!(&o.at, crate::OpeningAt::From { expr: Some(e), .. } if uses(e))
                })
                .map(|(o, _)| format!("opening {o}")),
        );
        users
    }

    /// Remove a variable that nothing uses (variant values for it go too).
    pub fn remove_variable(&mut self, id: VariableId) -> Result<(), DocumentError> {
        if !self.variables.contains_key(&id) {
            return Err(DocumentError::MissingVariable(id));
        }
        let users = self.users_of_variable(id);
        if !users.is_empty() {
            return Err(DocumentError::InUse(users.join(", ")));
        }
        self.touch_key(Key::Variable(id));
        self.variables.remove(&id);
        let variants: Vec<_> = self
            .variants
            .iter()
            .filter(|(_, v)| v.values.contains_key(&id))
            .map(|(k, _)| *k)
            .collect();
        for k in variants {
            self.touch_key(Key::Variant(k));
            if let Some(v) = self.variants.get_mut(&k) {
                v.values.remove(&id);
            }
        }
        Ok(())
    }

    /// The expression a dimension's value is linked to, if any.
    pub fn expression(&self, id: ConstraintId) -> Option<&str> {
        self.expressions.get(&id).map(String::as_str)
    }

    /// All dimension expressions.
    pub fn expressions(&self) -> impl Iterator<Item = (ConstraintId, &str)> {
        self.expressions.iter().map(|(id, e)| (*id, e.as_str()))
    }

    /// Link a dimension to an expression, or unlink it (`None`).
    pub fn set_expression(&mut self, id: ConstraintId, expr: Option<String>) {
        if self.expressions.get(&id) != expr.as_ref() {
            self.touch_key(Key::Expression(id));
            match expr {
                Some(e) => {
                    self.expressions.insert(id, e);
                }
                None => {
                    self.expressions.remove(&id);
                }
            }
        }
    }

    pub fn variants(&self) -> impl Iterator<Item = (VariantId, &Variant)> {
        self.variants.iter().map(|(id, v)| (*id, v))
    }

    pub fn variant(&self, id: VariantId) -> Option<&Variant> {
        self.variants.get(&id)
    }

    pub fn add_variant(&mut self, variant: Variant) -> VariantId {
        let id = VariantId(self.next_param());
        self.touch_key(Key::Variant(id));
        self.variants.insert(id, variant);
        id
    }

    /// Add or replace a variant with a given id.
    pub fn set_variant(&mut self, id: VariantId, variant: Variant) {
        self.reserve_param(id.0);
        if self.variants.get(&id) != Some(&variant) {
            self.touch_key(Key::Variant(id));
            self.variants.insert(id, variant);
        }
    }

    pub fn remove_variant(&mut self, id: VariantId) {
        if self.variants.contains_key(&id) {
            self.touch_key(Key::Variant(id));
            self.variants.remove(&id);
        }
    }

    pub fn measurements(&self) -> impl Iterator<Item = (MeasurementId, &Constraint)> {
        self.measurements.iter().map(|(id, c)| (*id, c))
    }

    /// Pin a measurement (a dimension definition that is only measured).
    pub fn add_measurement(&mut self, c: Constraint) -> Result<MeasurementId, DocumentError> {
        let id = MeasurementId(self.next_param_id);
        self.insert_measurement(id, c)?;
        Ok(id)
    }

    pub fn insert_measurement(
        &mut self,
        id: MeasurementId,
        c: Constraint,
    ) -> Result<(), DocumentError> {
        if !c.is_dimension() {
            return Err(DocumentError::InvalidValue);
        }
        // The value is irrelevant: validate the references only.
        self.validate(&c.with_value(1.0))?;
        self.reserve_param(id.0);
        self.touch_key(Key::Measurement(id));
        self.measurements.insert(id, c);
        Ok(())
    }

    pub fn remove_measurement(&mut self, id: MeasurementId) {
        if self.measurements.contains_key(&id) {
            self.touch_key(Key::Measurement(id));
            self.measurements.remove(&id);
        }
    }
}

/// The kind of value a dimension holds.
impl Document {
    // ----- Drafting: fills, annotations, dimension notes, drafting style ---------------------

    fn next_drafting(&mut self) -> u32 {
        let id = self.next_drafting_id;
        self.next_drafting_id += 1;
        id
    }

    pub fn fills(&self) -> impl Iterator<Item = (FillId, &Fill)> {
        self.fills.iter().map(|(id, f)| (*id, f))
    }

    pub fn fill(&self, id: FillId) -> Option<&Fill> {
        self.fills.get(&id)
    }

    /// Add a fill on the current layer, on top of the others there.
    pub fn add_fill(&mut self, mut fill: Fill) -> FillId {
        fill.layer = self.current_layer;
        fill.order = self
            .fills
            .values()
            .filter(|f| f.layer == fill.layer)
            .map(|f| f.order + 1)
            .max()
            .unwrap_or(0);
        let id = FillId(self.next_drafting());
        self.touch_key(Key::Fill(id));
        self.fills.insert(id, fill);
        id
    }

    /// Insert a fill with a known id (loading a file). Its layer must exist.
    pub fn insert_fill(&mut self, id: FillId, fill: Fill) -> Result<(), DocumentError> {
        if !self.layers.contains_key(&fill.layer) {
            return Err(DocumentError::MissingLayer(fill.layer));
        }
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Fill(id));
        self.fills.insert(id, fill);
        Ok(())
    }

    pub fn set_fill(&mut self, id: FillId, fill: Fill) {
        if self.fills.get(&id).is_some_and(|f| *f != fill) {
            self.touch_key(Key::Fill(id));
            self.fills.insert(id, fill);
        }
    }

    pub fn remove_fill(&mut self, id: FillId) {
        if self.fills.contains_key(&id) {
            self.touch_key(Key::Fill(id));
            self.fills.remove(&id);
        }
    }

    pub fn annotations(&self) -> impl Iterator<Item = (AnnotationId, &Annotation)> {
        self.annotations.iter().map(|(id, a)| (*id, a))
    }

    pub fn annotation(&self, id: AnnotationId) -> Option<&Annotation> {
        self.annotations.get(&id)
    }

    fn validate_annotation(&self, a: &Annotation) -> Result<(), DocumentError> {
        for e in a.entities() {
            if !self.entities.contains_key(&e) {
                return Err(DocumentError::Missing(e));
            }
        }
        Ok(())
    }

    pub fn add_annotation(&mut self, a: Annotation) -> Result<AnnotationId, DocumentError> {
        self.validate_annotation(&a)?;
        let id = AnnotationId(self.next_drafting());
        self.touch_key(Key::Annotation(id));
        self.annotations.insert(id, a);
        Ok(id)
    }

    /// Insert an annotation with a known id (loading a file).
    pub fn insert_annotation(
        &mut self,
        id: AnnotationId,
        a: Annotation,
    ) -> Result<(), DocumentError> {
        self.validate_annotation(&a)?;
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Annotation(id));
        self.annotations.insert(id, a);
        Ok(())
    }

    pub fn set_annotation(&mut self, id: AnnotationId, a: Annotation) -> Result<(), DocumentError> {
        self.validate_annotation(&a)?;
        if self.annotations.get(&id).is_some_and(|x| *x != a) {
            self.touch_key(Key::Annotation(id));
            self.annotations.insert(id, a);
        }
        Ok(())
    }

    pub fn remove_annotation(&mut self, id: AnnotationId) {
        if self.annotations.contains_key(&id) {
            self.touch_key(Key::Annotation(id));
            self.annotations.remove(&id);
        }
    }

    /// Where an anchor is now, if its geometry exists.
    pub fn anchor_pos(&self, anchor: &Anchor) -> Option<DVec2> {
        match *anchor {
            Anchor::Free(p) => Some(p),
            Anchor::Point(id) => self.ref_point(id),
            Anchor::Curve { curve, t } => match self.shape(curve)? {
                Shape::Line { a, b } => Some(a + (b - a) * t),
                Shape::Circle { center, radius } | Shape::Arc { center, radius, .. } => {
                    Some(center + DVec2::from_angle(t) * radius)
                }
                Shape::Ellipse {
                    center,
                    major,
                    minor,
                    ..
                } => Some(crate::shape::ellipse_point(center, major, minor, t)),
                Shape::Bezier { p } => Some(crate::shape::bezier_point(&p, t)),
                Shape::Point(p) => Some(p),
            },
        }
    }

    /// Remember where notes' leaders point, so a note whose target is deleted can stay put.
    /// Called before deleting geometry.
    pub fn remember_leader_targets(&mut self) {
        let targets: Vec<(AnnotationId, DVec2)> = self
            .annotations
            .iter()
            .filter_map(|(id, a)| match a {
                Annotation::Note { leaders, .. } => leaders
                    .first()
                    .and_then(|l| self.anchor_pos(&l.to))
                    .map(|p| (*id, p)),
                _ => None,
            })
            .collect();
        self.last_anchor_pos = targets.into_iter().collect();
    }

    pub fn dim_note(&self, id: ConstraintId) -> Option<&DimNote> {
        self.dim_notes.get(&id)
    }

    pub fn dim_notes(&self) -> impl Iterator<Item = (ConstraintId, &DimNote)> {
        self.dim_notes.iter().map(|(id, n)| (*id, n))
    }

    /// Where a dimension or annotation is shown (everywhere, unless set otherwise).
    pub fn shown(&self, r: crate::AnnotationRef) -> crate::Shown {
        self.shown.get(&r).copied().unwrap_or_default()
    }

    /// The dimensions and annotations not shown everywhere, that still exist.
    pub fn shown_settings(
        &self,
    ) -> impl Iterator<Item = (crate::AnnotationRef, crate::Shown)> + '_ {
        self.shown
            .iter()
            .filter(|(r, _)| self.annotation_ref_exists(**r))
            .map(|(r, s)| (*r, *s))
    }

    /// Whether the dimension or annotation is there.
    pub fn annotation_ref_exists(&self, r: crate::AnnotationRef) -> bool {
        match r {
            crate::AnnotationRef::Dimension(id) => {
                self.constraints.get(&id).is_some_and(|c| c.is_dimension())
            }
            crate::AnnotationRef::Annotation(id) => self.annotations.contains_key(&id),
        }
    }

    /// Set where a dimension or annotation is shown (the default, everywhere, is not kept).
    pub fn set_shown(&mut self, r: crate::AnnotationRef, shown: crate::Shown) {
        if self.shown(r) == shown || !self.annotation_ref_exists(r) {
            return;
        }
        self.touch_key(Key::Shown(r));
        if shown.is_default() {
            self.shown.remove(&r);
        } else {
            self.shown.insert(r, shown);
        }
    }

    /// The views a dimension or annotation is drawn in, if only some (`None`: every view).
    pub fn only_in(&self, r: crate::AnnotationRef) -> Option<&BTreeSet<ViewId>> {
        self.only_in.get(&r)
    }

    /// The dimensions and annotations drawn in some views only, that still exist.
    pub fn only_in_settings(
        &self,
    ) -> impl Iterator<Item = (crate::AnnotationRef, &BTreeSet<ViewId>)> + '_ {
        self.only_in
            .iter()
            .filter(|(r, _)| self.annotation_ref_exists(**r))
            .map(|(r, v)| (*r, v))
    }

    /// Whether a dimension or annotation is drawn in a view.
    pub fn in_view(&self, r: crate::AnnotationRef, view: ViewId) -> bool {
        self.only_in.get(&r).is_none_or(|v| v.contains(&view))
    }

    /// Draw a dimension or annotation in these views only (empty: in every view).
    pub fn set_only_in(&mut self, r: crate::AnnotationRef, views: BTreeSet<ViewId>) {
        let now = self.only_in.get(&r).cloned().unwrap_or_default();
        if now == views || !self.annotation_ref_exists(r) {
            return;
        }
        self.touch_key(Key::OnlyIn(r));
        if views.is_empty() {
            self.only_in.remove(&r);
        } else {
            self.only_in.insert(r, views);
        }
    }

    /// What a dimension or annotation sets for itself over the drawing's style.
    pub fn style_override(&self, r: crate::AnnotationRef) -> crate::StyleOverride {
        self.styles.get(&r).copied().unwrap_or_default()
    }

    /// The dimensions and annotations with settings of their own, that still exist.
    pub fn style_overrides(
        &self,
    ) -> impl Iterator<Item = (crate::AnnotationRef, crate::StyleOverride)> + '_ {
        self.styles
            .iter()
            .filter(|(r, _)| self.annotation_ref_exists(**r))
            .map(|(r, s)| (*r, *s))
    }

    /// Set what a dimension or annotation overrides (nothing overridden is not kept).
    pub fn set_style_override(&mut self, r: crate::AnnotationRef, style: crate::StyleOverride) {
        if self.style_override(r) == style || !self.annotation_ref_exists(r) {
            return;
        }
        self.touch_key(Key::Style(r));
        if style.is_empty() {
            self.styles.remove(&r);
        } else {
            self.styles.insert(r, style);
        }
    }

    /// The style a dimension or annotation is drawn with: the drawing's, with its own
    /// settings in place.
    pub fn draft_style_for(&self, r: crate::AnnotationRef) -> DraftStyle {
        match self.styles.get(&r) {
            Some(o) => o.apply(&self.draft),
            None => self.draft,
        }
    }

    /// Set (or clear, with `None` or an empty note) a dimension's prefix, suffix and tolerance.
    pub fn set_dim_note(&mut self, id: ConstraintId, note: Option<DimNote>) {
        let note = note.filter(|n| !n.is_empty());
        if self.dim_notes.get(&id) != note.as_ref() {
            if note.is_some() && !self.constraints.contains_key(&id) {
                return;
            }
            self.touch_key(Key::DimNote(id));
            match note {
                Some(n) => {
                    self.dim_notes.insert(id, n);
                }
                None => {
                    self.dim_notes.remove(&id);
                }
            }
        }
    }

    pub fn blocks(&self) -> impl Iterator<Item = (BlockId, &Block)> {
        self.blocks.iter().map(|(id, b)| (*id, b))
    }

    pub fn block(&self, id: BlockId) -> Option<&Block> {
        self.blocks.get(&id)
    }

    pub fn block_named(&self, name: &str) -> Option<BlockId> {
        self.blocks
            .iter()
            .find(|(_, b)| b.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| *id)
    }

    pub fn add_block(&mut self, block: Block) -> BlockId {
        let id = BlockId(self.next_drafting());
        self.touch_key(Key::Block(id));
        self.blocks.insert(id, block);
        id
    }

    /// Insert a block with a known id (loading a file).
    pub fn insert_block(&mut self, id: BlockId, block: Block) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Block(id));
        self.blocks.insert(id, block);
    }

    /// Change a block's definition: every copy updates.
    pub fn set_block(&mut self, id: BlockId, block: Block) {
        if self.blocks.get(&id).is_some_and(|b| *b != block) {
            self.touch_key(Key::Block(id));
            self.blocks.insert(id, block);
        }
    }

    /// Delete a block and every copy of it.
    pub fn remove_block(&mut self, id: BlockId) {
        let copies: Vec<InstanceId> = self
            .instances
            .iter()
            .filter(|(_, i)| i.block == id)
            .map(|(iid, _)| *iid)
            .collect();
        for i in copies {
            self.remove_instance(i);
        }
        if self.blocks.contains_key(&id) {
            self.touch_key(Key::Block(id));
            self.blocks.remove(&id);
        }
    }

    pub fn instances(&self) -> impl Iterator<Item = (InstanceId, &Instance)> {
        self.instances.iter().map(|(id, i)| (*id, i))
    }

    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.instances.get(&id)
    }

    fn check_instance(&self, i: &Instance) -> Result<(), DocumentError> {
        if !self.blocks.contains_key(&i.block) {
            return Err(DocumentError::BadName(format!(
                "block {} does not exist",
                i.block
            )));
        }
        if !self.layers.contains_key(&i.layer) {
            return Err(DocumentError::MissingLayer(i.layer));
        }
        if !(i.at.is_finite() && i.angle.is_finite() && i.scale.is_finite() && i.scale > 0.0) {
            return Err(DocumentError::InvalidValue);
        }
        Ok(())
    }

    /// Place a copy of a block (on the current layer).
    pub fn add_instance(&mut self, mut i: Instance) -> Result<InstanceId, DocumentError> {
        i.layer = self.current_layer;
        self.check_instance(&i)?;
        let id = InstanceId(self.next_drafting());
        self.touch_key(Key::Instance(id));
        self.instances.insert(id, i);
        Ok(id)
    }

    /// Insert a copy with a known id (loading a file).
    pub fn insert_instance(&mut self, id: InstanceId, i: Instance) -> Result<(), DocumentError> {
        self.check_instance(&i)?;
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Instance(id));
        self.instances.insert(id, i);
        Ok(())
    }

    pub fn set_instance(&mut self, id: InstanceId, i: Instance) -> Result<(), DocumentError> {
        self.check_instance(&i)?;
        if self.instances.get(&id).is_some_and(|x| *x != i) {
            self.touch_key(Key::Instance(id));
            self.instances.insert(id, i);
        }
        Ok(())
    }

    pub fn remove_instance(&mut self, id: InstanceId) {
        if self.instances.contains_key(&id) {
            self.touch_key(Key::Instance(id));
            self.instances.remove(&id);
        }
    }

    /// A copy's curves in the world, with their style and look.
    pub fn instance_items(&self, id: InstanceId) -> Vec<BlockItem> {
        let (Some(i), Some(b)) = (
            self.instances.get(&id),
            self.instances
                .get(&id)
                .and_then(|i| self.blocks.get(&i.block)),
        ) else {
            return Vec::new();
        };
        b.items
            .iter()
            .map(|item| BlockItem {
                shape: i.apply_shape(&item.shape),
                ..*item
            })
            .collect()
    }

    pub fn groups(&self) -> impl Iterator<Item = (GroupId, &Group)> {
        self.groups.iter().map(|(id, g)| (*id, g))
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.get(&id)
    }

    /// The group an entity belongs to.
    pub fn group_of(&self, entity: EntityId) -> Option<GroupId> {
        self.groups
            .iter()
            .find(|(_, g)| g.members.contains(&entity))
            .map(|(id, _)| *id)
    }

    /// Take `members` out of any group they are in (dropping groups left empty).
    fn ungroup_members(&mut self, members: &BTreeSet<EntityId>) {
        let touched: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(_, g)| !g.members.is_disjoint(members))
            .map(|(id, _)| *id)
            .collect();
        for id in touched {
            self.touch_key(Key::Group(id));
            if let Some(g) = self.groups.get_mut(&id) {
                g.members.retain(|e| !members.contains(e));
                if g.members.is_empty() {
                    self.groups.remove(&id);
                }
            }
        }
    }

    /// Make a group. Its members leave any group they were in.
    pub fn add_group(&mut self, group: Group) -> Result<GroupId, DocumentError> {
        if let Some(e) = group
            .members
            .iter()
            .find(|e| !self.entities.contains_key(e))
        {
            return Err(DocumentError::Missing(*e));
        }
        self.ungroup_members(&group.members);
        let id = GroupId(self.next_drafting());
        self.touch_key(Key::Group(id));
        self.groups.insert(id, group);
        Ok(id)
    }

    /// Insert a group with a known id (loading a file).
    pub fn insert_group(&mut self, id: GroupId, group: Group) -> Result<(), DocumentError> {
        if let Some(e) = group
            .members
            .iter()
            .find(|e| !self.entities.contains_key(e))
        {
            return Err(DocumentError::Missing(*e));
        }
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Group(id));
        self.groups.insert(id, group);
        Ok(())
    }

    /// Add entities to a group (they leave any other group).
    pub fn add_group_members(
        &mut self,
        id: GroupId,
        members: impl IntoIterator<Item = EntityId>,
    ) -> Result<(), DocumentError> {
        let members: BTreeSet<EntityId> = members.into_iter().collect();
        if let Some(e) = members.iter().find(|e| !self.entities.contains_key(e)) {
            return Err(DocumentError::Missing(*e));
        }
        if !self.groups.contains_key(&id) || members.is_empty() {
            return Ok(());
        }
        let others: BTreeSet<EntityId> = members
            .iter()
            .copied()
            .filter(|e| self.group_of(*e).is_some_and(|g| g != id))
            .collect();
        self.ungroup_members(&others);
        self.touch_key(Key::Group(id));
        if let Some(g) = self.groups.get_mut(&id) {
            g.members.extend(members);
        }
        Ok(())
    }

    pub fn rename_group(&mut self, id: GroupId, name: &str) {
        if let Some(g) = self.groups.get(&id)
            && g.name != name
        {
            self.touch_key(Key::Group(id));
            if let Some(g) = self.groups.get_mut(&id) {
                g.name = name.to_owned();
            }
        }
    }

    /// Dissolve a group (its members stay).
    pub fn remove_group(&mut self, id: GroupId) {
        if self.groups.contains_key(&id) {
            self.touch_key(Key::Group(id));
            self.groups.remove(&id);
        }
    }

    pub fn paths(&self) -> impl Iterator<Item = (PathId, &Path)> {
        self.paths.iter().map(|(id, p)| (*id, p))
    }

    pub fn path(&self, id: PathId) -> Option<&Path> {
        self.paths.get(&id)
    }

    /// The path a curve belongs to.
    pub fn path_of(&self, curve: EntityId) -> Option<PathId> {
        self.paths
            .iter()
            .find(|(_, p)| p.curves.contains(&curve))
            .map(|(id, _)| *id)
    }

    /// Take `curves` out of any path they are in (a path left with none goes, and its fill
    /// is unbound).
    fn unpath_curves(&mut self, curves: &[EntityId]) {
        let touched: Vec<PathId> = self
            .paths
            .iter()
            .filter(|(_, p)| p.curves.iter().any(|c| curves.contains(c)))
            .map(|(id, _)| *id)
            .collect();
        for id in touched {
            self.touch_key(Key::Path(id));
            let empty = match self.paths.get_mut(&id) {
                Some(p) => {
                    p.curves.retain(|c| !curves.contains(c));
                    p.curves.is_empty()
                }
                None => false,
            };
            if empty {
                self.paths.remove(&id);
                self.unbind_fills(id);
            }
        }
    }

    fn check_path(&self, path: &Path) -> Result<(), DocumentError> {
        if path.curves.is_empty() {
            return Err(DocumentError::Degenerate("path"));
        }
        match path
            .curves
            .iter()
            .find(|c| !self.entities.get(c).is_some_and(|e| !e.kind.is_point()))
        {
            Some(c) => Err(DocumentError::Missing(*c)),
            None => Ok(()),
        }
    }

    /// Make a path: curves drawn as one shape, in order. They leave any path they were in.
    pub fn add_path(&mut self, path: Path) -> Result<PathId, DocumentError> {
        self.check_path(&path)?;
        self.unpath_curves(&path.curves);
        let id = PathId(self.next_drafting());
        self.touch_key(Key::Path(id));
        self.paths.insert(id, path);
        Ok(id)
    }

    /// Insert a path with a known id (loading a file).
    pub fn insert_path(&mut self, id: PathId, path: Path) -> Result<(), DocumentError> {
        self.check_path(&path)?;
        self.unpath_curves(&path.curves);
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Path(id));
        self.paths.insert(id, path);
        Ok(())
    }

    /// Replace a path's curves and closure. Curves it takes leave any other path.
    pub fn set_path(&mut self, id: PathId, path: Path) -> Result<(), DocumentError> {
        if !self.paths.contains_key(&id) || self.paths.get(&id) == Some(&path) {
            return Ok(());
        }
        self.check_path(&path)?;
        let taken: Vec<EntityId> = path
            .curves
            .iter()
            .copied()
            .filter(|c| self.path_of(*c).is_some_and(|p| p != id))
            .collect();
        self.unpath_curves(&taken);
        self.touch_key(Key::Path(id));
        self.paths.insert(id, path);
        Ok(())
    }

    /// Remove a path (its curves stay); its fill fills an area among the sketch's curves
    /// again.
    pub fn remove_path(&mut self, id: PathId) {
        if self.paths.contains_key(&id) {
            self.touch_key(Key::Path(id));
            self.paths.remove(&id);
            self.unbind_fills(id);
        }
    }

    // ----- Walls ---------------------------------------------------------------------------

    pub fn walls(&self) -> impl Iterator<Item = (WallId, &Wall)> {
        self.walls.iter().map(|(id, w)| (*id, w))
    }

    pub fn wall(&self, id: WallId) -> Option<&Wall> {
        self.walls.get(&id)
    }

    pub fn has_walls(&self) -> bool {
        !self.walls.is_empty()
    }

    fn check_wall(&self, wall: &Wall) -> Result<(), DocumentError> {
        if !self.paths.contains_key(&wall.centre) {
            return Err(DocumentError::BadName(
                "a wall's centreline must be a path".into(),
            ));
        }
        if self.layer(wall.layer).is_none() {
            return Err(DocumentError::MissingLayer(wall.layer));
        }
        if !(wall.thickness > 0.0 && wall.thickness.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        Ok(())
    }

    fn next_wall(&mut self) -> u32 {
        let id = self.next_wall_id;
        self.next_wall_id += 1;
        id
    }

    /// Add a wall along an existing path. Its faces are made by the walls' regeneration
    /// (`drift_geom::walls`), not here.
    pub fn add_wall(&mut self, wall: Wall) -> Result<WallId, DocumentError> {
        self.check_wall(&wall)?;
        let id = WallId(self.next_wall());
        self.touch_key(Key::Wall(id));
        self.walls.insert(id, wall);
        Ok(id)
    }

    /// Insert a wall with a known id (loading a file).
    pub fn insert_wall(&mut self, id: WallId, wall: Wall) -> Result<(), DocumentError> {
        self.check_wall(&wall)?;
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::Wall(id));
        self.walls.insert(id, wall);
        Ok(())
    }

    /// Replace a wall's record (its settings, joins or what it made).
    pub fn set_wall(&mut self, id: WallId, wall: Wall) -> Result<(), DocumentError> {
        if !self.walls.contains_key(&id) || self.walls.get(&id) == Some(&wall) {
            return Ok(());
        }
        self.check_wall(&wall)?;
        self.touch_key(Key::Wall(id));
        self.walls.insert(id, wall);
        Ok(())
    }

    /// Remove a wall's record, and its property dimensions. What it made stays, as plain
    /// geometry (Explode), unless the caller deletes it.
    pub fn remove_wall(&mut self, id: WallId) {
        if self.walls.contains_key(&id) {
            self.touch_key(Key::Wall(id));
            self.walls.remove(&id);
        }
        let dims: Vec<PropertyDimId> = self
            .property_dims
            .iter()
            .filter(|(_, d)| d.target.wall() == id)
            .map(|(d, _)| *d)
            .collect();
        for d in dims {
            self.remove_property_dim(d);
        }
    }

    /// The wall whose centreline is `path`.
    pub fn wall_of_path(&self, path: PathId) -> Option<WallId> {
        self.walls
            .iter()
            .find(|(_, w)| w.centre == path)
            .map(|(id, _)| *id)
    }

    /// Who owns which entity, path and fill among the walls' (built afresh: callers that ask
    /// often cache it by revision).
    pub fn wall_owners(&self) -> Owners {
        let mut out = Owners::default();
        for (&id, w) in self.walls.iter() {
            for e in w.made.entities() {
                out.of.insert(e, Owner::Wall(id));
            }
            for p in w.made.outlines.values() {
                out.paths.insert(*p, id);
            }
            for f in w.made.fills.values() {
                out.fills.insert(*f, id);
            }
            out.centres.insert(w.centre, id);
            if let Some(path) = self.paths.get(&w.centre) {
                for c in &path.curves {
                    out.of.insert(*c, Owner::Centre(id));
                    if let Some(e) = self.entities.get(c) {
                        for p in e.kind.points() {
                            out.of.entry(p).or_insert(Owner::Centre(id));
                        }
                    }
                }
            }
        }
        for (&id, o) in self.openings.iter() {
            for e in o.made.entities() {
                out.of.insert(e, Owner::Opening(id, o.wall));
            }
            // Its jambs (the wall's edges across its cut) pick it too.
            if let Some(w) = self.walls.get(&o.wall) {
                for (k, e) in &w.made.curves {
                    if k.part == crate::Part::Jamb
                        && matches!(k.from, crate::Station::Jamb(x, _) if x == id)
                    {
                        out.of.insert(*e, Owner::Opening(id, o.wall));
                    }
                }
            }
        }
        out
    }

    /// The entities walls (and the openings in them) generated: never solver unknowns.
    pub fn generated_entities(&self) -> std::collections::HashSet<EntityId> {
        self.walls
            .values()
            .flat_map(|w| w.made.entities())
            .chain(self.openings.values().flat_map(|o| o.made.entities()))
            .collect()
    }

    // ----- Openings ------------------------------------------------------------------------

    pub fn openings(&self) -> impl Iterator<Item = (OpeningId, &Opening)> {
        self.openings.iter().map(|(id, o)| (*id, o))
    }

    pub fn opening(&self, id: OpeningId) -> Option<&Opening> {
        self.openings.get(&id)
    }

    pub fn has_openings(&self) -> bool {
        !self.openings.is_empty()
    }

    /// The openings in wall `w`.
    pub fn openings_in(&self, w: WallId) -> impl Iterator<Item = (OpeningId, &Opening)> {
        self.openings().filter(move |(_, o)| o.wall == w)
    }

    fn check_opening(&self, o: &Opening) -> Result<(), DocumentError> {
        if !self.walls.contains_key(&o.wall) {
            return Err(DocumentError::Degenerate("opening without its wall"));
        }
        if !(o.width > 0.0 && o.width.is_finite() && o.leaf.is_finite() && o.height.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        if let crate::OpeningAt::From { offset, .. } = o.at
            && !offset.is_finite()
        {
            return Err(DocumentError::InvalidValue);
        }
        if let Some(l) = o.layer
            && self.layer(l).is_none()
        {
            return Err(DocumentError::MissingLayer(l));
        }
        Ok(())
    }

    /// Add an opening to a wall. Its cut and symbol are made by the walls' regeneration.
    pub fn add_opening(&mut self, o: Opening) -> Result<OpeningId, DocumentError> {
        self.check_opening(&o)?;
        let id = OpeningId(self.next_wall());
        self.touch_key(Key::Opening(id));
        self.openings.insert(id, o);
        Ok(id)
    }

    /// Insert an opening with a known id (loading a file).
    pub fn insert_opening(&mut self, id: OpeningId, o: Opening) -> Result<(), DocumentError> {
        self.check_opening(&o)?;
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::Opening(id));
        self.openings.insert(id, o);
        Ok(())
    }

    /// Replace an opening's record (where it is, its size and swing, or what it made).
    pub fn set_opening(&mut self, id: OpeningId, o: Opening) -> Result<(), DocumentError> {
        if !self.openings.contains_key(&id) || self.openings.get(&id) == Some(&o) {
            return Ok(());
        }
        self.check_opening(&o)?;
        self.touch_key(Key::Opening(id));
        self.openings.insert(id, o);
        Ok(())
    }

    /// Remove an opening's record. What its symbol made stays, as plain geometry (callers
    /// delete it, or regenerating does).
    pub fn remove_opening(&mut self, id: OpeningId) {
        if self.openings.contains_key(&id) {
            self.touch_key(Key::Opening(id));
            self.openings.remove(&id);
        }
    }

    pub fn property_dims(&self) -> impl Iterator<Item = (PropertyDimId, &PropertyDim)> {
        self.property_dims.iter().map(|(id, d)| (*id, d))
    }

    pub fn property_dim(&self, id: PropertyDimId) -> Option<&PropertyDim> {
        self.property_dims.get(&id)
    }

    pub fn add_property_dim(&mut self, dim: PropertyDim) -> Result<PropertyDimId, DocumentError> {
        let id = PropertyDimId(self.next_wall());
        self.insert_property_dim(id, dim)?;
        Ok(id)
    }

    /// Insert a property dimension with a known id (loading a file).
    pub fn insert_property_dim(
        &mut self,
        id: PropertyDimId,
        dim: PropertyDim,
    ) -> Result<(), DocumentError> {
        if !self.walls.contains_key(&dim.target.wall()) {
            return Err(DocumentError::BadName(
                "a property dimension needs its wall".into(),
            ));
        }
        if !(dim.along.is_finite() && dim.out.is_finite()) {
            return Err(DocumentError::InvalidValue);
        }
        self.next_wall_id = self.next_wall_id.max(id.0 + 1);
        self.touch_key(Key::PropertyDim(id));
        self.property_dims.insert(id, dim);
        Ok(())
    }

    pub fn set_property_dim(&mut self, id: PropertyDimId, dim: PropertyDim) {
        if self.property_dims.contains_key(&id) && self.property_dims.get(&id) != Some(&dim) {
            self.touch_key(Key::PropertyDim(id));
            self.property_dims.insert(id, dim);
        }
    }

    pub fn remove_property_dim(&mut self, id: PropertyDimId) {
        if self.property_dims.contains_key(&id) {
            self.touch_key(Key::PropertyDim(id));
            self.property_dims.remove(&id);
        }
    }

    pub fn sheets(&self) -> impl Iterator<Item = (SheetId, &Sheet)> {
        self.sheets.iter().map(|(id, s)| (*id, s))
    }

    pub fn sheet(&self, id: SheetId) -> Option<&Sheet> {
        self.sheets.get(&id)
    }

    pub fn add_sheet(&mut self, sheet: Sheet) -> SheetId {
        let id = SheetId(self.next_drafting());
        self.touch_key(Key::Sheet(id));
        self.sheets.insert(id, sheet);
        id
    }

    /// Insert a sheet with a known id (loading a file).
    pub fn insert_sheet(&mut self, id: SheetId, sheet: Sheet) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Sheet(id));
        self.sheets.insert(id, sheet);
    }

    pub fn set_sheet(&mut self, id: SheetId, sheet: Sheet) {
        if self.sheets.get(&id).is_some_and(|s| *s != sheet) {
            self.touch_key(Key::Sheet(id));
            self.sheets.insert(id, sheet);
        }
    }

    pub fn remove_sheet(&mut self, id: SheetId) {
        if self.sheets.contains_key(&id) {
            self.touch_key(Key::Sheet(id));
            self.sheets.remove(&id);
        }
    }

    /// Move a sheet to position `to` among the sheets, the others closing up. Sheets are in
    /// the order of their ids, so the sheets move between ids rather than the ids moving:
    /// the result says where each sheet went (its old id → its new one).
    pub fn move_sheet(&mut self, id: SheetId, to: usize) -> BTreeMap<SheetId, SheetId> {
        let ids: Vec<SheetId> = self.sheets.keys().copied().collect();
        let Some(from) = ids.iter().position(|s| *s == id) else {
            return BTreeMap::new();
        };
        let mut order = ids.clone();
        let moved = order.remove(from);
        order.insert(to.min(order.len()), moved);
        let before = self.sheets.clone();
        let mut went = BTreeMap::new();
        for (old, new) in order.into_iter().zip(ids) {
            went.insert(old, new);
            self.set_sheet(new, before[&old].clone());
        }
        went
    }

    pub fn views(&self) -> impl Iterator<Item = (ViewId, &View)> {
        self.views.iter().map(|(id, v)| (*id, v))
    }

    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.get(&id)
    }

    pub fn add_view(&mut self, view: View) -> ViewId {
        let id = ViewId(self.next_drafting());
        self.touch_key(Key::View(id));
        self.views.insert(id, view);
        id
    }

    /// Insert a view with a known id (loading a file).
    pub fn insert_view(&mut self, id: ViewId, view: View) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::View(id));
        self.views.insert(id, view);
    }

    pub fn set_view(&mut self, id: ViewId, view: View) {
        if self.views.get(&id).is_some_and(|v| *v != view) {
            self.touch_key(Key::View(id));
            self.views.insert(id, view);
        }
    }

    /// Delete a view, take it off every sheet it is on, and unlink the marks that point at it.
    pub fn remove_view(&mut self, id: ViewId) {
        if !self.views.contains_key(&id) {
            return;
        }
        self.touch_key(Key::View(id));
        self.views.remove(&id);
        let showing: Vec<SheetId> = self.sheets_showing(id);
        for s in showing {
            let mut sheet = self.sheets[&s].clone();
            sheet.viewports.retain(|p| p.view != id);
            self.set_sheet(s, sheet);
        }
        for mark in self.marks_of_view(id) {
            self.touch_key(Key::Annotation(mark));
            if let Some(a) = self.annotations.get_mut(&mark) {
                a.set_view(None);
            }
        }
        // What was drawn in it alone stays off sheets, rather than showing in every view.
        let only: Vec<crate::AnnotationRef> = self
            .only_in
            .iter()
            .filter(|(_, v)| v.contains(&id))
            .map(|(r, _)| *r)
            .collect();
        for r in only {
            let mut views = self.only_in[&r].clone();
            views.remove(&id);
            if views.is_empty() {
                let mut shown = self.shown(r);
                shown.sheets = false;
                self.set_shown(r, shown);
            }
            self.touch_key(Key::OnlyIn(r));
            if views.is_empty() {
                self.only_in.remove(&r);
            } else {
                self.only_in.insert(r, views);
            }
        }
    }

    /// The section and elevation marks that point at a view.
    pub fn marks_of_view(&self, id: ViewId) -> Vec<AnnotationId> {
        self.annotations
            .iter()
            .filter(|(_, a)| a.view() == Some(id))
            .map(|(aid, _)| *aid)
            .collect()
    }

    /// How a mark linked to a view refers to it: the view's number (its place among the
    /// viewports of the first sheet showing it, from 1) and that sheet's number. `None` while
    /// the view is on no sheet.
    pub fn view_reference(&self, id: ViewId) -> Option<(String, String)> {
        let sheet = &self.sheets[self.sheets_showing(id).first()?];
        let n = sheet.viewports.iter().position(|p| p.view == id)? + 1;
        let number = match sheet.fields.number.trim() {
            "" => "–".to_owned(),
            s => s.to_owned(),
        };
        Some((n.to_string(), number))
    }

    /// The sheets a view is placed on, in order.
    pub fn sheets_showing(&self, id: ViewId) -> Vec<SheetId> {
        self.sheets
            .iter()
            .filter(|(_, s)| s.viewports.iter().any(|p| p.view == id))
            .map(|(sid, _)| *sid)
            .collect()
    }

    /// A sheet's viewports with their views, in drawing order (one whose view is missing is
    /// left out).
    pub fn placed_views(&self, sheet: &Sheet) -> Vec<PlacedView> {
        sheet
            .viewports
            .iter()
            .filter_map(|p| Some(PlacedView::new(p.view, self.views.get(&p.view)?, p)))
            .collect()
    }

    /// A sheet's title block scale: its views' scale if they share one, else `As shown`.
    pub fn sheet_scale_text(&self, sheet: &Sheet) -> String {
        crate::sheets::scale_text(
            sheet
                .viewports
                .iter()
                .filter_map(|p| self.views.get(&p.view).map(|v| v.scale)),
        )
    }

    /// Write back viewport `index` of sheet `id` (and its view, which every sheet showing
    /// it follows).
    pub fn set_placed_view(&mut self, id: SheetId, index: usize, placed: &PlacedView) {
        let Some(mut sheet) = self.sheets.get(&id).cloned() else {
            return;
        };
        let Some(port) = sheet.viewports.get_mut(index) else {
            return;
        };
        let Some(mut view) = self.views.get(&port.view).cloned() else {
            return;
        };
        let view_id = port.view;
        placed.apply(&mut view, port);
        self.set_view(view_id, view);
        self.set_sheet(id, sheet);
    }

    pub fn texts(&self) -> impl Iterator<Item = (TextId, &SketchText)> {
        self.texts.iter().map(|(id, t)| (*id, t))
    }

    pub fn text(&self, id: TextId) -> Option<&SketchText> {
        self.texts.get(&id)
    }

    /// Add a text on the current layer.
    pub fn add_text(&mut self, mut text: SketchText) -> TextId {
        text.layer = self.current_layer;
        let id = TextId(self.next_drafting());
        self.touch_key(Key::Text(id));
        self.texts.insert(id, text);
        id
    }

    /// Insert a text with a known id (loading a file).
    pub fn insert_text(&mut self, id: TextId, text: SketchText) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Text(id));
        self.texts.insert(id, text);
    }

    pub fn set_text(&mut self, id: TextId, text: SketchText) {
        if self.texts.get(&id).is_some_and(|t| *t != text) {
            self.touch_key(Key::Text(id));
            self.texts.insert(id, text);
        }
    }

    pub fn remove_text(&mut self, id: TextId) {
        if self.texts.contains_key(&id) {
            self.touch_key(Key::Text(id));
            self.texts.remove(&id);
        }
    }

    pub fn underlays(&self) -> impl Iterator<Item = (UnderlayId, &Underlay)> {
        self.underlays.iter().map(|(id, u)| (*id, u))
    }

    pub fn underlay(&self, id: UnderlayId) -> Option<&Underlay> {
        self.underlays.get(&id)
    }

    pub fn add_underlay(&mut self, underlay: Underlay) -> UnderlayId {
        let id = UnderlayId(self.next_drafting());
        self.touch_key(Key::Underlay(id));
        self.underlays.insert(id, underlay);
        id
    }

    /// Insert an underlay with a known id (loading a file).
    pub fn insert_underlay(&mut self, id: UnderlayId, underlay: Underlay) {
        self.next_drafting_id = self.next_drafting_id.max(id.0 + 1);
        self.touch_key(Key::Underlay(id));
        self.underlays.insert(id, underlay);
    }

    pub fn set_underlay(&mut self, id: UnderlayId, underlay: Underlay) {
        if self.underlays.get(&id).is_some_and(|u| *u != underlay) {
            self.touch_key(Key::Underlay(id));
            self.underlays.insert(id, underlay);
        }
    }

    pub fn remove_underlay(&mut self, id: UnderlayId) {
        if self.underlays.contains_key(&id) {
            self.touch_key(Key::Underlay(id));
            self.underlays.remove(&id);
        }
    }

    /// Imported hatch patterns, in `.pat` format.
    pub fn custom_patterns(&self) -> &str {
        &self.patterns
    }

    pub fn set_custom_patterns(&mut self, pat: String) {
        if *self.patterns != pat {
            self.touch_key(Key::Patterns);
            self.patterns = pat.into();
        }
    }

    /// The logo drawn in the sheets' title blocks, as SVG text ("" for none).
    pub fn logo(&self) -> &str {
        &self.logo
    }

    pub fn set_logo(&mut self, svg: String) {
        if *self.logo != svg {
            self.touch_key(Key::Logo);
            self.logo = svg.into();
        }
    }

    /// How the drawing's closed areas and fills behave (saved with it).
    pub fn rules(&self) -> crate::DrawingRules {
        self.rules
    }

    pub fn set_rules(&mut self, rules: crate::DrawingRules) {
        if self.rules != rules {
            self.touch_key(Key::Rules);
            self.rules = rules;
        }
    }

    /// The project the sheets are for: their shared title block fields.
    pub fn project(&self) -> &crate::Project {
        &self.project
    }

    pub fn set_project(&mut self, project: crate::Project) {
        if self.project != project {
            self.touch_key(Key::Project);
            self.project = project;
        }
    }

    /// The fonts kept in the drawing, and the one its annotations are set in.
    pub fn fonts(&self) -> &crate::fonts::FontSet {
        &self.fonts
    }

    pub fn set_fonts(&mut self, fonts: crate::fonts::FontSet) {
        if *self.fonts != fonts {
            self.touch_key(Key::Fonts);
            self.fonts = fonts.into();
        }
    }

    /// The families the drawing uses: its annotations' font, and its texts' system fonts.
    pub fn fonts_in_use(&self) -> BTreeSet<String> {
        let mut used: BTreeSet<String> = self.fonts.drawing.iter().cloned().collect();
        for text in self.texts.values() {
            if let crate::texts::TextFont::System(family) = &text.font {
                used.insert(family.clone());
            }
        }
        used
    }

    pub fn draft_style(&self) -> DraftStyle {
        self.draft
    }

    pub fn set_draft_style(&mut self, style: DraftStyle) {
        if self.draft != style {
            self.touch_key(Key::Draft);
            self.draft = style;
        }
    }
}

pub fn dimension_kind(c: &Constraint) -> Kind {
    match c {
        Constraint::Angle { .. } => Kind::Angle,
        _ => Kind::Length,
    }
}

/// Resolve an arc's points into a [`Shape::Arc`].
pub fn arc_shape(center: DVec2, start: DVec2, end: DVec2) -> Shape {
    let a0 = (start - center).y.atan2((start - center).x);
    let a1 = (end - center).y.atan2((end - center).x);
    let mut sweep = (a1 - a0).rem_euclid(TAU);
    if sweep == 0.0 {
        sweep = TAU;
    }
    Shape::Arc {
        center,
        radius: (start - center).length(),
        start: a0,
        sweep,
    }
}

/// Undo/redo stacks of [`Transaction`]s.
#[derive(Clone, Debug, Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// Steps recorded so far and still steps of their own: unlike the undo stack, which
    /// forgets old ones, this only goes down when steps are squashed together or discarded.
    recorded: u64,
}

impl History {
    /// How many steps can be undone; older ones are forgotten, so a long session's history
    /// can't grow without bound.
    pub const MAX_STEPS: usize = 500;

    /// Record a committed transaction. Empty transactions are dropped.
    pub fn push(&mut self, tx: Transaction) {
        if !tx.is_empty() {
            self.undo.push(tx);
            self.recorded += 1;
            self.redo.clear();
            if self.undo.len() > Self::MAX_STEPS {
                let extra = self.undo.len() - Self::MAX_STEPS;
                self.undo.drain(..extra);
            }
        }
    }

    /// Append a transaction's changes to the most recent undo step (so they undo together).
    /// With nothing to merge into, it is pushed as its own step.
    pub fn merge_into_last(&mut self, tx: Transaction) {
        if tx.is_empty() {
            return;
        }
        // A change made now can't be redone over.
        self.redo.clear();
        match self.undo.last_mut() {
            Some(last) => {
                for (key, before, after) in tx.changes {
                    match last.changes.iter_mut().find(|(k, ..)| *k == key) {
                        Some(existing) => existing.2 = after,
                        None => last.changes.push((key, before, after)),
                    }
                }
            }
            None => {
                self.undo.push(tx);
                self.recorded += 1;
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|t| t.label.as_str())
    }

    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(tx) = self.undo.pop() else {
            return false;
        };
        doc.apply(&tx, false);
        self.redo.push(tx);
        true
    }

    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(tx) = self.redo.pop() else {
            return false;
        };
        doc.apply(&tx, true);
        self.undo.push(tx);
        true
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// How many steps have been recorded: the difference between two readings is how many
    /// steps were added between them (steps squashed into one count as one, so nested
    /// squashes count right).
    pub fn recorded(&self) -> u64 {
        self.recorded
    }

    /// How many steps can be undone.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// How many steps can be redone.
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Fold the last `n` undo steps into one labelled `label`, so several edits undo
    /// together.
    pub fn squash_last(&mut self, n: usize, label: &str) {
        let n = n.min(self.undo.len());
        if n == 0 {
            return;
        }
        let mut steps = self.undo.split_off(self.undo.len() - n);
        let mut first = steps.remove(0);
        first.label = label.to_owned();
        self.undo.push(first);
        for tx in steps {
            self.merge_into_last(tx);
        }
        self.recorded -= (n - 1) as u64;
    }

    /// Undo the last `n` steps and forget them (they can't be redone).
    pub fn discard_last(&mut self, n: usize, doc: &mut Document) {
        for _ in 0..n {
            if let Some(tx) = self.undo.pop() {
                doc.apply(&tx, false);
                self.recorded = self.recorded.saturating_sub(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A unit square drawn as a closed chain sharing its corners.
    fn square(doc: &mut Document) -> ([EntityId; 4], [EntityId; 4]) {
        let p = [v(0.0, 0.0), v(1.0, 0.0), v(1.0, 1.0), v(0.0, 1.0)].map(|at| doc.add_point(at));
        let l = [0, 1, 2, 3].map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
        (p, l)
    }

    #[test]
    fn views_are_shared_by_sheets_and_follow_their_crop() {
        use crate::sheets::{View, Viewport};
        let mut doc = Document::new();
        let mut history = History::default();
        let crop = crate::Bounds {
            min: v(0.0, 0.0),
            max: v(1000.0, 500.0),
        };
        doc.begin();
        let plan = doc.add_view(View::new("Plan", crop, 50.0));
        let mut a = Sheet::new("A-101");
        a.viewports.push(Viewport::new(plan, v(200.0, 150.0)));
        let a = doc.add_sheet(a);
        let mut b = Sheet::new("A-102");
        b.viewports.push(Viewport::new(plan, v(100.0, 100.0)));
        let b = doc.add_sheet(b);
        history.push(doc.commit("Set up"));
        assert_eq!(doc.sheets_showing(plan), [a, b]);
        let placed = doc.placed_views(doc.sheet(a).unwrap());
        assert_eq!(placed[0].frame.size, v(20.0, 10.0), "the crop at 1:50");
        assert_eq!(doc.sheet_scale_text(doc.sheet(a).unwrap()), "1:50");

        // Cropping it on one sheet reframes the view: the other sheet follows.
        let mut wider = placed[0].clone();
        wider.frame.size = v(30.0, 10.0);
        wider.frame.at = v(205.0, 150.0);
        doc.begin();
        doc.set_placed_view(a, 0, &wider);
        history.push(doc.commit("Crop"));
        assert_eq!(doc.view(plan).unwrap().size, v(1500.0, 500.0));
        let on_b = doc.placed_views(doc.sheet(b).unwrap());
        assert_eq!(on_b[0].frame.size, v(30.0, 10.0));
        assert_eq!(
            on_b[0].frame.at,
            v(100.0, 100.0),
            "only its own sheet's place moved"
        );

        // Deleting the view takes it off both sheets; undo brings it all back.
        let before = doc.clone();
        doc.begin();
        doc.remove_view(plan);
        history.push(doc.commit("Delete view"));
        assert!(doc.view(plan).is_none());
        assert!(doc.sheets().all(|(_, s)| s.viewports.is_empty()));
        assert_eq!(doc.sheet_scale_text(doc.sheet(a).unwrap()), "-");
        assert!(history.undo(&mut doc));
        assert_eq!(doc, before);
        assert!(history.undo(&mut doc));
        assert_eq!(doc.view(plan).unwrap().size, v(1000.0, 500.0));
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut doc = Document::new();
        let mut history = History::default();
        let empty = doc.clone();

        doc.begin();
        square(&mut doc);
        history.push(doc.commit("Rectangle"));
        let drawn = doc.clone();
        assert_eq!(doc.len(), 8);

        assert!(history.undo(&mut doc));
        assert_eq!(doc, empty);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, drawn);
        assert_eq!(history.undo_label(), Some("Rectangle"));
    }

    #[test]
    fn a_dimension_sets_its_own_style_over_the_drawings() {
        use crate::{AnnotationRef, ArrowStyle, StyleOverride};
        let mut doc = Document::new();
        let mut history = History::default();
        let (p, _) = square(&mut doc);
        let dim = doc
            .add_constraint(Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 1.0,
            })
            .unwrap();
        let r = AnnotationRef::Dimension(dim);
        assert_eq!(doc.draft_style_for(r), doc.draft_style());
        doc.begin();
        doc.set_style_override(
            r,
            StyleOverride {
                text_height: Some(5.0),
                arrow: Some(ArrowStyle::Tick),
                ..Default::default()
            },
        );
        history.push(doc.commit("Annotation style"));
        let own = doc.draft_style_for(r);
        assert_eq!((own.text_height, own.arrow), (5.0, ArrowStyle::Tick));
        // Everything else, and what the drawing changes later, still comes from the drawing.
        let mut drawing = doc.draft_style();
        drawing.extension_gap = 2.5;
        drawing.text_height = 7.0;
        doc.set_draft_style(drawing);
        let own = doc.draft_style_for(r);
        assert_eq!((own.extension_gap, own.text_height), (2.5, 5.0));
        assert_eq!(doc.style_overrides().count(), 1);
        // Undone, it follows the drawing again; nothing overridden is not kept.
        assert!(history.undo(&mut doc));
        assert_eq!(doc.draft_style_for(r).text_height, 7.0);
        doc.set_style_override(r, StyleOverride::default());
        assert_eq!(doc.style_overrides().count(), 0);
    }

    #[test]
    fn translate_moves_shared_points_once_and_stretches_neighbours() {
        let mut doc = Document::new();
        let (p, l) = square(&mut doc);
        doc.translate([l[2]], v(0.0, 1.0)); // the top edge
        assert_eq!(doc.point(p[2]).unwrap(), v(1.0, 2.0));
        assert_eq!(doc.point(p[3]).unwrap(), v(0.0, 2.0));
        assert_eq!(doc.point(p[0]).unwrap(), v(0.0, 0.0));
    }

    #[test]
    fn deleting_a_line_keeps_shared_points_and_drops_orphans() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(1.0, 0.0));
        let c = doc.add_point(v(2.0, 0.0));
        let l1 = doc.add_line(a, b).unwrap();
        let l2 = doc.add_line(b, c).unwrap();
        doc.delete([l1]);
        assert!(!doc.contains(a), "orphaned start point removed");
        assert!(doc.contains(b), "shared point kept");
        assert!(doc.contains(l2));
    }

    #[test]
    fn deleting_a_point_deletes_its_curves() {
        let mut doc = Document::new();
        let (p, _) = square(&mut doc);
        doc.delete([p[0]]);
        // Two lines used p0; the other two lines and their three points remain.
        assert_eq!(doc.iter().filter(|(_, e)| !e.kind.is_point()).count(), 2);
        assert_eq!(doc.iter().filter(|(_, e)| e.kind.is_point()).count(), 3);
    }

    #[test]
    fn arc_end_stays_on_circle() {
        let mut doc = Document::new();
        let c = doc.add_point(v(0.0, 0.0));
        let s = doc.add_point(v(2.0, 0.0));
        let e = doc.add_point(v(0.0, 5.0));
        let arc = doc.add_arc(c, s, e).unwrap();
        assert_eq!(doc.point(e).unwrap(), v(0.0, 2.0));

        // Moving the start changes the radius; the end follows.
        doc.translate([s], v(1.0, 0.0));
        let end = doc.point(e).unwrap();
        assert!((end.length() - 3.0).abs() < 1e-12);
        let Some(Shape::Arc { radius, sweep, .. }) = doc.shape(arc) else {
            panic!()
        };
        assert!((radius - 3.0).abs() < 1e-12);
        assert!((sweep - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn merge_points_connects_lines_and_refuses_collapse() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(1.0, 0.0));
        let c = doc.add_point(v(1.0, 0.0));
        let d = doc.add_point(v(1.0, 1.0));
        let l1 = doc.add_line(a, b).unwrap();
        let l2 = doc.add_line(c, d).unwrap();
        doc.merge_points(c, b).unwrap();
        assert!(!doc.contains(c));
        assert_eq!(doc.get(l2).unwrap().kind, EntityKind::Line { a: b, b: d });
        // Merging a line's two ends would collapse it.
        assert_eq!(
            doc.merge_points(a, b),
            Err(DocumentError::Degenerate("merge"))
        );
        assert!(doc.contains(l1) && doc.contains(a));
    }

    #[test]
    fn joining_a_point_onto_its_curves_end_drops_the_point_on_curve() {
        // Regression: the relation survived the join, was saved, and the file then refused
        // to open ("degenerate point on its own curve").
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(10.0, 0.0));
        let p = doc.add_point(v(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        doc.add_constraint(Constraint::PointOnCurve {
            point: p,
            curve: line,
        })
        .unwrap();
        doc.merge_points(p, b).unwrap();
        assert_eq!(doc.constraints().count(), 0);
        // And a document holding one anyway is opened without it.
        let entities: Vec<_> = doc.iter().map(|(id, e)| (id, *e)).collect();
        let bad = Constraint::PointOnCurve {
            point: b,
            curve: line,
        };
        assert!(Document::from_parts(entities.clone(), [(ConstraintId(4), bad)]).is_err());
        let (doc, dropped) =
            Document::from_parts_dropping_degenerate(entities, [(ConstraintId(4), bad)]).unwrap();
        assert_eq!(dropped, [(ConstraintId(4), "point on its own curve")]);
        assert_eq!(doc.constraints().count(), 0);
    }

    #[test]
    fn copies_stay_apart_through_edits_undo_and_merged_steps() {
        let mut doc = Document::new();
        let mut history = History::default();
        let (pts, lines) = square(&mut doc);
        doc.begin();
        doc.set_custom_patterns(
            "*A, a
0, 0,0, 0,1
"
            .into(),
        );
        history.push(doc.commit("Patterns"));
        let copy = doc.clone();
        assert_eq!(copy, doc);
        doc.begin();
        doc.translate([lines[0]], v(5.0, 5.0));
        history.push(doc.commit("Move"));
        // A second change folded into the same step.
        doc.begin();
        doc.set_point(pts[2], v(40.0, 40.0)).unwrap();
        let tx = doc.commit("Move");
        history.merge_into_last(tx);
        assert_ne!(copy, doc);
        assert!(history.undo(&mut doc));
        assert_eq!(doc, copy, "one undo takes back both merged changes");
        assert!(history.redo(&mut doc));
        assert_eq!(doc.point(pts[2]).unwrap(), v(40.0, 40.0));
        // Undoing past the copy changes the document, never the copy.
        assert!(history.undo(&mut doc));
        assert!(history.undo(&mut doc));
        assert_eq!(doc.custom_patterns(), "");
        assert!(copy.custom_patterns().starts_with("*A"));
    }

    #[test]
    fn rollback_restores_everything() {
        let mut doc = Document::new();
        let (_, l) = square(&mut doc);
        let before = doc.clone();
        doc.begin();
        doc.translate([l[0]], v(5.0, 5.0));
        doc.delete([l[1]]);
        doc.rollback();
        assert_eq!(doc, before);
    }

    #[test]
    fn constraints_are_validated_undoable_and_cascade_on_delete() {
        let mut doc = Document::new();
        let mut history = History::default();
        let (p, l) = square(&mut doc);

        assert_eq!(
            doc.add_constraint(Constraint::Horizontal { line: p[0] }),
            Err(DocumentError::WrongKind {
                id: p[0],
                expected: "a line"
            })
        );
        assert_eq!(
            doc.add_constraint(Constraint::Distance {
                a: p[0],
                b: p[1],
                value: -1.0
            }),
            Err(DocumentError::InvalidValue)
        );

        doc.begin();
        let h = doc
            .add_constraint(Constraint::Horizontal { line: l[0] })
            .unwrap();
        let d = doc
            .add_constraint(Constraint::Distance {
                a: p[0],
                b: p[1],
                value: 1.0,
            })
            .unwrap();
        history.push(doc.commit("Constrain"));
        assert_eq!(h, ConstraintId(1));
        assert_eq!(doc.constraints_on(l[0]).collect::<Vec<_>>(), vec![h]);

        // Deleting the line removes its relation; the distance on its (still used) points stays.
        doc.begin();
        doc.delete([l[0]]);
        history.push(doc.commit("Delete"));
        assert!(doc.constraint(h).is_none());
        assert!(doc.constraint(d).is_some());

        assert!(history.undo(&mut doc));
        assert!(doc.constraint(h).is_some());
        assert!(history.undo(&mut doc));
        assert_eq!(doc.constraint_count(), 0);
    }

    #[test]
    fn merging_points_redirects_constraints_and_drops_implied_ones() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(1.0, 0.0));
        let c = doc.add_point(v(1.0, 0.0));
        let d = doc.add_point(v(1.0, 2.0));
        doc.add_line(a, b).unwrap();
        doc.add_line(c, d).unwrap();
        let coincident = doc
            .add_constraint(Constraint::Coincident { a: b, b: c })
            .unwrap();
        let dist = doc
            .add_constraint(Constraint::Distance {
                a: c,
                b: d,
                value: 2.0,
            })
            .unwrap();
        doc.merge_points(c, b).unwrap();
        assert!(doc.constraint(coincident).is_none());
        assert_eq!(
            doc.constraint(dist),
            Some(&Constraint::Distance {
                a: b,
                b: d,
                value: 2.0
            })
        );
    }

    #[test]
    fn rejects_bad_references_and_degenerate_geometry() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(0.0, 0.0));
        assert_eq!(doc.add_line(a, b), Err(DocumentError::Degenerate("line")));
        assert_eq!(
            doc.add_circle(a, 0.0),
            Err(DocumentError::Degenerate("circle"))
        );
        let line_like = EntityId(999);
        assert_eq!(
            doc.add_circle(line_like, 1.0),
            Err(DocumentError::Missing(line_like))
        );
    }
}

#[cfg(test)]
mod sheet_order_tests {
    use super::*;

    #[test]
    fn sheets_move_between_ids_in_one_undoable_step() {
        let mut doc = Document::new();
        doc.begin();
        let ids: Vec<SheetId> = ["A", "B", "C"]
            .map(|n| doc.add_sheet(Sheet::new(n)))
            .to_vec();
        doc.commit("Setup");
        let names =
            |doc: &Document| -> Vec<String> { doc.sheets().map(|(_, s)| s.name.clone()).collect() };
        doc.begin();
        let went = doc.move_sheet(ids[2], 0);
        let mut history = History::default();
        history.push(doc.commit("Move sheet"));
        assert_eq!(names(&doc), ["C", "A", "B"]);
        assert_eq!(went[&ids[2]], ids[0], "C is now first");
        assert_eq!(went[&ids[0]], ids[1]);
        assert_eq!(doc.sheet(went[&ids[1]]).unwrap().name, "B");
        assert!(history.undo(&mut doc));
        assert_eq!(names(&doc), ["A", "B", "C"]);
        // Past the end goes last; an unknown sheet moves nothing.
        doc.move_sheet(ids[0], 9);
        assert_eq!(names(&doc), ["B", "C", "A"]);
        assert!(doc.move_sheet(SheetId(999), 0).is_empty());
    }
}
