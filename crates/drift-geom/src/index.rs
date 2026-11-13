use std::collections::{BTreeSet, HashMap, HashSet};

use drift_core::{Bounds, DVec2, Document, EntityId, EntityKind, Shape};
use rstar::{AABB, Envelope, RTree, RTreeObject};

use crate::primitives::{distance, shape_intersects_rect, shape_within_rect};

/// How a point entity is used by the curves around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointRole {
    /// End of a line or arc.
    Endpoint,
    /// Centre of a circle or arc (and not also an end point).
    Center,
    /// A sketch point not used by any curve.
    Standalone,
    /// A Bézier's handle: dragged to shape the curve, but not snapped to.
    Handle,
}

#[derive(PartialEq)]
struct Entry {
    id: EntityId,
    envelope: AABB<[f64; 2]>,
}

fn envelope(shape: &Shape) -> AABB<[f64; 2]> {
    let b = shape.bounds();
    AABB::from_corners(b.min.to_array(), b.max.to_array())
}

impl RTreeObject for Entry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

/// Resolved shapes of a document plus an R-tree over them, for fast picking and snapping.
/// Rebuild with [`update`](Self::update) whenever the document changes.
pub struct SceneIndex {
    revision: Option<u64>,
    tree: RTree<Entry>,
    shapes: HashMap<EntityId, Shape>,
    /// What each entity was at the last update (a change means roles need working out).
    kinds: HashMap<EntityId, EntityKind>,
    /// Shapes moved during a drag whose places in the tree are out of date. Searches test
    /// them one by one; the tree is rebuilt once there are many of them. (Ordered, so results
    /// don't depend on hash order.)
    moved: BTreeSet<EntityId>,
    roles: HashMap<EntityId, PointRole>,
    /// The document indexed ([`Document::lineage`]).
    lineage: Option<u64>,
    /// The indexed curves using each point (what a point's move reshapes, and its role).
    users: HashMap<EntityId, BTreeSet<EntityId>>,
    /// The curves left out because their layer is hidden, and the points each uses (as of
    /// the last full pass; empty while every layer is shown). A point used only by such
    /// curves is left out too, whatever its own layer (#359): see
    /// [`hidden_only`](Self::hidden_only).
    hidden_curves: HashSet<EntityId>,
    hidden_users: HashMap<EntityId, BTreeSet<EntityId>>,
}

impl Default for SceneIndex {
    fn default() -> Self {
        Self {
            revision: None,
            tree: RTree::new(),
            shapes: HashMap::new(),
            kinds: HashMap::new(),
            moved: BTreeSet::new(),
            roles: HashMap::new(),
            lineage: None,
            users: HashMap::new(),
            hidden_curves: HashSet::new(),
            hidden_users: HashMap::new(),
        }
    }
}

/// The points an entity uses (none for a point), each once.
fn used_points(kind: &EntityKind) -> BTreeSet<EntityId> {
    if kind.is_point() {
        BTreeSet::new()
    } else {
        kind.points().collect()
    }
}

/// How `kind` (of an indexed entity) bears on point `p`'s role, as the full pass works
/// roles out: an end overrides anything; a centre or a handle only a lone point.
fn apply_role(kind: &EntityKind, p: EntityId, role: &mut Option<PointRole>) {
    let end = |q: EntityId, role: &mut Option<PointRole>| {
        if q == p {
            *role = Some(PointRole::Endpoint);
        }
    };
    let weak = |q: EntityId, as_role: PointRole, role: &mut Option<PointRole>| {
        if q == p && matches!(role, None | Some(PointRole::Standalone)) {
            *role = Some(as_role);
        }
    };
    match *kind {
        EntityKind::Point(_) => {}
        EntityKind::Line { a, b } => {
            end(a, role);
            end(b, role);
        }
        EntityKind::Circle { center, .. } => weak(center, PointRole::Center, role),
        EntityKind::Arc {
            center,
            start,
            end: e,
        } => {
            weak(center, PointRole::Center, role);
            end(start, role);
            end(e, role);
        }
        EntityKind::Ellipse {
            center,
            major,
            minor,
        } => {
            weak(center, PointRole::Center, role);
            end(major, role);
            end(minor, role);
        }
        EntityKind::Bezier { a, c1, c2, b } => {
            end(a, role);
            end(b, role);
            weak(c1, PointRole::Handle, role);
            weak(c2, PointRole::Handle, role);
        }
        EntityKind::EllipseArc {
            center,
            major,
            minor,
            start,
            end: e,
        } => {
            weak(center, PointRole::Center, role);
            for q in [major, minor, start, e] {
                end(q, role);
            }
        }
    }
}

impl SceneIndex {
    pub fn build(doc: &Document) -> Self {
        let mut index = Self::default();
        index.update(doc);
        index
    }

