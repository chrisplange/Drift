//! The sketch document: entities, their connectivity, and transactional edits for undo/redo.
//!
//! Curves reference [`EntityKind::Point`] entities for their defining points, so connected
//! geometry shares points. Each point's `x`/`y` and each circle's radius are the unknowns the
//! constraint solver (M2) will solve for.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use glam::DVec2;

use crate::{LineKind, Shape};

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
    NotAPoint(EntityId),
    Degenerate(&'static str),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentError::Missing(id) => write!(f, "entity {id} does not exist"),
            DocumentError::NotAPoint(id) => write!(f, "entity {id} is not a point"),
            DocumentError::Degenerate(what) => write!(f, "degenerate {what}"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// One undoable step: the before and after state of every entity it touched.
#[derive(Clone, Debug, PartialEq)]
pub struct Transaction {
    pub label: String,
    changes: Vec<(EntityId, Option<Entity>, Option<Entity>)>,
}

impl Transaction {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    entities: BTreeMap<EntityId, Entity>,
    next_id: u32,
    /// Incremented on every change, so caches (spatial index, autosave) can tell when to refresh.
    revision: u64,
    /// Pre-images of entities touched by the open transaction.
    journal: Option<HashMap<EntityId, Option<Entity>>>,
}

impl PartialEq for Document {
    fn eq(&self, other: &Self) -> bool {
        self.entities == other.entities
    }
}

impl Default for Document {
    fn default() -> Self {
        Self {
            entities: BTreeMap::new(),
            // Ids start at 1; `e0` is not a valid id in `.drift` files.
            next_id: 1,
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
        let entities: BTreeMap<_, _> = entities.into_iter().collect();
        let next_id = entities.keys().last().map_or(1, |id| id.0 + 1);
        let doc = Self {
            entities,
            next_id,
            ..Self::default()
        };
        for entity in doc.entities.values() {
            for point in entity.kind.points() {
                doc.point(point)?;
            }
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
            .filter_map(|(id, before)| {
                let after = self.entities.get(&id).copied();
                (before != after).then_some((id, before, after))
            })
            .collect();
        changes.sort_by_key(|(id, ..)| *id);
        Transaction {
            label: label.into(),
            changes,
        }
    }

    /// Abandon the open transaction, restoring every touched entity.
    pub fn rollback(&mut self) {
        if let Some(journal) = self.journal.take() {
            for (id, before) in journal {
                self.restore(id, before);
            }
            self.revision += 1;
        }
    }

    fn touch(&mut self, id: EntityId) {
        if let Some(journal) = &mut self.journal {
            let current = self.entities.get(&id).copied();
            journal.entry(id).or_insert(current);
        }
        self.revision += 1;
    }

    fn restore(&mut self, id: EntityId, entity: Option<Entity>) {
        match entity {
            Some(e) => {
                self.entities.insert(id, e);
            }
            None => {
                self.entities.remove(&id);
            }
        }
    }

    fn apply(&mut self, tx: &Transaction, forward: bool) {
        for (id, before, after) in &tx.changes {
            self.restore(*id, if forward { *after } else { *before });
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
        self.touch(from);
        self.entities.remove(&from);
        self.normalize_arcs_using(&BTreeSet::from([into]));
        Ok(())
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
