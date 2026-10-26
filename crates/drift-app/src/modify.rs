//! Modify operations on the document: trim, extend, split, fillet, chamfer, offset, mirror and
//! patterns. Each is one undoable step that is solved and committed, or refused with a reason.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use drift_core::{Constraint, ConstraintId, DVec2, EntityId, EntityKind, Shape, arc_shape};
use drift_geom::intersect::{self, intersect, intersect_extended, intersect_unbounded};
use drift_solver::Param;

use crate::editor::{EditError, Editor};

fn invalid(msg: impl Into<String>) -> EditError {
    EditError::Invalid(msg.into())
}

/// How far off a constraint may be and still count as met, relative to the coordinates' size.
const UNMET: f64 = 1e-7;

/// The constraints in a document before an edit, and those it didn't meet already.
struct ConstraintState {
    all: BTreeSet<ConstraintId>,
    unmet: std::collections::HashSet<ConstraintId>,
}

/// One curve of an offset, in travel order along the chain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OffsetPiece {
    pub shape: Shape,
    /// Which curve of the chain it is the offset of.
    pub source: usize,
    /// Whether travel runs the way the shape is stored (arcs are stored counter-clockwise
    /// whichever way the chain passes through them).
    pub forward: bool,
    /// A straight bridge across a corner that opened up, rather than an offset of `source`.
    pub bridge: bool,
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
        Shape::Bezier { p: q } => drift_geom::nearest_on_bezier(&q, p).1,
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
        Shape::Bezier { p } => drift_core::shape::bezier_point(&p, t),
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
        Shape::Bezier { p } => Shape::Bezier {
            p: drift_core::shape::bezier_piece(&p, t0, t1),
        },
        Shape::Point(p) => Shape::Point(p),
    }
}

/// An existing chamfer: the corner its two lines would meet at, their far ends, the lines, the
/// cut's end points (on the first and second line), and how far it cuts along each.
#[derive(Clone, Copy, Debug)]
pub struct ChamferInfo {
    pub corner: DVec2,
    pub far: [DVec2; 2],
    pub lines: [EntityId; 2],
    pub ends: [EntityId; 2],
    pub legs: [f64; 2],
}

impl ChamferInfo {
    /// Half the angle between the lines, and the shorter line's length from the corner.
    pub fn half_angle_and_room(&self) -> (f64, f64) {
        let (a, b) = (self.far[0] - self.corner, self.far[1] - self.corner);
        let cos = (a.normalize().dot(b.normalize())).clamp(-1.0, 1.0);
        (cos.acos() / 2.0, a.length().min(b.length()))
    }
}

/// An existing fillet: the corner its two curves (lines or arcs) would meet at, how they run
/// from there, the curves, the arc's end points (on the first and second curve) and centre
/// point, and its radius.
#[derive(Clone, Copy, Debug)]
pub struct FilletInfo {
    pub corner: DVec2,
    pub arms: [Arm; 2],
    /// The two curves it joins.
    pub curves: [EntityId; 2],
    pub ends: [EntityId; 2],
    pub center: EntityId,
    pub radius: f64,
}

impl FilletInfo {
    /// Half the angle between the curves at the corner, and the shorter one's length from it.
    pub fn half_angle_and_room(&self) -> (f64, f64) {
        corner_angle(self.corner, self.arms)
    }
}

/// Why a point isn't a corner: `ending` curves end there, not two.
fn not_a_corner(ending: usize, what: &str) -> EditError {
    invalid(match ending {
        0 => format!("no curve ends here: pick a corner where two {what} meet"),
        1 => format!("only one curve ends here: pick a corner where two {what} meet"),
        n => format!("{n} curves meet here: pick a corner where only two {what} meet"),
    })
}

/// Half the angle between two arms where they leave `corner`, and the shorter one's length.
pub fn corner_angle(corner: DVec2, arms: [Arm; 2]) -> (f64, f64) {
    let (u1, u2) = (arms[0].dir(corner), arms[1].dir(corner));
    let cos = u1.dot(u2).clamp(-1.0, 1.0);
    (cos.acos() / 2.0, arms[0].length.min(arms[1].length))
}

/// One side of a corner: how the curve runs from the corner, and how far before it ends.
#[derive(Clone, Copy, Debug)]
pub struct Arm {
    pub run: Run,
    pub length: f64,
}

/// How a curve runs from a corner: a line in a direction, or around a circle one way.
#[derive(Clone, Copy, Debug)]
pub enum Run {
    Line {
        dir: DVec2,
    },
    Circle {
        center: DVec2,
        radius: f64,
        ccw: bool,
    },
}

impl Arm {
    /// The direction it leaves `corner` in.
    pub fn dir(&self, corner: DVec2) -> DVec2 {
        match self.run {
            Run::Line { dir } => dir,
            Run::Circle { center, ccw, .. } => {
                let out = (corner - center).normalize_or_zero();
                if ccw { out.perp() } else { -out.perp() }
            }
        }
    }

    /// How far along it from `corner` the point `p` (on it) is.
    fn along(&self, corner: DVec2, p: DVec2) -> f64 {
        match self.run {
            Run::Line { dir } => (p - corner).dot(dir),
            Run::Circle {
                center,
                radius,
                ccw,
            } => {
                let angle = |q: DVec2| (q.y - center.y).atan2(q.x - center.x);
                let turn = angle(p) - angle(corner);
                (if ccw { turn } else { -turn }).rem_euclid(TAU) * radius
            }
        }
    }

    /// The point on it nearest `c`.
    fn foot(&self, corner: DVec2, c: DVec2) -> DVec2 {
        match self.run {
            Run::Line { dir } => corner + dir * (c - corner).dot(dir),
            Run::Circle { center, radius, .. } => {
                center + (c - center).normalize_or_zero() * radius
            }
        }
    }

    /// Its whole line or circle, through `corner`.
    fn path(&self, corner: DVec2) -> Path {
        match self.run {
            Run::Line { dir } => Path::Line(corner, dir),
            Run::Circle { center, radius, .. } => Path::Circle(center, radius),
        }
    }
}

/// A whole line (a point and a direction) or circle.
#[derive(Clone, Copy, Debug)]
enum Path {
    Line(DVec2, DVec2),
    Circle(DVec2, f64),
}

/// Where two paths cross (touching counts once).
fn crossings(a: Path, b: Path) -> Vec<DVec2> {
    fn line_circle(p: DVec2, d: DVec2, c: DVec2, r: f64) -> Vec<DVec2> {
        let f = p + d * (c - p).dot(d);
        let h2 = r * r - f.distance_squared(c);
        if h2 < -1e-9 * r * r {
            Vec::new()
        } else if h2 <= 0.0 {
            vec![f]
        } else {
            let h = h2.sqrt();
            vec![f - d * h, f + d * h]
        }
    }
    match (a, b) {
        (Path::Line(p1, d1), Path::Line(p2, d2)) => {
            let cross = d1.perp_dot(d2);
            if cross.abs() < 1e-12 {
                Vec::new()
            } else {
                vec![p1 + d1 * ((p2 - p1).perp_dot(d2) / cross)]
            }
        }
        (Path::Line(p, d), Path::Circle(c, r)) | (Path::Circle(c, r), Path::Line(p, d)) => {
            line_circle(p, d, c, r)
        }
        (Path::Circle(c1, r1), Path::Circle(c2, r2)) => {
            let d = c1.distance(c2);
            if d < 1e-12 {
                return Vec::new();
            }
            let u = (c2 - c1) / d;
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            // Where the circles cross is where the line through the chord meets the first.
            line_circle(c1 + u * a, u.perp(), c1, r1)
        }
    }
}

