//! The sketch document: entities, their connectivity, and transactional edits for undo/redo.
//!
//! Curves reference [`EntityKind::Point`] entities for their defining points, so connected
//! geometry shares points. Each point's `x`/`y` and each circle's radius are the unknowns the
//! constraint solver solves for; [`Constraint`]s relate them.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use glam::DVec2;

use crate::constraint::Slot;
use crate::units::{Kind, Units};
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
}

impl EntityKind {
    /// The point entities this entity is defined by.
    pub fn points(&self) -> impl Iterator<Item = EntityId> {
        let ids: [Option<EntityId>; 3] = match *self {
            EntityKind::Point(_) => [None; 3],
            EntityKind::Line { a, b } => [Some(a), Some(b), None],
            EntityKind::Circle { center, .. } => [Some(center), None, None],
            EntityKind::Arc { center, start, end } => [Some(center), Some(start), Some(end)],
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
        }
    }

    fn replace_point(&mut self, from: EntityId, to: EntityId) {
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

/// A named set of variable expressions that can be applied in one step.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: String,
    pub values: BTreeMap<VariableId, String>,
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
    pub style: LineKind,
}

impl Entity {
    pub fn new(kind: EntityKind) -> Self {
        Self {
            kind,
            style: LineKind::Solid,
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
    /// Present (with `Record::Driven`) when the dimension is driven.
    Driven(ConstraintId),
    Variable(VariableId),
    /// The expression a dimension's value is linked to.
    Expression(ConstraintId),
    Variant(VariantId),
    Measurement(MeasurementId),
    Units,
}

#[derive(Clone, Debug, PartialEq)]
enum Record {
    Entity(Entity),
    Constraint(Constraint),
    Placement(DVec2),
    Driven,
    Variable(Variable),
    Expression(String),
    Variant(Variant),
    Measurement(Constraint),
    Units(Units),
}

/// One undoable step: the before and after state of everything it touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub label: String,
    changes: Vec<(Key, Option<Record>, Option<Record>)>,
}

impl Transaction {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    entities: BTreeMap<EntityId, Entity>,
    constraints: BTreeMap<ConstraintId, Constraint>,
    /// Where dimension labels sit: an offset in world units from the dimension's anchor
    /// (e.g. the midpoint of a measured line). Missing means "default placement".
    placements: BTreeMap<ConstraintId, DVec2>,
    /// Driven (reference) dimensions: they show a measurement but don't constrain.
    driven: BTreeSet<ConstraintId>,
    variables: BTreeMap<VariableId, Variable>,
    /// Dimensions whose value comes from an expression (e.g. `width / 2`).
    expressions: BTreeMap<ConstraintId, String>,
    variants: BTreeMap<VariantId, Variant>,
    /// Pinned measurements: dimension definitions that are only measured, never solved.
    measurements: BTreeMap<MeasurementId, Constraint>,
    units: Units,
    next_id: u32,
    next_constraint_id: u32,
    /// Next id for variables, variants and measurements (one counter, ids never reused).
    next_param_id: u32,
    /// Incremented on every change, so caches (spatial index, autosave) can tell when to refresh.
    revision: u64,
    /// Pre-images of everything touched by the open transaction.
    journal: Option<HashMap<Key, Option<Record>>>,
}

impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.entities == other.entities
            && self.constraints == other.constraints
            && self.placements == other.placements
            && self.driven == other.driven
            && self.variables == other.variables
            && self.expressions == other.expressions
            && self.variants == other.variants
            && self.measurements == other.measurements
            && self.units == other.units
    }
}

impl Default for Document {
    fn default() -> Self {
        Self {
            entities: BTreeMap::new(),
            constraints: BTreeMap::new(),
            placements: BTreeMap::new(),
            driven: BTreeSet::new(),
            variables: BTreeMap::new(),
            expressions: BTreeMap::new(),
            variants: BTreeMap::new(),
            measurements: BTreeMap::new(),
            units: Units::default(),
            // Ids start at 1; `e0` and `c0` are not valid ids in `.drift` files.
            next_id: 1,
            next_constraint_id: 1,
            next_param_id: 1,
            revision: 0,
            journal: None,
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
    pub fn from_parts(
        entities: impl IntoIterator<Item = (EntityId, Entity)>,
        constraints: impl IntoIterator<Item = (ConstraintId, Constraint)>,
    ) -> Result<Self, DocumentError> {
        let entities: BTreeMap<_, _> = entities.into_iter().collect();
        let constraints: BTreeMap<_, _> = constraints.into_iter().collect();
        let next_id = entities.keys().last().map_or(1, |id| id.0 + 1);
        let next_constraint_id = constraints.keys().last().map_or(1, |id| id.0 + 1);
        let doc = Self {
            entities,
            constraints,
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
        })
    }

    // ----- Transactions --------------------------------------------------------------------

    /// Start recording changes. Nested calls are ignored; the outer transaction wins.
    pub fn begin(&mut self) {
        if self.journal.is_none() {
            self.journal = Some(HashMap::new());
        }
    }

    pub fn in_transaction(&self) -> bool {
        self.journal.is_some()
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
            Key::Driven(id) => self.driven.contains(&id).then_some(Record::Driven),
            Key::Variable(id) => self.variables.get(&id).cloned().map(Record::Variable),
            Key::Expression(id) => self.expressions.get(&id).cloned().map(Record::Expression),
            Key::Variant(id) => self.variants.get(&id).cloned().map(Record::Variant),
            Key::Measurement(id) => self.measurements.get(&id).copied().map(Record::Measurement),
            Key::Units => Some(Record::Units(self.units)),
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

    fn insert(&mut self, entity: Entity) -> EntityId {
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
        self.entities.insert(
            id,
            Entity {
                kind: EntityKind::Point(at),
                ..self.entities[&id]
            },
        );
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
                matches!(e.kind, EntityKind::Arc { .. })
                    && e.kind.points().any(|p| points.contains(&p))
            })
            .map(|(id, _)| *id)
            .collect();
        for arc in arcs {
            self.normalize_arc(arc);
        }
    }