    /// Bring the index up to date with the document. Shapes are compared in place, point
    /// roles are worked out again only when the sketch's structure changed, and the tree is
    /// rebuilt only when something moved. Shapes moved by [`update_moved`](Self::update_moved)
    /// wait outside the tree until the document's transaction ends, or until there are many.
    pub fn update(&mut self, doc: &Document) {
        if self.revision == Some(doc.revision()) {
            self.rebuild_if_many_moved(doc);
            return;
        }
        // Since the last update only some entities changed (an edit, an undo): only those
        // are looked at.
        if let (Some(revision), Some(lineage)) = (self.revision, self.lineage)
            && let Some(changes) = doc.changes_since(lineage, revision)
            && !changes.layers
            && !self.touches_hidden_curves(doc, &changes.entities)
        {
            self.revision = Some(doc.revision());
            self.update_changed(doc, &changes.entities);
            return;
        }
        self.revision = Some(doc.revision());
        self.lineage = Some(doc.lineage());

        // The points only curves on hidden layers use, left out with them.
        let hidden_only = self.find_hidden(doc);
        let mut structure_changed = false;
        let mut changed: Vec<(EntityId, Option<Shape>)> = Vec::new();
        let mut seen = 0;
        for (id, entity) in doc.iter() {
            if !doc.is_visible(id) || (!hidden_only.is_empty() && hidden_only.contains(&id)) {
                continue;
            }
            let Some(shape) = doc.shape(id) else { continue };
            seen += 1;
            if self.kinds.get(&id) != Some(&entity.kind) {
                structure_changed = true;
            }
            match self.shapes.get_mut(&id) {
                Some(old) if *old == shape => {}
                Some(old) => {
                    changed.push((id, Some(*old)));
                    *old = shape;
                }
                None => {
                    changed.push((id, None));
                    self.shapes.insert(id, shape);
                }
            }
        }
        // Gone (deleted, hidden, or now used only by hidden curves).
        let mut removed: Vec<(EntityId, Shape)> = Vec::new();
        if seen != self.shapes.len() {
            structure_changed = true;
            removed = self
                .shapes
                .iter()
                .filter(|(id, _)| {
                    !doc.contains(**id) || !doc.is_visible(**id) || hidden_only.contains(*id)
                })
                .map(|(id, s)| (*id, *s))
                .collect();
            for (id, _) in &removed {
                self.shapes.remove(id);
                self.moved.remove(id);
            }
        }
        if structure_changed {
            self.kinds.clear();
            self.roles.clear();
            self.users.clear();
            for (id, entity) in doc.iter() {
                if !self.shapes.contains_key(&id) {
                    continue;
                }
                self.kinds.insert(id, entity.kind);
                for p in used_points(&entity.kind) {
                    self.users.entry(p).or_default().insert(id);
                }
                match entity.kind {
                    EntityKind::Point(_) => {
                        self.roles.entry(id).or_insert(PointRole::Standalone);
                    }
                    EntityKind::Line { a, b } => {
                        self.roles.insert(a, PointRole::Endpoint);
                        self.roles.insert(b, PointRole::Endpoint);
                    }
                    EntityKind::Circle { center, .. } => self.mark_center(center),
                    EntityKind::Arc { center, start, end } => {
                        self.mark_center(center);
                        self.roles.insert(start, PointRole::Endpoint);
                        self.roles.insert(end, PointRole::Endpoint);
                    }
                    EntityKind::Ellipse {
                        center,
                        major,
                        minor,
                    } => {
                        self.mark_center(center);
                        self.roles.insert(major, PointRole::Endpoint);
                        self.roles.insert(minor, PointRole::Endpoint);
                    }
                    EntityKind::Bezier { a, c1, c2, b } => {
                        self.roles.insert(a, PointRole::Endpoint);
                        self.roles.insert(b, PointRole::Endpoint);
                        for h in [c1, c2] {
                            // A point that's also an end elsewhere stays an end.
                            let role = self.roles.entry(h).or_insert(PointRole::Handle);
                            if *role == PointRole::Standalone {
                                *role = PointRole::Handle;
                            }
                        }
                    }
                    EntityKind::EllipseArc {
                        center,
                        major,
                        minor,
                        start,
                        end,
                    } => {
                        self.mark_center(center);
                        for p in [major, minor, start, end] {
                            self.roles.insert(p, PointRole::Endpoint);
                        }
                    }
                }
            }
        }
        // The tree is rebuilt in one go when anything moved: bulk loading is fast (about
        // 6 ms for 28,000 entries) and rstar 0.13 can panic on removes mixed with inserts.
        if !changed.is_empty() || !removed.is_empty() || !self.moved.is_empty() {
            // Within a transaction (a drag), geometry that only changed shape (walls' faces
            // following a dragged node, say) waits for the end of it like moved geometry;
            // anything added or removed rebuilds now, taking the moved shapes with it.
            let added = changed.iter().any(|(_, old)| old.is_none());
            if doc.in_transaction() && !added && removed.is_empty() {
                self.moved.extend(changed.iter().map(|(id, _)| *id));
                self.rebuild_if_many_moved(doc);
                return;
            }
            self.rebuild_tree();
        }
    }

    /// How many shapes may wait outside the tree (tested one by one) before it is rebuilt.
    fn moved_limit(&self) -> usize {
        (self.shapes.len() / 64).max(256)
    }