/// Fillet geometry where two curves leave `corner` along `arms`: tangent points, centre, and
/// whether the arc runs counter-clockwise from the first tangent point. The centre is where
/// the curves cross once each is offset by `r` towards the other.
pub fn fillet_geometry(
    corner: DVec2,
    arms: [Arm; 2],
    r: f64,
) -> Result<(DVec2, DVec2, DVec2, bool), EditError> {
    let lines = arms.iter().all(|a| matches!(a.run, Run::Line { .. }));
    let curves = if lines { "lines" } else { "curves" };
    let u = [arms[0].dir(corner), arms[1].dir(corner)];
    let alpha = u[0].dot(u[1]).clamp(-1.0, 1.0).acos();
    if !(1e-6..=std::f64::consts::PI - 1e-6).contains(&alpha) {
        return Err(invalid(format!(
            "the {curves} are in line: nothing to round"
        )));
    }
    if !(r > 0.0 && r.is_finite()) {
        return Err(invalid("the radius must be positive"));
    }
    let too_large = || invalid(format!("the radius is too large for these {curves}"));
    let offset = |i: usize| -> Option<Path> {
        let mut n = u[i].perp();
        if n.dot(u[1 - i]) < 0.0 {
            n = -n;
        }
        match arms[i].run {
            Run::Line { dir } => Some(Path::Line(corner + n * r, dir)),
            Run::Circle { center, radius, .. } => {
                let grown = if n.dot(corner - center) > 0.0 {
                    radius + r
                } else {
                    radius - r
                };
                (grown > 1e-12).then_some(Path::Circle(center, grown))
            }
        }
    };
    let (Some(o1), Some(o2)) = (offset(0), offset(1)) else {
        return Err(too_large());
    };
    // Of the places that are both, the one nearest where a fillet between the curves'
    // directions would have its centre.
    let guess = corner + (u[0] + u[1]).normalize() * (r / (alpha / 2.0).sin());
    let c = crossings(o1, o2)
        .into_iter()
        .min_by(|a, b| a.distance(guess).total_cmp(&b.distance(guess)))
        .ok_or_else(too_large)?;
    let t1 = arms[0].foot(corner, c);
    let t2 = arms[1].foot(corner, c);
    for (arm, t) in arms.iter().zip([t1, t2]) {
        let d = arm.along(corner, t);
        if !(d > 1e-9 && d < arm.length) {
            return Err(too_large());
        }
    }
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
                Some(Shape::Bezier { p }) => return drift_core::shape::bezier_point(&p, 0.5),
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
        let bounding = self.fill_edges();
        self.doc
            .iter()
            .filter(|(id, e)| {
                !e.kind.is_point()
                    && (e.style == drift_core::LineKind::Solid || bounding.contains(id))
            })
            .filter_map(|(id, _)| Some((id, self.doc.shape(id)?)))
            .filter(|(_, shape)| {
                // The parameter at the curve's end (see `point_at`).
                let end = match *shape {
                    Shape::Line { .. } | Shape::Bezier { .. } => 1.0,
                    Shape::Arc { sweep, .. } | Shape::Ellipse { sweep, .. } => sweep,
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
            .and_then(|e| e.kind.ends())
            .into_iter()
            .flatten()
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
                let near = tol.max(1e-7 * (1.0 + hit.at.length()));
                if own_ends.iter().any(|e| e.distance(hit.at) <= near) {
                    continue;
                }
                // Numeric hits (ellipses, curves) are a little looser than exact ones.
                let near = tol.max(1e-7 * (1.0 + hit.at.length()));
                let point = self.doc.get(other).and_then(|e| {
                    e.kind
                        .ends()
                        .into_iter()
                        .flatten()
                        .find(|p| self.doc.point(*p).is_ok_and(|q| q.distance(hit.at) <= near))
                });
                if cuts.iter().any(|c| c.at.distance(hit.at) <= near) {
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
        self.relate(Constraint::PointOnCurve {
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
            .ok_or_else(|| invalid("pick a curve"))?;
        let cuts = self.cuts(id);
        let tc = param_of(&shape, click);
        let closed = match shape {
            Shape::Circle { .. } => true,
            Shape::Ellipse { sweep, .. } => sweep >= TAU,
            _ => false,
        };
        if closed {
            if cuts.len() < 2 {
                return Err(invalid(
                    "a circle or ellipse needs two cutting curves to trim",
                ));
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
            Shape::Line { .. } | Shape::Bezier { .. } => 1.0,
            Shape::Arc { sweep, .. } => sweep,
            Shape::Ellipse { sweep, .. } if sweep < TAU => sweep,
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
        self.refuse_owned(&[id], crate::walls::Op::Cut)?;
        let (lo, hi) = self.trim_span(id, click)?;
        let kind = self.doc.get(id).map(|e| e.kind).expect("checked");
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        let before = self.constraint_state();
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
                        self.relate(Constraint::Collinear { a: id, b: rest });
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
                        self.relate(Constraint::Equal { a: id, b: rest });
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
                (
                    EntityKind::Ellipse {
                        center,
                        major,
                        minor,
                    },
                    Some(l),
                    Some(h),
                ) => {
                    let pl = self.cut_point(&l);
                    let ph = self.cut_point(&h);
                    // As for a circle: what remains runs counter-clockwise from the cut
                    // after the click round to the cut before it.
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::EllipseArc {
                                center,
                                major,
                                minor,
                                start: ph,
                                end: pl,
                            },
                        )
                        .map_err(doc_err)?;
                }
                (
                    EntityKind::EllipseArc {
                        center,
                        major,
                        minor,
                        start,
                        end,
                    },
                    lo,
                    hi,
                ) => {
                    let arc = |start, end| EntityKind::EllipseArc {
                        center,
                        major,
                        minor,
                        start,
                        end,
                    };
                    match (lo, hi) {
                        (None, Some(h)) => {
                            let p = self.cut_point(&h);
                            self.doc.set_entity_kind(id, arc(p, end)).map_err(doc_err)?;
                            self.doc.remove_if_orphaned(start);
                        }
                        (Some(l), None) => {
                            let p = self.cut_point(&l);
                            self.doc
                                .set_entity_kind(id, arc(start, p))
                                .map_err(doc_err)?;
                            self.doc.remove_if_orphaned(end);
                        }
                        (Some(l), Some(h)) => {
                            let pl = self.cut_point(&l);
                            let ph = self.cut_point(&h);
                            self.doc
                                .set_entity_kind(id, arc(start, pl))
                                .map_err(doc_err)?;
                            // The other piece shares the centre and axes: one ellipse still.
                            let rest = self
                                .doc
                                .add_ellipse_arc(center, major, minor, ph, end)
                                .map_err(doc_err)?;
                            self.doc.copy_look(id, rest);
                        }
                        (None, None) => unreachable!(),
                    }
                }
                (EntityKind::Bezier { a, c1, c2, b }, lo, hi) => {
                    let Some(Shape::Bezier { p: whole }) = self.doc.shape(id) else {
                        return Err(invalid("nothing to trim here"));
                    };
                    let part = |t0: f64, t1: f64| drift_core::shape::bezier_piece(&whole, t0, t1);
                    match (lo, hi) {
                        (None, Some(h)) => {
                            // Keep the far part: its handles shrink along their own lines,
                            // so a smooth join at the far end stays smooth.
                            let q = part(h.t, 1.0);
                            let p = self.cut_point(&h);
                            self.forget_smooth(a, c1);
                            self.doc.set_point(c1, q[1]).map_err(doc_err)?;
                            self.doc.set_point(c2, q[2]).map_err(doc_err)?;
                            self.doc
                                .set_entity_kind(id, EntityKind::Bezier { a: p, c1, c2, b })
                                .map_err(doc_err)?;
                            self.doc.remove_if_orphaned(a);
                        }
                        (Some(l), None) => {
                            let q = part(0.0, l.t);
                            let p = self.cut_point(&l);
                            self.forget_smooth(b, c2);
                            self.doc.set_point(c1, q[1]).map_err(doc_err)?;
                            self.doc.set_point(c2, q[2]).map_err(doc_err)?;
                            self.doc
                                .set_entity_kind(id, EntityKind::Bezier { a, c1, c2, b: p })
                                .map_err(doc_err)?;
                            self.doc.remove_if_orphaned(b);
                        }
                        (Some(l), Some(h)) => {
                            // Two pieces: each keeps the handle at its original end and
                            // gets a new one at its cut.
                            let (first, second) = (part(0.0, l.t), part(h.t, 1.0));
                            let pl = self.cut_point(&l);
                            let ph = self.cut_point(&h);
                            self.doc.set_point(c1, first[1]).map_err(doc_err)?;
                            let c2_first = self.doc.add_point(first[2]);
                            self.doc
                                .set_entity_kind(
                                    id,
                                    EntityKind::Bezier {
                                        a,
                                        c1,
                                        c2: c2_first,
                                        b: pl,
                                    },
                                )
                                .map_err(doc_err)?;
                            let c1_second = self.doc.add_point(second[1]);
                            self.doc.set_point(c2, second[2]).map_err(doc_err)?;
                            let rest =
                                self.doc.add_bezier(ph, c1_second, c2, b).map_err(doc_err)?;
                            self.doc.copy_look(id, rest);
                        }
                        (None, None) => unreachable!(),
                    }
                }
                _ => return Err(invalid("nothing to trim here")),
            }
            Ok(())
        })();
        self.finish_reshape(result, "Trim", &before)
    }

    /// Drop the smooth-join relation holding `handle` in line through `end`: the curve no
    /// longer ends there once that end is trimmed away.
    fn forget_smooth(&mut self, end: EntityId, handle: EntityId) {
        let gone: Vec<ConstraintId> = self
            .doc
            .constraints()
            .filter(|(_, c)| {
                matches!(c, Constraint::Smooth { point, a, b }
                    if *point == end && (*a == handle || *b == handle))
            })
            .map(|(id, _)| id)
            .collect();
        for id in gone {
            self.doc.remove_constraint(id);
        }
    }

    /// Roll back on error, or solve and commit (reporting conflicts).
    /// The constraints there are, and those the geometry doesn't meet already: what
    /// [`Editor::finish_reshape`] compares with.
    fn constraint_state(&self) -> ConstraintState {
        ConstraintState {
            all: self.doc.constraints().map(|(id, _)| id).collect(),
            unmet: drift_solver::unmet(&self.doc, UNMET),
        }
    }

    /// Commit a change to curves' extent (a trim, an extension, a split, a fillet or a
    /// chamfer). Relations it no longer meets as drawn, such as a trimmed line's equal
    /// length to another or a midpoint, come off, rather than the solve pulling the rest of
    /// the drawing round to meet them; the notice says which. A dimension or a fix it no
    /// longer meets refuses the change instead.
    fn finish_reshape(
        &mut self,
        result: Result<(), EditError>,
        label: &str,
        before: &ConstraintState,
    ) -> Result<(), EditError> {
        let mut freed: Vec<&'static str> = Vec::new();
        let result = result.and_then(|()| {
            let broken: Vec<(ConstraintId, Constraint)> = drift_solver::unmet(&self.doc, UNMET)
                .into_iter()
                .filter(|id| before.all.contains(id) && !before.unmet.contains(id))
                .filter(|id| !self.doc.is_driven(*id))
                .filter_map(|id| Some((id, *self.doc.constraint(id)?)))
                .collect();
            if let Some((_, c)) = broken
                .iter()
                .find(|(_, c)| c.is_dimension() || matches!(c, Constraint::Fix { .. }))
            {
                return Err(invalid(format!(
                    "a {} holds it as it is",
                    c.type_name().to_lowercase()
                )));
            }
            for (id, c) in broken {
                self.doc.remove_constraint(id);
                freed.push(c.type_name());
            }
            Ok(())
        });
        self.finish(result, label)?;
        if !freed.is_empty() {
            freed.sort_unstable();
            let mut names: Vec<String> = Vec::new();
            for name in &freed {
                let n = freed.iter().filter(|f| *f == name).count();
                let named = if n > 1 {
                    format!("{name} ×{n}")
                } else {
                    (*name).to_string()
                };
                if !names.contains(&named) {
                    names.push(named);
                }
            }
            self.notice = Some(format!(
                "{label} took off {} that no longer held: {}",
                if freed.len() == 1 {
                    "a relation"
                } else {
                    "relations"
                },
                names.join(", ")
            ));
        }
        Ok(())
    }

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
        // The extended arc is its full circle, whose hits are angles from +x.
        let (start, sweep) = match shape {
            Shape::Arc { start, sweep, .. } => (start, sweep),
            _ => (0.0, 1.0),
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
                let t = match shape {
                    Shape::Line { .. } => hit.t[0],
                    _ => (hit.t[0] - start).rem_euclid(TAU),
                };
                let beyond = match (shape, at_end) {
                    (Shape::Line { .. }, true) => t - 1.0,
                    (Shape::Line { .. }, false) => -t,
                    (_, true) => t - sweep,
                    (_, false) => {
                        if t > sweep {
                            TAU - t
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
        self.refuse_owned(&[id], crate::walls::Op::Cut)?;
        let (end, at, other, existing) = self.extend_target(id, click)?;
        if self.doc.users_of(end).count() > 1 {
            return Err(invalid("that end is connected to other geometry"));
        }
        let before = self.constraint_state();
        self.doc.begin();
        let result = (|| -> Result<(), EditError> {
            let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
            self.doc.set_point(end, at).map_err(doc_err)?;
            self.doc.normalize_arcs_using(&BTreeSet::from([end]));
            match existing {
                Some(q) => self.doc.merge_points(end, q).map_err(doc_err)?,
                None => {
                    self.relate(Constraint::PointOnCurve {
                        point: end,
                        curve: other,
                    });
                }
            }
            Ok(())
        })();
        self.finish_reshape(result, "Extend", &before)
    }

    /// Cut a curve in two at the point nearest `at`, keeping the pieces on the original
    /// path: lines stay collinear, arcs keep their circle, elliptical arcs their ellipse,
    /// and pen curves join smoothly. A circle or whole ellipse, which has no ends, is cut
    /// into two halves: at the point and opposite it. Returns the point at the cut.
    pub fn split_at(&mut self, id: EntityId, at: DVec2) -> Result<EntityId, EditError> {
        self.refuse_owned(&[id], crate::walls::Op::Cut)?;
        let shape = self
            .curve_shape(id)
            .ok_or_else(|| invalid("pick a curve"))?;
        let kind = self.doc.get(id).map(|e| e.kind).expect("checked");
        let t = param_of(&shape, at);
        let p = point_at(&shape, t);
        let tol = self.tol_for(&shape);
        let near_end = kind.ends().into_iter().flatten().any(|e| {
            self.doc
                .point(e)
                .is_ok_and(|q| q.distance(p) <= tol.max(1e-6))
        });
        if near_end {
            return Err(invalid("that's an end already"));
        }
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        let before = self.constraint_state();
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
                    self.relate(Constraint::Collinear { a: id, b: rest });
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
                    self.relate(Constraint::Equal { a: id, b: rest });
                }
                EntityKind::Circle { center, .. } => {
                    // Two half circles on the one centre.
                    let far = self.doc.add_point(point_at(&shape, t + TAU / 2.0));
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::Arc {
                                center,
                                start: m,
                                end: far,
                            },
                        )
                        .map_err(doc_err)?;
                    let rest = self.doc.add_arc(center, far, m).map_err(doc_err)?;
                    self.doc.copy_look(id, rest);
                    self.relate(Constraint::Equal { a: id, b: rest });
                }
                EntityKind::Ellipse {
                    center,
                    major,
                    minor,
                } => {
                    // Two halves sharing the centre and axes: one ellipse still.
                    let far = self.doc.add_point(point_at(&shape, t + TAU / 2.0));
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::EllipseArc {
                                center,
                                major,
                                minor,
                                start: m,
                                end: far,
                            },
                        )
                        .map_err(doc_err)?;
                    let rest = self
                        .doc
                        .add_ellipse_arc(center, major, minor, far, m)
                        .map_err(doc_err)?;
                    self.doc.copy_look(id, rest);
                }
                EntityKind::EllipseArc {
                    center,
                    major,
                    minor,
                    start,
                    end,
                } => {
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::EllipseArc {
                                center,
                                major,
                                minor,
                                start,
                                end: m,
                            },
                        )
                        .map_err(doc_err)?;
                    let rest = self
                        .doc
                        .add_ellipse_arc(center, major, minor, m, end)
                        .map_err(doc_err)?;
                    self.doc.copy_look(id, rest);
                }
                EntityKind::Bezier { a, c1, c2, b } => {
                    let Some(Shape::Bezier { p: whole }) = self.doc.shape(id) else {
                        return Err(invalid("nothing to split here"));
                    };
                    // Each piece keeps the handle at its original end (shortened along its
                    // own line, so a smooth join there stays smooth) and gets a new one at
                    // the cut; the two new handles are kept in line, so the path stays
                    // smooth through the cut.
                    let first = drift_core::shape::bezier_piece(&whole, 0.0, t);
                    let second = drift_core::shape::bezier_piece(&whole, t, 1.0);
                    self.doc.set_point(c1, first[1]).map_err(doc_err)?;
                    let before = self.doc.add_point(first[2]);
                    self.doc
                        .set_entity_kind(
                            id,
                            EntityKind::Bezier {
                                a,
                                c1,
                                c2: before,
                                b: m,
                            },
                        )
                        .map_err(doc_err)?;
                    let after = self.doc.add_point(second[1]);
                    self.doc.set_point(c2, second[2]).map_err(doc_err)?;
                    let rest = self.doc.add_bezier(m, after, c2, b).map_err(doc_err)?;
                    self.doc.copy_look(id, rest);
                    self.relate(Constraint::Smooth {
                        point: m,
                        a: before,
                        b: after,
                    });
                }
                EntityKind::Point(_) => unreachable!("curve_shape gave a curve"),
            }
            Ok(())
        })();
        self.finish_reshape(result, "Split", &before)?;
        Ok(point.expect("added"))
    }

    /// The curves that end at point `p`. Anything else using it (an arc's or circle's centre
    /// there, a curve's handle) doesn't make it less of a corner.
    fn curves_ending_at(&self, p: EntityId) -> Vec<EntityId> {
        self.doc
            .users_of(p)
            .filter(|id| {
                self.doc
                    .get(*id)
                    .and_then(|e| e.kind.ends())
                    .is_some_and(|ends| ends.contains(&p))
            })
            .collect()
    }

    /// The two lines meeting at a corner point (and their far ends), for fillets and
    /// chamfers.
    pub fn corner(&self, p: EntityId) -> Result<[(EntityId, EntityId); 2], EditError> {
        let ending = self.curves_ending_at(p);
        let [c1, c2] = ending[..] else {
            return Err(not_a_corner(ending.len(), "lines"));
        };
        let line = |id: EntityId| match self.doc.get(id).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) => Ok((id, if a == p { b } else { a })),
            _ => Err(invalid(
                "a chamfer joins two lines, and one here isn't a line (Fillet rounds arcs too)",
            )),
        };
        Ok([line(c1)?, line(c2)?])
    }

    /// How a line or an arc runs from its end point `end`: towards its other end.
    pub fn arm(&self, curve: EntityId, end: EntityId) -> Option<Arm> {
        let pos = |id| self.doc.point(id).ok();
        match self.doc.get(curve)?.kind {
            EntityKind::Line { a, b } if a == end || b == end => {
                let (at, far) = (pos(end)?, pos(if a == end { b } else { a })?);
                let length = at.distance(far);
                (length > 0.0).then(|| Arm {
                    run: Run::Line {
                        dir: (far - at) / length,
                    },
                    length,
                })
            }
            EntityKind::Arc {
                center,
                start,
                end: last,
            } if start != last && (start == end || last == end) => {
                let (c, s, e) = (pos(center)?, pos(start)?, pos(last)?);
                let radius = s.distance(c);
                let angle = |q: DVec2| (q.y - c.y).atan2(q.x - c.x);
                let sweep = (angle(e) - angle(s)).rem_euclid(TAU);
                (radius > 0.0 && sweep > 0.0).then_some(Arm {
                    // Stored counter-clockwise from its start.
                    run: Run::Circle {
                        center: c,
                        radius,
                        ccw: start == end,
                    },
                    length: radius * sweep,
                })
            }
            _ => None,
        }
    }

    /// The two curves (lines or arcs) meeting at a corner point, and how each runs from it,
    /// for fillets.
    pub fn fillet_corner(&self, p: EntityId) -> Result<[(EntityId, Arm); 2], EditError> {
        let ending = self.curves_ending_at(p);
        let [c1, c2] = ending[..] else {
            return Err(not_a_corner(ending.len(), "lines or arcs"));
        };
        let arm = |id: EntityId| match self.arm(id, p) {
            Some(arm) => Ok((id, arm)),
            None => Err(invalid(match self.doc.get(id).map(|e| e.kind) {
                Some(EntityKind::Line { .. } | EntityKind::Arc { .. }) => {
                    "one of the curves here has no length"
                }
                _ => "only lines and arcs can be filleted, and one here is another curve",
            })),
        };
        Ok([arm(c1)?, arm(c2)?])
    }

    /// The fillet arc at corner `p` with radius `r`, for previews.
    pub fn fillet_preview(&self, p: EntityId, r: f64) -> Option<Shape> {
        let [(_, a1), (_, a2)] = self.fillet_corner(p).ok()?;
        let (t1, t2, c, ccw) = fillet_geometry(self.doc.point(p).ok()?, [a1, a2], r).ok()?;
        Some(if ccw {
            arc_shape(c, t1, t2)
        } else {
            arc_shape(c, t2, t1)
        })
    }

    /// Round the corner at point `p` with an arc of radius `r`, tangent to both curves.
    pub fn fillet(&mut self, p: EntityId, r: f64) -> Result<EntityId, EditError> {
        self.refuse_owned(&[p], crate::walls::Op::Round)?;
        let [(l1, a1), (l2, a2)] = self.fillet_corner(p)?;
        let at = self.doc.point(p).unwrap_or_default();
        let (t1, t2, c, ccw) = fillet_geometry(at, [a1, a2], r)?;
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        let before = self.constraint_state();
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
            // The round takes the corner's look (construction stays construction).
            self.doc.copy_look(l1, a);
            arc = Some(a);
            for line in [l1, l2] {
                self.relate(Constraint::Tangent { a: line, b: a });
            }
            self.relate(Constraint::Radius { curve: a, value: r });
            self.doc.remove_if_orphaned(p);
            Ok(())
        })();
        self.finish_reshape(result, "Fillet", &before)?;
        Ok(arc.expect("added"))
    }

    /// The one line that meets curve `of` at its end point `tip` (nothing else may use the
    /// point): the line, where `tip` is, and where the line's other end is.
    fn line_at_end(&self, of: EntityId, tip: EntityId) -> Option<(EntityId, DVec2, DVec2)> {
        let mut others = self.doc.users_of(tip).filter(|u| *u != of);
        let line = others.next()?;
        if others.next().is_some() {
            return None;
        }
        let EntityKind::Line { a, b } = self.doc.get(line)?.kind else {
            return None;
        };
        let far = if a == tip { b } else { a };
        Some((line, self.doc.point(tip).ok()?, self.doc.point(far).ok()?))
    }

    /// The chamfer a line is, if it is one: a line whose ends each join one other line, the
    /// two of which would meet at a corner beyond it. `legs` are how far it cuts along each.
    pub fn chamfer_at(&self, cut: EntityId) -> Option<ChamferInfo> {
        let EntityKind::Line { a, b } = self.doc.get(cut)?.kind else {
            return None;
        };
        let ((line1, t1, far1), (line2, t2, far2)) =
            (self.line_at_end(cut, a)?, self.line_at_end(cut, b)?);
        let (l1, l2) = (far1.distance(t1), far2.distance(t2));
        if line1 == line2 || l1 <= 0.0 || l2 <= 0.0 {
            return None;
        }
        let (u1, u2) = ((far1 - t1) / l1, (far2 - t2) / l2);
        let cross = u1.perp_dot(u2);
        if cross.abs() < 1e-9 {
            return None;
        }
        // Where the two lines meet, behind both ends of the cut.
        let k = (t2 - t1).perp_dot(u2) / cross;
        let corner = t1 + u1 * k;
        let legs = [-k, (t2 - corner).dot(u2)];
        if legs[0] <= 1e-9 || legs[1] <= 1e-9 {
            return None;
        }
        Some(ChamferInfo {
            corner,
            far: [far1, far2],
            lines: [line1, line2],
            ends: [a, b],
            legs,
        })
    }

    /// The cut a chamfer would be at distances `d1` / `d2` from its corner, for previews.
    pub fn rechamfer_preview(&self, cut: EntityId, d1: f64, d2: f64) -> Option<Shape> {
        let c = self.chamfer_at(cut)?;
        let fits = |d: f64, far: DVec2| d > 0.0 && d < far.distance(c.corner);
        (fits(d1, c.far[0]) && fits(d2, c.far[1])).then(|| Shape::Line {
            a: c.corner + (c.far[0] - c.corner).normalize() * d1,
            b: c.corner + (c.far[1] - c.corner).normalize() * d2,
        })
    }

    /// Give a chamfer new distances (one undo step): its ends slide along their lines, which
    /// keep their directions.
    pub fn rechamfer(&mut self, cut: EntityId, d1: f64, d2: f64) -> Result<(), EditError> {
        let Some(c) = self.chamfer_at(cut) else {
            return Err(invalid("that line isn't a chamfer between two lines"));
        };
        let Some(Shape::Line { a, b }) = self.rechamfer_preview(cut, d1, d2) else {
            return Err(invalid(
                "the distances must be positive and shorter than the lines",
            ));
        };
        self.doc.begin();
        let _ = self.doc.set_point(c.ends[0], a);
        let _ = self.doc.set_point(c.ends[1], b);
        let dragged: Vec<Param> = c.ends.iter().flat_map(|p| Param::point(*p)).collect();
        let report = self.solve(&dragged);
        let held = [(c.ends[0], a), (c.ends[1], b)].iter().all(|(id, at)| {
            self.doc
                .point(*id)
                .is_ok_and(|p| p.distance(*at) <= 1e-6 * d1.max(d2).max(1.0))
        });
        let result = if self.new_conflicts(&report) || !held {
            Err(EditError::Conflict)
        } else {
            Ok(())
        };
        self.finish(result, "Chamfer size")
    }

    /// Put the sharp corner back where a fillet or a chamfer is: the two lines run on to
    /// where they meet, and the arc or cut goes (one undo step).
    pub fn remove_corner_cut(&mut self, cut: EntityId) -> Result<(), EditError> {
        let (corner, lines, ends, label) = if let Some(f) = self.fillet_at(cut) {
            (f.corner, f.curves, f.ends, "Remove fillet")
        } else if let Some(c) = self.chamfer_at(cut) {
            (c.corner, c.lines, c.ends, "Remove chamfer")
        } else {
            return Err(invalid(
                "that isn't a fillet or a chamfer between two lines",
            ));
        };
        self.doc.begin();
        let result = (|| -> Result<(), EditError> {
            let p = self.doc.add_point(corner);
            self.replace_end(lines[0], ends[0], p)?;
            self.replace_end(lines[1], ends[1], p)?;
            // The arc's centre goes with it; its ends are now unused.
            let centre = match self.doc.get(cut).map(|e| e.kind) {
                Some(EntityKind::Arc { center, .. }) => Some(center),
                _ => None,
            };
            // The lines meet again: fills of the corner keep their area.
            self.release_from_fills(cut);
            self.doc.delete([cut]);
            for point in ends.into_iter().chain(centre) {
                self.doc.remove_if_orphaned(point);
            }
            Ok(())
        })();
        self.selection.remove(&cut);
        self.finish(result, label)
    }

    /// The one line or arc that meets curve `of` at its end point `tip` (nothing else may use
    /// the point), and how it runs from there.
    fn curve_at_end(&self, of: EntityId, tip: EntityId) -> Option<(EntityId, Arm)> {
        let mut others = self.doc.users_of(tip).filter(|u| *u != of);
        let curve = others.next()?;
        if others.next().is_some() {
            return None;
        }
        Some((curve, self.arm(curve, tip)?))
    }

    /// The fillet an arc is, if it is one: an arc whose ends each join one line or arc,
    /// tangent to it. The corner is where those would meet, run on back past the arc.
    pub fn fillet_at(&self, arc: EntityId) -> Option<FilletInfo> {
        let EntityKind::Arc { center, start, end } = self.doc.get(arc)?.kind else {
            return None;
        };
        let c = self.doc.point(center).ok()?;
        let ((curve1, a1), (curve2, a2)) =
            (self.curve_at_end(arc, start)?, self.curve_at_end(arc, end)?);
        let (t1, t2) = (self.doc.point(start).ok()?, self.doc.point(end).ok()?);
        let radius = t1.distance(c);
        if curve1 == curve2 || radius <= 0.0 {
            return None;
        }
        // Tangent at both ends (to a tolerance that survives a solve).
        let tangent = |t: DVec2, a: &Arm| ((t - c) / radius).dot(a.dir(t)).abs() < 1e-6;
        if !tangent(t1, &a1) || !tangent(t2, &a2) {
            return None;
        }
        // The arms from a corner behind both tangent points: the same curves, further back.
        let from = |corner: DVec2, t: DVec2, a: Arm| -> Option<Arm> {
            let run = a.run;
            let back = Arm {
                run,
                length: f64::INFINITY,
            }
            .along(corner, t);
            let whole = match a.run {
                Run::Line { .. } => f64::INFINITY,
                Run::Circle { radius, .. } => TAU * radius,
            };
            (back > 1e-9 && back + a.length < whole).then_some(Arm {
                run,
                length: back + a.length,
            })
        };
        let middle = (t1 + t2) * 0.5;
        let (corner, arms) = crossings(a1.path(t1), a2.path(t2))
            .into_iter()
            .filter_map(|k| Some((k, [from(k, t1, a1)?, from(k, t2, a2)?])))
            .min_by(|(k1, _), (k2, _)| k1.distance(middle).total_cmp(&k2.distance(middle)))?;
        Some(FilletInfo {
            corner,
            arms,
            curves: [curve1, curve2],
            ends: [start, end],
            center,
            radius,
        })
    }

    /// The arc a fillet would be at radius `r`, for previews.
    pub fn refillet_preview(&self, arc: EntityId, r: f64) -> Option<Shape> {
        let f = self.fillet_at(arc)?;
        let (t1, t2, c, _) = fillet_geometry(f.corner, f.arms, r).ok()?;
        // The arc keeps its direction: its start stays on the first line.
        Some(arc_shape(c, t1, t2))
    }

    /// Resize a fillet in place, inside an open transaction: its ends slide along their
    /// lines and its centre along the bisector, so the lines keep their directions and
    /// nothing else has to move. A radius dimension on it takes the new value. False, with
    /// nothing changed, if it isn't a fillet, the radius doesn't fit, or relations hold it.
    pub fn drag_fillet(&mut self, arc: EntityId, r: f64) -> bool {
        let Some(f) = self.fillet_at(arc) else {
            return false;
        };
        let Ok((t1, t2, c, _)) = fillet_geometry(f.corner, f.arms, r) else {
            return false;
        };
        let points = [(f.ends[0], t1), (f.ends[1], t2), (f.center, c)];
        let before: Vec<(EntityId, DVec2)> = points
            .iter()
            .filter_map(|(id, _)| Some((*id, self.doc.point(*id).ok()?)))
            .collect();
        // A driving radius or diameter dimension follows.
        let sized: Vec<(drift_core::ConstraintId, Constraint)> = self
            .doc
            .constraints_on(arc)
            .filter(|id| !self.doc.is_driven(*id))
            .filter_map(|id| Some((id, *self.doc.constraint(id)?)))
            .filter(|(_, k)| matches!(k, Constraint::Radius { .. } | Constraint::Diameter { .. }))
            .collect();
        for (id, at) in points {
            let _ = self.doc.set_point(id, at);
        }
        for (id, k) in &sized {
            let value = if matches!(k, Constraint::Diameter { .. }) {
                2.0 * r
            } else {
                r
            };
            let _ = self.doc.set_constraint(*id, k.with_value(value));
        }
        let dragged: Vec<Param> = points
            .iter()
            .flat_map(|(id, _)| Param::point(*id))
            .collect();
        let report = self.solve(&dragged);
        let held = points.iter().all(|(id, at)| {
            self.doc
                .point(*id)
                .is_ok_and(|p| p.distance(*at) <= 1e-6 * r.max(1.0))
        });
        let ok = !self.new_conflicts(&report) && held;
        if !ok {
            for (id, at) in before {
                let _ = self.doc.set_point(id, at);
            }
            for (id, k) in sized {
                let _ = self.doc.set_constraint(id, k);
            }
            self.solve(&dragged);
        }
        self.index.update(&self.doc);
        ok
    }

    /// Before a radius or diameter dimension on a fillet is solved for: put the fillet where
    /// that radius has it (ends along their lines, centre on the bisector), inside the open
    /// transaction. The solve then has nothing to move, where it would otherwise be free to
    /// swing the lines. Does nothing for other curves, or a radius the lines have no room for.
    pub(crate) fn settle_fillet(&mut self, sized: &Constraint) {
        let (arc, r) = match *sized {
            Constraint::Radius { curve, value } => (curve, value),
            Constraint::Diameter { curve, value } => (curve, value / 2.0),
            _ => return,
        };
        let Some(f) = self.fillet_at(arc) else {
            return;
        };
        if let Ok((t1, t2, c, _)) = fillet_geometry(f.corner, f.arms, r) {
            for (id, at) in [(f.ends[0], t1), (f.ends[1], t2), (f.center, c)] {
                let _ = self.doc.set_point(id, at);
            }
        }
    }

    /// Give a fillet a new radius (one undo step). See [`Editor::drag_fillet`].
    pub fn refillet(&mut self, arc: EntityId, r: f64) -> Result<(), EditError> {
        let Some(f) = self.fillet_at(arc) else {
            return Err(invalid("that arc isn't a fillet between two curves"));
        };
        fillet_geometry(f.corner, f.arms, r)?;
        self.doc.begin();
        let result = if self.drag_fillet(arc, r) {
            Ok(())
        } else {
            Err(EditError::Conflict)
        };
        self.finish(result, "Fillet radius")
    }

    /// Replace a line's or an arc's end point `from` with `to`.
    fn replace_end(
        &mut self,
        line: EntityId,
        from: EntityId,
        to: EntityId,
    ) -> Result<(), EditError> {
        let kind = match self.doc.get(line).map(|e| e.kind) {
            Some(EntityKind::Line { a, b }) if a == from => EntityKind::Line { a: to, b },
            Some(EntityKind::Line { a, .. }) => EntityKind::Line { a, b: to },
            Some(EntityKind::Arc { center, start, end }) if start == from => EntityKind::Arc {
                center,
                start: to,
                end,
            },
            Some(EntityKind::Arc { center, start, .. }) => EntityKind::Arc {
                center,
                start,
                end: to,
            },
            _ => return Err(invalid("not a line or an arc")),
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
        self.refuse_owned(&[p], crate::walls::Op::Round)?;
        let [(l1, _), (l2, _)] = self.corner(p)?;
        let Some(Shape::Line { a, b }) = self.chamfer_preview(p, d1, d2) else {
            return Err(invalid(
                "the distances must be positive and shorter than the lines",
            ));
        };
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        let before = self.constraint_state();
        self.doc.begin();
        let mut line = None;
        let result = (|| -> Result<(), EditError> {
            let (p1, p2) = (self.doc.add_point(a), self.doc.add_point(b));
            self.replace_end(l1, p, p1)?;
            self.replace_end(l2, p, p2)?;
            let cut = self.doc.add_line(p1, p2).map_err(doc_err)?;
            // The cut takes the corner's look (construction stays construction).
            self.doc.copy_look(l1, cut);
            line = Some(cut);
            self.doc.remove_if_orphaned(p);
            Ok(())
        })();
        self.finish_reshape(result, "Chamfer", &before)?;
        Ok(line.expect("added"))
    }

    /// The connected chain of curves through `id` (lines, arcs, elliptical arcs and pen
    /// curves joined end to end, stopping where more than two meet), in order, with each
    /// curve's direction (true = as stored), and whether it closes.
    pub fn chain(&self, id: EntityId) -> (Vec<(EntityId, bool)>, bool) {
        let ends = |c: EntityId| {
            self.doc
                .get(c)
                .and_then(|e| e.kind.ends())
                .map(|[a, b]| (a, b))
        };
        let Some((a0, b0)) = ends(id) else {
            return (
                vec![(id, true)],
                matches!(
                    self.doc.get(id).map(|e| e.kind),
                    Some(EntityKind::Circle { .. } | EntityKind::Ellipse { .. })
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
    /// travel), in travel order, with corners rejoined. Circles and whole ellipses:
    /// positive = outward. Ellipses and pen curves, whose offsets aren't curves of their own
    /// kind, come out as runs of Béziers fitted to the true offset.
    pub fn offset_shapes(
        &self,
        chain: &[(EntityId, bool)],
        closed: bool,
        d: f64,
    ) -> Result<Vec<OffsetPiece>, EditError> {
        let mut pieces: Vec<OffsetPiece> = Vec::new();
        let fitted = |e: drift_geom::offset::OffsetError| match e {
            drift_geom::offset::OffsetError::TooTight => {
                invalid("the offset is larger than the curve's tightest bend on that side")
            }
            drift_geom::offset::OffsetError::Degenerate => invalid("the curve is too short"),
        };
        for (source, &(id, forward)) in chain.iter().enumerate() {
            let shape = self.curve_shape(id).ok_or_else(|| invalid("not a curve"))?;
            let exact = |shape| OffsetPiece {
                shape,
                source,
                forward,
                bridge: false,
            };
            // How closely fitted curves follow the true offset.
            let tol = 1e-4 * shape.bounds().size().max_element().max(d.abs()).max(1e-6);
            let mut curves = |at: &dyn Fn(f64) -> DVec2,
                              span: f64,
                              d: f64,
                              start_with: usize|
             -> Result<(), EditError> {
                let fit = drift_geom::offset::offset_curve(at, span, d, tol, start_with);
                for p in fit.map_err(fitted)? {
                    pieces.push(OffsetPiece {
                        shape: Shape::Bezier { p },
                        source,
                        // Fitted in travel order.
                        forward: true,
                        bridge: false,
                    });
                }
                Ok(())
            };
            match shape {
                Shape::Line { a, b } => {
                    let (p, q) = if forward { (a, b) } else { (b, a) };
                    let n = (q - p).perp().normalize() * d;
                    pieces.push(exact(Shape::Line { a: p + n, b: q + n }));
                }
                Shape::Circle { center, radius } => {
                    let r = radius + d;
                    if r <= 0.0 {
                        return Err(invalid("the offset is larger than the radius"));
                    }
                    pieces.push(exact(Shape::Circle { center, radius: r }));
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
                    pieces.push(exact(Shape::Arc {
                        center,
                        radius: r,
                        start,
                        sweep,
                    }));
                }
                Shape::Ellipse {
                    center,
                    major,
                    minor,
                    start,
                    sweep,
                } => {
                    let point = |a: f64| drift_core::shape::ellipse_point(center, major, minor, a);
                    if sweep >= TAU {
                        // Counter-clockwise, outward being to the right.
                        curves(&|u| point(start + u), TAU, -d, 4)?;
                    } else {
                        let quarters = (sweep / (TAU / 4.0)).ceil().max(1.0) as usize;
                        if forward {
                            curves(&|u| point(start + u), sweep, d, quarters)?;
                        } else {
                            curves(&|u| point(start + sweep - u), sweep, d, quarters)?;
                        }
                    }
                }
                Shape::Bezier { p } => {
                    if forward {
                        curves(&|u| drift_core::shape::bezier_point(&p, u), 1.0, d, 1)?;
                    } else {
                        curves(&|u| drift_core::shape::bezier_point(&p, 1.0 - u), 1.0, d, 1)?;
                    }
                }
                Shape::Point(_) => return Err(invalid("not a curve")),
            }
        }
        // Rejoin the offsets of neighbouring curves at their corners (pieces fitted to one
        // curve already run on from each other).
        let mut k = 0;
        while k < pieces.len() {
            let n = pieces.len();
            let j = if k + 1 < n {
                k + 1
            } else if closed && n > 1 {
                0
            } else {
                break;
            };
            if pieces[k].source == pieces[j].source {
                k += 1;
                continue;
            }
            let end_i = piece_end(&pieces[k].shape, pieces[k].forward, true);
            let start_j = piece_end(&pieces[j].shape, pieces[j].forward, false);
            let tol = self.tol_for(&pieces[k].shape).max(1e-9);
            if end_i.distance(start_j) <= tol * 10.0 {
                k += 1;
                continue;
            }
            let (orig, orig_forward) = chain[pieces[k].source];
            let corner = self
                .doc
                .shape(orig)
                .map(|s| piece_end(&s, orig_forward, true))
                .unwrap_or(end_i);
            let hit = intersect_unbounded(&pieces[k].shape, &pieces[j].shape)
                .into_iter()
                .min_by(|a, b| a.at.distance(corner).total_cmp(&b.at.distance(corner)));
            let curved = |s: &Shape| matches!(s, Shape::Bezier { .. });
            match hit {
                Some(h) => {
                    let (fk, fj) = (pieces[k].forward, pieces[j].forward);
                    set_piece_end(&mut pieces[k].shape, fk, true, h.at);
                    set_piece_end(&mut pieces[j].shape, fj, false, h.at);
                }
                // Fitted curves can't be extended to meet: bridge the opened corner.
                None if curved(&pieces[k].shape) || curved(&pieces[j].shape) => {
                    let bridge = OffsetPiece {
                        shape: Shape::Line {
                            a: end_i,
                            b: start_j,
                        },
                        source: pieces[k].source,
                        forward: true,
                        bridge: true,
                    };
                    pieces.insert(k + 1, bridge);
                    k += 1;
                }
                None => {}
            }
            k += 1;
        }
        // Lines whose direction flipped have been offset past their length.
        for piece in pieces.iter().filter(|p| !p.bridge) {
            let (id, forward) = chain[piece.source];
            if let (Some(Shape::Line { a, b }), Shape::Line { a: oa, b: ob }) =
                (self.doc.shape(id), piece.shape)
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
            Some((shape @ (Shape::Ellipse { .. } | Shape::Bezier { .. }), forward)) => {
                // Which side of the curve's direction at its nearest point.
                let t = param_of(&shape, side);
                let p = point_at(&shape, t);
                let along = point_at(&shape, t + 1e-6) - point_at(&shape, t - 1e-6);
                let left = along.perp_dot(side - p) >= 0.0;
                match shape {
                    // A whole ellipse: outward is positive (and to the right, anticlockwise).
                    Shape::Ellipse { sweep, .. } if sweep >= TAU => {
                        if left {
                            -1.0
                        } else {
                            1.0
                        }
                    }
                    _ if left == forward => 1.0,
                    _ => -1.0,
                }
            }
            _ => 1.0,
        }
    }

    /// Offset the chain through curve `id` by `distance` towards `side`. Lines stay parallel
    /// at the distance; arcs and circles share their centre. Ellipses and pen curves give
    /// smooth pen curves along the offset (a copy: they don't follow later changes).
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
            // A whole circle's offset is one circle on the same centre, with no ends.
            if let [piece] = pieces.as_slice()
                && let Shape::Circle { radius, .. } = piece.shape
                && let Some(EntityKind::Circle { center, .. }) =
                    self.doc.get(chain[piece.source].0).map(|x| x.kind)
            {
                let new = self.doc.add_circle(center, radius).map_err(doc_err)?;
                self.doc.copy_look(chain[piece.source].0, new);
                created.push(new);
                return Ok(());
            }
            // Points at each piece's start and end in travel order; pieces that meet share
            // one.
            let n = pieces.len();
            let same = |a: DVec2, b: DVec2| a.distance(b) <= 1e-9 * b.abs().max_element().max(1.0);
            let mut starts = Vec::with_capacity(n);
            let mut ends: Vec<(EntityId, DVec2)> = Vec::with_capacity(n);
            for (k, piece) in pieces.iter().enumerate() {
                let s = piece_end(&piece.shape, piece.forward, false);
                let e = piece_end(&piece.shape, piece.forward, true);
                let start_point = match ends.last() {
                    Some(&(prev, at)) if same(at, s) => prev,
                    _ => self.doc.add_point(s),
                };
                starts.push(start_point);
                let closes = closed
                    && k == n - 1
                    && starts[0] != start_point
                    && self.doc.point(starts[0]).is_ok_and(|q| same(q, e));
                let end_point = if closes {
                    starts[0]
                } else {
                    self.doc.add_point(e)
                };
                ends.push((end_point, e));
            }
            // The handles of the fitted curves made so far: (piece, its start handle, its
            // end handle), to keep runs of them smooth where they join.
            let mut handles: Vec<(usize, EntityId, EntityId)> = Vec::new();
            for (k, piece) in pieces.iter().enumerate() {
                let (orig, _) = chain[piece.source];
                let (s, e) = (starts[k], ends[k].0);
                let new = match (piece.shape, self.doc.get(orig).map(|x| x.kind)) {
                    (Shape::Line { .. }, _) if piece.bridge => {
                        self.doc.add_line(s, e).map_err(doc_err)?
                    }
                    (Shape::Line { .. }, _) => {
                        let line = self.doc.add_line(s, e).map_err(doc_err)?;
                        self.relate(Constraint::Parallel { a: orig, b: line });
                        self.relate(Constraint::PointLineDistance {
                            point: s,
                            line: orig,
                            value: distance,
                        });
                        line
                    }
                    (Shape::Arc { .. }, Some(EntityKind::Arc { center, .. })) => {
                        // Stored counter-clockwise: start/end follow the original's.
                        let (a, b) = if piece.forward { (s, e) } else { (e, s) };
                        self.doc.add_arc(center, a, b).map_err(doc_err)?
                    }
                    (Shape::Bezier { p }, _) => {
                        let c1 = self.doc.add_point(p[1]);
                        let c2 = self.doc.add_point(p[2]);
                        let curve = self.doc.add_bezier(s, c1, c2, e).map_err(doc_err)?;
                        // Smooth where it runs on from the piece before.
                        if let Some(&(prev, _, before)) = handles.last()
                            && prev + 1 == k
                            && ends[prev].0 == s
                        {
                            self.relate(Constraint::Smooth {
                                point: s,
                                a: before,
                                b: c1,
                            });
                        }
                        handles.push((k, c1, c2));
                        curve
                    }
                    _ => return Err(invalid("unexpected curve")),
                };
                self.doc.copy_look(orig, new);
                created.push(new);
            }
            // A closed run of fitted curves (a whole ellipse's offset) is smooth where its
            // last piece meets its first.
            if let (Some(&(0, first, _)), Some(&(last, _, before))) =
                (handles.first(), handles.last())
                && n > 1
                && last == n - 1
                && ends[last].0 == starts[0]
            {
                self.relate(Constraint::Smooth {
                    point: starts[0],
                    a: before,
                    b: first,
                });
            }
            Ok(())
        })();
        self.finish(result, "Offset")?;
        Ok(created)
    }

    /// Mirror entities about a line: copies kept symmetric to the originals (points on the
    /// line are shared).
    pub fn mirror(&mut self, ids: &[EntityId], axis: EntityId) -> Result<Vec<EntityId>, EditError> {
        self.mirror_with(ids, axis, &crate::picks::Riders::default())
            .map(|(copies, _)| copies)
    }

    /// Mirror entities, and the text and annotations picked with them, about a line: the
    /// copies of the entities, and of the text and annotations.
    #[allow(clippy::type_complexity)]
    pub fn mirror_with(
        &mut self,
        ids: &[EntityId],
        axis: EntityId,
        riders: &crate::picks::Riders,
    ) -> Result<
        (
            Vec<EntityId>,
            (Vec<drift_core::TextId>, Vec<drift_core::AnnotationId>),
        ),
        EditError,
    > {
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
        if ids.is_empty() && riders.is_empty() {
            return Err(invalid("select geometry to mirror, then the line"));
        }
        let doc_err = |e: drift_core::DocumentError| invalid(e.to_string());
        self.doc.begin();
        let mut created = Vec::new();
        let mut copied = Default::default();
        let result = (|| -> Result<(), EditError> {
            let mut map: HashMap<EntityId, EntityId> = HashMap::new();
            for p in self.doc.defining_points(ids.iter().copied()) {
                let at = self.doc.point(p).map_err(doc_err)?;
                if on_axis(at) {
                    map.insert(p, p);
                } else {
                    let q = self.doc.add_point(reflect(at));
                    map.insert(p, q);
                    self.relate(Constraint::Symmetric { a: p, b: q, axis });
                    if self.doc.users_of(p).next().is_none() {
                        created.push(q);
                    }
                }
            }
            let m = |p: EntityId| map[&p];
            // Each curve's mirror image (a line on the axis is its own).
            let mut images: HashMap<EntityId, EntityId> = HashMap::from([(axis, axis)]);
            for &id in &ids {
                let Some(entity) = self.doc.get(id).copied() else {
                    continue;
                };
                let new = match entity.kind {
                    EntityKind::Point(_) => continue,
                    EntityKind::Line { a, b } => {
                        if m(a) == a && m(b) == b {
                            images.insert(id, id);
                            continue; // lies on the axis
                        }
                        self.doc.add_line(m(a), m(b)).map_err(doc_err)?
                    }
                    EntityKind::Circle { center, radius } => {
                        let c = self.doc.add_circle(m(center), radius).map_err(doc_err)?;
                        self.relate(Constraint::Equal { a: id, b: c });
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
                    EntityKind::Bezier { a, c1, c2, b } => self
                        .doc
                        .add_bezier(m(a), m(c1), m(c2), m(b))
                        .map_err(doc_err)?,
                };
                self.doc.copy_look(id, new);
                images.insert(id, new);
                created.push(new);
            }
            self.copy_fills_onto(|e| images.get(&e).copied(), reflect);
            self.copy_walls_onto(&|e| images.get(&e).copied());
            copied = self.copy_riders(
                riders,
                &|e| images.get(&e).or_else(|| map.get(&e)).copied(),
                &reflect,
            );
            Ok(())
        })();
        self.finish(result, "Mirror")?;
        Ok((created, copied))
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
                        EntityKind::Bezier { a, c1, c2, b } => {
                            self.doc.add_bezier(m(a), m(c1), m(c2), m(b))
                        }
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
                        for (end, s) in self.doc.extension_starts(cid).into_iter().enumerate() {
                            let _ = self.doc.set_extension_start(new, end, s);
                        }
                        if let Some(e) = self.doc.expression(cid) {
                            self.doc.set_expression(new, Some(e.to_owned()));
                        }
                        if self.doc.is_driven(cid) {
                            let _ = self.doc.set_driven(new, true);
                        }
                    }
                }
                self.copy_fills_onto(|e| map.get(&e).copied(), |p| transform(k, p));
                self.copy_walls_onto(&|e| map.get(&e).copied());
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
        Shape::Bezier { p } => (p[0], p[3]),
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
        // Fitted curves run in travel order: keep the part before (or after) the point.
        Shape::Bezier { p } => {
            let t = drift_geom::nearest_on_bezier(p, to).1;
            if last && t > 1e-6 {
                *p = drift_core::shape::bezier_piece(p, 0.0, t);
                p[3] = to;
            } else if !last && t < 1.0 - 1e-6 {
                *p = drift_core::shape::bezier_piece(p, t, 1.0);
                p[0] = to;
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

    /// An empty sketch with automatic relations on, as the app starts.
    fn related() -> Editor {
        let mut ed = Editor::default();
        ed.auto_relations = true;
        ed
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-9
    }

    /// An editor with lines between the given point pairs; returns the line ids.
    fn with_lines(lines: &[(DVec2, DVec2)]) -> (Editor, Vec<EntityId>) {
        let mut ed = related();
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
    fn extend_arc_stops_at_the_nearest_crossing() {
        // An arc from 20° round to 340°, with a circle over its gap: extending either end
        // stops where the circle is first met, not at its far side (#73).
        let (mut ed, _) = with_lines(&[]);
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let s = ed
            .doc
            .add_point(DVec2::from_angle(20f64.to_radians()) * 10.0);
        let e = ed
            .doc
            .add_point(DVec2::from_angle(340f64.to_radians()) * 10.0);
        let arc = ed.doc.add_arc(c, s, e).unwrap();
        let small = ed.doc.add_point(v(12.0, 0.0));
        ed.doc.add_circle(small, 4.0).unwrap();
        ed.commit("Arcs");
        // The circles cross at x = 9.5, y = ±sqrt(100 - 9.5²).
        let y = (100.0f64 - 9.5 * 9.5).sqrt();
        ed.extend_at(arc, ed.doc.point(s).unwrap()).unwrap();
        assert!(close(ed.doc.point(s).unwrap(), v(9.5, y)));
        ed.extend_at(arc, ed.doc.point(e).unwrap()).unwrap();
        assert!(close(ed.doc.point(e).unwrap(), v(9.5, -y)));
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
        let mut ed = related();
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
    fn fillets_and_chamfers_of_construction_lines_are_construction() {
        for chamfer in [false, true] {
            let (mut ed, p) = corner();
            let lines: Vec<EntityId> = ed.doc.users_of(p).collect();
            ed.doc.begin();
            for l in &lines {
                ed.doc.set_style(*l, drift_core::LineKind::Construction);
            }
            ed.commit("Construction");
            let made = if chamfer {
                ed.chamfer(p, 3.0, 3.0).unwrap()
            } else {
                ed.fillet(p, 3.0).unwrap()
            };
            assert_eq!(
                ed.doc.get(made).unwrap().style,
                drift_core::LineKind::Construction
            );
            for l in lines {
                assert_eq!(
                    ed.doc.get(l).unwrap().style,
                    drift_core::LineKind::Construction
                );
            }
        }
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
        let mut ed = related();
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
        let mut ed = related();
        ed.doc.begin();
        let c = ed.doc.add_point(v(0.0, 0.0));
        let circle = ed.doc.add_circle(c, 5.0).unwrap();
        ed.commit("Circle");
        let before = ed.doc.iter().count();
        let out = ed.offset(circle, 2.0, v(10.0, 0.0)).unwrap();
        assert!(matches!(ed.doc.get(out[0]).map(|e| e.kind),
            Some(EntityKind::Circle { center, radius }) if center == c && (radius - 7.0).abs() < 1e-12));
        // Just the circle: no stray end points on it (#74).
        assert_eq!(ed.doc.iter().count(), before + 1);
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

    #[test]
    fn trim_cuts_lines_and_ellipses_where_they_cross() {
        let mut ed = related();
        ed.doc.begin();
        let [c, m, n] = [v(0.0, 0.0), v(20.0, 0.0), v(0.0, 10.0)].map(|p| ed.doc.add_point(p));
        let ellipse = ed.doc.add_ellipse(c, m, n).unwrap();
        let [a, b, p, q] =
            [v(-30.0, 0.0), v(30.0, 0.0), v(0.0, -20.0), v(0.0, 20.0)].map(|p| ed.doc.add_point(p));
        let across = ed.doc.add_line(a, b).unwrap();
        let upright = ed.doc.add_line(p, q).unwrap();
        ed.commit("Setup");
        // The stretch of the line between the upright and the ellipse goes: one piece ends at
        // the crossing, the other on the ellipse.
        ed.trim_at(across, v(8.0, 0.0)).unwrap();
        let mut xs: Vec<f64> = lines(&ed)
            .into_iter()
            .filter(|(a, b)| a.y.abs() < 1e-6 && b.y.abs() < 1e-6)
            .flat_map(|(a, b)| [a.x, b.x])
            .collect();
        xs.sort_by(f64::total_cmp);
        assert_eq!(xs.len(), 4);
        for (x, want) in xs.iter().zip([-30.0, 0.0, 20.0, 30.0]) {
            assert!((x - want).abs() < 1e-6, "{xs:?}");
        }
        // The upright's top, outside the ellipse, goes: it now ends on the ellipse.
        ed.trim_at(upright, v(0.0, 16.0)).unwrap();
        let (lo, hi) = ends(&ed, upright);
        assert!(
            close(lo, v(0.0, -20.0)) && hi.distance(v(0.0, 10.0)) < 1e-6,
            "{hi:?}"
        );
        // The ellipse's first quarter, between the line's end and the upright's, goes: an
        // elliptical arc the long way round.
        ed.trim_at(ellipse, v(14.142, 7.071)).unwrap();
        assert!(matches!(
            ed.doc.get(ellipse).unwrap().kind,
            EntityKind::EllipseArc { .. }
        ));
        let Some(Shape::Ellipse { start, sweep, .. }) = ed.doc.shape(ellipse) else {
            panic!("an elliptical arc")
        };
        assert!((start - TAU / 4.0).abs() < 1e-6 && (sweep - 0.75 * TAU).abs() < 1e-6);
        // And a piece out of the arc's middle (the third quarter): two arcs of one ellipse.
        ed.trim_at(ellipse, v(-14.142, -7.071)).unwrap();
        let arcs: Vec<f64> = ed
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::EllipseArc { .. }))
            .filter_map(|(id, _)| match ed.doc.shape(id) {
                Some(Shape::Ellipse { sweep, .. }) => Some(sweep),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        assert!(
            arcs.iter().all(|s| (s - TAU / 4.0).abs() < 1e-6),
            "{arcs:?}"
        );
        // Each trim is one undo step.
        for _ in 0..4 {
            ed.undo();
        }
        assert!(matches!(
            ed.doc.get(ellipse).unwrap().kind,
            EntityKind::Ellipse { .. }
        ));
        assert_eq!(lines(&ed).len(), 2);
    }

    #[test]
    fn trim_cuts_lines_and_pen_curves_where_they_cross() {
        let mut ed = related();
        ed.doc.begin();
        let [a, c1, c2, b] =
            [v(0.0, 0.0), v(0.0, 20.0), v(40.0, 20.0), v(40.0, 0.0)].map(|p| ed.doc.add_point(p));
        let arch = ed.doc.add_bezier(a, c1, c2, b).unwrap();
        let [p, q] = [v(-10.0, 10.0), v(50.0, 10.0)].map(|p| ed.doc.add_point(p));
        let across = ed.doc.add_line(p, q).unwrap();
        ed.commit("Setup");
        let original = [v(0.0, 0.0), v(0.0, 20.0), v(40.0, 20.0), v(40.0, 0.0)];
        let on_arch = |p: DVec2| drift_geom::nearest_on_bezier(&original, p).0.distance(p) < 1e-6;
        // The line's left end, outside the arch, goes: it starts on the curve.
        ed.trim_at(across, v(-5.0, 10.0)).unwrap();
        let (start, end) = ends(&ed, across);
        assert!(on_arch(start) && close(end, v(50.0, 10.0)), "{start:?}");
        // The arch's top, between the two crossings, goes: two curves, each a part of the
        // original, ending where the line crosses.
        ed.trim_at(arch, v(20.0, 15.0)).unwrap();
        let pieces: Vec<[DVec2; 4]> = ed
            .doc
            .iter()
            .filter_map(|(id, e)| match (e.kind, ed.doc.shape(id)) {
                (EntityKind::Bezier { .. }, Some(Shape::Bezier { p })) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(pieces.len(), 2);
        for piece in &pieces {
            for k in 0..=8 {
                let at = drift_core::shape::bezier_point(piece, f64::from(k) / 8.0);
                assert!(on_arch(at), "{at:?}");
            }
            // One end at the line (y = 10), the other an original end.
            let ys = [piece[0].y, piece[3].y];
            assert!(
                ys.iter().any(|y| (y - 10.0).abs() < 1e-6) && ys.iter().any(|y| y.abs() < 1e-9)
            );
        }
        // What's left of the first piece beyond... nothing cuts it now but the line at its
        // end, so trimming it removes it whole.
        ed.trim_at(arch, pieces[0][0]).unwrap();
        assert!(ed.doc.get(arch).is_none());
        ed.undo();
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc.shape(arch), Some(Shape::Bezier { p: original }));
    }

    /// The pen curves in the sketch, as control points.
    fn beziers(ed: &Editor) -> Vec<[DVec2; 4]> {
        ed.doc
            .iter()
            .filter_map(|(id, e)| match (e.kind, ed.doc.shape(id)) {
                (EntityKind::Bezier { .. }, Some(Shape::Bezier { p })) => Some(p),
                _ => None,
            })
            .collect()
    }

    /// An ellipse with semi-axes 20 and 10 about the origin.
    fn with_ellipse() -> (Editor, EntityId) {
        let mut ed = related();
        ed.doc.begin();
        let [c, m, n] = [v(0.0, 0.0), v(20.0, 0.0), v(0.0, 10.0)].map(|p| ed.doc.add_point(p));
        let ellipse = ed.doc.add_ellipse(c, m, n).unwrap();
        ed.commit("Ellipse");
        (ed, ellipse)
    }

    const ARCH: [DVec2; 4] = [
        DVec2::new(0.0, 0.0),
        DVec2::new(0.0, 20.0),
        DVec2::new(40.0, 20.0),
        DVec2::new(40.0, 0.0),
    ];

    fn with_arch() -> (Editor, EntityId) {
        let mut ed = related();
        ed.doc.begin();
        let [a, c1, c2, b] = ARCH.map(|p| ed.doc.add_point(p));
        let arch = ed.doc.add_bezier(a, c1, c2, b).unwrap();
        ed.commit("Arch");
        (ed, arch)
    }

    #[test]
    fn split_cuts_a_pen_curve_into_two_on_the_same_path() {
        let (mut ed, arch) = with_arch();
        let at = drift_core::shape::bezier_point(&ARCH, 0.3);
        let m = ed.split_at(arch, at).unwrap();
        assert!(ed.doc.point(m).unwrap().distance(at) < 1e-9);
        let pieces = beziers(&ed);
        assert_eq!(pieces.len(), 2);
        // Both lie on the original, one from each end to the cut.
        for piece in &pieces {
            for k in 0..=8 {
                let p = drift_core::shape::bezier_point(piece, f64::from(k) / 8.0);
                assert!(drift_geom::nearest_on_bezier(&ARCH, p).0.distance(p) < 1e-9);
            }
        }
        assert!(
            pieces
                .iter()
                .any(|p| close(p[0], ARCH[0]) && p[3].distance(at) < 1e-9)
        );
        assert!(
            pieces
                .iter()
                .any(|p| p[0].distance(at) < 1e-9 && close(p[3], ARCH[3]))
        );
        // Smooth through the cut, and it stays so when the cut point is dragged.
        assert_eq!(count(&ed, |c| matches!(c, Constraint::Smooth { .. })), 1);
        assert!(ed.move_point_to(m, at + v(1.0, 3.0)));
        let pieces = beziers(&ed);
        let (into, out_of) = pieces.iter().fold((DVec2::ZERO, DVec2::ZERO), |(i, o), p| {
            if p[3].distance(at + v(1.0, 3.0)) < 1e-6 {
                (p[3] - p[2], o)
            } else {
                (i, p[1] - p[0])
            }
        });
        assert!(into.normalize().perp_dot(out_of.normalize()).abs() < 1e-6);
        // An end isn't a place to split; one undo step each.
        assert!(ed.split_at(arch, ARCH[0]).is_err());
        ed.undo();
        ed.undo();
        assert_eq!(beziers(&ed), [ARCH]);
    }

    #[test]
    fn split_cuts_ellipses_and_circles() {
        // A whole ellipse: two halves, at the point and opposite it, on the same axes.
        let (mut ed, ellipse) = with_ellipse();
        let m = ed.split_at(ellipse, v(0.0, 10.0)).unwrap();
        assert!(ed.doc.point(m).unwrap().distance(v(0.0, 10.0)) < 1e-9);
        let arcs: Vec<(EntityId, f64, f64)> = ed
            .doc
            .iter()
            .filter_map(|(id, _)| match ed.doc.shape(id) {
                Some(Shape::Ellipse { start, sweep, .. }) => Some((id, start, sweep)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        assert!(
            arcs.iter()
                .all(|(_, _, sweep)| (sweep - TAU / 2.0).abs() < 1e-9)
        );
        // A half splits again where it's clicked.
        let (half, start, _) = arcs[0];
        let Some(shape) = ed.doc.shape(half) else {
            panic!()
        };
        let at = point_at(&shape, TAU / 8.0);
        ed.split_at(half, at).unwrap();
        let sweeps: Vec<f64> = ed
            .doc
            .iter()
            .filter_map(|(id, _)| match ed.doc.shape(id) {
                Some(Shape::Ellipse { sweep, .. }) => Some(sweep),
                _ => None,
            })
            .collect();
        assert_eq!(sweeps.len(), 3);
        assert!(
            (sweeps.iter().sum::<f64>() - TAU).abs() < 1e-9,
            "{sweeps:?} from {start}"
        );
        // A circle splits into two half arcs.
        let mut ed = related();
        ed.doc.begin();
        let c = ed.doc.add_point(v(5.0, 5.0));
        let circle = ed.doc.add_circle(c, 8.0).unwrap();
        ed.commit("Circle");
        ed.split_at(circle, v(13.0, 5.0)).unwrap();
        let arcs: Vec<f64> = ed
            .doc
            .iter()
            .filter_map(|(id, _)| match ed.doc.shape(id) {
                Some(Shape::Arc { radius, sweep, .. }) => {
                    assert!((radius - 8.0).abs() < 1e-9);
                    Some(sweep)
                }
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2);
        assert!(arcs.iter().all(|s| (s - TAU / 2.0).abs() < 1e-9));
    }

    /// How far `p` is from the ellipse of `with_ellipse`.
    fn off_ellipse(p: DVec2) -> f64 {
        (0..20_000)
            .map(|k| {
                let a = TAU * f64::from(k) / 20_000.0;
                v(20.0 * a.cos(), 10.0 * a.sin()).distance(p)
            })
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn offset_follows_ellipses() {
        // Outside: a closed, smooth run of pen curves 3 from the ellipse all round.
        let (mut ed, ellipse) = with_ellipse();
        let made = ed.offset(ellipse, 3.0, v(30.0, 0.0)).unwrap();
        let curves = beziers(&ed);
        assert_eq!(curves.len(), made.len());
        assert!(curves.len() >= 4);
        for c in &curves {
            for k in 0..=8 {
                let p = drift_core::shape::bezier_point(c, f64::from(k) / 8.0);
                assert!((off_ellipse(p) - 3.0).abs() < 5e-3, "{}", off_ellipse(p));
                assert!(p.length() > 10.0, "outside");
            }
        }
        // Closed: every end is another's start; smooth at every join.
        for c in &curves {
            assert_eq!(
                curves.iter().filter(|o| o[0].distance(c[3]) < 1e-9).count(),
                1
            );
        }
        assert_eq!(
            count(&ed, |c| matches!(c, Constraint::Smooth { .. })),
            curves.len()
        );
        // Inside works up to the tightest bend (10² / 20 = 5), then says why not.
        let inner = ed.offset(ellipse, 4.0, v(0.0, 0.0)).unwrap();
        let p = match ed.doc.shape(inner[0]) {
            Some(Shape::Bezier { p }) => p[0],
            _ => panic!("a pen curve"),
        };
        assert!((off_ellipse(p) - 4.0).abs() < 5e-3 && p.length() < 20.0);
        let err = ed.offset(ellipse, 6.0, v(0.0, 0.0)).unwrap_err();
        assert!(err.to_string().contains("tightest bend"), "{err}");
        // One undo step each.
        ed.undo();
        ed.undo();
        assert!(beziers(&ed).is_empty());
    }

    #[test]
    fn offset_follows_pen_curves_and_mixed_paths() {
        // A line running into an arch (a corner between them), then a line out of it.
        let mut ed = related();
        ed.doc.begin();
        let [a, c1, c2, b] = ARCH.map(|p| ed.doc.add_point(p));
        let arch = ed.doc.add_bezier(a, c1, c2, b).unwrap();
        let start = ed.doc.add_point(v(-30.0, 0.0));
        let end = ed.doc.add_point(v(70.0, 0.0));
        let lead_in = ed.doc.add_line(start, a).unwrap();
        ed.doc.add_line(b, end).unwrap();
        ed.commit("Path");
        let (chain, closed) = ed.chain(arch);
        assert_eq!(chain.len(), 3);
        assert!(!closed);
        // Above the path (outside the arch): the corners open, so they're bridged.
        let made = ed.offset(lead_in, 4.0, v(-20.0, 10.0)).unwrap();
        let on_offset = |p: DVec2| {
            let to_arch = drift_geom::nearest_on_bezier(&ARCH, p).0.distance(p);
            let to_lines = if p.x < 0.0 || p.x > 40.0 {
                p.y.abs()
            } else {
                f64::INFINITY
            };
            to_arch.min(to_lines)
        };
        let mut lines_made = 0;
        let mut curves_made = 0;
        for id in &made {
            match ed.doc.shape(*id) {
                Some(Shape::Line { a, b }) => {
                    lines_made += 1;
                    // The two offsets of the lines sit 4 above them.
                    if (a.y - 4.0).abs() < 1e-6 && (b.y - 4.0).abs() < 1e-6 {
                        assert!(a.x.min(b.x) < -20.0 || a.x.max(b.x) > 60.0);
                    }
                }
                Some(Shape::Bezier { p }) => {
                    curves_made += 1;
                    for k in 0..=8 {
                        let q = drift_core::shape::bezier_point(&p, f64::from(k) / 8.0);
                        assert!((on_offset(q) - 4.0).abs() < 5e-3, "{}", on_offset(q));
                    }
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert!(
            curves_made >= 1 && lines_made >= 2,
            "{curves_made} curves, {lines_made} lines"
        );
        // The result is one connected path: every piece's end meets the next piece's start.
        let mut ends: Vec<DVec2> = Vec::new();
        for id in &made {
            match ed.doc.shape(*id) {
                Some(Shape::Line { a, b }) => ends.extend([a, b]),
                Some(Shape::Bezier { p }) => ends.extend([p[0], p[3]]),
                _ => {}
            }
        }
        let loose = ends
            .iter()
            .filter(|p| ends.iter().filter(|q| q.distance(**p) < 1e-6).count() == 1)
            .count();
        assert_eq!(loose, 2, "only the path's two ends are free");
        // Below (inside the arch) the offsets cross and are cut where they meet.
        ed.undo();
        let made = ed.offset(lead_in, 2.0, v(-20.0, -10.0)).unwrap();
        let mut ends: Vec<DVec2> = Vec::new();
        for id in &made {
            match ed.doc.shape(*id) {
                Some(Shape::Line { a, b }) => ends.extend([a, b]),
                Some(Shape::Bezier { p }) => ends.extend([p[0], p[3]]),
                _ => {}
            }
        }
        let loose = ends
            .iter()
            .filter(|p| ends.iter().filter(|q| q.distance(**p) < 1e-6).count() == 1)
            .count();
        assert_eq!(loose, 2);
        // A lone arch offsets to curves ending square to its ends.
        let (mut ed, arch) = with_arch();
        ed.offset(arch, 5.0, v(20.0, 30.0)).unwrap();
        let curves: Vec<[DVec2; 4]> = beziers(&ed).into_iter().filter(|p| *p != ARCH).collect();
        assert!(curves.iter().any(|p| p[0].distance(v(-5.0, 0.0)) < 1e-6));
        assert!(curves.iter().any(|p| p[3].distance(v(45.0, 0.0)) < 1e-6));
    }

    #[test]
    fn nothing_is_related_when_automatic_relations_are_off() {
        // A square crossed by a line, and a corner to fillet: with relations off, trimming,
        // splitting, filleting, offsetting and mirroring leave plain geometry.
        let (mut ed, ids) = with_lines(&[
            (v(0.0, 0.0), v(20.0, 0.0)),
            (v(10.0, -10.0), v(10.0, 10.0)),
            (v(30.0, 0.0), v(30.0, 10.0)),
        ]);
        ed.auto_relations = false;
        ed.trim_at(ids[0], v(15.0, 0.0)).unwrap();
        ed.split_at(ids[1], v(10.0, 5.0)).unwrap();
        ed.offset(ids[2], 2.0, v(40.0, 5.0)).unwrap();
        ed.selection = [ids[2]].into();
        ed.mirror(&[ids[2]], ids[1]).unwrap();
        assert_eq!(ed.doc.constraint_count(), 0, "no relations at all");
        // The cut end is where the lines crossed, and free to move.
        let (a, b) = ends(&ed, ids[0]);
        assert!(close(a, v(0.0, 0.0)) && close(b, v(10.0, 0.0)));
        // Trimming the crossing line back to that corner joins the two (one shared point),
        // so a closed outline made by trimming stays closed.
        let (mut ed, ids) =
            with_lines(&[(v(0.0, 0.0), v(20.0, 0.0)), (v(10.0, -10.0), v(10.0, 10.0))]);
        ed.auto_relations = false;
        ed.trim_at(ids[0], v(15.0, 0.0)).unwrap();
        ed.trim_at(ids[1], v(10.0, -5.0)).unwrap();
        let corner = |id| match ed.doc.get(id).unwrap().kind {
            EntityKind::Line { a, b } => [a, b],
            _ => unreachable!(),
        };
        let shared = corner(ids[0])
            .iter()
            .filter(|p| corner(ids[1]).contains(p))
            .count();
        assert_eq!(shared, 1, "the trimmed ends share a point");
        assert_eq!(ed.doc.constraint_count(), 0);
    }
}
