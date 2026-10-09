//! Intersections between lines, circles and arcs.
//!
//! Every hit records where it lies along both curves (a [`Param`]), which is what trimming,
//! splitting and region building need. The rules for touching cases are explicit:
//!
//! - **Tolerance:** two things closer than [`tolerance`] (relative to the coordinates
//!   involved, at least 1e-9 mm) are the same. Hits just past a segment's or arc's end within
//!   tolerance count, snapped onto the end.
//! - **Tangency:** a line or circle that touches a circle within tolerance gives one hit,
//!   flagged `tangent`, at the exact touch point, never two nearly identical hits.
//! - **Overlap:** parallel lines and concentric circles give no hits, even when they overlap
//!   (overlapping curves don't cut each other).

use std::f64::consts::TAU;

use drift_core::{DVec2, Shape};

use crate::primitives::angle_in_sweep;

/// Where a point lies along a curve: a fraction from `a` to `b` for lines (0..1 on the
/// segment), or the counter-clockwise angle from the start for arcs and circles (radians,
/// 0..sweep on an arc; 0..2π on a circle, measured from the +x axis).
pub type Param = f64;

/// An intersection of two curves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub at: DVec2,
    /// Position along the first and second curve.
    pub t: [Param; 2],
    /// The curves touch rather than cross.
    pub tangent: bool,
}

/// The distance under which two points count as the same, for coordinates around `scale`.
pub fn tolerance(scale: f64) -> f64 {
    1e-9 * scale.abs().max(1.0)
}

fn scale_of(shape: &Shape) -> f64 {
    let b = shape.bounds();
    b.min.abs().max(b.max.abs()).max_element()
}

/// A curve in the form intersections work with.
#[derive(Clone, Copy, Debug)]
enum Curve {
    /// Segment `a`→`b`; `infinite` drops the end limits.
    Line { a: DVec2, b: DVec2, infinite: bool },
    /// Circle, optionally limited to the counter-clockwise sweep from `start`.
    Circle {
        center: DVec2,
        radius: f64,
        span: Option<(f64, f64)>,
    },
}

fn curve(shape: &Shape, extended: bool) -> Option<Curve> {
    Some(match *shape {
        Shape::Line { a, b } => Curve::Line {
            a,
            b,
            infinite: extended,
        },
        Shape::Circle { center, radius } => Curve::Circle {
            center,
            radius,
            span: None,
        },
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => Curve::Circle {
            center,
            radius,
            span: (!extended).then_some((start, sweep)),
        },
        // Ellipses don't cut or get cut yet.
        Shape::Point(_) | Shape::Ellipse { .. } => return None,
    })
}

impl Curve {
    /// The parameter of a point on this curve (assumed on it), and whether it lies within the
    /// curve's limits (with tolerance `tol`, in length units). Points just past an end are
    /// snapped to it.
    fn locate(&self, p: DVec2, tol: f64) -> Option<Param> {
        match *self {
            Curve::Line { a, b, infinite } => {
                let d = b - a;
                let len2 = d.length_squared();
                let t = (p - a).dot(d) / len2;
                if infinite {
                    return Some(t);
                }
                let slack = tol / len2.sqrt();
                if t < -slack || t > 1.0 + slack {
                    None
                } else {
                    Some(t.clamp(0.0, 1.0))
                }
            }
            Curve::Circle {
                center,
                radius,
                span,
            } => {
                let rel = p - center;
                let angle = rel.y.atan2(rel.x);
                match span {
                    None => Some(angle.rem_euclid(TAU)),
                    Some((start, sweep)) => {
                        let slack = tol / radius;
                        let t = (angle - start).rem_euclid(TAU);
                        if t <= sweep + slack {
                            Some(t.min(sweep))
                        } else if TAU - t <= slack {
                            // Just before the start.
                            Some(0.0)
                        } else {
                            None
                        }
                    }
                }
            }
        }
    }

