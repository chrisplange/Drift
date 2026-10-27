//! Bézier curves in the editor: which handles show (as in Illustrator, only around picked or
//! hovered curves), handle lines, anchors carrying their handles when moved, smooth and
//! corner anchors, and curvature combs.

use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use drift_core::shape::{bezier_curvature, bezier_point, bezier_tangent};
use drift_core::{Constraint, DVec2, EntityId, EntityKind, Shape, SketchItem};

use crate::editor::Editor;

/// Handle point → (its curve, the anchor it belongs to).
pub type HandleOwners = HashMap<EntityId, (EntityId, EntityId)>;

/// Every Bézier with its anchors and handles (a, c1, c2, b).
type Beziers = Rc<Vec<(EntityId, [EntityId; 4])>>;

/// [`Editor::handle_owners`] and the Béziers, for one structure of the document.
pub(crate) struct HandleCache {
    structure: u64,
    owners: Rc<HandleOwners>,
    beziers: Beziers,
}

impl Editor {
    /// Every Bézier handle point, with its curve and anchor (cached while the document's
    /// structure is unchanged).
    pub fn handle_owners(&self) -> Rc<HandleOwners> {
        self.handles().0
    }

    /// The handle owners and every Bézier.
    fn handles(&self) -> (Rc<HandleOwners>, Beziers) {
        let structure = self.doc.structure();
        if let Some(cache) = &*self.handle_cache.borrow()
            && cache.structure == structure
        {
            return (cache.owners.clone(), cache.beziers.clone());
        }
        let mut map = HandleOwners::new();
        let mut beziers = Vec::new();
        for (id, e) in self.doc.iter() {
            if let EntityKind::Bezier { a, c1, c2, b } = e.kind {
                map.insert(c1, (id, a));
                map.insert(c2, (id, b));
                beziers.push((id, [a, c1, c2, b]));
            }
        }
        // A point that is also an end of some curve isn't (only) a handle.
        let ends: BTreeSet<EntityId> = self
            .doc
            .iter()
            .filter_map(|(_, e)| e.kind.ends())
            .flatten()
            .collect();
        map.retain(|h, _| !ends.contains(h));
        let (map, beziers) = (Rc::new(map), Rc::new(beziers));
        *self.handle_cache.borrow_mut() = Some(HandleCache {
            structure,
            owners: map.clone(),
            beziers: beziers.clone(),
        });
        (map, beziers)
    }

    /// The curves whose handles show: picked or hovered, or with a picked or hovered anchor
    /// or handle.
    pub fn curves_showing_handles(&self) -> BTreeSet<EntityId> {
        let touched = |id: &EntityId| {
            self.selection.contains(id) || self.hovered == Some(*id) || self.highlight.contains(id)
        };
        // (Only the Béziers are looked at: none in most drawings.)
        self.handles()
            .1
            .iter()
            .filter(|(id, points)| touched(id) || points.iter().any(touched))
            .map(|(id, _)| *id)
            .collect()
    }

    /// Handles not showing (they're neither drawn nor picked).
    pub fn hidden_handles(&self) -> BTreeSet<EntityId> {
        let shown = self.curves_showing_handles();
        let mut hidden: BTreeSet<EntityId> = self
            .handle_owners()
            .iter()
            .filter(|(_, (curve, _))| !shown.contains(curve))
            .map(|(h, _)| *h)
            .collect();
        // (And walls' centrelines, unless shown.)
        hidden.extend(self.hidden_centrelines());
        hidden
    }

    /// Whether an entity can be picked: not on a locked layer, and not a hidden handle.
    pub fn pickable(&self, id: EntityId, hidden: &BTreeSet<EntityId>) -> bool {
        !self.is_locked(id) && !hidden.contains(&id) && self.pick_filter.allows(&self.doc, id)
    }

    /// Thin lines from anchors to their showing handles.
    pub fn handle_lines(&self, color: [u8; 3]) -> Vec<SketchItem> {
        let shown = self.curves_showing_handles();
        let mut items = Vec::new();
        for (h, (curve, anchor)) in self.handle_owners().iter() {
            if !shown.contains(curve) {
                continue;
            }
            if let (Ok(a), Ok(p)) = (self.doc.point(*anchor), self.doc.point(*h)) {
                items.push(SketchItem::new(Shape::Line { a, b: p }).with_look(
                    Some(color),
                    0,
                    drift_core::LineType::Continuous,
                ));
            }
        }
        items
    }

