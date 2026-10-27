//! Transform handles: a box around the picked geometry whose handles scale and stretch it,
//! with a turn just outside each corner.
//!
//! Scaling moves each defining point towards or away from an anchor and lets the solver
//! settle the rest, so relations and driving dimensions keep their say: a dimensioned
//! width stays as it is while the undimensioned height stretches. Scaling the dimensions
//! too (Alt) gives each driving length inside the picked geometry what the stretch makes
//! of it: stretched sideways, a width grows and a height doesn't.

use std::collections::BTreeSet;

use drift_core::{Bounds, Constraint, DVec2, EntityId, EntityKind};
use drift_solver::Param;

use crate::editor::Editor;

/// The smallest a scale may shrink to along an axis (no flipping through zero).
pub const MIN_SCALE: f64 = 0.01;

/// How near a transform handle the pointer must be to take it, for a pick tolerance: a
/// little inside it, so the handles don't crowd out what is next to them.
pub fn handle_reach(tolerance: f64) -> f64 {
    tolerance * 0.75
}

/// A handle on the box: which side it is on along each axis (-1, 0 or 1; both 0 is not a
/// handle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// Scale from a corner, or stretch from the middle of a side.
    Scale { x: i8, y: i8 },
    /// Turn, from just outside a corner.
    Turn { x: i8, y: i8 },
}

/// The box transform handles sit on.
#[derive(Clone, Debug, PartialEq)]
pub struct TransformBox {
    /// The curves it transforms.
    pub ids: Vec<EntityId>,
    /// Their bounds.
    pub bounds: Bounds,
    /// Around one closed curve: it sits further out, clear of the curve's own grips (a
    /// circle's edge resizes it, a path's points move).
    pub lone: bool,
    /// Scales keep the shape (one circle: a stretch would leave it as it was).
    pub uniform: bool,
}

impl TransformBox {
    /// The box drawn, `pad` outside the geometry (so its handles don't sit on the
    /// geometry's own points, which can still be dragged).
    pub fn padded(&self, pad: f64) -> Bounds {
        let pad = if self.lone { 2.0 * pad } else { pad };
        Bounds {
            min: self.bounds.min - DVec2::splat(pad),
            max: self.bounds.max + DVec2::splat(pad),
        }
    }

    /// Where a handle is, on the box padded by `pad`.
    pub fn handle_at_side(&self, x: i8, y: i8, pad: f64) -> DVec2 {
        let b = self.padded(pad);
        let pick = |side: i8, lo: f64, hi: f64| match side {
            -1 => lo,
            1 => hi,
            _ => (lo + hi) / 2.0,
        };
        DVec2::new(pick(x, b.min.x, b.max.x), pick(y, b.min.y, b.max.y))
    }

    /// The eight scale handles and where they are.
    pub fn scale_handles(&self, pad: f64) -> Vec<(Handle, DVec2)> {
        let mut out = Vec::new();
        for y in [-1i8, 0, 1] {
            for x in [-1i8, 0, 1] {
                if (x, y) == (0, 0) {
                    continue;
                }
                // A side too short to grab separately from its corners has no middle handle.
                let b = self.bounds;
                if (x == 0 && b.max.x - b.min.x < 1e-9) || (y == 0 && b.max.y - b.min.y < 1e-9) {
                    continue;
                }
                out.push((Handle::Scale { x, y }, self.handle_at_side(x, y, pad)));
            }
        }
        out
    }

    /// The handle under `p`: a scale handle within `tol` of it, or a turn just outside a
    /// corner (up to three times `tol` beyond it, outside the box).
    pub fn handle_at(&self, p: DVec2, tol: f64, pad: f64) -> Option<Handle> {
        let scale = self
            .scale_handles(pad)
            .into_iter()
            .filter(|(_, at)| (p - *at).abs().max_element() <= tol)
            .min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
            .map(|(h, _)| h);
        if scale.is_some() {
            return scale;
        }
        let b = self.padded(pad);
        let outside = p.x < b.min.x || p.x > b.max.x || p.y < b.min.y || p.y > b.max.y;
        if !outside {
            return None;
        }
        [(-1i8, -1i8), (1, -1), (-1, 1), (1, 1)]
            .into_iter()
            .find(|&(x, y)| self.handle_at_side(x, y, pad).distance(p) <= 3.0 * tol)
            .map(|(x, y)| Handle::Turn { x, y })
    }