    /// The point at a parameter (after snapping), so hits at ends are exactly the ends.
    fn point_at(&self, t: Param) -> DVec2 {
        match *self {
            Curve::Line { a, b, .. } => {
                if t == 0.0 {
                    a
                } else if t == 1.0 {
                    b
                } else {
                    a + (b - a) * t
                }
            }
            Curve::Circle {
                center,
                radius,
                span,
            } => {
                let base = span.map_or(0.0, |(start, _)| start);
                center + radius * DVec2::from_angle(base + t)
            }
        }
    }
}

/// Intersections of two curves within their extents (points give none).
pub fn intersect(a: &Shape, b: &Shape) -> Vec<Hit> {
    intersect_with(a, false, b, false)
}

/// Intersections with `a` taken as unbounded (a line extended both ways, an arc's full
/// circle) and `b` within its extent: what extending `a` to `b` needs.
pub fn intersect_extended(a: &Shape, b: &Shape) -> Vec<Hit> {
    intersect_with(a, true, b, false)
}

/// Intersections with both curves unbounded (lines extended, arcs as full circles): for
/// rejoining offset curves at corners.
pub fn intersect_unbounded(a: &Shape, b: &Shape) -> Vec<Hit> {
    intersect_with(a, true, b, true)
}

fn intersect_with(a: &Shape, a_ext: bool, b: &Shape, b_ext: bool) -> Vec<Hit> {
    let (Some(ca), Some(cb)) = (curve(a, a_ext), curve(b, b_ext)) else {
        return Vec::new();
    };
    let tol = tolerance(scale_of(a).max(scale_of(b)));
    let raw: Vec<(DVec2, bool)> = match (ca, cb) {
        (Curve::Line { a: p, b: q, .. }, Curve::Line { a: r, b: s, .. }) => line_line(p, q, r, s)
            .into_iter()
            .map(|x| (x, false))
            .collect(),
        (Curve::Line { a: p, b: q, .. }, Curve::Circle { center, radius, .. })
        | (Curve::Circle { center, radius, .. }, Curve::Line { a: p, b: q, .. }) => {
            line_circle(p, q, center, radius, tol)
        }
        (
            Curve::Circle {
                center: c1,
                radius: r1,
                ..
            },
            Curve::Circle {
                center: c2,
                radius: r2,
                ..
            },
        ) => circle_circle(c1, r1, c2, r2, tol),
    };
    let mut hits: Vec<Hit> = Vec::new();
    for (p, tangent) in raw {
        let (Some(ta), Some(tb)) = (ca.locate(p, tol), cb.locate(p, tol)) else {
            continue;
        };
        // Snap hits at an end onto the end exactly.
        let at = if matches!(
            ca,
            Curve::Line {
                infinite: false,
                ..
            }
        ) && (ta == 0.0 || ta == 1.0)
        {
            ca.point_at(ta)
        } else if matches!(
            cb,
            Curve::Line {
                infinite: false,
                ..
            }
        ) && (tb == 0.0 || tb == 1.0)
        {
            cb.point_at(tb)
        } else {
            p
        };
        if hits.iter().any(|h| h.at.distance(at) <= tol) {
            continue;
        }
        hits.push(Hit {
            at,
            t: [ta, tb],
            tangent,
        });
    }
    hits
}

/// Intersection of the infinite lines through `p`→`q` and `r`→`s`; none if parallel.
fn line_line(p: DVec2, q: DVec2, r: DVec2, s: DVec2) -> Option<DVec2> {
    let d1 = q - p;
    let d2 = s - r;
    let cross = d1.perp_dot(d2);
    if cross.abs() <= 1e-12 * d1.length() * d2.length() {
        return None;
    }
    let t = (r - p).perp_dot(d2) / cross;
    Some(p + d1 * t)
}

/// Intersections of the infinite line through `p`→`q` with a circle. A line within `tol` of
/// touching gives one tangent hit.
fn line_circle(p: DVec2, q: DVec2, center: DVec2, radius: f64, tol: f64) -> Vec<(DVec2, bool)> {
    let d = q - p;
    let len = d.length();
    if len == 0.0 {
        return Vec::new();
    }
    let u = d / len;
    // Foot of the perpendicular from the centre (differences keep precision far from the
    // origin).
    let foot = p + u * (center - p).dot(u);
    let h = foot.distance(center);
    if (h - radius).abs() <= tol {
        return vec![(foot, true)];
    }
    if h > radius {
        return Vec::new();
    }
    let half = (radius * radius - h * h).sqrt();
    vec![(foot - u * half, false), (foot + u * half, false)]
}

