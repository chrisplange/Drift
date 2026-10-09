//! Modify operations on the document: trim, extend, split, fillet, chamfer, offset, mirror and
//! patterns. Each is one undoable step that is solved and committed, or refused with a reason.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use drift_core::{Constraint, ConstraintId, DVec2, EntityId, EntityKind, Shape, arc_shape};
use drift_geom::intersect::{self, intersect, intersect_extended, intersect_unbounded};

use crate::editor::{EditError, Editor};

fn invalid(msg: impl Into<String>) -> EditError {
    EditError::Invalid(msg.into())
}

/// Where a curve is cut by another one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// Position along the curve (see [`intersect::Param`]).
    pub t: f64,
    pub at: DVec2,
    /// The cutting curve.
    pub other: EntityId,
    /// An existing end point of the cutting curve at the cut, to join instead of adding a
    /// point.
    pub point: Option<EntityId>,
}

/// The parameter of the point of `shape` nearest `p` (fraction along a line, angle from the
/// start for arcs, angle from +x for circles).
pub fn param_of(shape: &Shape, p: DVec2) -> f64 {
    match *shape {
        Shape::Line { a, b } => {
            let d = b - a;
            ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
        }
        Shape::Circle { center, .. } => {
            let r = p - center;
            r.y.atan2(r.x).rem_euclid(TAU)
        }
        Shape::Arc {
            center,
            start,
            sweep,
            ..
        } => {
            let r = p - center;
            let t = (r.y.atan2(r.x) - start).rem_euclid(TAU);
            if t <= sweep {
                t
            } else if t - sweep < TAU - t {
                sweep
            } else {
                0.0
            }
        }
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => {
            let t =
                (drift_core::shape::ellipse_angle(center, major, minor, p) - start).rem_euclid(TAU);
            if sweep >= TAU || t <= sweep {
                t
            } else if t - sweep < TAU - t {
                sweep
            } else {
                0.0
            }
        }
        Shape::Point(_) => 0.0,
    }
}

/// The point at parameter `t` of a curve.
pub fn point_at(shape: &Shape, t: f64) -> DVec2 {
    match *shape {
        Shape::Line { a, b } => a + (b - a) * t,
        Shape::Circle { center, radius } => center + radius * DVec2::from_angle(t),
        Shape::Arc {
            center,
            radius,
            start,
            ..
        } => center + radius * DVec2::from_angle(start + t),
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            ..
        } => drift_core::shape::ellipse_point(center, major, minor, start + t),
        Shape::Point(p) => p,
    }
}

/// The part of `shape` between parameters `t0` < `t1` (circles: counter-clockwise from `t0`
/// to `t1`, which may wrap), for previews.
pub fn piece(shape: &Shape, t0: f64, t1: f64) -> Shape {
    match *shape {
        Shape::Line { .. } => Shape::Line {
            a: point_at(shape, t0),
            b: point_at(shape, t1),
        },
        Shape::Circle { center, radius } => Shape::Arc {
            center,
            radius,
            start: t0,
            sweep: (t1 - t0).rem_euclid(TAU).max(1e-12),
        },
        Shape::Arc {
            center,
            radius,
            start,
            ..
        } => Shape::Arc {
            center,
            radius,
            start: start + t0,
            sweep: t1 - t0,
        },
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => Shape::Ellipse {
            center,
            major,
            minor,
            start: start + t0,
            sweep: if sweep >= TAU {
                (t1 - t0).rem_euclid(TAU).max(1e-12)
            } else {
                t1 - t0
            },
        },
        Shape::Point(p) => Shape::Point(p),
    }
}

/// Fillet geometry at a corner `p` between rays towards `o1` and `o2`: tangent points,
/// centre, and whether the arc runs counter-clockwise from the first tangent point.
pub fn fillet_geometry(
    p: DVec2,
    o1: DVec2,
    o2: DVec2,
    r: f64,
) -> Result<(DVec2, DVec2, DVec2, bool), EditError> {
    let (l1, l2) = (o1.distance(p), o2.distance(p));
    let (u1, u2) = ((o1 - p) / l1, (o2 - p) / l2);
    let cos = u1.dot(u2).clamp(-1.0, 1.0);
    let alpha = cos.acos();
    if !(1e-6..=std::f64::consts::PI - 1e-6).contains(&alpha) {
        return Err(invalid("the lines are in line: nothing to round"));
    }
    if !(r > 0.0 && r.is_finite()) {
        return Err(invalid("the radius must be positive"));
    }
    let d = r / (alpha / 2.0).tan();
    if d >= l1 || d >= l2 {
        return Err(invalid("the radius is too large for these lines"));
    }
    let t1 = p + u1 * d;
    let t2 = p + u2 * d;
    let bisector = (u1 + u2).normalize();
    let c = p + bisector * (r / (alpha / 2.0).sin());
    let ccw = (t1 - c).perp_dot(t2 - c) > 0.0;
    Ok((t1, t2, c, ccw))
}

/// How far the selection moves for the pointer during a move from `start` (the pointer where
/// the move began) with base point `base`: it follows the pointer's motion, and when the
/// pointer snaps (to geometry or the grid), the base point lands exactly on the snap.
pub fn move_offset(start: DVec2, base: DVec2, p: &crate::editor::Pointer) -> DVec2 {
    if p.snap.is_some() {
        p.pos() - base
    } else {
        p.raw - start
    }
}

impl Editor {
    /// Points a move can be based on: the selection's defining points and line midpoints.
    pub fn base_candidates(&self, ids: &[EntityId]) -> Vec<DVec2> {
        let mut out: Vec<DVec2> = self
            .doc
            .defining_points(ids.iter().copied())
            .into_iter()
            .filter_map(|p| self.doc.point(p).ok())
            .collect();
        out.extend(ids.iter().filter_map(|id| match self.doc.shape(*id) {
            Some(Shape::Line { a, b }) => Some((a + b) * 0.5),
            _ => None,
        }));
        out
    }

    /// The base point for moving `ids`: a single point is its own base, a line its
    /// midpoint, a circle or arc its centre; otherwise the candidate nearest `near`. With
    /// `nearest` (mouse drags), always the candidate nearest where the geometry was grabbed.
    pub fn move_base(&self, ids: &[EntityId], near: DVec2, nearest: bool) -> DVec2 {
        if !nearest && let [id] = ids {
            match self.doc.shape(*id) {
                Some(Shape::Point(p)) => return p,
                Some(Shape::Line { a, b }) => return (a + b) * 0.5,
                Some(
                    Shape::Circle { center, .. }
                    | Shape::Arc { center, .. }
                    | Shape::Ellipse { center, .. },
                ) => return center,
                None => {}
            }
        }
        self.base_candidates(ids)
            .into_iter()
            .min_by(|a, b| a.distance(near).total_cmp(&b.distance(near)))
            .unwrap_or(near)
    }