    /// How many shapes may wait outside the tree in the middle of a drag (a transaction):
    /// rebuilding it every frame costs a few milliseconds, which only pays once searches
    /// would otherwise look through a large share of the drawing one shape at a time (a drag
    /// of most of a big drawing, #253).
    fn moved_limit_in_drag(&self) -> usize {
        (self.shapes.len() / 8).max(256)
    }

    /// Rebuild the tree once too many shapes wait outside it (`moved_limit`; in a
    /// transaction, `moved_limit_in_drag`). Done by [`update`](Self::update), and to be
    /// called once a drag frame's [`update_moved`](Self::update_moved) calls are done.
    pub fn rebuild_if_many_moved(&mut self, doc: &Document) {
        let limit = if doc.in_transaction() {
            self.moved_limit_in_drag()
        } else {
            self.moved_limit()
        };
        if self.moved.len() > limit {
            self.rebuild_tree();
        }
    }

    /// [`update`](Self::update) for a few `changed` entities (sorted): their shapes, the
    /// shapes of curves on points among them, and the roles of the points concerned. Shapes
    /// that changed wait outside the tree, tested one by one (as during a drag), until there
    /// are many of them.
    fn update_changed(&mut self, doc: &Document, changed: &[EntityId]) {
        // Curves on a changed point change shape with it.
        let mut look: BTreeSet<EntityId> = changed.iter().copied().collect();
        for id in changed {
            if let Some(users) = self.users.get(id) {
                look.extend(users.iter().copied());
            }
        }
        let mut points: BTreeSet<EntityId> = BTreeSet::new();
        for id in look {
            let entity = doc
                .get(id)
                .filter(|e| doc.is_visible(id) && !(e.kind.is_point() && self.hidden_only(id)))
                .and_then(|e| Some((e.kind, doc.shape(id)?)));
            let old_kind = self.kinds.get(&id).copied();
            let new_kind = entity.map(|(k, _)| k);
            if old_kind != new_kind {
                let (before, after) = (
                    old_kind.as_ref().map(used_points).unwrap_or_default(),
                    new_kind.as_ref().map(used_points).unwrap_or_default(),
                );
                if before != after || old_kind.is_none() != new_kind.is_none() {
                    for p in before.difference(&after) {
                        if let Some(u) = self.users.get_mut(p) {
                            u.remove(&id);
                            if u.is_empty() {
                                self.users.remove(p);
                            }
                        }
                    }
                    for p in after.difference(&before) {
                        self.users.entry(*p).or_default().insert(id);
                    }
                    points.extend(before);
                    points.extend(after);
                    points.insert(id);
                }
                match new_kind {
                    Some(k) => self.kinds.insert(id, k),
                    None => self.kinds.remove(&id),
                };
            }
            match (entity, self.shapes.get_mut(&id)) {
                (Some((_, shape)), Some(old)) => {
                    if *old != shape {
                        *old = shape;
                        self.moved.insert(id);
                    }
                }
                (Some((_, shape)), None) => {
                    self.shapes.insert(id, shape);
                    self.moved.insert(id);
                }
                (None, Some(_)) => {
                    self.shapes.remove(&id);
                    self.moved.insert(id);
                }
                (None, None) => {}
            }
        }
        for p in points {
            self.reconcile_point(doc, p);
            let mut role = self
                .kinds
                .get(&p)
                .filter(|k| k.is_point())
                .map(|_| PointRole::Standalone);
            for u in self.users.get(&p).into_iter().flatten() {
                if let Some(kind) = self.kinds.get(u) {
                    apply_role(kind, p, &mut role);
                }
            }
            match role {
                Some(r) => self.roles.insert(p, r),
                None => self.roles.remove(&p),
            };
        }
        self.rebuild_if_many_moved(doc);
    }

    fn rebuild_tree(&mut self) {
        self.moved.clear();
        let entries = self
            .shapes
            .iter()
            .map(|(id, shape)| Entry {
                id: *id,
                envelope: envelope(shape),
            })
            .collect();
        self.tree = RTree::bulk_load(entries);
    }

    /// During a drag: refresh just the shapes of `ids` (all that can have moved). The tree
    /// keeps their old places while there are few of them (rebuilding it costs more than
    /// the drag); meanwhile searches test the moved shapes one by one, so they're found where
    /// they are now. An id that was added or deleted leaves the index for the next
    /// [`update`](Self::update) to bring up to date.
    pub fn update_moved(&mut self, doc: &Document, ids: impl IntoIterator<Item = EntityId>) {
        let mut known = true;
        for id in ids {
            let shape = doc
                .shape(id)
                .filter(|_| doc.is_visible(id) && !self.hidden_only(id));
            match (shape, self.shapes.get_mut(&id)) {
                (Some(shape), Some(old)) => {
                    if *old != shape {
                        *old = shape;
                        self.moved.insert(id);
                    }
                }
                (None, None) => {}
                _ => known = false,
            }
        }
        if known {
            self.revision = Some(doc.revision());
        }
    }

