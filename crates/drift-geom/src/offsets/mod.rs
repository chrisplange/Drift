//! Offsetting a chain of curves: the chain through a curve, the offset pieces of every
//! curve kind with their corners rejoined (lines and arcs meet where they cross, fitted
//! curves are bridged across a corner that opens), and which side of the chain a point is
//! on. `Editor::offset` turns the pieces into entities; the record and its regeneration
//! (#247) build on the same pieces.

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use drift_core::{DVec2, Document, EntityId, EntityKind, Shape, Side, tolerance};

use crate::intersect::{self, intersect_unbounded};
use crate::offset::{OffsetError, offset_curve};

pub mod regen;
#[cfg(test)]
#[allow(
    clippy::unreachable,
    reason = "a test fails loudly on a shape it didn't expect"
)]
mod tests;
pub use regen::{Report, rechain, regenerate, regenerate_some};

/// A step along a curve's parameter, to tell which way it runs at a point.
const PARAM_STEP: f64 = 1e-6;

/// How closely fitted curves follow a true offset, relative to the curve's size.
const OFFSET_FIT: f64 = 1e-4;

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

fn curve_shape(doc: &Document, id: EntityId) -> Option<Shape> {
    doc.get(id)
        .filter(|e| !e.kind.is_point())
        .and_then(|_| doc.shape(id))
}

fn tol_for(shape: &Shape) -> f64 {
    let b = shape.bounds();
    intersect::tolerance(b.min.abs().max(b.max.abs()).max_element())
}

/// The connected chain of curves through `id` (lines, arcs, elliptical arcs and pen
/// curves joined end to end, stopping where more than two meet), in order, with each
/// curve's direction (true = as stored), and whether it closes.
pub fn chain_through(doc: &Document, id: EntityId) -> (Vec<(EntityId, bool)>, bool) {
    let ends = |c: EntityId| doc.get(c).and_then(|e| e.kind.ends()).map(|[a, b]| (a, b));
    let Some((a0, b0)) = ends(id) else {
        return (
            vec![(id, true)],
            matches!(
                doc.get(id).map(|e| e.kind),
                Some(EntityKind::Circle { .. } | EntityKind::Ellipse { .. })
            ),
        );
    };
    // The other curve at `point` (with its ends) when exactly two meet there.
    let next_from = |point: EntityId, from: EntityId| -> Option<(EntityId, (EntityId, EntityId))> {
        let users: Vec<(EntityId, (EntityId, EntityId))> = doc
            .users_of(point)
            .filter_map(|u| Some((u, ends(u)?)))
            .collect();
        if users.len() == 2 {
            users.into_iter().find(|(u, _)| *u != from)
        } else {
            None
        }
    };
    // Walk backwards from the start to find the chain's first curve.
    let mut first = (id, true);
    let mut first_ends = (a0, b0);
    let mut point = a0;
    let mut seen = BTreeSet::from([id]);
    while let Some((prev, (pa, pb))) = next_from(point, first.0) {
        if seen.contains(&prev) {
            break; // closed loop
        }
        seen.insert(prev);
        // `prev` ends at `point` when traversed forward.
        let forward = pb == point;
        first = (prev, forward);
        first_ends = (pa, pb);
        point = if forward { pa } else { pb };
    }
    // Walk forward from the first curve.
    let mut out = vec![first];
    let (fa, fb) = first_ends;
    let mut point = if first.1 { fb } else { fa };
    let mut last = first.0;
    let mut closed = false;
    while let Some((next, (na, nb))) = next_from(point, last) {
        if next == first.0 {
            closed = true;
            break;
        }
        if out.iter().any(|(c, _)| *c == next) {
            break;
        }
        let forward = na == point;
        out.push((next, forward));
        last = next;
        point = if forward { nb } else { na };
    }
    (out, closed)
}

/// Offset shapes for a chain at signed distance `d` (positive = left of the direction of
/// travel), in travel order, with corners rejoined. Circles and whole ellipses:
/// positive = outward. Ellipses and pen curves, whose offsets aren't curves of their own
/// kind, come out as runs of Béziers fitted to the true offset.
pub fn pieces(
    doc: &Document,
    chain: &[(EntityId, bool)],
    closed: bool,
    d: f64,
) -> Result<Vec<OffsetPiece>, OffsetError> {
    let mut pieces: Vec<OffsetPiece> = Vec::new();
    for (source, &(id, forward)) in chain.iter().enumerate() {
        let shape = curve_shape(doc, id).ok_or(OffsetError::NotACurve)?;
        let exact = |shape| OffsetPiece {
            shape,
            source,
            forward,
            bridge: false,
        };
        // How closely fitted curves follow the true offset.
        let tol = OFFSET_FIT
            * shape
                .bounds()
                .size()
                .max_element()
                .max(d.abs())
                .max(tolerance::TINY_MM);
        let mut curves = |at: &dyn Fn(f64) -> DVec2,
                          span: f64,
                          d: f64,
                          start_with: usize|
         -> Result<(), OffsetError> {
            for p in offset_curve(at, span, d, tol, start_with)? {
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
                    return Err(OffsetError::LargerThanRadius);
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
                    return Err(OffsetError::LargerThanRadius);
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
            Shape::Point(_) => return Err(OffsetError::NotACurve),
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
        let tol = tol_for(&pieces[k].shape).max(tolerance::SAME);
        if end_i.distance(start_j) <= tol * 10.0 {
            k += 1;
            continue;
        }
        let (orig, orig_forward) = chain[pieces[k].source];
        let corner = doc
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
            (doc.shape(id), piece.shape)
        {
            let dir = if forward { b - a } else { a - b };
            if (ob - oa).dot(dir) <= 0.0 {
                return Err(OffsetError::TooLarge);
            }
        }
    }
    Ok(pieces)
}

/// The signed offset distance for a chain so the result lies on the side of `side`.
fn side_sign(doc: &Document, chain: &[(EntityId, bool)], side: DVec2) -> f64 {
    // The chain curve nearest the side point decides.
    let nearest = chain
        .iter()
        .filter_map(|(id, f)| curve_shape(doc, *id).map(|s| (s, *f)))
        .min_by(|(a, _), (b, _)| crate::distance(a, side).total_cmp(&crate::distance(b, side)));
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
            let t = crate::param_of(&shape, side);
            let p = shape.point_at(t);
            let along = shape.point_at(t + PARAM_STEP) - shape.point_at(t - PARAM_STEP);
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

/// Which side of the chain `at` lies on (see [`Side`]): the chain curve nearest it decides.
pub fn side_of(doc: &Document, chain: &[(EntityId, bool)], at: DVec2) -> Side {
    if side_sign(doc, chain, at) > 0.0 {
        Side::Left
    } else {
        Side::Right
    }
}

/// A piece's end point in travel order (`last` = where travel ends).
pub fn piece_end(shape: &Shape, forward: bool, last: bool) -> DVec2 {
    let (s, e) = match *shape {
        Shape::Line { a, b } => (a, b),
        Shape::Arc { .. } | Shape::Ellipse { .. } => {
            (shape.point_at(0.0), shape.point_at(sweep_of(shape)))
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
pub(crate) fn set_piece_end(shape: &mut Shape, forward: bool, last: bool, to: DVec2) {
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
            let t = crate::nearest_on_bezier(p, to).1;
            if last && t > PARAM_STEP {
                *p = drift_core::shape::bezier_piece(p, 0.0, t);
                p[3] = to;
            } else if !last && t < 1.0 - PARAM_STEP {
                *p = drift_core::shape::bezier_piece(p, t, 1.0);
                p[0] = to;
            }
        }
        _ => {}
    }
}
