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

mod blocks;
mod constraints;
mod drafting;
mod entity;
mod geometry;
mod grouping;
mod journal;
mod layers;
mod settings;
mod sheets;
#[cfg(test)]
mod tests;
mod texts;
mod variables;
mod walls;

pub use entity::{DocumentError, Entity, EntityId, EntityKind};
pub use journal::{Dropped, History, Transaction};
use journal::{Key, Record};
pub use variables::{MeasurementId, Variable, VariableId, Variant, VariantId};

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
    /// Changed by every change except geometry moving: see [`structure`](Self::structure).
    structure: u64,
    /// Changed by every change to entities: see [`geometry`](Self::geometry).
    geometry: u64,
    /// Pre-images of everything touched by the open transaction.
    journal: Option<HashMap<Key, Option<Record>>>,
    /// Counts transactions begun, so work spanning calls (a drag) can tell it is still in the
    /// same one (not part of the content).
    transaction_serial: u64,
    /// The revision when the open (or last) transaction began.
    begun_at: u64,
    /// Leader targets as of the last [`remember_leader_targets`](Self::remember_leader_targets)
    /// (not part of the content).
    last_anchor_pos: HashMap<AnnotationId, DVec2>,
    /// Recent changes, for caches that follow them (see [`changes_since`](Self::changes_since);
    /// not part of the content).
    changes: ChangeLog,
}

/// Which document a [`ChangeLog`] is of: a fresh value for every document, a copy included
/// (a copy's changes are its own).
#[derive(Debug, PartialEq, Eq)]
struct Lineage(u64);

impl Default for Lineage {
    fn default() -> Self {
        Self(next_structure())
    }
}

impl Clone for Lineage {
    fn clone(&self) -> Self {
        Self::default()
    }
}

/// What changed, revision by revision: entities by id, and whether layers did.
#[derive(Debug, Default)]
struct ChangeLog {
    lineage: Lineage,
    /// (revision the change was made at, entity changed or `None` for a layer).
    entries: Vec<(u64, Option<EntityId>)>,
    /// Changes made before this revision have been let go.
    floor: u64,
}

impl ChangeLog {
    /// Entries kept: past this, the older half goes (a cache that far behind starts over).
    const LIMIT: usize = 1 << 16;

    fn note(&mut self, revision: u64, what: Option<EntityId>) {
        if self.entries.len() >= Self::LIMIT {
            let half = self.entries.len() / 2;
            self.entries.drain(..half);
            // (Entries at this revision may have gone too: it is no longer whole.)
            self.floor = self.entries.first().map_or(revision, |e| e.0 + 1);
        }
        self.entries.push((revision, what));
    }
}

/// A copy starts a log of its own: it has a new lineage, so no cache can ask it for changes
/// from before it was made, and copying the entries (up to a megabyte) would only slow
/// every copy (undo steps, design options) down.
impl Clone for ChangeLog {
    fn clone(&self) -> Self {
        Self {
            lineage: self.lineage.clone(),
            entries: Vec::new(),
            // Every change to an entity or layer is noted, so none came after the last one.
            floor: self.entries.last().map_or(self.floor, |e| e.0 + 1),
        }
    }
}

/// The changes to a document since a revision (see [`Document::changes_since`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// Entities added, removed or changed (sorted, once each).
    pub entities: Vec<EntityId>,
    /// Some layer was added, removed or changed (shown, hidden, ...).
    pub layers: bool,
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
            structure: next_structure(),
            geometry: next_structure(),
            journal: None,
            transaction_serial: 0,
            begun_at: 0,
            last_anchor_pos: HashMap::new(),
            changes: ChangeLog::default(),
        }
    }
}