    /// [`update_moved`](Self::update_moved) for `ids` that may be points moved without the
    /// curves on them (what a wall's regeneration touched: its faces' points, not the faces):
    /// those curves are refreshed too.
    pub fn update_moved_with_curves(
        &mut self,
        doc: &Document,
        ids: impl IntoIterator<Item = EntityId>,
    ) {
        let mut all: Vec<EntityId> = Vec::new();
        for id in ids {
            all.push(id);
            if let Some(users) = self.users.get(&id) {
                all.extend(users.iter().copied());
            }
        }
        all.sort_unstable();
        all.dedup();
        self.update_moved(doc, all);
    }

    /// For the full pass: note the curves on hidden layers and the points each uses (only
    /// when a layer is hidden), and return the points only they use that it would index. (Kept out of
    /// [`update`](Self::update), which stays as it was when nothing is hidden.)
    #[inline(never)]
    fn find_hidden(&mut self, doc: &Document) -> HashSet<EntityId> {
        self.hidden_curves.clear();
        self.hidden_users.clear();
        if !doc.layers().any(|(_, l)| !l.visible) {
            return HashSet::new();
        }
        let mut shown_users: HashSet<EntityId> = HashSet::new();
        for (id, entity) in doc.iter().filter(|(_, e)| !e.kind.is_point()) {
            if !doc.is_visible(id) {
                self.hidden_curves.insert(id);
                for p in used_points(&entity.kind) {
                    self.hidden_users.entry(p).or_default().insert(id);
                }
            } else if doc.shape(id).is_some() {
                shown_users.extend(used_points(&entity.kind));
            }
        }
        // (Those the full pass would index otherwise: on a shown layer, with a shape.)
        self.hidden_users
            .keys()
            .filter(|p| !shown_users.contains(p) && doc.is_visible(**p) && doc.shape(**p).is_some())
            .copied()
            .collect()
    }

    /// Whether `id` is a point used only by curves on hidden layers (not drawn, picked or
    /// snapped to, #359). A point no curve uses goes by its own layer.
    fn hidden_only(&self, id: EntityId) -> bool {
        self.hidden_users.contains_key(&id) && !self.users.contains_key(&id)
    }

    /// Whether changes to `ids` reach a curve on a hidden layer (made, changed, deleted,
    /// shown or hidden), which the points it uses follow: the full pass works them out
    /// again. A drag of shown geometry never does.
    fn touches_hidden_curves(&self, doc: &Document, ids: &[EntityId]) -> bool {
        ids.iter().any(|id| {
            self.hidden_curves.contains(id)
                || doc
                    .get(*id)
                    .is_some_and(|e| !e.kind.is_point() && !doc.is_visible(*id))
        })
    }

    /// Bring point `p` in or out of the index once the curves using it are up to date: out
    /// if only hidden curves use it.
    fn reconcile_point(&mut self, doc: &Document, p: EntityId) {
        let Some(e) = doc.get(p).filter(|e| e.kind.is_point()) else {
            return;
        };
        let want = doc.is_visible(p) && !self.hidden_only(p);
        match (want, self.shapes.contains_key(&p)) {
            (true, false) => {
                if let Some(shape) = doc.shape(p) {
                    self.shapes.insert(p, shape);
                    self.kinds.insert(p, e.kind);
                    self.moved.insert(p);
                }
            }
            (false, true) => {
                self.shapes.remove(&p);
                self.kinds.remove(&p);
                self.moved.insert(p);
            }
            _ => {}
        }
    }

    fn mark_center(&mut self, id: EntityId) {
        let role = self.roles.entry(id).or_insert(PointRole::Center);
        if *role == PointRole::Standalone {
            *role = PointRole::Center;
        }
    }

    pub fn shape(&self, id: EntityId) -> Option<&Shape> {
        self.shapes.get(&id)
    }

    /// Whether the index is up to date with `doc` (as of its last update).
    pub fn is_current(&self, doc: &Document) -> bool {
        self.revision == Some(doc.revision()) && self.lineage == Some(doc.lineage())
    }

    /// The curves on point `id` (as of the last update).
    pub fn users_of(&self, id: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        self.users.get(&id).into_iter().flatten().copied()
    }

    pub fn role(&self, id: EntityId) -> Option<PointRole> {
        self.roles.get(&id).copied()
    }