    /// The solid curves that bound a region (outline and holes).
    pub fn region_curves(&self, region: &drift_geom::regions::Region) -> Vec<EntityId> {
        let edges: Vec<Shape> = std::iter::once(&region.outer)
            .chain(&region.holes)
            .flat_map(|lp| lp.edges.iter().map(|e| e.shape()))
            .collect();
        self.doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point() && e.style == drift_core::LineKind::Solid)
            .filter_map(|(id, _)| Some((id, self.doc.shape(id)?)))
            .filter(|(_, shape)| {
                let end = match *shape {
                    Shape::Line { .. } => 1.0,
                    Shape::Arc { sweep, .. } => sweep,
                    _ => TAU,
                };
                let tol = self.tol_for(shape) * 1e3;
                // Curves may be split where others cross them: probe a few points.
                [0.2, 0.5, 0.8].iter().any(|f| {
                    let probe = point_at(shape, end * f);
                    edges.iter().any(|e| drift_geom::distance(e, probe) <= tol)
                })
            })
            .map(|(id, _)| id)
            .collect()
    }

    fn curve_shape(&self, id: EntityId) -> Option<Shape> {
        self.doc
            .get(id)
            .filter(|e| !e.kind.is_point())
            .and_then(|_| self.doc.shape(id))
    }

    fn tol_for(&self, shape: &Shape) -> f64 {
        let b = shape.bounds();
        intersect::tolerance(b.min.abs().max(b.max.abs()).max_element())
    }

    /// Where other curves cut curve `id`, in order along it (its own ends excluded).
    pub fn cuts(&self, id: EntityId) -> Vec<Cut> {
        let Some(shape) = self.curve_shape(id) else {
            return Vec::new();
        };
        let tol = self.tol_for(&shape);
        let own_ends: Vec<DVec2> = self
            .doc
            .get(id)
            .map(|e| match e.kind {
                EntityKind::Line { a, b } => vec![a, b],
                EntityKind::Arc { start, end, .. } => vec![start, end],
                _ => Vec::new(),
            })
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| self.doc.point(p).ok())
            .collect();
        let mut bounds = shape.bounds();
        bounds.include(bounds.min - DVec2::splat(tol));
        bounds.include(bounds.max + DVec2::splat(tol));
        let mut cuts: Vec<Cut> = Vec::new();
        for other in self.index.query(bounds) {
            if other == id {
                continue;
            }
            let Some(other_shape) = self.curve_shape(other) else {
                continue;
            };
            for hit in intersect(&shape, &other_shape) {
                if own_ends.iter().any(|e| e.distance(hit.at) <= tol) {
                    continue;
                }
                let point = self.doc.get(other).and_then(|e| {
                    let ends: Vec<EntityId> = match e.kind {
                        EntityKind::Line { a, b } => vec![a, b],
                        EntityKind::Arc { start, end, .. } => vec![start, end],
                        _ => Vec::new(),
                    };
                    ends.into_iter()
                        .find(|p| self.doc.point(*p).is_ok_and(|q| q.distance(hit.at) <= tol))
                });
                if cuts.iter().any(|c| c.at.distance(hit.at) <= tol) {
                    continue;
                }
                cuts.push(Cut {
                    t: hit.t[0],
                    at: hit.at,
                    other,
                    point,
                });
            }
        }
        cuts.sort_by(|a, b| a.t.total_cmp(&b.t));
        cuts
    }

    /// The point entity for a cut: an existing end of the cutting curve, or a new point kept
    /// on the cutting curve. Call inside a transaction.
    fn cut_point(&mut self, cut: &Cut) -> EntityId {
        if let Some(p) = cut.point {
            return p;
        }
        let p = self.doc.add_point(cut.at);
        let _ = self.doc.add_constraint(Constraint::PointOnCurve {
            point: p,
            curve: cut.other,
        });
        p
    }

    /// The piece of curve `id` that trimming at `click` would remove: (start, end)
    /// parameters, or `None` to remove the whole curve. Errors explain why it can't be
    /// trimmed there.
    pub fn trim_span(
        &self,
        id: EntityId,
        click: DVec2,
    ) -> Result<(Option<Cut>, Option<Cut>), EditError> {
        let shape = self
            .curve_shape(id)
            .ok_or_else(|| invalid("pick a line, arc or circle"))?;
        let cuts = self.cuts(id);
        let tc = param_of(&shape, click);
        if let Shape::Circle { .. } = shape {
            if cuts.len() < 2 {
                return Err(invalid("a circle needs two cutting curves to trim"));
            }
            let lo = cuts
                .iter()
                .rev()
                .find(|c| c.t < tc)
                .or(cuts.last())
                .copied();
            let hi = cuts.iter().find(|c| c.t > tc).or(cuts.first()).copied();
            return Ok((lo, hi));
        }
        let lo = cuts.iter().rev().find(|c| c.t < tc).copied();
        let hi = cuts.iter().find(|c| c.t > tc).copied();
        Ok((lo, hi))
    }

    /// The shape trimming at `click` would remove, for previews.
    pub fn trim_preview(&self, id: EntityId, click: DVec2) -> Option<Shape> {
        let shape = self.curve_shape(id)?;
        let (lo, hi) = self.trim_span(id, click).ok()?;
        let end = match shape {
            Shape::Line { .. } => 1.0,
            Shape::Arc { sweep, .. } => sweep,
            _ => 0.0,
        };
        Some(match (lo, hi) {
            (None, None) => shape,
            (lo, hi) => piece(&shape, lo.map_or(0.0, |c| c.t), hi.map_or(end, |c| c.t)),
        })
    }

    /// Remove the piece of curve `id` around `click` between the nearest cutting curves (the
    /// whole curve if nothing cuts it).
    pub fn trim_at(&mut self, id: EntityId, click: DVec2) -> Result<(), EditError> {
        let (lo, hi) = self.trim_span(id, click)?;
        let kind = self.doc.get(id).map(|e| e.kind).expect("checked");
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let result = (|| -> Result<(), EditError> {
            match (kind, lo, hi) {
                (_, None, None) => self.doc.delete([id]),
                (EntityKind::Line { a, b }, lo, hi) => match (lo, hi) {
                    (None, Some(h)) => {
                        let p = self.cut_point(&h);
                        self.doc
                            .set_entity_kind(id, EntityKind::Line { a: p, b })
                            .map_err(doc_err)?;
                        self.doc.remove_if_orphaned(a);
                    }
                    (Some(l), None) => {
                        let p = self.cut_point(&l);
                        self.doc
                            .set_entity_kind(id, EntityKind::Line { a, b: p })
                            .map_err(doc_err)?;
                        self.doc.remove_if_orphaned(b);
                    }
                    (Some(l), Some(h)) => {
                        let pl = self.cut_point(&l);
                        let ph = self.cut_point(&h);
                        self.doc
                            .set_entity_kind(id, EntityKind::Line { a, b: pl })
                            .map_err(doc_err)?;
                        let rest = self.doc.add_line(ph, b).map_err(doc_err)?;
                        self.doc.copy_look(id, rest);
                        let _ = self
                            .doc
                            .add_constraint(Constraint::Collinear { a: id, b: rest });
                    }
                    (None, None) => unreachable!(),
                },
                (EntityKind::Arc { center, start, end }, lo, hi) => match (lo, hi) {
                    (None, Some(h)) => {
                        let p = self.cut_point(&h);
                        self.doc
                            .set_entity_kind(
                                id,
                                EntityKind::Arc {
                                    center,
                                    start: p,
                                    end,
                                },
                            )
                            .map_err(doc_err)?;
                        self.doc.remove_if_orphaned(start);
                    }
                    (Some(l), None) => {
                        let p = self.cut_point(&l);
                        self.doc
                            .set_entity_kind(
                                id,
                                EntityKind::Arc {
                                    center,
                                    start,
                                    end: p,
                                },
                            )
                            .map_err(doc_err)?;
                        self.doc.remove_if_orphaned(end);
                    }
                    (Some(l), Some(h)) => {
                        let pl = self.cut_point(&l);
                        let ph = self.cut_point(&h);
                        self.doc
                            .set_entity_kind(
                                id,
                                EntityKind::Arc {
                                    center,
                                    start,
                                    end: pl,
                                },
                            )
                            .map_err(doc_err)?;
                        let rest = self.doc.add_arc(center, ph, end).map_err(doc_err)?;
                        self.doc.copy_look(id, rest);
                        let _ = self
                            .doc
                            .add_constraint(Constraint::Equal { a: id, b: rest });
                    }
                    (None, None) => unreachable!(),
                },
                (EntityKind::Circle { center, .. }, Some(l), Some(h)) => {
                    let pl = self.cut_point(&l);
                    let ph = self.cut_point(&h);
                    // What remains runs counter-clockwise from the cut after the click round
                    // to the cut before it.
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::Arc {
                                center,
                                start: ph,
                                end: pl,
                            },
                        )
                        .map_err(doc_err)?;
                }
                _ => return Err(invalid("nothing to trim here")),
            }
            Ok(())
        })();
        self.finish(result, "Trim")
    }

    /// Roll back on error, or solve and commit (reporting conflicts).
    fn finish(&mut self, result: Result<(), EditError>, label: &str) -> Result<(), EditError> {
        match result {
            Err(e) => {
                self.doc.rollback();
                self.sync();
                Err(e)
            }
            Ok(()) => {
                if self.commit(label) {
                    Ok(())
                } else {
                    Err(EditError::Conflict)
                }
            }
        }
    }

    /// Where extending the end of curve `id` nearest `click` would reach: (end point, the
    /// hit, the curve it meets, an existing end point there).
    pub fn extend_target(
        &self,
        id: EntityId,
        click: DVec2,
    ) -> Result<(EntityId, DVec2, EntityId, Option<EntityId>), EditError> {
        let shape = self
            .curve_shape(id)
            .ok_or_else(|| invalid("pick a line or arc"))?;
        let kind = self.doc.get(id).map(|e| e.kind).expect("checked");
        let (first, last) = match kind {
            EntityKind::Line { a, b } => (a, b),
            EntityKind::Arc { start, end, .. } => (start, end),
            _ => return Err(invalid("only lines and arcs can be extended")),
        };
        let (p0, p1) = (
            self.doc.point(first).unwrap_or_default(),
            self.doc.point(last).unwrap_or_default(),
        );
        let at_end = click.distance(p1) < click.distance(p0);
        let end_point = if at_end { last } else { first };
        let tol = self.tol_for(&shape);
        let sweep = match shape {
            Shape::Arc { sweep, .. } => sweep,
            _ => 1.0,
        };
        let mut best: Option<(f64, DVec2, EntityId)> = None;
        for (other, _) in self.doc.iter() {
            if other == id {
                continue;
            }
            let Some(other_shape) = self.curve_shape(other) else {
                continue;
            };
            for hit in intersect_extended(&shape, &other_shape) {
                // How far beyond the chosen end the hit is (in parameter units).
                let beyond = match (shape, at_end) {
                    (Shape::Line { .. }, true) => hit.t[0] - 1.0,
                    (Shape::Line { .. }, false) => -hit.t[0],
                    (_, true) => hit.t[0] - sweep,
                    (_, false) => {
                        if hit.t[0] > sweep {
                            TAU - hit.t[0]
                        } else {
                            -1.0
                        }
                    }
                };
                let gap = hit.at.distance(if at_end { p1 } else { p0 });
                if beyond > 0.0 && gap > tol && best.is_none_or(|(b, ..)| beyond < b) {
                    best = Some((beyond, hit.at, other));
                }
            }
        }
        let (_, at, other) = best.ok_or_else(|| invalid("nothing to extend to"))?;
        let existing = self.doc.get(other).and_then(|e| {
            let ends: Vec<EntityId> = match e.kind {
                EntityKind::Line { a, b } => vec![a, b],
                EntityKind::Arc { start, end, .. } => vec![start, end],
                _ => Vec::new(),
            };
            ends.into_iter()
                .find(|p| self.doc.point(*p).is_ok_and(|q| q.distance(at) <= tol))
        });
        Ok((end_point, at, other, existing))
    }

    /// Extend the end of curve `id` nearest `click` to the next curve it would meet.
    pub fn extend_at(&mut self, id: EntityId, click: DVec2) -> Result<(), EditError> {
        let (end, at, other, existing) = self.extend_target(id, click)?;
        if self.doc.users_of(end).count() > 1 {
            return Err(invalid("that end is connected to other geometry"));
        }
        self.doc.begin();
        let result = (|| -> Result<(), EditError> {
            let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
            self.doc.set_point(end, at).map_err(doc_err)?;
            self.doc.normalize_arcs_using(&BTreeSet::from([end]));
            match existing {
                Some(q) => self.doc.merge_points(end, q).map_err(doc_err)?,
                None => {
                    let _ = self.doc.add_constraint(Constraint::PointOnCurve {
                        point: end,
                        curve: other,
                    });
                }
            }
            Ok(())
        })();
        self.finish(result, "Extend")
    }

    /// Split curve `id` at the point nearest `at`, returning the new shared point.
    pub fn split_at(&mut self, id: EntityId, at: DVec2) -> Result<EntityId, EditError> {
        let shape = self
            .curve_shape(id)
            .ok_or_else(|| invalid("pick a line or arc"))?;
        let kind = self.doc.get(id).map(|e| e.kind).expect("checked");
        let t = param_of(&shape, at);
        let p = point_at(&shape, t);
        let tol = self.tol_for(&shape);
        let near_end = match kind {
            EntityKind::Line { a, b }
            | EntityKind::Arc {
                start: a, end: b, ..
            } => [a, b].iter().any(|e| {
                self.doc
                    .point(*e)
                    .is_ok_and(|q| q.distance(p) <= tol.max(1e-6))
            }),
            _ => {
                return Err(invalid(
                    "a circle can't be split at one point: trim it instead",
                ));
            }
        };
        if near_end {
            return Err(invalid("that's an end already"));
        }
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut point = None;
        let result = (|| -> Result<(), EditError> {
            let m = self.doc.add_point(p);
            point = Some(m);
            match kind {
                EntityKind::Line { a, b } => {
                    self.doc
                        .set_entity_kind(id, EntityKind::Line { a, b: m })
                        .map_err(doc_err)?;
                    let rest = self.doc.add_line(m, b).map_err(doc_err)?;
                    self.doc.copy_look(id, rest);
                    let _ = self
                        .doc
                        .add_constraint(Constraint::Collinear { a: id, b: rest });
                }
                EntityKind::Arc { center, start, end } => {
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::Arc {
                                center,
                                start,
                                end: m,
                            },
                        )
                        .map_err(doc_err)?;
                    let rest = self.doc.add_arc(center, m, end).map_err(doc_err)?;
                    self.doc.copy_look(id, rest);
                    let _ = self
                        .doc
                        .add_constraint(Constraint::Equal { a: id, b: rest });
                }
                _ => unreachable!(),
            }
            Ok(())
        })();
        self.finish(result, "Split")?;
        Ok(point.expect("added"))
    }

    /// The two lines meeting at a corner point (and their far ends), for fillets and
    /// chamfers.
    pub fn corner(&self, p: EntityId) -> Result<[(EntityId, EntityId); 2], EditError> {
        let users: Vec<EntityId> = self.doc.users_of(p).collect();
        let lines: Vec<(EntityId, EntityId)> = users
            .iter()
            .filter_map(|id| match self.doc.get(*id)?.kind {
                EntityKind::Line { a, b } => Some((*id, if a == p { b } else { a })),
                _ => None,
            })
            .collect();
        match lines.as_slice() {
            [l1, l2] if users.len() == 2 => Ok([*l1, *l2]),
            _ => Err(invalid("pick a corner where exactly two lines meet")),
        }
    }

    /// The fillet arc at corner `p` with radius `r`, for previews.
    pub fn fillet_preview(&self, p: EntityId, r: f64) -> Option<Shape> {
        let [(_, o1), (_, o2)] = self.corner(p).ok()?;
        let (t1, t2, c, ccw) = fillet_geometry(
            self.doc.point(p).ok()?,
            self.doc.point(o1).ok()?,
            self.doc.point(o2).ok()?,
            r,
        )
        .ok()?;
        Some(if ccw {
            arc_shape(c, t1, t2)
        } else {
            arc_shape(c, t2, t1)
        })
    }

    /// Round the corner at point `p` with an arc of radius `r`, tangent to both lines.
    pub fn fillet(&mut self, p: EntityId, r: f64) -> Result<EntityId, EditError> {
        let [(l1, o1), (l2, o2)] = self.corner(p)?;
        let pos = |id| self.doc.point(id).unwrap_or_default();
        let (t1, t2, c, ccw) = fillet_geometry(pos(p), pos(o1), pos(o2), r)?;
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut arc = None;
        let result = (|| -> Result<(), EditError> {
            let (p1, p2, pc) = (
                self.doc.add_point(t1),
                self.doc.add_point(t2),
                self.doc.add_point(c),
            );
            self.replace_end(l1, p, p1)?;
            self.replace_end(l2, p, p2)?;
            let (s, e) = if ccw { (p1, p2) } else { (p2, p1) };
            let a = self.doc.add_arc(pc, s, e).map_err(doc_err)?;
            arc = Some(a);
            for line in [l1, l2] {
                let _ = self
                    .doc
                    .add_constraint(Constraint::Tangent { a: line, b: a });
            }
            let _ = self
                .doc
                .add_constraint(Constraint::Radius { curve: a, value: r });
            self.doc.remove_if_orphaned(p);
            Ok(())
        })();
        self.finish(result, "Fillet")?;
        Ok(arc.expect("added"))
    }

    /// Replace a line's end point `from` with `to`.
    fn replace_end(
        &mut self,
        line: EntityId,
        from: EntityId,
        to: EntityId,
    ) -> Result<(), EditError> {
        let Some(EntityKind::Line { a, b }) = self.doc.get(line).map(|e| e.kind) else {
            return Err(invalid("not a line"));
        };
        let kind = if a == from {
            EntityKind::Line { a: to, b }
        } else {
            EntityKind::Line { a, b: to }
        };
        self.doc
            .set_entity_kind(line, kind)
            .map_err(|e| invalid(e.to_string()))
    }

    /// The chamfer line at corner `p` (distances along each line), for previews.
    pub fn chamfer_preview(&self, p: EntityId, d1: f64, d2: f64) -> Option<Shape> {
        let [(_, o1), (_, o2)] = self.corner(p).ok()?;
        let at = self.doc.point(p).ok()?;
        let (q1, q2) = (self.doc.point(o1).ok()?, self.doc.point(o2).ok()?);
        (d1 > 0.0 && d2 > 0.0 && d1 < q1.distance(at) && d2 < q2.distance(at)).then(|| {
            Shape::Line {
                a: at + (q1 - at).normalize() * d1,
                b: at + (q2 - at).normalize() * d2,
            }
        })
    }

    /// Cut the corner at point `p` with a line, `d1` / `d2` along the two lines.
    pub fn chamfer(&mut self, p: EntityId, d1: f64, d2: f64) -> Result<EntityId, EditError> {
        let [(l1, _), (l2, _)] = self.corner(p)?;
        let Some(Shape::Line { a, b }) = self.chamfer_preview(p, d1, d2) else {
            return Err(invalid(
                "the distances must be positive and shorter than the lines",
            ));
        };
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut line = None;
        let result = (|| -> Result<(), EditError> {
            let (p1, p2) = (self.doc.add_point(a), self.doc.add_point(b));
            self.replace_end(l1, p, p1)?;
            self.replace_end(l2, p, p2)?;
            line = Some(self.doc.add_line(p1, p2).map_err(doc_err)?);
            self.doc.remove_if_orphaned(p);
            Ok(())
        })();
        self.finish(result, "Chamfer")?;
        Ok(line.expect("added"))
    }

    /// The connected chain of lines and arcs through `id` (stopping where more than two curves
    /// meet), in order, with each curve's direction (true = as stored), and whether it
    /// closes.
    pub fn chain(&self, id: EntityId) -> (Vec<(EntityId, bool)>, bool) {
        let ends = |c: EntityId| match self.doc.get(c).map(|e| e.kind) {
            Some(EntityKind::Line { a, b })
            | Some(EntityKind::Arc {
                start: a, end: b, ..
            }) => Some((a, b)),
            _ => None,
        };
        let Some((a0, b0)) = ends(id) else {
            return (
                vec![(id, true)],
                matches!(
                    self.doc.get(id).map(|e| e.kind),
                    Some(EntityKind::Circle { .. })
                ),
            );
        };
        let next_from = |point: EntityId, from: EntityId| -> Option<EntityId> {
            let users: Vec<EntityId> = self
                .doc
                .users_of(point)
                .filter(|u| ends(*u).is_some())
                .collect();
            if users.len() == 2 {
                users.into_iter().find(|u| *u != from)
            } else {
                None
            }
        };
        // Walk backwards from the start to find the chain's first curve.
        let mut first = (id, true);
        let mut point = a0;
        let mut seen = BTreeSet::from([id]);
        while let Some(prev) = next_from(point, first.0) {
            if seen.contains(&prev) {
                break; // closed loop
            }
            seen.insert(prev);
            let (pa, pb) = ends(prev).expect("curve");
            // `prev` ends at `point` when traversed forward.
            let forward = pb == point;
            first = (prev, forward);
            point = if forward { pa } else { pb };
        }
        // Walk forward from the first curve.
        let mut out = vec![first];
        let (fa, fb) = ends(first.0).expect("curve");
        let mut point = if first.1 { fb } else { fa };
        let mut closed = false;
        loop {
            let last = out.last().expect("non-empty").0;
            let Some(next) = next_from(point, last) else {
                break;
            };
            if next == first.0 {
                closed = true;
                break;
            }
            if out.iter().any(|(c, _)| *c == next) {
                break;
            }
            let (na, nb) = ends(next).expect("curve");
            let forward = na == point;
            out.push((next, forward));
            point = if forward { nb } else { na };
        }
        let _ = b0;
        (out, closed)
    }

    /// Offset shapes for a chain at signed distance `d` (positive = left of the direction of
    /// travel), with corners rejoined. Circles: positive = outward.
    pub fn offset_shapes(
        &self,
        chain: &[(EntityId, bool)],
        closed: bool,
        d: f64,
    ) -> Result<Vec<Shape>, EditError> {
        let mut pieces: Vec<Shape> = Vec::new();
        for &(id, forward) in chain {
            let shape = self.curve_shape(id).ok_or_else(|| invalid("not a curve"))?;
            pieces.push(match shape {
                Shape::Line { a, b } => {
                    let (p, q) = if forward { (a, b) } else { (b, a) };
                    let n = (q - p).perp().normalize() * d;
                    Shape::Line { a: p + n, b: q + n }
                }
                Shape::Circle { center, radius } => {
                    let r = radius + d;
                    if r <= 0.0 {
                        return Err(invalid("the offset is larger than the radius"));
                    }
                    Shape::Circle { center, radius: r }
                }
                Shape::Ellipse { .. } => {
                    return Err(invalid("offsetting ellipses isn't supported yet"));
                }
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    // Left of counter-clockwise travel is the centre.
                    let r = if forward { radius - d } else { radius + d };
                    if r <= 0.0 {
                        return Err(invalid("the offset is larger than an arc's radius"));
                    }
                    Shape::Arc {
                        center,
                        radius: r,
                        start,
                        sweep,
                    }
                }
                Shape::Point(_) => return Err(invalid("not a curve")),
            });
        }
        // Rejoin consecutive pieces at their corners.
        let n = pieces.len();
        let joins = if closed { n } else { n.saturating_sub(1) };
        for i in 0..joins {
            let j = (i + 1) % n;
            let (fi, fj) = (chain[i].1, chain[j].1);
            let end_i = piece_end(&pieces[i], fi, true);
            let start_j = piece_end(&pieces[j], fj, false);
            let tol = self.tol_for(&pieces[i]).max(1e-9);
            if end_i.distance(start_j) <= tol * 10.0 {
                continue;
            }
            let corner = self
                .doc
                .shape(chain[i].0)
                .map(|s| piece_end(&s, fi, true))
                .unwrap_or(end_i);
            let hit = intersect_unbounded(&pieces[i], &pieces[j])
                .into_iter()
                .min_by(|a, b| a.at.distance(corner).total_cmp(&b.at.distance(corner)));
            if let Some(h) = hit {
                set_piece_end(&mut pieces[i], fi, true, h.at);
                set_piece_end(&mut pieces[j], fj, false, h.at);
            }
        }
        // Lines whose direction flipped have been offset past their length.
        for (k, &(id, forward)) in chain.iter().enumerate() {
            if let (Some(Shape::Line { a, b }), Shape::Line { a: oa, b: ob }) =
                (self.doc.shape(id), pieces[k])
            {
                let dir = if forward { b - a } else { a - b };
                if (ob - oa).dot(dir) <= 0.0 {
                    return Err(invalid("the offset is too large for the shape"));
                }
            }
        }
        Ok(pieces)
    }

    /// The signed offset distance for a chain so the result lies on the side of `side`.
    pub fn offset_side(&self, chain: &[(EntityId, bool)], side: DVec2) -> f64 {
        // The chain curve nearest the side point decides.
        let nearest = chain
            .iter()
            .filter_map(|(id, f)| self.curve_shape(*id).map(|s| (s, *f)))
            .min_by(|(a, _), (b, _)| {
                drift_geom::distance(a, side).total_cmp(&drift_geom::distance(b, side))
            });
        match nearest {
            Some((Shape::Line { a, b }, forward)) => {
                let (p, q) = if forward { (a, b) } else { (b, a) };
                if (q - p).perp_dot(side - p) >= 0.0 {
                    1.0
                } else {
                    -1.0
                }
            }
            Some((Shape::Circle { center, radius }, _)) => {
                if side.distance(center) >= radius {
                    1.0
                } else {
                    -1.0
                }
            }
            Some((Shape::Arc { center, radius, .. }, forward)) => {
                let inside = side.distance(center) < radius;
                // Left of counter-clockwise travel is inside.
                if inside == forward { 1.0 } else { -1.0 }
            }
            _ => 1.0,
        }
    }

    /// Offset the chain through curve `id` by `distance` towards `side`. Lines stay parallel
    /// at the distance; arcs and circles share their centre.
    pub fn offset(
        &mut self,
        id: EntityId,
        distance: f64,
        side: DVec2,
    ) -> Result<Vec<EntityId>, EditError> {
        if !(distance > 0.0 && distance.is_finite()) {
            return Err(invalid("the offset distance must be positive"));
        }
        let (chain, closed) = self.chain(id);
        let d = self.offset_side(&chain, side) * distance;
        let pieces = self.offset_shapes(&chain, closed, d)?;
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut created = Vec::new();
        let result = (|| -> Result<(), EditError> {
            // Points at each piece's start and end in travel order; joined corners share one.
            let n = pieces.len();
            let mut starts = Vec::with_capacity(n);
            let mut ends = Vec::with_capacity(n);
            for (k, piece) in pieces.iter().enumerate() {
                let fwd = chain[k].1;
                let s = piece_end(piece, fwd, false);
                let e = piece_end(piece, fwd, true);
                let start_point = match ends.last() {
                    Some(&(prev, at))
                        if k > 0
                            && DVec2::distance(at, s) <= 1e-9 * s.abs().max_element().max(1.0) =>
                    {
                        prev
                    }
                    _ => self.doc.add_point(s),
                };
                starts.push(start_point);
                let end_point = if closed
                    && k == n - 1
                    && starts[0] != start_point
                    && self
                        .doc
                        .point(starts[0])
                        .is_ok_and(|q| q.distance(e) <= 1e-9 * e.abs().max_element().max(1.0))
                {
                    starts[0]
                } else {
                    self.doc.add_point(e)
                };
                ends.push((end_point, e));
            }
            for (k, piece) in pieces.iter().enumerate() {
                let (orig, fwd) = chain[k];
                let (s, e) = (starts[k], ends[k].0);
                let new = match (*piece, self.doc.get(orig).map(|x| x.kind)) {
                    (Shape::Line { .. }, _) => {
                        let line = self.doc.add_line(s, e).map_err(doc_err)?;
                        let _ = self
                            .doc
                            .add_constraint(Constraint::Parallel { a: orig, b: line });
                        let _ = self.doc.add_constraint(Constraint::PointLineDistance {
                            point: s,
                            line: orig,
                            value: distance,
                        });
                        line
                    }
                    (Shape::Circle { radius, .. }, Some(EntityKind::Circle { center, .. })) => {
                        self.doc.add_circle(center, radius).map_err(doc_err)?
                    }
                    (Shape::Arc { .. }, Some(EntityKind::Arc { center, .. })) => {
                        // Stored counter-clockwise: start/end follow the original's.
                        let (a, b) = if fwd { (s, e) } else { (e, s) };
                        self.doc.add_arc(center, a, b).map_err(doc_err)?
                    }
                    _ => return Err(invalid("unexpected curve")),
                };
                self.doc.copy_look(orig, new);
                created.push(new);
            }
            Ok(())
        })();
        self.finish(result, "Offset")?;
        Ok(created)
    }

    /// Mirror entities about a line: copies kept symmetric to the originals (points on the
    /// line are shared).
    pub fn mirror(&mut self, ids: &[EntityId], axis: EntityId) -> Result<Vec<EntityId>, EditError> {
        let Some(EntityKind::Line { a: ax, b: bx }) = self.doc.get(axis).map(|e| e.kind) else {
            return Err(invalid("mirror about a line"));
        };
        let (pa, pb) = (
            self.doc.point(ax).unwrap_or_default(),
            self.doc.point(bx).unwrap_or_default(),
        );
        let u = (pb - pa).normalize();
        let reflect = |p: DVec2| {
            let foot = pa + u * (p - pa).dot(u);
            foot * 2.0 - p
        };
        let on_axis =
            |p: DVec2| (p - pa).perp_dot(u).abs() <= intersect::tolerance(p.abs().max_element());
        let ids: Vec<EntityId> = ids.iter().copied().filter(|id| *id != axis).collect();
        if ids.is_empty() {
            return Err(invalid("select geometry to mirror, then the line"));
        }
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut created = Vec::new();
        let result = (|| -> Result<(), EditError> {
            let mut map: HashMap<EntityId, EntityId> = HashMap::new();
            for p in self.doc.defining_points(ids.iter().copied()) {
                let at = self.doc.point(p).map_err(doc_err)?;
                if on_axis(at) {
                    map.insert(p, p);
                } else {
                    let q = self.doc.add_point(reflect(at));
                    map.insert(p, q);
                    let _ = self
                        .doc
                        .add_constraint(Constraint::Symmetric { a: p, b: q, axis });
                    if self.doc.users_of(p).next().is_none() {
                        created.push(q);
                    }
                }
            }
            let m = |p: EntityId| map[&p];
            for &id in &ids {
                let Some(entity) = self.doc.get(id).copied() else {
                    continue;
                };
                let new = match entity.kind {
                    EntityKind::Point(_) => continue,
                    EntityKind::Line { a, b } => {
                        if m(a) == a && m(b) == b {
                            continue; // lies on the axis
                        }
                        self.doc.add_line(m(a), m(b)).map_err(doc_err)?
                    }
                    EntityKind::Circle { center, radius } => {
                        let c = self.doc.add_circle(m(center), radius).map_err(doc_err)?;
                        let _ = self.doc.add_constraint(Constraint::Equal { a: id, b: c });
                        c
                    }
                    // A reflection reverses direction: swap the ends to stay counter-clockwise.
                    EntityKind::Arc { center, start, end } => self
                        .doc
                        .add_arc(m(center), m(end), m(start))
                        .map_err(doc_err)?,
                    EntityKind::Ellipse {
                        center,
                        major,
                        minor,
                    } => self
                        .doc
                        .add_ellipse(m(center), m(major), m(minor))
                        .map_err(doc_err)?,
                    EntityKind::EllipseArc {
                        center,
                        major,
                        minor,
                        start,
                        end,
                    } => self
                        .doc
                        .add_ellipse_arc(m(center), m(major), m(minor), m(end), m(start))
                        .map_err(doc_err)?,
                };
                self.doc.copy_look(id, new);
                created.push(new);
            }
            Ok(())
        })();
        self.finish(result, "Mirror")?;
        Ok(created)
    }

    /// Copies of entities moved by `transform` (applied `count` times, 1..=count), with the
    /// relations among them copied (orientation-dependent ones only if `keep_orientation`).
    fn pattern(
        &mut self,
        ids: &[EntityId],
        copies: usize,
        transform: impl Fn(usize, DVec2) -> DVec2,
        keep_orientation: bool,
        label: &str,
    ) -> Result<Vec<EntityId>, EditError> {
        if ids.is_empty() {
            return Err(invalid("select geometry to pattern first"));
        }
        if copies == 0 {
            return Err(invalid("the count must be at least 2"));
        }
        let points = self.doc.defining_points(ids.iter().copied());
        let mut members: BTreeSet<EntityId> = points.clone();
        members.extend(ids.iter().copied());
        let internal: Vec<(ConstraintId, Constraint)> = self
            .doc
            .constraints()
            .filter(|(_, c)| c.entities().all(|e| members.contains(&e)))
            .filter(|(_, c)| !matches!(c, Constraint::Fix { .. }))
            .filter(|(_, c)| keep_orientation || !c.is_orientation_dependent())
            .map(|(id, c)| (id, *c))
            .collect();
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut created = Vec::new();
        let result = (|| -> Result<(), EditError> {
            for k in 1..=copies {
                let mut map: BTreeMap<EntityId, EntityId> = BTreeMap::new();
                for &p in &points {
                    let at = self.doc.point(p).map_err(doc_err)?;
                    map.insert(p, self.doc.add_point(transform(k, at)));
                }
                for &id in &members {
                    let Some(entity) = self.doc.get(id).copied() else {
                        continue;
                    };
                    let m = |p: EntityId| map[&p];
                    let new = match entity.kind {
                        EntityKind::Point(_) => continue,
                        EntityKind::Line { a, b } => self.doc.add_line(m(a), m(b)),
                        EntityKind::Circle { center, radius } => {
                            self.doc.add_circle(m(center), radius)
                        }
                        EntityKind::Arc { center, start, end } => {
                            self.doc.add_arc(m(center), m(start), m(end))
                        }
                        EntityKind::Ellipse {
                            center,
                            major,
                            minor,
                        } => self.doc.add_ellipse(m(center), m(major), m(minor)),
                        EntityKind::EllipseArc {
                            center,
                            major,
                            minor,
                            start,
                            end,
                        } => self.doc.add_ellipse_arc(
                            m(center),
                            m(major),
                            m(minor),
                            m(start),
                            m(end),
                        ),
                    }
                    .map_err(doc_err)?;
                    self.doc.copy_look(id, new);
                    map.insert(id, new);
                    created.push(new);
                }
                for &(cid, c) in &internal {
                    let copy = c.map_entities(|e| map.get(&e).copied().unwrap_or(e));
                    if let Ok(new) = self.doc.add_constraint(copy) {
                        if let Some(p) = self.doc.placement(cid) {
                            let _ = self.doc.set_placement(new, p);
                        }
                        if let Some(e) = self.doc.expression(cid) {
                            self.doc.set_expression(new, Some(e.to_owned()));
                        }
                        if self.doc.is_driven(cid) {
                            let _ = self.doc.set_driven(new, true);
                        }
                    }
                }
            }
            Ok(())
        })();
        self.finish(result, label)?;
        Ok(created)
    }

    /// `count` instances (the original plus copies) spaced by `step`.
    pub fn linear_pattern(
        &mut self,
        ids: &[EntityId],
        count: usize,
        step: DVec2,
    ) -> Result<Vec<EntityId>, EditError> {
        if step == DVec2::ZERO {
            return Err(invalid("the spacing can't be zero"));
        }
        self.pattern(
            ids,
            count.saturating_sub(1),
            |k, p| p + step * k as f64,
            true,
            "Linear pattern",
        )
    }

    /// `count` instances (the original plus copies) rotated about `center`, spread over
    /// `total_degrees` (360 = evenly round the full circle).
    pub fn circular_pattern(
        &mut self,
        ids: &[EntityId],
        count: usize,
        center: DVec2,
        total_degrees: f64,
    ) -> Result<Vec<EntityId>, EditError> {
        if count < 2 {
            return Err(invalid("the count must be at least 2"));
        }
        let full = (total_degrees.abs() - 360.0).abs() < 1e-9;
        let step = if full {
            total_degrees / count as f64
        } else {
            total_degrees / (count - 1) as f64
        };
        self.pattern(
            ids,
            count - 1,
            |k, p| center + DVec2::from_angle((step * k as f64).to_radians()).rotate(p - center),
            false,
            "Circular pattern",
        )
    }
}

