use std::collections::{BTreeSet, HashMap};

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
    /// them one by one; the tree is rebuilt once the transaction ends. (Ordered, so results
    /// don't depend on hash order.)
    moved: BTreeSet<EntityId>,
    roles: HashMap<EntityId, PointRole>,
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
    /// wait for the tree until the document's transaction ends.
    pub fn update(&mut self, doc: &Document) {
        if self.revision == Some(doc.revision()) {
            if !self.moved.is_empty() && !doc.in_transaction() {
                self.rebuild_tree();
            }
            return;
        }
        self.revision = Some(doc.revision());

        let mut structure_changed = false;
        let mut changed: Vec<(EntityId, Option<Shape>)> = Vec::new();
        let mut seen = 0;
        for (id, entity) in doc.iter() {
            if !doc.is_visible(id) {
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
        // Gone (deleted or hidden).
        let mut removed: Vec<(EntityId, Shape)> = Vec::new();
        if seen != self.shapes.len() {
            structure_changed = true;
            removed = self
                .shapes
                .iter()
                .filter(|(id, _)| !doc.contains(**id) || !doc.is_visible(**id))
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
            for (id, entity) in doc.iter() {
                if !self.shapes.contains_key(&id) {
                    continue;
                }
                self.kinds.insert(id, entity.kind);
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
                return;
            }
            self.rebuild_tree();
        }
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
    /// keeps their old places until the transaction ends (rebuilding it costs more than the
    /// drag); meanwhile searches test the moved shapes one by one, so they're found where
    /// they are now. An id that was added or deleted leaves the index for the next
    /// [`update`](Self::update) to bring up to date.
    pub fn update_moved(&mut self, doc: &Document, ids: impl IntoIterator<Item = EntityId>) {
        let mut known = true;
        for id in ids {
            let shape = doc.shape(id).filter(|_| doc.is_visible(id));
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

    fn mark_center(&mut self, id: EntityId) {
        let role = self.roles.entry(id).or_insert(PointRole::Center);
        if *role == PointRole::Standalone {
            *role = PointRole::Center;
        }
    }

    pub fn shape(&self, id: EntityId) -> Option<&Shape> {
        self.shapes.get(&id)
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
    pub fn pick(
        &self,
        p: DVec2,
        tolerance: f64,
        accept: impl Fn(EntityId) -> bool,
    ) -> Option<EntityId> {
        self.near(p, tolerance)
            .filter(|(id, ..)| accept(*id))
            .min_by(|(_, sa, da), (_, sb, db)| {
                let rank = |s: &Shape| u8::from(!matches!(s, Shape::Point(_)));
                (rank(sa), da).partial_cmp(&(rank(sb), db)).unwrap()
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

#[cfg(test)]
mod tests {
    use drift_core::DVec2;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
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
        // A change the index finds itself (not told of) waits too, unless it adds something.
        doc.set_point(q, v(-8.0, -8.0)).unwrap();
        index.update(&doc);
        assert!(index.moved.contains(&q));
        assert_eq!(index.pick(v(-8.0, -8.0), 0.1, |_| true), Some(q));
        let r = doc.add_point(v(30.0, 30.0));
        index.update(&doc);
        assert!(index.moved.is_empty(), "an addition rebuilds the tree");
        assert_eq!(index.pick(v(30.0, 30.0), 0.1, |_| true), Some(r));
        assert_eq!(index.pick(v(5.0, 5.0), 0.1, |_| true), Some(p));
        doc.set_point(p, v(6.0, 6.0)).unwrap();
        index.update_moved(&doc, [p]);
        doc.commit("move");
        index.update(&doc);
        assert!(index.moved.is_empty(), "rebuilt once the transaction ends");
        assert_eq!(index.pick(v(6.0, 6.0), 0.1, |_| true), Some(p));
        assert_eq!(index.pick(v(1.0, 1.0), 0.1, |_| true), None);
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