    /// Entities whose bounding boxes intersect `rect`.
    pub fn query(&self, rect: Bounds) -> impl Iterator<Item = EntityId> + '_ {
        let envelope = AABB::from_corners(rect.min.to_array(), rect.max.to_array());
        // Moved shapes are left out of the tree's answer (their places there are old) and
        // tested where they are now.
        let moved = self.moved.iter().copied().filter(move |id| {
            self.shapes
                .get(id)
                .is_some_and(|s| self::envelope(s).intersects(&envelope))
        });
        self.tree
            .locate_in_envelope_intersecting(envelope)
            .map(|e| e.id)
            .filter(|id| !self.moved.contains(id))
            .chain(moved)
    }

    /// Entities within `tolerance` of `p`, with their distances.
    pub fn near(&self, p: DVec2, tolerance: f64) -> impl Iterator<Item = (EntityId, &Shape, f64)> {
        let rect = Bounds::from_center_half_size(p, DVec2::splat(tolerance));
        self.query(rect).filter_map(move |id| {
            let shape = self.shapes.get(&id)?;
            let d = distance(shape, p);
            (d <= tolerance).then_some((id, shape, d))
        })
    }

    /// The entity to select at `p`: the nearest point within tolerance if any, otherwise the
    /// nearest curve. `accept` filters candidates.
    #[expect(
        clippy::unwrap_used,
        reason = "`near` keeps only distances <= tolerance, so none is NaN and they all compare"
    )]
    pub fn pick(
        &self,
        p: DVec2,
        tolerance: f64,
        accept: impl Fn(EntityId) -> bool,
    ) -> Option<EntityId> {
        self.near(p, tolerance)
            .filter(|(id, ..)| accept(*id))
            // (Ties go to the older entity, whatever order the search finds them in.)
            .min_by(|(ia, sa, da), (ib, sb, db)| {
                let rank = |s: &Shape| u8::from(!matches!(s, Shape::Point(_)));
                (rank(sa), da)
                    .partial_cmp(&(rank(sb), db))
                    .unwrap()
                    .then(ia.cmp(ib))
            })
            .map(|(id, ..)| id)
    }

    /// The line whose midpoint is nearest to `p` within `tolerance`, if any.
    pub fn pick_midpoint(
        &self,
        p: DVec2,
        tolerance: f64,
        accept: impl Fn(EntityId) -> bool,
    ) -> Option<EntityId> {
        let rect = Bounds::from_center_half_size(p, DVec2::splat(tolerance));
        self.query(rect)
            .filter(|id| accept(*id))
            .filter_map(|id| match self.shapes.get(&id) {
                Some(Shape::Line { a, b }) => Some((id, ((*a + *b) * 0.5).distance(p))),
                _ => None,
            })
            .filter(|(_, d)| *d <= tolerance)
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .map(|(id, _)| id)
    }

    /// Box selection of curves and standalone points. A crossing selection takes anything that
    /// touches the box; a window selection only what lies fully inside.
    pub fn select_in_rect(&self, rect: &Bounds, crossing: bool) -> Vec<EntityId> {
        let mut ids: Vec<_> = self
            .query(*rect)
            .filter(|id| {
                let shape = &self.shapes[id];
                let selectable = !matches!(shape, Shape::Point(_))
                    || self.role(*id) == Some(PointRole::Standalone);
                selectable
                    && if crossing {
                        shape_intersects_rect(shape, rect)
                    } else {
                        shape_within_rect(shape, rect)
                    }
            })
            .collect();
        ids.sort();
        ids
    }
}

/// Boxes in a tree, to find the ones near a box quickly. Answers are a little generous
/// (padded for rounding) and in index order, for the caller to test exactly as it would
/// without the tree.
pub struct BoxIndex {
    /// None when a box isn't finite (every box is then an answer).
    tree: Option<
        RTree<rstar::primitives::GeomWithData<rstar::primitives::Rectangle<[f64; 2]>, usize>>,
    >,
    len: usize,
}

impl BoxIndex {
    pub fn new(boxes: impl ExactSizeIterator<Item = Bounds>) -> Self {
        let len = boxes.len();
        let mut finite = true;
        let entries: Vec<_> = boxes
            .enumerate()
            .map(|(i, b)| {
                finite &= b.min.is_finite() && b.max.is_finite();
                rstar::primitives::GeomWithData::new(
                    rstar::primitives::Rectangle::from_corners(b.min.to_array(), b.max.to_array()),
                    i,
                )
            })
            .collect();
        Self {
            tree: finite.then(|| RTree::bulk_load(entries)),
            len,
        }
    }

    /// The boxes that may meet `near` (every one that does), in index order.
    pub fn near(&self, near: &Bounds) -> Vec<usize> {
        let tree = match &self.tree {
            Some(tree) if near.min.is_finite() && near.max.is_finite() => tree,
            _ => return (0..self.len).collect(),
        };
        let pad = DVec2::splat(1e-9 * near.min.abs().max(near.max.abs()).max_element().max(1.0));
        let mut found: Vec<usize> = tree
            .locate_in_envelope_intersecting(AABB::from_corners(
                (near.min - pad).to_array(),
                (near.max + pad).to_array(),
            ))
            .map(|g| g.data)
            .collect();
        found.sort_unstable();
        found
    }
}