    /// The handles of Bézier curves that leave or arrive at `anchor`.
    fn handles_at(&self, anchor: EntityId) -> Vec<EntityId> {
        self.doc
            .iter()
            .filter_map(|(_, e)| match e.kind {
                EntityKind::Bezier { a, c1, .. } if a == anchor => Some(c1),
                EntityKind::Bezier { c2, b, .. } if b == anchor => Some(c2),
                _ => None,
            })
            .collect()
    }

    /// `ids` plus the handles of any anchors among them, so moving an anchor carries its
    /// handles with it (as in Illustrator).
    pub fn with_handles(&self, ids: &[EntityId]) -> Vec<EntityId> {
        let mut out: Vec<EntityId> = ids.to_vec();
        for &id in ids {
            if self.doc.get(id).is_some_and(|e| e.kind.is_point()) {
                for h in self.handles_at(id) {
                    if !out.contains(&h) {
                        out.push(h);
                    }
                }
            }
        }
        out
    }

    /// The Smooth relation at an anchor, if it has one.
    pub fn smooth_at(&self, anchor: EntityId) -> Option<drift_core::ConstraintId> {
        self.doc.constraints().find_map(|(id, c)| match c {
            Constraint::Smooth { point, .. } if *point == anchor => Some(id),
            _ => None,
        })
    }

    /// Make an anchor between two curve handles smooth (its handles in line) or a corner.
    pub fn toggle_smooth(&mut self, anchor: EntityId) -> Result<bool, String> {
        if let Some(id) = self.smooth_at(anchor) {
            self.doc.begin();
            self.doc.remove_constraint(id);
            self.commit("Corner point");
            return Ok(false);
        }
        let handles = self.handles_at(anchor);
        let [a, b] = handles[..] else {
            return Err("A smooth point needs a curve on each side".into());
        };
        self.doc.begin();
        if let Err(e) = self.doc.add_constraint(Constraint::Smooth {
            point: anchor,
            a,
            b,
        }) {
            self.doc.rollback();
            self.sync();
            return Err(e.to_string());
        }
        if !self.commit("Smooth point") {
            return Err("It can't be made smooth with its other relations".into());
        }
        Ok(true)
    }

