use std::collections::HashMap;

use drift_core::{Bounds, DVec2, Document, EntityId, EntityKind, Shape};
use rstar::{AABB, RTree, RTreeObject};

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
}

struct Entry {
    id: EntityId,
    envelope: AABB<[f64; 2]>,
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
    roles: HashMap<EntityId, PointRole>,
}

impl Default for SceneIndex {
    fn default() -> Self {
        Self {
            revision: None,
            tree: RTree::new(),
            shapes: HashMap::new(),
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

    /// Rebuild if the document changed since the last build.
    pub fn update(&mut self, doc: &Document) {
        if self.revision == Some(doc.revision()) {
            return;
        }
        self.revision = Some(doc.revision());
        self.shapes.clear();
        self.roles.clear();

        let mut entries = Vec::with_capacity(doc.len());
        for (id, entity) in doc.iter() {
            if !doc.is_visible(id) {
                continue;
            }
            let Some(shape) = doc.shape(id) else { continue };
            let b = shape.bounds();
            entries.push(Entry {
                id,
                envelope: AABB::from_corners(b.min.to_array(), b.max.to_array()),
            });
            self.shapes.insert(id, shape);
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
            }
        }
        self.tree = RTree::bulk_load(entries);
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
        self.tree
            .locate_in_envelope_intersecting(envelope)
            .map(|e| e.id)
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