#[cfg(test)]
mod tests {
    use drift_core::DVec2;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// Following a document's changes gives the index a fresh build would: shapes, kinds,
    /// roles, and what searches find, through random edits, undo, redo and drags.
    #[test]
    fn following_changes_matches_a_fresh_build() {
        use drift_core::{History, Layer};
        let mut seed = 5u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut doc = Document::new();
        let layers: Vec<_> = (0..3)
            .map(|k| doc.add_layer(Layer::new(format!("L{k}"))))
            .collect();
        let mut history = History::default();
        let mut index = SceneIndex::build(&doc);
        for step in 0..1500 {
            let points: Vec<EntityId> = doc
                .iter()
                .filter(|(_, e)| e.kind.is_point())
                .map(|(id, _)| id)
                .collect();
            let all: Vec<EntityId> = doc.iter().map(|(id, _)| id).collect();
            let pick = |list: &[EntityId], r: f64| list[(r * list.len() as f64) as usize];
            let op = (rnd() * 11.0) as u32;
            let drag = op == 10 && !points.is_empty();
            doc.begin();
            match op {
                _ if points.len() < 4 || op == 0 => {
                    doc.add_point(v(rnd() * 100.0, rnd() * 100.0));
                }
                1 => {
                    let (a, b) = (pick(&points, rnd()), pick(&points, rnd()));
                    let _ = doc.add_line(a, b);
                }
                2 => {
                    let _ = doc.add_circle(pick(&points, rnd()), 1.0 + rnd() * 9.0);
                }
                3 => {
                    let ps: Vec<EntityId> = (0..3).map(|_| pick(&points, rnd())).collect();
                    let _ = doc.add_arc(ps[0], ps[1], ps[2]);
                }
                4 => {
                    let ps: Vec<EntityId> = (0..4).map(|_| pick(&points, rnd())).collect();
                    let _ = doc.add_bezier(ps[0], ps[1], ps[2], ps[3]);
                }
                5 | 10 => {
                    let _ = doc.set_point(pick(&points, rnd()), v(rnd() * 100.0, rnd() * 100.0));
                }
                6 => doc.delete([pick(&all, rnd())]),
                7 => {
                    let _ = doc.set_entity_layer(pick(&all, rnd()), layers[(rnd() * 3.0) as usize]);
                }
                8 => {
                    let l = layers[(rnd() * 3.0) as usize];
                    let mut layer = doc.layer(l).unwrap().clone();
                    layer.visible = !layer.visible;
                    doc.set_layer(l, layer);
                }
                _ => {
                    let curves: Vec<EntityId> = all
                        .iter()
                        .copied()
                        .filter(|id| doc.get(*id).is_some_and(|e| !e.kind.is_point()))
                        .collect();
                    if let Some(&c) = curves.first() {
                        let _ = doc.set_radius(c, 2.0 + rnd() * 5.0);
                    }
                }
            }
            if drag {
                // Mid-drag the index follows moves only, then the drag is let go.
                index.update_moved_with_curves(&doc, doc.entities_touched_in_transaction());
                index.update(&doc);
                assert_eq!(
                    index.shapes,
                    SceneIndex::build(&doc).shapes,
                    "shapes mid-drag, step {step}"
                );
                doc.rollback();
            } else {
                let tx = doc.commit("Edit");
                history.push(tx);
            }
            if rnd() < 0.15 {
                history.undo(&mut doc);
            } else if rnd() < 0.1 {
                history.redo(&mut doc);
            }
            index.update(&doc);
            let fresh = SceneIndex::build(&doc);
            assert_eq!(index.shapes, fresh.shapes, "shapes, step {step}");
            assert_eq!(index.kinds, fresh.kinds, "kinds, step {step}");
            assert_eq!(index.roles, fresh.roles, "roles, step {step}");
            for _ in 0..4 {
                let c = v(rnd() * 100.0, rnd() * 100.0);
                let r = Bounds::from_center_half_size(c, v(rnd() * 30.0, rnd() * 30.0));
                let found = |i: &SceneIndex| {
                    let mut f: Vec<EntityId> = i.query(r).collect();
                    f.sort_unstable();
                    f
                };
                assert_eq!(found(&index), found(&fresh), "query, step {step}");
                assert_eq!(
                    index.pick(c, 3.0, |_| true),
                    fresh.pick(c, 3.0, |_| true),
                    "pick, step {step}"
                );
            }
        }
    }