    /// What a scale from handle (`x`, `y`) is about: the opposite side, or the middle.
    pub fn anchor(&self, x: i8, y: i8, about_centre: bool) -> DVec2 {
        let b = self.bounds;
        let centre = b.center();
        let opposite = |side: i8, lo: f64, hi: f64, mid: f64| {
            if about_centre {
                mid
            } else {
                match side {
                    1 => lo,
                    -1 => hi,
                    _ => mid,
                }
            }
        };
        DVec2::new(
            opposite(x, b.min.x, b.max.x, centre.x),
            opposite(y, b.min.y, b.max.y, centre.y),
        )
    }
}

/// Round a scale so the box's size comes to a whole number of `step`s (mm) along each axis
/// the handle moves; keeping the shape, along the axis that decides it, and the other the
/// same. `size` is the box's size before the scale. Never below one step.
pub fn round_scale(
    scale: DVec2,
    handle: (i8, i8),
    size: DVec2,
    step: f64,
    proportional: bool,
) -> DVec2 {
    let round = |s: f64, length: f64| {
        if length < 1e-9 || step <= 0.0 {
            return s;
        }
        ((length * s / step).round() * step).max(step) / length
    };
    let (x, y) = (
        handle.0 != 0 && size.x > 1e-9,
        handle.1 != 0 && size.y > 1e-9,
    );
    if proportional {
        // The axis the factor came from: the one that grew the most, if both move.
        let along_x = x && (!y || scale.x >= scale.y);
        let s = if along_x {
            round(scale.x, size.x)
        } else if y {
            round(scale.y, size.y)
        } else {
            scale.x
        };
        return DVec2::splat(s);
    }
    DVec2::new(
        if x { round(scale.x, size.x) } else { scale.x },
        if y { round(scale.y, size.y) } else { scale.y },
    )
}

/// The scale a handle drag comes to: the pointer's distance from the anchor over the
/// press's, along each axis the handle moves. `proportional` keeps the shape (the larger
/// of the two, or the one axis an edge handle moves). Never below [`MIN_SCALE`].
pub fn scale_for(
    handle: (i8, i8),
    anchor: DVec2,
    press: DVec2,
    now: DVec2,
    proportional: bool,
) -> DVec2 {
    let along = |side: i8, a: f64, from: f64, to: f64| {
        let span = from - a;
        if side == 0 || span.abs() < 1e-12 {
            None
        } else {
            Some(((to - a) / span).max(MIN_SCALE))
        }
    };
    let sx = along(handle.0, anchor.x, press.x, now.x);
    let sy = along(handle.1, anchor.y, press.y, now.y);
    if proportional {
        let s = match (sx, sy) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => 1.0,
        };
        DVec2::splat(s)
    } else {
        DVec2::new(sx.unwrap_or(1.0), sy.unwrap_or(1.0))
    }
}