    /// Curvature combs for the picked curves: spikes along the normal, as long as the
    /// curvature (scaled so the longest is `reach` world units), and the line joining their
    /// tips.
    pub fn curvature_combs(&self, reach: f64, color: [u8; 3]) -> Vec<SketchItem> {
        let curves: Vec<[DVec2; 4]> = self
            .selection
            .iter()
            .filter_map(|id| match self.doc.shape(*id)? {
                Shape::Bezier { p } => Some(p),
                _ => None,
            })
            .collect();
        if curves.is_empty() {
            return Vec::new();
        }
        const SAMPLES: usize = 40;
        let samples: Vec<Vec<(DVec2, DVec2, f64)>> = curves
            .iter()
            .map(|p| {
                (0..=SAMPLES)
                    .map(|i| {
                        let t = i as f64 / SAMPLES as f64;
                        let n = bezier_tangent(p, t).normalize_or_zero().perp();
                        (bezier_point(p, t), n, bezier_curvature(p, t))
                    })
                    .collect()
            })
            .collect();
        let peak = samples
            .iter()
            .flatten()
            .map(|(_, _, k)| k.abs())
            .fold(0.0, f64::max);
        if peak < 1e-12 {
            return Vec::new();
        }
        let scale = reach / peak;
        let look = |s: Shape| {
            SketchItem::new(s).with_look(Some(color), 0, drift_core::LineType::Continuous)
        };
        let mut items = Vec::new();
        for curve in samples {
            // Spikes point away from the centre of curvature, as in CAD combs.
            let tips: Vec<DVec2> = curve
                .iter()
                .map(|(q, n, k)| *q - *n * (k * scale))
                .collect();
            for ((q, ..), tip) in curve.iter().zip(&tips) {
                items.push(look(Shape::Line { a: *q, b: *tip }));
            }
            for w in tips.windows(2) {
                items.push(look(Shape::Line { a: w[0], b: w[1] }));
            }
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_solver::Param;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// Two curves meeting at a smooth anchor.
    fn s_curve(ed: &mut Editor) -> ([EntityId; 7], [EntityId; 2]) {
        ed.doc.begin();
        let p = [
            v(0.0, 0.0),
            v(10.0, 10.0),
            v(20.0, 10.0),
            v(30.0, 0.0),
            v(40.0, -10.0),
            v(50.0, -10.0),
            v(60.0, 0.0),
        ]
        .map(|q| ed.doc.add_point(q));
        let c1 = ed.doc.add_bezier(p[0], p[1], p[2], p[3]).unwrap();
        let c2 = ed.doc.add_bezier(p[3], p[4], p[5], p[6]).unwrap();
        ed.commit("Curves");
        (p, [c1, c2])
    }

    #[test]
    fn handles_show_around_picked_curves_and_move_with_anchors() {
        let mut ed = Editor::default();
        let (p, [c1, _]) = s_curve(&mut ed);
        assert_eq!(ed.handle_owners().len(), 4);
        assert_eq!(ed.hidden_handles().len(), 4, "nothing picked: all hidden");
        ed.selection = [c1].into();
        assert_eq!(ed.hidden_handles(), [p[4], p[5]].into());
        assert_eq!(ed.handle_lines([0, 0, 255]).len(), 2);
        // Picking the shared anchor shows both curves' handles.
        ed.selection = [p[3]].into();
        assert!(ed.hidden_handles().is_empty());
        let moved = ed.with_handles(&[p[3]]);
        assert_eq!(moved.len(), 3);
        assert!(moved.contains(&p[2]) && moved.contains(&p[4]));
    }

    #[test]
    fn smooth_anchors_keep_their_handles_in_line() {
        let mut ed = Editor::default();
        let (p, [c1, _]) = s_curve(&mut ed);
        assert_eq!(ed.toggle_smooth(p[3]), Ok(true));
        // Dragging one handle swings the other round.
        ed.doc.begin();
        assert!(ed.drag(&[p[2]], v(0.0, 8.0), &[]));
        ed.commit("Drag");
        let [a, h1, h2] = [p[3], p[2], p[4]].map(|q| ed.doc.point(q).unwrap());
        assert!((h1 - a).perp_dot(h2 - a).abs() < 1e-6, "in line");
        assert!((h1 - a).dot(h2 - a) < 0.0, "on opposite sides");
        assert_eq!(ed.toggle_smooth(p[3]), Ok(false));
        assert!(ed.smooth_at(p[3]).is_none());
        assert!(ed.toggle_smooth(p[0]).is_err(), "an end has one curve");
        // A comb for the picked curve.
        ed.selection = [c1].into();
        assert_eq!(ed.curvature_combs(5.0, [0, 0, 0]).len(), 41 + 40);
    }

    /// As a drawing program does it: dragging one handle of a smooth anchor swings the other
    /// round at its own length, the anchor and everything else staying where they are; dragging a curve carries the next curve's handle along with the shared anchor.
    #[test]
    fn smooth_anchors_move_like_a_pen_tool() {
        let mut ed = Editor::default();
        let (p, [c1, _]) = s_curve(&mut ed);
        assert_eq!(ed.toggle_smooth(p[3]), Ok(true));
        let before = p.map(|q| ed.doc.point(q).unwrap());
        let reach = before[4].distance(before[3]);
        ed.doc.begin();
        let dragged: Vec<Param> = Param::point(p[2]).into_iter().collect();
        assert!(ed.drag(&[p[2]], v(-4.0, 9.0), &dragged));
        ed.commit("Drag");
        let after = p.map(|q| ed.doc.point(q).unwrap());
        let (anchor, h1, h2) = (after[3], after[2], after[4]);
        assert!(anchor.distance(before[3]) < 1e-12, "the anchor stays");
        assert!((h2.distance(anchor) - reach).abs() < 1e-12, "at its length");
        assert!(((h1 - anchor).normalize() + (h2 - anchor).normalize()).length() < 1e-12);
        for i in [0, 1, 5, 6] {
            assert!(after[i].distance(before[i]) < 1e-12, "point {i} moved");
        }
        // The first curve dragged: the anchor takes the next curve's handle along.
        ed.doc.begin();
        let points = ed.doc.defining_points([c1]);
        let dragged: Vec<Param> = points.iter().copied().flat_map(Param::point).collect();
        let delta = v(3.0, -2.0);
        assert!(ed.drag(&[c1], delta, &dragged));
        ed.commit("Drag");
        assert!(ed.doc.point(p[4]).unwrap().distance(after[4] + delta) < 1e-12);
        for i in [5, 6] {
            let now = ed.doc.point(p[i]).unwrap();
            assert!(now.distance(after[i]) < 1e-12, "point {i} moved");
        }
    }
}