/// A fresh [`Document::structure`] value (one counter for every document, so a cache keyed
/// on it can't mistake one drawing for another).
fn next_structure() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// True if `a` and `b` are the same entity on the same points, differing at most in a
/// circle's radius or a point's place.
fn same_points(a: &EntityKind, b: &EntityKind) -> bool {
    match (a, b) {
        (EntityKind::Point(_), EntityKind::Point(_)) => true,
        (EntityKind::Circle { center: c, .. }, EntityKind::Circle { center: d, .. }) => c == d,
        _ => a == b,
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
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

    /// Make sure no id the drawing hands out from now on is one of `ids`, spelled as files
    /// write them (`e12`, `c3`, `f2`, ...): ids that something the drawing doesn't hold refers
    /// to, such as what a newer Drift wrote that this build carries through without knowing it
    /// (`drift-io`'s `carry.rs`). Spellings no kind of id has are passed over. Not an edit: no
    /// undo step, and not part of what the drawing is equal to.
    pub fn reserve_ids<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) {
        for (kind, n) in ids.into_iter().filter_map(spelled_id) {
            let next = n + 1;
            match kind {
                'e' => self.next_id = self.next_id.max(next),
                'c' => self.next_constraint_id = self.next_constraint_id.max(next),
                // Variables, options, measurements and layers.
                'v' | 'k' | 'm' | 'l' => self.next_param_id = self.next_param_id.max(next),
                // Fills, annotations, blocks, instances, groups, paths, sheets, texts,
                // underlays and views (`w`, like walls).
                'f' | 'n' | 'b' | 'i' | 'g' | 'p' | 's' | 't' | 'u' => {
                    self.next_drafting_id = self.next_drafting_id.max(next);
                }
                'w' => {
                    self.next_drafting_id = self.next_drafting_id.max(next);
                    self.next_wall_id = self.next_wall_id.max(next);
                }
                // Openings and property dimensions.
                'o' | 'q' => self.next_wall_id = self.next_wall_id.max(next),
                _ => {}
            }
        }
    }

    /// Whether the drawing has something with the id `id`, spelled as files write it (`e12`,
    /// `c3`, `f2`, ...; see [`reserve_ids`](Self::reserve_ids)).
    pub fn holds_id(&self, id: &str) -> bool {
        use crate::{
            AnnotationId, BlockId, FillId, GroupId, InstanceId, OpeningId, PathId, PropertyDimId,
            SheetId, TextId, UnderlayId, ViewId, WallId,
        };
        let Some((kind, n)) = spelled_id(id) else {
            return false;
        };
        match kind {
            'e' => self.get(EntityId(n)).is_some(),
            'c' => self.constraint(ConstraintId(n)).is_some(),
            'v' => self.variable(VariableId(n)).is_some(),
            'k' => self.variant(VariantId(n)).is_some(),
            'm' => self.measurements().any(|(m, _)| m == MeasurementId(n)),
            'l' => self.layer(LayerId(n)).is_some(),
            'f' => self.fill(FillId(n)).is_some(),
            'n' => self.annotation(AnnotationId(n)).is_some(),
            'b' => self.block(BlockId(n)).is_some(),
            'i' => self.instance(InstanceId(n)).is_some(),
            'g' => self.group(GroupId(n)).is_some(),
            'p' => self.path(PathId(n)).is_some(),
            's' => self.sheet(SheetId(n)).is_some(),
            't' => self.text(TextId(n)).is_some(),
            'u' => self.underlay(UnderlayId(n)).is_some(),
            'w' => self.view(ViewId(n)).is_some() || self.wall(WallId(n)).is_some(),
            'o' => self.opening(OpeningId(n)).is_some(),
            'q' => self.property_dim(PropertyDimId(n)).is_some(),
            _ => false,
        }
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

    /// Changes with every edit except geometry moving (points placed, circles resized, a
    /// dimension's label dragged): adding, removing or rewiring entities, restyling them,
    /// relations, layers, groups, fills, walls, undo and redo. Caches of the drawing's
    /// make-up key on it, so they stay warm through a drag, when the revision changes every
    /// frame. Values are never reused, not even by another document.
    pub fn structure(&self) -> u64 {
        self.structure
    }

    /// Changes with every change to the sketch's entities (added, removed, moved, restyled,
    /// put on another layer), undo and redo of one included, and with nothing else (fills,
    /// relations, layers' settings). Caches made from the curves alone, such as the closed
    /// areas, key on it, so an edit that changes fills after them doesn't throw them away.
    /// Values are never reused, not even by another document.
    pub fn geometry(&self) -> u64 {
        self.geometry
    }

    /// Which document this is, for [`changes_since`](Self::changes_since): every document
    /// has its own, and a copy gets a new one.
    pub fn lineage(&self) -> u64 {
        self.changes.lineage.0
    }

    /// The changes made to this document (of [`lineage`](Self::lineage) `lineage`) since
    /// it was at `revision`, if they are all still known: what a cache made then needs to
    /// look at again. None for another document or when it is too far behind.
    pub fn changes_since(&self, lineage: u64, revision: u64) -> Option<Changes> {
        if lineage != self.lineage() || revision < self.changes.floor || revision > self.revision {
            return None;
        }
        let entries = &self.changes.entries;
        let from = entries.partition_point(|(r, _)| *r < revision);
        let mut out = Changes::default();
        for (_, what) in &entries[from..] {
            match what {
                Some(id) => out.entities.push(*id),
                None => out.layers = true,
            }
        }
        out.entities.sort_unstable();
        out.entities.dedup();
        Some(out)
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
    /// The first point with a position, or circle with a radius, that isn't a finite number:
    /// saved, it would make the file unreadable. Nothing should ever store one; this is the
    /// last check before a save.
    pub fn first_non_finite(&self) -> Option<EntityId> {
        self.iter().find_map(|(id, e)| {
            let finite = match e.kind {
                EntityKind::Point(p) => p.is_finite(),
                EntityKind::Circle { radius, .. } => radius.is_finite(),
                _ => true,
            };
            (!finite).then_some(id)
        })
    }

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
}

/// The kind of value a dimension holds.
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

/// An id as files spell it (`e12`): its kind's letter and its number. (Numbers far beyond
/// any id a drawing makes are a stray string, not an id.)
fn spelled_id(id: &str) -> Option<(char, u32)> {
    let mut chars = id.chars();
    let kind = chars.next().filter(char::is_ascii_lowercase)?;
    let digits = chars.as_str();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u32>()
        .ok()
        .filter(|n| *n < u32::MAX / 2)
        .map(|n| (kind, n))
}