impl Editor {
    /// The box for transform handles: around a picked area's outline, around one picked
    /// closed curve (a circle, an ellipse, a path closed on itself), or around two or more
    /// picked curves. None for a lone open curve (a line or an arc has handles of its own)
    /// and while copies of blocks or text are picked with them (they don't scale).
    pub fn transform_box(&mut self) -> Option<TransformBox> {
        if !self.selected_instances.is_empty() || self.picks_annotations() {
            return None;
        }
        // (An opening slides along its wall and flips by its arrows: no box.)
        if self.doc.has_openings()
            && !self.selection.is_empty()
            && self
                .selection
                .iter()
                .all(|id| self.opening_of(*id).is_some())
        {
            return None;
        }
        let region = self.region().filter(|r| {
            let outline = self.region_curves(r);
            self.selection.iter().all(|id| outline.contains(id))
        });
        let ids: Vec<EntityId> = match &region {
            // (No handles on a locked outline.)
            Some(r) => Some(self.region_curves(r))
                .filter(|ids| !ids.iter().any(|id| self.is_locked(*id)))?,
            None => {
                let curves: Vec<EntityId> = self
                    .selection
                    .iter()
                    .copied()
                    .filter(|id| self.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
                    .collect();
                let closed = || {
                    self.doc
                        .shape(curves[0])
                        .is_some_and(|s| s.is_closed() || self.closed_bezier(curves[0]))
                };
                if curves.is_empty()
                    || curves.len() != self.selection.len()
                    || (curves.len() == 1 && !closed())
                {
                    return None;
                }
                curves
            }
        };
        if ids.iter().any(|id| self.is_locked(*id)) {
            return None;
        }
        let mut bounds = Bounds::EMPTY;
        for id in &ids {
            if let Some(shape) = self.doc.shape(*id) {
                let b = shape.bounds();
                bounds.include(b.min);
                bounds.include(b.max);
            }
        }
        let size = bounds.max - bounds.min;
        let lone = region.is_none() && ids.len() == 1;
        let uniform = ids.len() == 1
            && matches!(
                self.doc.get(ids[0]).map(|e| e.kind),
                Some(EntityKind::Circle { .. })
            );
        (!bounds.is_empty() && size.max_element() > 1e-9).then_some(TransformBox {
            ids,
            bounds,
            lone,
            uniform,
        })
    }

    /// Whether a curve is a Bézier that ends where it starts (a path closed on itself).
    fn closed_bezier(&self, id: EntityId) -> bool {
        matches!(
            self.doc.get(id).map(|e| e.kind),
            Some(EntityKind::Bezier { a, b, .. }) if a == b
        )
    }

    /// Scale geometry about `anchor` by `scale` along each axis, inside an open transaction.
    /// `points` (the geometry's defining points) and circle radii are put where the scale
    /// takes them, then the solver settles: driving dimensions and relations hold, so what
    /// they fix keeps its size. With `dimensions`, each driving length entirely within the
    /// geometry first takes the value it measures once scaled, so it changes only as far as
    /// the stretch runs along it (angles are left alone). Returns false if the solve
    /// conflicts.
    ///
    /// Circles stay circles: they scale with a scale that keeps the shape, keep their size
    /// when stretched along one axis, and otherwise take the mean of the two factors.
    pub fn scale_geometry(
        &mut self,
        ids: &[EntityId],
        points: &BTreeSet<EntityId>,
        anchor: DVec2,
        scale: DVec2,
        dimensions: bool,
    ) -> bool {
        let to = |p: DVec2| anchor + (p - anchor) * scale;
        // Where the anchor is in the box, along each axis (0 at its low side, 1 at its high).
        let fraction = self.bounds_of(ids).map(|b| {
            let size = b.max - b.min;
            let along =
                |a: f64, lo: f64, span: f64| if span > 1e-12 { (a - lo) / span } else { 0.5 };
            DVec2::new(
                along(anchor.x, b.min.x, size.x),
                along(anchor.y, b.min.y, size.y),
            )
        });
        let factor = if (scale.x - scale.y).abs() < 1e-12 {
            scale.x
        } else if scale.x == 1.0 || scale.y == 1.0 {
            1.0
        } else {
            (scale.x * scale.y).abs().sqrt()
        };
        let inside: BTreeSet<EntityId> =
            ids.iter().copied().chain(points.iter().copied()).collect();
        let seeds: Vec<EntityId> = inside.iter().copied().collect();
        let scope = self.drag_scope(&seeds);
        let mut dragged: Vec<Param> = points.iter().copied().flat_map(Param::point).collect();
        for id in ids {
            if let Some(EntityKind::Circle { radius, .. }) = self.doc.get(*id).map(|e| e.kind)
                && self.doc.set_radius(*id, radius * factor).is_ok()
            {
                dragged.push(Param::Radius(*id));
            }
        }
        for p in points {
            if let Ok(at) = self.doc.point(*p) {
                let _ = self.doc.set_point(*p, to(at));
            }
        }
        self.doc.normalize_arcs_using(points);
        if dimensions {
            // The geometry is exactly scaled now: each length takes what it measures.
            let lengths: Vec<_> = self
                .doc
                .constraints()
                .filter(|(id, c)| {
                    c.is_dimension()
                        && !matches!(c, Constraint::Angle { .. })
                        && !self.doc.is_driven(*id)
                        && c.entities().all(|e| inside.contains(&e))
                })
                .map(|(id, c)| (id, *c))
                .collect();
            for (id, c) in lengths {
                if let Some(v) = drift_solver::measure(&self.doc, &c)
                    && c.with_value(v).value_is_valid()
                {
                    let _ = self.doc.set_constraint(id, c.with_value(v));
                }
            }
        }
        let mut report = self.solve_in(&dragged, scope.clone());
        // Where dimensions held a size, the solver met them by moving both sides: put the
        // anchor back where it was in the box (a corner stays put, the middle stays the
        // middle).
        if let (Some(fraction), Some(after)) = (fraction, self.bounds_of(ids)) {
            let shift = anchor - (after.min + (after.max - after.min) * fraction);
            if shift.length() > 1e-9 {
                self.doc.translate(points.iter().copied(), shift);
                report = self.solve_in(&dragged, scope.clone());
            }
        }
        let ok = !self.new_conflicts(&report);
        self.index.update_moved(&self.doc, scope.iter().copied());
        ok
    }

    /// The bounds of some curves as they are now.
    pub(crate) fn bounds_of(&self, ids: &[EntityId]) -> Option<Bounds> {
        let mut bounds = Bounds::EMPTY;
        for id in ids {
            if let Some(shape) = self.doc.shape(*id) {
                let b = shape.bounds();
                bounds.include(b.min);
                bounds.include(b.max);
            }
        }
        (!bounds.is_empty()).then_some(bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::Constraint;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    /// A 10 × 6 rectangle with horizontal and vertical sides, picked.
    fn rectangle(ed: &mut Editor) -> ([EntityId; 4], [EntityId; 4]) {
        ed.doc.begin();
        let c = [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 6.0), v(0.0, 6.0)].map(|p| ed.doc.add_point(p));
        let l = [0, 1, 2, 3].map(|i| ed.doc.add_line(c[i], c[(i + 1) % 4]).unwrap());
        for (i, line) in l.iter().enumerate() {
            let c = if i % 2 == 0 {
                Constraint::Horizontal { line: *line }
            } else {
                Constraint::Vertical { line: *line }
            };
            ed.doc.add_constraint(c).unwrap();
        }
        ed.commit("Setup");
        ed.selection = l.iter().copied().collect();
        (c, l)
    }

    #[test]
    fn the_box_and_its_handles() {
        let mut ed = Editor::default();
        let (_, l) = rectangle(&mut ed);
        let b = ed.transform_box().expect("two or more curves");
        assert_eq!((b.bounds.min, b.bounds.max), (v(0.0, 0.0), v(10.0, 6.0)));
        assert_eq!(b.scale_handles(1.0).len(), 8);
        // A corner handle, padded out from the corner; the middle of a side.
        assert_eq!(
            b.handle_at(v(11.2, 7.1), 0.5, 1.0),
            Some(Handle::Scale { x: 1, y: 1 })
        );
        assert_eq!(
            b.handle_at(v(5.0, -1.0), 0.5, 1.0),
            Some(Handle::Scale { x: 0, y: -1 })
        );
        // Just outside a corner turns; inside the box or far away is nothing.
        assert_eq!(
            b.handle_at(v(12.0, 8.0), 0.5, 1.0),
            Some(Handle::Turn { x: 1, y: 1 })
        );
        assert_eq!(
            b.handle_at(v(-2.0, -2.0), 0.5, 1.0),
            Some(Handle::Turn { x: -1, y: -1 })
        );
        assert_eq!(b.handle_at(v(5.0, 3.0), 0.5, 1.0), None);
        assert_eq!(b.handle_at(v(30.0, 30.0), 0.5, 1.0), None);
        // Anchors: the opposite corner or side, or the middle.
        assert_eq!(b.anchor(1, 1, false), v(0.0, 0.0));
        assert_eq!(b.anchor(0, -1, false), v(5.0, 6.0));
        assert_eq!(b.anchor(1, 1, true), v(5.0, 3.0));

        // A lone curve has no box; nor does a pick with a point in it.
        ed.selection = [l[0]].into();
        assert!(ed.transform_box().is_none());
        ed.selection = [l[0], l[1]].into();
        assert!(ed.transform_box().is_some());
        let corner = ed.doc.get(l[0]).unwrap().kind.points().next().unwrap();
        ed.selection.insert(corner);
        assert!(ed.transform_box().is_none());
    }

    #[test]
    fn a_lone_closed_curve_has_a_box_and_a_lone_open_one_none() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let o = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(o, 2.0).unwrap();
        let p = [v(10.0, 0.0), v(14.0, 0.0), v(10.0, 2.0)].map(|p| ed.doc.add_point(p));
        let ellipse = ed.doc.add_ellipse(p[0], p[1], p[2]).unwrap();
        let q = [v(20.0, 0.0), v(22.0, 0.0), v(20.0, 2.0)].map(|p| ed.doc.add_point(p));
        let arc = ed.doc.add_arc(q[0], q[1], q[2]).unwrap();
        ed.commit("Setup");
        // A circle: a box set further out, clear of its edge, that keeps it round.
        ed.selection = [circle].into();
        let b = ed.transform_box().expect("a circle has a box");
        assert!(b.lone && b.uniform);
        assert_eq!(b.padded(1.0).max, v(4.0, 4.0));
        assert_eq!(
            b.handle_at(v(4.0, 4.0), 0.75, 1.0),
            Some(Handle::Scale { x: 1, y: 1 })
        );
        // Its edge, within a pick of it, is clear of the middle handles.
        assert_eq!(b.handle_at(v(0.0, 2.9), 0.75, 1.0), None);
        // A closed ellipse: a box that may stretch it.
        ed.selection = [ellipse].into();
        let b = ed.transform_box().expect("an ellipse has a box");
        assert!(b.lone && !b.uniform);
        // An arc is open: its own grips.
        ed.selection = [arc].into();
        assert!(ed.transform_box().is_none());
        // On a locked layer: none.
        ed.doc.begin();
        let layer = ed.doc.add_layer(drift_core::Layer {
            locked: true,
            ..drift_core::Layer::new("Locked")
        });
        ed.doc.set_entity_layer(circle, layer).unwrap();
        ed.commit("Lock");
        ed.selection = [circle].into();
        assert!(ed.transform_box().is_none());
    }

    #[test]
    fn scale_factors_from_the_pointer() {
        let a = v(0.0, 0.0);
        // A corner: each axis by its own distance; proportional, the larger.
        assert_eq!(
            scale_for((1, 1), a, v(10.0, 6.0), v(20.0, 9.0), false),
            v(2.0, 1.5)
        );
        assert_eq!(
            scale_for((1, 1), a, v(10.0, 6.0), v(20.0, 9.0), true),
            v(2.0, 2.0)
        );
        // A side stretches one way; proportional, both by it.
        assert_eq!(
            scale_for((1, 0), a, v(10.0, 3.0), v(15.0, 40.0), false),
            v(1.5, 1.0)
        );
        assert_eq!(
            scale_for((1, 0), a, v(10.0, 3.0), v(15.0, 40.0), true),
            v(1.5, 1.5)
        );
        // Never flips through the anchor.
        assert_eq!(
            scale_for((1, 0), a, v(10.0, 3.0), v(-5.0, 3.0), false).x,
            MIN_SCALE
        );
    }

    #[test]
    fn rounded_scales_give_round_sizes() {
        let size = v(100.0, 60.0);
        // Stretched sideways to 2.987×: 298.7 wide rounds to 300 (5 mm steps), 60 stays.
        let s = round_scale(v(2.987, 1.0), (1, 0), size, 5.0, false);
        assert!((s.x * 100.0 - 300.0).abs() < 1e-9 && s.y == 1.0, "{s:?}");
        // A corner: each axis to its own round size.
        let s = round_scale(v(1.234, 1.567), (1, 1), size, 1.0, false);
        assert!((s.x * 100.0 - 123.0).abs() < 1e-9 && (s.y * 60.0 - 94.0).abs() < 1e-9);
        // Keeping the shape: the wider-growing axis is rounded and the other follows.
        let s = round_scale(v(1.234, 1.234), (1, 1), size, 10.0, true);
        assert!((s.x * 100.0 - 120.0).abs() < 1e-9 && s.x == s.y, "{s:?}");
        let s = round_scale(v(1.0, 1.51), (0, 1), size, 10.0, true);
        assert!((s.y * 60.0 - 90.0).abs() < 1e-9 && s.x == s.y, "{s:?}");
        // Never smaller than a step.
        let s = round_scale(v(0.001, 1.0), (1, 0), size, 5.0, false);
        assert!((s.x * 100.0 - 5.0).abs() < 1e-9);
    }

    #[test]
    fn scaling_keeps_driving_dimensions_unless_they_scale_too() {
        let mut ed = Editor::default();
        let (c, l) = rectangle(&mut ed);
        // The width is dimensioned at 10; the height is free.
        ed.doc.begin();
        let width = ed
            .doc
            .add_constraint(Constraint::Distance {
                a: c[0],
                b: c[1],
                value: 10.0,
            })
            .unwrap();
        ed.commit("Width");
        let b = ed.transform_box().unwrap();
        let points = ed.doc.defining_points(b.ids.iter().copied());

        // Doubled from the bottom-left corner: the width holds, the height doubles.
        ed.doc.begin();
        assert!(ed.scale_geometry(&b.ids, &points, v(0.0, 0.0), v(2.0, 2.0), false));
        ed.commit("Scale");
        let size = ed.doc.point(c[2]).unwrap() - ed.doc.point(c[0]).unwrap();
        assert!(
            ed.doc.point(c[0]).unwrap().length() < 1e-6,
            "the anchor corner stays"
        );
        assert!((size.x - 10.0).abs() < 1e-6, "{size:?}");
        assert!((size.y - 12.0).abs() < 1e-6, "{size:?}");
        assert_eq!(ed.doc.constraint(width).unwrap().value(), Some(10.0));
        // Still a rectangle.
        assert!(ed.doc.constraints().count() >= 5);
        ed.undo();

        // With the dimensions scaled too, it all doubles, and so does the 10.
        ed.doc.begin();
        assert!(ed.scale_geometry(&b.ids, &points, v(0.0, 0.0), v(2.0, 2.0), true));
        ed.commit("Scale");
        let size = ed.doc.point(c[2]).unwrap() - ed.doc.point(c[0]).unwrap();
        assert!((size - v(20.0, 12.0)).length() < 1e-6, "{size:?}");
        assert_eq!(ed.doc.constraint(width).unwrap().value(), Some(20.0));
        ed.undo();

        // Stretched sideways only, undimensioned: the width follows.
        ed.doc.begin();
        ed.doc.remove_constraint(width);
        ed.commit("No width");
        ed.doc.begin();
        assert!(ed.scale_geometry(&b.ids, &points, v(0.0, 0.0), v(1.5, 1.0), false));
        ed.commit("Stretch");
        let size = ed.doc.point(c[2]).unwrap() - ed.doc.point(c[0]).unwrap();
        assert!((size - v(15.0, 6.0)).length() < 1e-6, "{size:?}");
        let _ = l;
    }

    #[test]
    fn circles_scale_their_radius() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let o = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(o, 2.0).unwrap();
        let p = [ed.doc.add_point(v(5.0, 0.0)), ed.doc.add_point(v(9.0, 0.0))];
        let line = ed.doc.add_line(p[0], p[1]).unwrap();
        ed.commit("Setup");
        ed.selection = [circle, line].into();
        let b = ed.transform_box().unwrap();
        assert_eq!(b.bounds.min, v(-2.0, -2.0));
        let points = ed.doc.defining_points(b.ids.iter().copied());
        ed.doc.begin();
        assert!(ed.scale_geometry(&b.ids, &points, v(-2.0, -2.0), v(3.0, 3.0), false));
        ed.commit("Scale");
        assert_eq!(drift_solver::radius_of(&ed.doc, circle), Some(6.0));
        assert!(ed.doc.point(o).unwrap().distance(v(4.0, 4.0)) < 1e-9);
        assert!(ed.doc.point(p[1]).unwrap().distance(v(31.0, 4.0)) < 1e-9);
    }
}