    /// The end points of curves on a hidden layer are left out with them, though the points
    /// are on a shown layer (layer 0, as importers leave them): not found, not picked. A
    /// point a shown curve also uses stays (#359).
    #[test]
    fn points_only_hidden_curves_use_are_left_out() {
        use drift_core::Layer;
        let mut doc = Document::new();
        let walls = doc.add_layer(Layer::new("Walls"));
        let notes = doc.add_layer(Layer::new("Notes"));
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(10.0, 0.0));
        let c = doc.add_point(v(10.0, 10.0));
        let d = doc.add_point(v(20.0, 10.0));
        let ab = doc.add_line(a, b).unwrap();
        let bc = doc.add_line(b, c).unwrap();
        let cd = doc.add_line(c, d).unwrap();
        doc.set_entity_layer(ab, walls).unwrap();
        doc.set_entity_layer(bc, walls).unwrap();
        doc.set_entity_layer(cd, notes).unwrap();
        let hide = |doc: &mut Document, on: bool| {
            let mut layer = doc.layer(walls).unwrap().clone();
            layer.visible = on;
            doc.set_layer(walls, layer);
        };
        hide(&mut doc, false);
        let index = SceneIndex::build(&doc);
        for p in [a, b] {
            assert_eq!(index.shape(p), None, "{p}");
            assert_eq!(index.role(p), None, "{p}");
        }
        assert_eq!(index.pick(v(0.0, 0.0), 1.0, |_| true), None);
        let all = Bounds::from_center_half_size(v(10.0, 5.0), v(50.0, 50.0));
        let mut found: Vec<EntityId> = index.query(all).collect();
        found.sort_unstable();
        assert_eq!(found, vec![c, d, cd]);
        // The shared point stays, an end of the shown line.
        assert_eq!(index.role(c), Some(PointRole::Endpoint));
        // Shown again, all back.
        hide(&mut doc, true);
        let index = SceneIndex::build(&doc);
        assert_eq!(index.role(a), Some(PointRole::Endpoint));
        assert_eq!(index.pick(v(0.0, 0.0), 1.0, |_| true), Some(a));
    }

    /// Followed edit by edit with a layer hidden, the index keeps leaving out the points only
    /// hidden curves use, as a fresh build does: the last shown line on a shared point
    /// deleted, a shown line drawn from a hidden-only point, a shown line moved onto the
    /// hidden layer and back, and a hidden line deleted (#359).
    #[test]
    fn hidden_only_points_follow_edits() {
        use drift_core::Layer;
        let mut doc = Document::new();
        let walls = doc.add_layer(Layer::new("Walls"));
        let p: Vec<EntityId> = (0..5)
            .map(|k| doc.add_point(v(k as f64 * 10.0, 0.0)))
            .collect();
        let hidden = doc.add_line(p[0], p[1]).unwrap();
        doc.set_entity_layer(hidden, walls).unwrap();
        let shown = doc.add_line(p[1], p[2]).unwrap();
        let other = doc.add_line(p[3], p[4]).unwrap();
        let mut layer = doc.layer(walls).unwrap().clone();
        layer.visible = false;
        doc.set_layer(walls, layer);
        let mut index = SceneIndex::build(&doc);
        let check = |index: &mut SceneIndex, doc: &Document, what: &str| {
            index.update(doc);
            let fresh = SceneIndex::build(doc);
            assert_eq!(index.shapes, fresh.shapes, "{what}");
            assert_eq!(index.kinds, fresh.kinds, "{what}");
            assert_eq!(index.roles, fresh.roles, "{what}");
        };
        assert!(index.shape(p[0]).is_none() && index.shape(p[1]).is_some());
        // The last shown line on p1 deleted: p1 is hidden-only now.
        doc.delete([shown]);
        check(&mut index, &doc, "shared point's shown line deleted");
        assert!(index.shape(p[1]).is_none());
        // A shown line drawn from it brings it back (the deleted line took p2 with it).
        let q = doc.add_point(v(10.0, 10.0));
        doc.add_line(p[1], q).unwrap();
        check(&mut index, &doc, "shown line from a hidden-only point");
        assert!(index.shape(p[1]).is_some());
        // A shown line moved onto the hidden layer.
        doc.set_entity_layer(other, walls).unwrap();
        check(&mut index, &doc, "shown line hidden");
        assert!(index.shape(p[3]).is_none());
        // A hidden line deleted (with p0, which nothing else uses).
        doc.delete([hidden]);
        check(&mut index, &doc, "hidden line deleted");
        assert!(!doc.contains(p[0]));
        assert_eq!(index.role(p[1]), Some(PointRole::Endpoint));
        // A line moved back onto a shown layer.
        doc.set_entity_layer(other, drift_core::LayerId::DEFAULT)
            .unwrap();
        check(&mut index, &doc, "hidden line shown");
        assert_eq!(index.role(p[3]), Some(PointRole::Endpoint));
    }

    #[test]
    fn roles_and_picking_prefer_points() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        let c = doc.add_point(v(5.0, 5.0));
        let circle = doc.add_circle(c, 2.0).unwrap();
        let lone = doc.add_point(v(20.0, 20.0));
        let index = SceneIndex::build(&doc);

        assert_eq!(index.role(a), Some(PointRole::Endpoint));
        assert_eq!(index.role(c), Some(PointRole::Center));
        assert_eq!(index.role(lone), Some(PointRole::Standalone));

        // Near the line's end: the end point wins over the line.
        assert_eq!(index.pick(v(0.3, 0.2), 1.0, |_| true), Some(a));
        // Mid-line: the line.
        assert_eq!(index.pick(v(5.0, 0.4), 1.0, |_| true), Some(line));
        // On the circle.
        assert_eq!(index.pick(v(5.0, 7.1), 0.5, |_| true), Some(circle));
        // Midpoints are pickable separately.
        assert_eq!(index.pick_midpoint(v(5.2, 0.3), 1.0, |_| true), Some(line));
        assert_eq!(index.pick_midpoint(v(2.0, 0.0), 1.0, |_| true), None);
        // Excluded candidates are skipped.
        assert_eq!(index.pick(v(0.3, 0.2), 1.0, |id| id != a), Some(line));
        assert_eq!(index.pick(v(50.0, 50.0), 1.0, |_| true), None);
    }

    #[test]
    fn rebuilds_only_on_change() {
        let mut doc = Document::new();
        let p = doc.add_point(v(1.0, 1.0));
        let mut index = SceneIndex::build(&doc);
        assert_eq!(index.pick(v(1.0, 1.0), 0.1, |_| true), Some(p));
        doc.set_point(p, v(5.0, 5.0)).unwrap();
        index.update(&doc);
        assert_eq!(index.pick(v(1.0, 1.0), 0.1, |_| true), None);
        assert_eq!(index.pick(v(5.0, 5.0), 0.1, |_| true), Some(p));
    }

    #[test]
    fn moves_in_a_transaction_are_found_before_the_rebuild() {
        let mut doc = Document::new();
        let p = doc.add_point(v(1.0, 1.0));
        let q = doc.add_point(v(-3.0, -3.0));
        let mut index = SceneIndex::build(&doc);
        doc.begin();
        doc.set_point(p, v(5.0, 5.0)).unwrap();
        index.update_moved(&doc, [p]);
        index.update(&doc);
        assert!(!index.moved.is_empty(), "no rebuild while the drag goes on");
        assert_eq!(index.pick(v(1.0, 1.0), 0.1, |_| true), None);
        assert_eq!(index.pick(v(5.0, 5.0), 0.1, |_| true), Some(p));
        assert_eq!(index.pick(v(-3.0, -3.0), 0.1, |_| true), Some(q));
        // A change the index finds itself (not told of) waits too, an addition included.
        doc.set_point(q, v(-8.0, -8.0)).unwrap();
        index.update(&doc);
        assert!(index.moved.contains(&q));
        assert_eq!(index.pick(v(-8.0, -8.0), 0.1, |_| true), Some(q));
        let r = doc.add_point(v(30.0, 30.0));
        index.update(&doc);
        assert!(
            index.moved.contains(&r),
            "an addition waits outside the tree too"
        );
        assert_eq!(index.pick(v(30.0, 30.0), 0.1, |_| true), Some(r));
        assert_eq!(index.pick(v(5.0, 5.0), 0.1, |_| true), Some(p));
        doc.set_point(p, v(6.0, 6.0)).unwrap();
        index.update_moved(&doc, [p]);
        doc.commit("move");
        index.update(&doc);
        // A few changes go on waiting after the transaction (the tree is rebuilt once
        // there are many).
        assert!(index.moved.contains(&p));
        assert_eq!(index.pick(v(6.0, 6.0), 0.1, |_| true), Some(p));
        assert_eq!(index.pick(v(1.0, 1.0), 0.1, |_| true), None);
    }

    /// A drag of most of a big drawing doesn't leave every search testing all it moved one
    /// by one (#253): the tree is rebuilt once many shapes wait outside it, mid-drag too.
    #[test]
    fn a_drag_of_many_shapes_puts_them_back_in_the_tree() {
        let mut doc = Document::new();
        let points: Vec<EntityId> = (0..2000).map(|k| doc.add_point(v(k as f64, 0.0))).collect();
        let mut index = SceneIndex::build(&doc);
        doc.begin();
        // A few hundred moved wait outside the tree: rebuilding it every frame for them
        // would cost more than searching them.
        for &p in &points[..200] {
            let at = doc.point(p).unwrap();
            doc.set_point(p, at + v(0.0, 50.0)).unwrap();
        }
        index.update_moved(&doc, points[..200].iter().copied());
        index.rebuild_if_many_moved(&doc);
        assert_eq!(index.moved.len(), 200, "rebuilt for a few");
        assert_eq!(index.pick(v(7.0, 50.0), 0.1, |_| true), Some(points[7]));
        for &p in &points[..200] {
            let at = doc.point(p).unwrap();
            doc.set_point(p, at - v(0.0, 50.0)).unwrap();
        }
        for &p in &points {
            let at = doc.point(p).unwrap();
            doc.set_point(p, at + v(0.0, 50.0)).unwrap();
        }
        index.update_moved(&doc, points.iter().copied());
        index.rebuild_if_many_moved(&doc);
        assert!(
            index.moved.is_empty(),
            "{} left out of the tree",
            index.moved.len()
        );
        assert_eq!(index.pick(v(7.0, 50.0), 0.1, |_| true), Some(points[7]));
        assert_eq!(index.pick(v(7.0, 0.0), 0.1, |_| true), None);
        // Found by an update as well (what the canvas runs every frame).
        for &p in &points {
            let at = doc.point(p).unwrap();
            doc.set_point(p, at + v(0.0, 50.0)).unwrap();
        }
        index.update(&doc);
        assert!(
            index.moved.is_empty(),
            "{} left out of the tree",
            index.moved.len()
        );
        assert_eq!(index.pick(v(7.0, 100.0), 0.1, |_| true), Some(points[7]));
        doc.commit("Move");
    }

    #[test]
    fn box_selection_skips_end_points() {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        let lone = doc.add_point(v(2.0, 1.0));
        let index = SceneIndex::build(&doc);
        let all = Bounds {
            min: v(-1.0, -1.0),
            max: v(11.0, 2.0),
        };
        assert_eq!(index.select_in_rect(&all, false), vec![line, lone]);
        let part = Bounds {
            min: v(-1.0, -1.0),
            max: v(5.0, 2.0),
        };
        assert_eq!(index.select_in_rect(&part, false), vec![lone]);
        assert_eq!(index.select_in_rect(&part, true), vec![line, lone]);
    }
}