    fn normalize_arc(&mut self, arc: EntityId) {
        let Some(Entity {
            kind: EntityKind::Arc { center, start, end },
            ..
        }) = self.entities.get(&arc).copied()
        else {
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

    /// Delete entities. Deleting a point also deletes the curves that use it; points left
    /// unused by deleting a curve are removed too (unless they were explicitly standalone).
    pub fn delete(&mut self, ids: impl IntoIterator<Item = EntityId>) {
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
            if c.is_degenerate() {
                // e.g. a coincidence between the two merged points: now implied.
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
            if c.is_degenerate() {
                self.measurements.remove(&id);
            } else {
                self.measurements.insert(id, c);
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
                Slot::LineOrCircular => !kind.is_point(),
            };
            if !ok {
                return Err(DocumentError::WrongKind {
                    id,
                    expected: slot.describe(),
                });
            }
            kinds.push(kind);
        }
        // A curve's own defining point can't be constrained onto it; tangency needs at least
        // one circular curve (enforced by the slot) and is meaningless between a curve and
        // itself (caught by `is_degenerate`).
        if let Constraint::PointOnCurve { point, curve } = constraint
            && self
                .entities
                .get(curve)
                .is_some_and(|e| e.kind.points().any(|p| p == *point))
        {
            return Err(DocumentError::Degenerate("point on its own curve"));
        }
        if let Constraint::Symmetric { a, b, axis } = constraint
            && self
                .entities
                .get(axis)
                .is_some_and(|e| e.kind.points().any(|p| p == *a || p == *b))
        {
            return Err(DocumentError::Degenerate(
                "symmetric about a line through the points",
            ));
        }
        // A line's own end point can't sit at its midpoint.
        if let Constraint::Midpoint { point, .. } = constraint
            && let EntityKind::Line { a, b } = kinds[1]
            && (*point == a || *point == b)
        {
            return Err(DocumentError::Degenerate("midpoint"));
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
        if self.driven.contains(&id) {
            self.touch_key(Key::Driven(id));
            self.driven.remove(&id);
        }
        self.set_expression(id, None);
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

    // ----- Parameters: units, variables, expressions, variants, measurements -----------------

    /// Every journal key with content in this document.
    fn keys(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = Vec::new();
        keys.extend(self.entities.keys().map(|id| Key::Entity(*id)));
        keys.extend(self.constraints.keys().map(|id| Key::Constraint(*id)));
        keys.extend(self.placements.keys().map(|id| Key::Placement(*id)));
        keys.extend(self.driven.iter().map(|id| Key::Driven(*id)));
        keys.extend(self.variables.keys().map(|id| Key::Variable(*id)));
        keys.extend(self.expressions.keys().map(|id| Key::Expression(*id)));
        keys.extend(self.variants.keys().map(|id| Key::Variant(*id)));
        keys.extend(self.measurements.keys().map(|id| Key::Measurement(*id)));
        keys.push(Key::Units);
        keys
    }

    /// Make this document's content equal to `other`'s, as journalled changes (so it can be
    /// undone). Used to restore snapshots.
    pub fn restore_from(&mut self, other: &Document) {
        let mut keys = self.keys();
        keys.extend(other.keys());
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
}

impl History {
    /// Record a committed transaction. Empty transactions are dropped.
    pub fn push(&mut self, tx: Transaction) {
        if !tx.is_empty() {
            self.undo.push(tx);
            self.redo.clear();
        }
    }

    /// Append a transaction's changes to the most recent undo step (so they undo together).
    /// With nothing to merge into, it is pushed as its own step.
    pub fn merge_into_last(&mut self, tx: Transaction) {
        if tx.is_empty() {
            return;
        }
        match self.undo.last_mut() {
            Some(last) => {
                for (key, before, after) in tx.changes {
                    match last.changes.iter_mut().find(|(k, ..)| *k == key) {
                        Some(existing) => existing.2 = after,
                        None => last.changes.push((key, before, after)),
                    }
                }
            }
            None => self.undo.push(tx),
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