/// Intersections of two circles. Circles within `tol` of touching give one tangent hit;
/// concentric circles give none.
fn circle_circle(c1: DVec2, r1: f64, c2: DVec2, r2: f64, tol: f64) -> Vec<(DVec2, bool)> {
    let v = c2 - c1;
    let d = v.length();
    if d <= tol {
        return Vec::new();
    }
    let u = v / d;
    if (d - (r1 + r2)).abs() <= tol {
        return vec![(c1 + u * r1, true)];
    }
    if (d - (r1 - r2).abs()).abs() <= tol {
        // Internal tangency: the touch point is on the far side of the smaller circle.
        let dir = if r1 >= r2 { u } else { -u };
        return vec![(c1 + dir * r1, true)];
    }
    if d > r1 + r2 || d < (r1 - r2).abs() {
        return Vec::new();
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let base = c1 + u * a;
    let n = u.perp();
    vec![(base + n * h, false), (base - n * h, false)]
}

/// Whether `angle` (radians) lies on an arc's sweep: re-exported for callers that work in
/// parameters.
pub fn on_sweep(angle: f64, start: f64, sweep: f64) -> bool {
    angle_in_sweep(angle, start, sweep)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn line(a: DVec2, b: DVec2) -> Shape {
        Shape::Line { a, b }
    }

    fn circle(center: DVec2, radius: f64) -> Shape {
        Shape::Circle { center, radius }
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-12
    }

    #[test]
    fn lines_cross_within_their_extents() {
        let h = intersect(
            &line(v(0.0, 0.0), v(10.0, 0.0)),
            &line(v(4.0, -3.0), v(4.0, 3.0)),
        );
        assert_eq!(h.len(), 1);
        assert!(close(h[0].at, v(4.0, 0.0)));
        assert_eq!(h[0].t, [0.4, 0.5]);
        assert!(!h[0].tangent);
        // Not reaching: no hit, but the extended line reaches.
        let short = line(v(0.0, 0.0), v(3.0, 0.0));
        let wall = line(v(4.0, -3.0), v(4.0, 3.0));
        assert!(intersect(&short, &wall).is_empty());
        let ext = intersect_extended(&short, &wall);
        assert!((ext[0].t[0] - 4.0 / 3.0).abs() < 1e-15);
        // Parallel and overlapping: nothing.
        assert!(
            intersect(
                &line(v(0.0, 0.0), v(10.0, 0.0)),
                &line(v(5.0, 0.0), v(15.0, 0.0))
            )
            .is_empty()
        );
    }

    #[test]
    fn touching_ends_snap_exactly() {
        // A T-junction whose stem ends a hair short of the bar still meets it, exactly.
        let bar = line(v(0.0, 0.0), v(10.0, 0.0));
        let stem = line(v(5.0, 1e-11), v(5.0, 8.0));
        let h = intersect(&bar, &stem);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].t[1], 0.0);
        assert_eq!(h[0].at, v(5.0, 1e-11));
        // Shared end points.
        let h = intersect(&bar, &line(v(10.0, 0.0), v(10.0, 5.0)));
        assert_eq!((h.len(), h[0].t), (1, [1.0, 0.0]));
        assert_eq!(h[0].at, v(10.0, 0.0));
    }

    #[test]
    fn lines_and_circles() {
        let c = circle(v(0.0, 0.0), 5.0);
        let h = intersect(&line(v(-10.0, 3.0), v(10.0, 3.0)), &c);
        assert_eq!(h.len(), 2);
        assert!(close(h[0].at, v(-4.0, 3.0)) && close(h[1].at, v(4.0, 3.0)));
        // Circle parameters are angles.
        assert!((h[1].t[1] - (3.0f64).atan2(4.0)).abs() < 1e-15);
        // Tangent: one hit, flagged, even when off by round-off.
        let h = intersect(&line(v(-10.0, 5.0 + 1e-12), v(10.0, 5.0 + 1e-12)), &c);
        assert_eq!(h.len(), 1);
        assert!(h[0].tangent);
        assert!((h[0].at.x).abs() < 1e-12);
        // Missing.
        assert!(intersect(&line(v(-10.0, 6.0), v(10.0, 6.0)), &c).is_empty());
    }

    #[test]
    fn arcs_limit_hits_to_their_sweep() {
        // Upper half circle.
        let arc = Shape::Arc {
            center: v(0.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: PI,
        };
        let vertical = line(v(3.0, -10.0), v(3.0, 10.0));
        let h = intersect(&arc, &vertical);
        assert_eq!(h.len(), 1);
        assert!(close(h[0].at, v(3.0, 4.0)));
        // The full circle meets it twice.
        assert_eq!(intersect_extended(&arc, &vertical).len(), 2);
        // Hits at the arc's end angles count.
        let h = intersect(&arc, &line(v(-10.0, 0.0), v(10.0, 0.0)));
        assert_eq!(h.len(), 2);
        let mut ts: Vec<f64> = h.iter().map(|x| x.t[0]).collect();
        ts.sort_by(f64::total_cmp);
        assert!(ts[0].abs() < 1e-15 && (ts[1] - PI).abs() < 1e-15);
    }

    #[test]
    fn circles() {
        let a = circle(v(0.0, 0.0), 5.0);
        let b = circle(v(8.0, 0.0), 5.0);
        let h = intersect(&a, &b);
        assert_eq!(h.len(), 2);
        assert!(close(h[0].at, v(4.0, 3.0)) || close(h[0].at, v(4.0, -3.0)));
        // Externally and internally tangent.
        let h = intersect(&a, &circle(v(10.0, 0.0), 5.0));
        assert!(h.len() == 1 && h[0].tangent && close(h[0].at, v(5.0, 0.0)));
        let h = intersect(&a, &circle(v(0.0, 3.0), 2.0));
        assert!(h.len() == 1 && h[0].tangent && close(h[0].at, v(0.0, 5.0)));
        // Concentric, inside, apart: none.
        assert!(intersect(&a, &circle(v(0.0, 0.0), 3.0)).is_empty());
        assert!(intersect(&a, &circle(v(1.0, 0.0), 1.0)).is_empty());
        assert!(intersect(&a, &circle(v(20.0, 0.0), 1.0)).is_empty());
    }

    #[test]
    fn precise_far_from_the_origin() {
        let o = v(1e7, -3e6);
        let h = intersect(&line(o + v(-10.0, 3.0), o + v(10.0, 3.0)), &circle(o, 5.0));
        assert_eq!(h.len(), 2);
        assert!(h[1].at.distance(o + v(4.0, 3.0)) < 1e-8);
    }

    #[test]
    fn random_hits_lie_on_both_curves() {
        // A small deterministic generator (no dependency needed).
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut rnd = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64 * 20.0 - 10.0
        };
        let mut shape = |kind: u64| match kind % 3 {
            0 => line(v(rnd(), rnd()), v(rnd(), rnd())),
            1 => circle(v(rnd(), rnd()), rnd().abs() + 0.1),
            _ => Shape::Arc {
                center: v(rnd(), rnd()),
                radius: rnd().abs() + 0.1,
                start: rnd(),
                sweep: (rnd().abs() / 10.0 * TAU).max(0.1),
            },
        };
        let mut count = 0;
        for i in 0..3000u64 {
            let a = shape(i);
            let b = shape(i / 3);
            for h in intersect(&a, &b) {
                count += 1;
                for s in [&a, &b] {
                    let d = crate::primitives::distance(s, h.at);
                    assert!(d < 1e-9, "{a:?} {b:?} {h:?}: off by {d}");
                }
            }
        }
        assert!(count > 1000, "{count} hits");
    }
}