/// A piece's end point in travel order (`last` = where travel ends).
fn piece_end(shape: &Shape, forward: bool, last: bool) -> DVec2 {
    let (s, e) = match *shape {
        Shape::Line { a, b } => (a, b),
        Shape::Arc { .. } | Shape::Ellipse { .. } => {
            (point_at(shape, 0.0), point_at(shape, sweep_of(shape)))
        }
        Shape::Circle { center, radius } => {
            let p = center + DVec2::new(radius, 0.0);
            (p, p)
        }
        Shape::Point(p) => (p, p),
    };
    // Lines are already built in travel order; arcs are stored counter-clockwise.
    let travel = match shape {
        Shape::Line { .. } => (s, e),
        _ if forward => (s, e),
        _ => (e, s),
    };
    if last { travel.1 } else { travel.0 }
}

fn sweep_of(shape: &Shape) -> f64 {
    match *shape {
        Shape::Arc { sweep, .. } | Shape::Ellipse { sweep, .. } => sweep,
        _ => 0.0,
    }
}

/// Move a piece's end (in travel order) to `to` (arcs: by changing the angle).
fn set_piece_end(shape: &mut Shape, forward: bool, last: bool, to: DVec2) {
    match shape {
        Shape::Line { a, b } => {
            if last {
                *b = to;
            } else {
                *a = to;
            }
        }
        Shape::Arc {
            center,
            start,
            sweep,
            ..
        } => {
            let angle = (to - *center).y.atan2((to - *center).x);
            // Which stored end moves: travel end = stored end when forward.
            let moves_end = last == forward;
            if moves_end {
                *sweep = (angle - *start).rem_euclid(TAU);
            } else {
                let end = *start + *sweep;
                *start = angle;
                *sweep = (end - angle).rem_euclid(TAU);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, TAU};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-9
    }

    /// An editor with lines between the given point pairs; returns the line ids.
    fn with_lines(lines: &[(DVec2, DVec2)]) -> (Editor, Vec<EntityId>) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let ids = lines
            .iter()
            .map(|(a, b)| {
                let pa = ed.doc.add_point(*a);
                let pb = ed.doc.add_point(*b);
                ed.doc.add_line(pa, pb).unwrap()
            })
            .collect();
        ed.commit("Setup");
        (ed, ids)
    }

    fn ends(ed: &Editor, line: EntityId) -> (DVec2, DVec2) {
        match ed.doc.shape(line) {
            Some(Shape::Line { a, b }) => (a, b),
            other => panic!("not a line: {other:?}"),
        }
    }

    fn count(ed: &Editor, f: impl Fn(&Constraint) -> bool) -> usize {
        ed.doc.constraints().filter(|(_, c)| f(c)).count()
    }

    fn lines(ed: &Editor) -> Vec<(DVec2, DVec2)> {
        ed.doc
            .iter()
            .filter_map(|(id, e)| match e.kind {
                EntityKind::Line { .. } => Some(ends(ed, id)),
                _ => None,
            })
            .collect()
    }

    /// A horizontal line crossed by two vertical ones at x = 10 and 20.
    fn crossed() -> (Editor, Vec<EntityId>) {
        with_lines(&[
            (v(0.0, 0.0), v(30.0, 0.0)),
            (v(10.0, -5.0), v(10.0, 5.0)),
            (v(20.0, -5.0), v(20.0, 5.0)),
        ])
    }

    #[test]
    fn trim_removes_the_middle_piece() {
        let (mut ed, ids) = crossed();
        ed.trim_at(ids[0], v(15.0, 0.3)).unwrap();
        assert_eq!(ends(&ed, ids[0]), (v(0.0, 0.0), v(10.0, 0.0)));
        assert!(
            lines(&ed)
                .iter()
                .any(|l| close(l.0, v(20.0, 0.0)) && close(l.1, v(30.0, 0.0)))
        );
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Collinear { .. })), 1);
        assert_eq!(
            count(&ed, |c| matches!(c, Constraint::PointOnCurve { .. })),
            2
        );
        assert!(ed.analysis().conflicts.is_empty());
        // One undo step.
        ed.undo();
        assert_eq!(ends(&ed, ids[0]), (v(0.0, 0.0), v(30.0, 0.0)));
        assert_eq!(lines(&ed).len(), 3);
    }

    #[test]
    fn trim_shortens_at_the_end_and_removes_uncut_curves() {
        let (mut ed, ids) = crossed();
        let before = ed.doc.len();
        ed.trim_at(ids[0], v(27.0, 0.0)).unwrap();
        assert_eq!(ends(&ed, ids[0]), (v(0.0, 0.0), v(20.0, 0.0)));
        // The old end point went; a new point (on the cutting line) replaced it.
        assert_eq!(ed.doc.len(), before);
        // A vertical line cut once: the piece below goes.
        ed.trim_at(ids[1], v(10.0, -3.0)).unwrap();
        assert_eq!(ends(&ed, ids[1]), (v(10.0, 0.0), v(10.0, 5.0)));
        // A line nothing cuts is removed entirely.
        let (mut ed, ids) = with_lines(&[(v(0.0, 0.0), v(5.0, 0.0))]);
        ed.trim_at(ids[0], v(2.0, 0.0)).unwrap();
        assert!(ed.doc.is_empty());
    }

    #[test]
    fn trim_joins_an_existing_end_point() {
        // A T: the stem's end sits on the bar.
        let (mut ed, ids) =
            with_lines(&[(v(0.0, 0.0), v(30.0, 0.0)), (v(10.0, 0.0), v(10.0, 8.0))]);
        ed.trim_at(ids[0], v(4.0, 0.0)).unwrap();
        let Some(EntityKind::Line { a: bar_a, .. }) = ed.doc.get(ids[0]).map(|e| e.kind) else {
            unreachable!()
        };
        let Some(EntityKind::Line { a: stem_a, .. }) = ed.doc.get(ids[1]).map(|e| e.kind) else {
            unreachable!()
        };
        assert_eq!(bar_a, stem_a, "now connected");
        assert_eq!(
            count(&ed, |c| matches!(c, Constraint::PointOnCurve { .. })),
            0
        );
    }

    #[test]
    fn trimming_a_circle_leaves_an_arc_with_its_relations() {
        let (mut ed, _) = with_lines(&[
            (v(-10.0, 3.0), v(10.0, 3.0)),
            (v(-10.0, -3.0), v(10.0, -3.0)),
        ]);
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(c, 5.0).unwrap();
        ed.doc
            .add_constraint(Constraint::Radius {
                curve: circle,
                value: 5.0,
            })
            .unwrap();
        ed.commit("Circle");
        // Remove the top cap.
        ed.trim_at(circle, v(0.0, 5.0)).unwrap();
        let Some(Shape::Arc {
            radius,
            start,
            sweep,
            ..
        }) = ed.doc.shape(circle)
        else {
            panic!("expected an arc")
        };
        assert!((radius - 5.0).abs() < 1e-9);
        // Kept: from the upper left cut (-4, 3) round the bottom to the upper right (4, 3).
        let s = 5.0 * DVec2::from_angle(start);
        assert!(close(s, v(-4.0, 3.0)), "{s}");
        assert!((sweep - (TAU - 2.0 * (4.0f64).atan2(3.0))).abs() < 1e-9);
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Radius { .. })), 1);
        // A circle cut once can't be trimmed.
        let (mut ed, _) = with_lines(&[(v(5.0, -10.0), v(5.0, 10.0))]);
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(c, 5.0).unwrap();
        ed.commit("Circle");
        assert!(ed.trim_at(circle, v(-5.0, 0.0)).is_err());
    }

    #[test]
    fn extend_reaches_the_next_curve() {
        let (mut ed, ids) = with_lines(&[
            (v(0.0, 0.0), v(5.0, 0.0)),
            (v(10.0, -5.0), v(10.0, 5.0)),
            (v(20.0, -5.0), v(20.0, 5.0)),
        ]);
        ed.extend_at(ids[0], v(4.0, 0.0)).unwrap();
        assert_eq!(ends(&ed, ids[0]), (v(0.0, 0.0), v(10.0, 0.0)));
        assert_eq!(
            count(&ed, |c| matches!(c, Constraint::PointOnCurve { .. })),
            1
        );
        // Nothing beyond the other end.
        assert!(ed.extend_at(ids[0], v(0.5, 0.0)).is_err());
        // An arc extends backwards from its start to a line below.
        let (mut ed, _) = with_lines(&[(v(-10.0, -3.0), v(10.0, -3.0))]);
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let s = ed.doc.add_point(v(5.0, 0.0));
        let e = ed.doc.add_point(v(0.0, 5.0));
        let arc = ed.doc.add_arc(c, s, e).unwrap();
        ed.commit("Arc");
        ed.extend_at(arc, v(4.9, 0.5)).unwrap();
        assert!(close(ed.doc.point(s).unwrap(), v(4.0, -3.0)));
    }

    #[test]
    fn split_keeps_the_pieces_collinear() {
        let (mut ed, ids) = with_lines(&[(v(0.0, 0.0), v(30.0, 0.0))]);
        let m = ed.split_at(ids[0], v(12.0, 0.4)).unwrap();
        assert!(close(ed.doc.point(m).unwrap(), v(12.0, 0.0)));
        assert_eq!(lines(&ed).len(), 2);
        assert_eq!(ed.doc.users_of(m).count(), 2);
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Collinear { .. })), 1);
        assert!(ed.split_at(ids[0], v(0.0, 0.0)).is_err(), "at an end");
    }

    /// Two lines meeting at the origin: along +x (length 20) and +y (length 10).
    fn corner() -> (Editor, EntityId) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let p = ed.doc.add_point(v(0.0, 0.0));
        let a = ed.doc.add_point(v(20.0, 0.0));
        let b = ed.doc.add_point(v(0.0, 10.0));
        ed.doc.add_line(p, a).unwrap();
        ed.doc.add_line(p, b).unwrap();
        ed.commit("Corner");
        (ed, p)
    }

    #[test]
    fn fillet_rounds_a_corner_tangentially() {
        let (mut ed, p) = corner();
        let arc = ed.fillet(p, 3.0).unwrap();
        let Some(Shape::Arc {
            center,
            radius,
            sweep,
            ..
        }) = ed.doc.shape(arc)
        else {
            panic!()
        };
        assert!(close(center, v(3.0, 3.0)) && (radius - 3.0).abs() < 1e-9);
        assert!((sweep - FRAC_PI_2).abs() < 1e-9);
        assert!(!ed.doc.contains(p), "the sharp corner is gone");
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Tangent { .. })), 2);
        assert!(ed.analysis().conflicts.is_empty());
        let ls = lines(&ed);
        assert!(ls.iter().any(|l| close(l.0, v(3.0, 0.0))));
        assert!(ls.iter().any(|l| close(l.0, v(0.0, 3.0))));
        // Too big.
        let (mut ed, p) = corner();
        assert!(ed.fillet(p, 15.0).is_err());
    }

    #[test]
    fn chamfer_cuts_a_corner() {
        let (mut ed, p) = corner();
        let line = ed.chamfer(p, 2.0, 3.0).unwrap();
        let (a, b) = ends(&ed, line);
        assert!(close(a, v(2.0, 0.0)) && close(b, v(0.0, 3.0)));
        assert!(!ed.doc.contains(p));
        // Not a two-line corner.
        let (mut ed, ids) = with_lines(&[(v(0.0, 0.0), v(5.0, 0.0))]);
        let Some(EntityKind::Line { a, .. }) = ed.doc.get(ids[0]).map(|e| e.kind) else {
            unreachable!()
        };
        assert!(ed.chamfer(a, 1.0, 1.0).is_err());
    }

    /// A connected polyline through the points.
    fn polyline(points: &[DVec2], closed: bool) -> (Editor, Vec<EntityId>) {
        let mut ed = Editor::default();
        ed.doc.begin();
        let ps: Vec<EntityId> = points.iter().map(|p| ed.doc.add_point(*p)).collect();
        let n = ps.len();
        let segs = if closed { n } else { n - 1 };
        let ids = (0..segs)
            .map(|i| ed.doc.add_line(ps[i], ps[(i + 1) % n]).unwrap())
            .collect();
        ed.commit("Polyline");
        (ed, ids)
    }

    #[test]
    fn offset_follows_a_chain_and_rejoins_corners() {
        let (mut ed, ids) = polyline(&[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0)], false);
        let (chain, closed) = ed.chain(ids[1]);
        assert_eq!(chain.len(), 2);
        assert!(!closed);
        let new = ed.offset(ids[0], 2.0, v(10.0, 5.0)).unwrap();
        assert_eq!(new.len(), 2);
        let (a0, b0) = ends(&ed, new[0]);
        let (a1, b1) = ends(&ed, new[1]);
        assert!(
            close(a0, v(0.0, 2.0)) && close(b0, v(18.0, 2.0)),
            "{a0} {b0}"
        );
        assert!(
            close(a1, v(18.0, 2.0)) && close(b1, v(18.0, 10.0)),
            "{a1} {b1}"
        );
        assert!(ed.analysis().conflicts.is_empty());
        // Closed rectangle, inwards.
        let (mut ed, ids) = polyline(
            &[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)],
            true,
        );
        let new = ed.offset(ids[0], 1.0, v(10.0, 5.0)).unwrap();
        assert_eq!(new.len(), 4);
        let corners: BTreeSet<_> = new
            .iter()
            .flat_map(|l| {
                let (a, b) = ends(&ed, *l);
                [
                    (a.x.round() as i64, a.y.round() as i64),
                    (b.x.round() as i64, b.y.round() as i64),
                ]
            })
            .collect();
        assert_eq!(corners, BTreeSet::from([(1, 1), (19, 1), (19, 9), (1, 9)]));
        // Every corner is shared: four distinct points.
        let points: BTreeSet<EntityId> = ed.doc.defining_points(new.iter().copied());
        assert_eq!(points.len(), 4);
        assert!(ed.analysis().conflicts.is_empty());
        // Too far.
        assert!(ed.offset(ids[0], 6.0, v(10.0, 5.0)).is_err());
    }

    #[test]
    fn offset_circles_share_their_centre() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(c, 5.0).unwrap();
        ed.commit("Circle");
        let out = ed.offset(circle, 2.0, v(10.0, 0.0)).unwrap();
        assert!(matches!(ed.doc.get(out[0]).map(|e| e.kind),
            Some(EntityKind::Circle { center, radius }) if center == c && (radius - 7.0).abs() < 1e-12));
        let inner = ed.offset(circle, 2.0, v(1.0, 0.0)).unwrap();
        assert!(matches!(ed.doc.get(inner[0]).map(|e| e.kind),
            Some(EntityKind::Circle { radius, .. }) if (radius - 3.0).abs() < 1e-12));
    }

    #[test]
    fn mirror_keeps_copies_symmetric() {
        let (mut ed, ids) =
            with_lines(&[(v(2.0, 1.0), v(5.0, 4.0)), (v(0.0, -10.0), v(0.0, 10.0))]);
        let copies = ed.mirror(&[ids[0]], ids[1]).unwrap();
        assert_eq!(copies.len(), 1);
        let (a, b) = ends(&ed, copies[0]);
        assert!(close(a, v(-2.0, 1.0)) && close(b, v(-5.0, 4.0)));
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Symmetric { .. })), 2);
        // With the axis fixed, moving the original moves the copy.
        let fixes: Vec<Constraint> = ed
            .doc
            .defining_points([ids[1]])
            .into_iter()
            .map(|point| Constraint::Fix {
                point,
                at: ed.doc.point(point).unwrap(),
            })
            .collect();
        ed.add_constraints("Fix", &fixes).unwrap();
        let Some(EntityKind::Line { a: pid, .. }) = ed.doc.get(ids[0]).map(|e| e.kind) else {
            unreachable!()
        };
        ed.doc.begin();
        assert!(ed.drag(&[pid], v(1.0, 0.0), &drift_solver::Param::point(pid)));
        ed.commit("Move");
        let (a, _) = ends(&ed, copies[0]);
        assert!(a.distance(v(-3.0, 1.0)) < 1e-5, "{a}");
        // A line touching the axis shares the point on it.
        let (mut ed, ids) =
            with_lines(&[(v(0.0, 2.0), v(5.0, 4.0)), (v(0.0, -10.0), v(0.0, 10.0))]);
        let copies = ed.mirror(&[ids[0]], ids[1]).unwrap();
        let Some(EntityKind::Line { a: orig_a, .. }) = ed.doc.get(ids[0]).map(|e| e.kind) else {
            unreachable!()
        };
        let Some(EntityKind::Line { a: copy_a, .. }) = ed.doc.get(copies[0]).map(|e| e.kind) else {
            unreachable!()
        };
        assert_eq!(orig_a, copy_a);
    }

    #[test]
    fn patterns_copy_geometry_and_relations() {
        let (mut ed, ids) = polyline(
            &[v(0.0, 0.0), v(10.0, 0.0), v(10.0, 5.0), v(0.0, 5.0)],
            true,
        );
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: ids[0] }])
            .unwrap();
        let ends0: Vec<EntityId> = ed.doc.defining_points([ids[0]]).into_iter().collect();
        ed.add_constraints(
            "Length",
            &[Constraint::Distance {
                a: ends0[0],
                b: ends0[1],
                value: 10.0,
            }],
        )
        .unwrap();
        let created = ed.linear_pattern(&ids, 3, v(15.0, 0.0)).unwrap();
        assert_eq!(created.len(), 8);
        assert_eq!(lines(&ed).len(), 12);
        assert_eq!(
            count(&ed, |c| matches!(c, Constraint::Horizontal { .. })),
            3
        );
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Distance { .. })), 3);
        assert!(lines(&ed).iter().any(|l| close(l.0, v(30.0, 0.0))));
        assert!(ed.analysis().conflicts.is_empty());
        // Circular: four instances of a line round the origin; horizontal relations are dropped.
        let (mut ed, ids) = with_lines(&[(v(5.0, 0.0), v(8.0, 0.0))]);
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: ids[0] }])
            .unwrap();
        ed.circular_pattern(&ids, 4, v(0.0, 0.0), 360.0).unwrap();
        let ls = lines(&ed);
        assert_eq!(ls.len(), 4);
        assert!(
            ls.iter()
                .any(|l| close(l.0, v(0.0, 5.0)) && close(l.1, v(0.0, 8.0)))
        );
        assert!(ls.iter().any(|l| close(l.0, v(-5.0, 0.0))));
        assert_eq!(
            count(&ed, |c| matches!(c, Constraint::Horizontal { .. })),
            1
        );
    }
}
