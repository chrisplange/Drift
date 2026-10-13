//! Intersections between curves: lines, circles and arcs exactly; ellipses and Béziers
//! numerically.
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
        // Ellipses and Béziers go through `intersect_numeric`.
        Shape::Point(_) | Shape::Ellipse { .. } | Shape::Bezier { .. } => return None,
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
    let free = |s: &Shape| matches!(s, Shape::Ellipse { .. } | Shape::Bezier { .. });
    if free(a) || free(b) {
        return intersect_numeric(a, a_ext, b, b_ext);
    }
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

// --- Ellipses and Béziers ---------------------------------------------------------------
//
// These have no closed-form intersections worth the trouble, so any pair involving one is
// solved numerically: both curves are sampled into chords, chords that cross (or come
// within their sag of each other, for tangent touches) give starting guesses, and each
// guess is polished by Newton's method on the two parameters.

/// A curve as a function of its parameter over `lo..=hi` (the same parameters as
/// [`Param`]; an ellipse's is its angle from its start).
#[derive(Clone, Copy)]
struct Para {
    shape: Shape,
    lo: f64,
    hi: f64,
    /// A full circle or ellipse: the parameter wraps.
    closed: bool,
}

impl Para {
    /// `extended` lengthens a line to span `reach` and takes an arc's whole circle.
    fn new(shape: &Shape, extended: bool, reach: &drift_core::Bounds) -> Option<Self> {
        let (shape, lo, hi, closed) = match *shape {
            Shape::Point(_) => return None,
            Shape::Line { a, b } => {
                let d = b - a;
                let len2 = d.length_squared();
                if len2 == 0.0 {
                    return None;
                }
                let (mut lo, mut hi) = (0.0f64, 1.0f64);
                if extended && !reach.is_empty() {
                    for c in [
                        reach.min,
                        reach.max,
                        DVec2::new(reach.min.x, reach.max.y),
                        DVec2::new(reach.max.x, reach.min.y),
                    ] {
                        let t = (c - a).dot(d) / len2;
                        lo = lo.min(t - 0.01);
                        hi = hi.max(t + 0.01);
                    }
                }
                (*shape, lo, hi, false)
            }
            Shape::Circle { .. } => (*shape, 0.0, TAU, true),
            Shape::Arc { center, radius, .. } if extended => {
                (Shape::Circle { center, radius }, 0.0, TAU, true)
            }
            Shape::Arc { sweep, .. } => (*shape, 0.0, sweep, false),
            Shape::Ellipse { sweep, .. } if sweep >= TAU => (*shape, 0.0, TAU, true),
            Shape::Ellipse { sweep, .. } => (*shape, 0.0, sweep, false),
            Shape::Bezier { .. } => (*shape, 0.0, 1.0, false),
        };
        Some(Self {
            shape,
            lo,
            hi,
            closed,
        })
    }

    fn at(&self, t: f64) -> DVec2 {
        match self.shape {
            Shape::Line { a, b } => {
                if t == 0.0 {
                    a
                } else if t == 1.0 {
                    b
                } else {
                    a + (b - a) * t
                }
            }
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

    /// The derivative with respect to the parameter (by central differences).
    fn tangent(&self, t: f64) -> DVec2 {
        let h = 1e-6 * (self.hi - self.lo);
        (self.at(t + h) - self.at(t - h)) / (2.0 * h)
    }

    /// Keep a parameter on the curve: wrapped round a closed one, clamped to an open one.
    fn fit(&self, t: f64) -> f64 {
        if self.closed {
            t.rem_euclid(TAU)
        } else {
            t.clamp(self.lo, self.hi)
        }
    }

    /// How many chords to sample it into.
    fn chords(&self) -> usize {
        let span = self.hi - self.lo;
        match self.shape {
            Shape::Line { .. } | Shape::Point(_) => 1,
            Shape::Circle { .. } | Shape::Arc { .. } => {
                ((span / TAU) * 96.0).ceil().max(6.0) as usize
            }
            Shape::Ellipse { .. } => ((span / TAU) * 128.0).ceil().max(8.0) as usize,
            Shape::Bezier { .. } => 48,
        }
    }

    /// The curve as chords: (start parameter, end parameter, start, end, sag).
    fn sample(&self) -> Vec<(f64, f64, DVec2, DVec2, f64)> {
        let n = self.chords();
        let step = (self.hi - self.lo) / n as f64;
        let mut out = Vec::with_capacity(n);
        let mut t0 = self.lo;
        let mut p0 = self.at(t0);
        for k in 1..=n {
            let t1 = if k == n {
                self.hi
            } else {
                self.lo + step * k as f64
            };
            let p1 = self.at(t1);
            // How far the curve bows from the chord (zero for a line).
            let sag = self.at((t0 + t1) * 0.5).distance((p0 + p1) * 0.5);
            out.push((t0, t1, p0, p1, sag));
            (t0, p0) = (t1, p1);
        }
        out
    }
}

/// The closest approach of segments `p0`→`p1` and `q0`→`q1`: fractions along each and the
/// distance between those points.
fn segment_approach(p0: DVec2, p1: DVec2, q0: DVec2, q1: DVec2) -> (f64, f64, f64) {
    let (d1, d2, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (d1.length_squared(), d2.length_squared(), d2.dot(r));
    let (s, u);
    if a <= f64::EPSILON && e <= f64::EPSILON {
        (s, u) = (0.0, 0.0);
    } else if a <= f64::EPSILON {
        (s, u) = (0.0, (f / e).clamp(0.0, 1.0));
    } else {
        let c = d1.dot(r);
        if e <= f64::EPSILON {
            (s, u) = ((-c / a).clamp(0.0, 1.0), 0.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s0 = if denom > f64::EPSILON * a * e {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut u0 = (b * s0 + f) / e;
            if u0 < 0.0 {
                u0 = 0.0;
                s0 = (-c / a).clamp(0.0, 1.0);
            } else if u0 > 1.0 {
                u0 = 1.0;
                s0 = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, u) = (s0, u0);
        }
    }
    let gap = (p0 + d1 * s).distance(q0 + d2 * u);
    (s, u, gap)
}

/// Intersections where either curve is an ellipse or a Bézier.
fn intersect_numeric(a: &Shape, a_ext: bool, b: &Shape, b_ext: bool) -> Vec<Hit> {
    let (Some(ca), Some(cb)) = (
        Para::new(a, a_ext, &b.bounds()),
        Para::new(b, b_ext, &a.bounds()),
    ) else {
        return Vec::new();
    };
    let scale = scale_of(a).max(scale_of(b));
    let tol = tolerance(scale);
    // Roots found from different starting guesses agree to well within this.
    let same = 1e-7 * scale.max(1.0);
    let (sa, sb) = (ca.sample(), cb.sample());
    let mut hits: Vec<Hit> = Vec::new();
    for &(t0, t1, p0, p1, sag_a) in &sa {
        let (lo, hi) = (p0.min(p1), p0.max(p1));
        for &(u0, u1, q0, q1, sag_b) in &sb {
            let reach = sag_a + sag_b + tol;
            let (qlo, qhi) = (q0.min(q1), q0.max(q1));
            if qlo.x > hi.x + reach
                || qhi.x < lo.x - reach
                || qlo.y > hi.y + reach
                || qhi.y < lo.y - reach
            {
                continue;
            }
            let (s, u, gap) = segment_approach(p0, p1, q0, q1);
            if gap > reach {
                continue;
            }
            let Some((t, v, tangent)) =
                polish(&ca, &cb, t0 + (t1 - t0) * s, u0 + (u1 - u0) * u, tol)
            else {
                continue;
            };
            let at = ca.at(t);
            if hits.iter().any(|h| h.at.distance(at) <= same) {
                continue;
            }
            hits.push(Hit {
                at,
                t: [t, v],
                tangent,
            });
            // More crossings than two such curves can have: they run along each other, and
            // overlapping curves don't cut each other.
            if hits.len() > 9 {
                return Vec::new();
            }
        }
    }
    hits
}

/// Newton's method on both parameters from a starting guess. Returns the parameters where
/// the curves meet (within `tol`) and whether they touch rather than cross.
fn polish(a: &Para, b: &Para, mut t: f64, mut u: f64, tol: f64) -> Option<(f64, f64, bool)> {
    for _ in 0..60 {
        let f = a.at(t) - b.at(u);
        if f.length() <= tol * 0.01 {
            break;
        }
        let (da, db) = (a.tangent(t), b.tangent(u));
        // Least squares with a little damping, so a tangent touch (where the two
        // directions agree) still makes progress.
        let (m00, m01, m11) = (da.dot(da), -da.dot(db), db.dot(db));
        let damp = 1e-12 * (m00 + m11);
        let det = (m00 + damp) * (m11 + damp) - m01 * m01;
        if det.abs() < f64::MIN_POSITIVE {
            break;
        }
        let (g0, g1) = (-da.dot(f), db.dot(f));
        let dt = ((m11 + damp) * g0 - m01 * g1) / det;
        let du = ((m00 + damp) * g1 - m01 * g0) / det;
        let (nt, nu) = (a.fit(t + dt), b.fit(u + du));
        if nt == t && nu == u {
            break;
        }
        (t, u) = (nt, nu);
    }
    if a.at(t).distance(b.at(u)) > tol.max(1e-9) * 10.0 {
        return None;
    }
    let (da, db) = (a.tangent(t), b.tangent(u));
    let cross = da.perp_dot(db).abs();
    let tangent = cross <= 1e-6 * da.length() * db.length();
    Some((t, u, tangent))
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

/// Whether two curves lie along each other for some length: lines on one line that share a
/// stretch, or arcs and circles of one circle whose sweeps share a stretch. Curves that only
/// meet at a point (end to end, or crossing) don't overlap. Ellipses and Béziers aren't
/// checked.
pub fn overlapping(a: &Shape, b: &Shape) -> bool {
    let tol = tolerance(scale_of(a).max(scale_of(b))) * 100.0;
    let span = |s: &Shape| match *s {
        Shape::Circle { center, radius } => Some((center, radius, 0.0, TAU)),
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => Some((center, radius, start, sweep)),
        _ => None,
    };
    match (a, b) {
        (Shape::Line { a: p, b: q }, Shape::Line { a: r, b: s }) => {
            let d = *q - *p;
            let len = d.length();
            if len <= tol {
                return false;
            }
            let u = d / len;
            // Both ends of the other line on this one's line...
            if u.perp_dot(*r - *p).abs() > tol || u.perp_dot(*s - *p).abs() > tol {
                return false;
            }
            // ...and a stretch in common.
            let (t0, t1) = ((*r - *p).dot(u), (*s - *p).dot(u));
            t0.max(t1).min(len) - t0.min(t1).max(0.0) > tol
        }
        _ => {
            let (Some((c1, r1, s1, w1)), Some((c2, r2, s2, w2))) = (span(a), span(b)) else {
                return false;
            };
            if c1.distance(c2) > tol || (r1 - r2).abs() > tol {
                return false;
            }
            if w1 >= TAU || w2 >= TAU {
                return true;
            }
            // The second arc's start, measured round from the first's.
            let from = (s2 - s1).rem_euclid(TAU);
            let slack = tol / r1;
            // It begins inside the first, or wraps round to cover the first's start.
            from < w1 - slack || from + w2 > TAU + slack
        }
    }
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

    fn ellipse(center: DVec2, major: DVec2, minor: f64) -> Shape {
        Shape::Ellipse {
            center,
            major,
            minor,
            start: 0.0,
            sweep: TAU,
        }
    }

    fn near(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-7
    }

    #[test]
    fn lines_cut_ellipses() {
        let e = ellipse(v(0.0, 0.0), v(10.0, 0.0), 5.0);
        // Along the major axis: both ends of it, with the ellipse's angle as its parameter.
        let mut h = intersect(&line(v(-20.0, 0.0), v(20.0, 0.0)), &e);
        h.sort_by(|a, b| a.at.x.total_cmp(&b.at.x));
        assert_eq!(h.len(), 2);
        assert!(near(h[0].at, v(-10.0, 0.0)) && near(h[1].at, v(10.0, 0.0)));
        assert!((h[0].t[0] - 0.25).abs() < 1e-7 && (h[1].t[0] - 0.75).abs() < 1e-7);
        assert!((h[0].t[1] - PI).abs() < 1e-7);
        assert!(h[1].t[1].abs() < 1e-7 || (h[1].t[1] - TAU).abs() < 1e-7);
        // A slanted chord of a turned ellipse: both hits lie on both curves.
        let turned = ellipse(v(3.0, -2.0), v(8.0, 6.0), 4.0);
        let chord = line(v(-12.0, -9.0), v(15.0, 8.0));
        let h = intersect(&chord, &turned);
        assert_eq!(h.len(), 2);
        for hit in &h {
            let Shape::Ellipse {
                center,
                major,
                minor,
                ..
            } = turned
            else {
                unreachable!()
            };
            let on = drift_core::shape::ellipse_point(center, major, minor, hit.t[1]);
            assert!(near(on, hit.at), "{hit:?}");
            assert!(near(v(-12.0, -9.0) + v(27.0, 17.0) * hit.t[0], hit.at));
            assert!(!hit.tangent);
        }
        // Touching the top: one tangent hit. Missing it: none. A segment that stops short: none.
        let h = intersect(&line(v(-20.0, 5.0), v(20.0, 5.0)), &e);
        assert_eq!(h.len(), 1);
        assert!(
            h[0].tangent && h[0].at.distance(v(0.0, 5.0)) < 1e-4,
            "{h:?}"
        );
        assert!(intersect(&line(v(-20.0, 5.5), v(20.0, 5.5)), &e).is_empty());
        assert!(intersect(&line(v(-20.0, 0.0), v(-12.0, 0.0)), &e).is_empty());
        // Extended, that segment reaches both sides.
        let h = intersect_extended(&line(v(-20.0, 0.0), v(-12.0, 0.0)), &e);
        assert_eq!(h.len(), 2);
        assert!(h.iter().all(|x| x.t[0] > 1.0));
    }

    #[test]
    fn curves_cut_and_are_cut() {
        let arch = Shape::Bezier {
            p: [v(0.0, 0.0), v(0.0, 20.0), v(40.0, 20.0), v(40.0, 0.0)],
        };
        // A line across the arch at y = 10: 60·t·(1 − t) = 10.
        let mut h = intersect(&arch, &line(v(-10.0, 10.0), v(50.0, 10.0)));
        h.sort_by(|a, b| a.t[0].total_cmp(&b.t[0]));
        assert_eq!(h.len(), 2);
        let root = 0.5 - (0.25f64 - 1.0 / 6.0).sqrt();
        assert!((h[0].t[0] - root).abs() < 1e-7 && (h[1].t[0] - (1.0 - root)).abs() < 1e-7);
        assert!(h.iter().all(|x| (x.at.y - 10.0).abs() < 1e-7));
        // A circle through it, an ellipse through a circle (four places), two curves.
        assert_eq!(intersect(&arch, &circle(v(20.0, 0.0), 16.0)).len(), 2);
        let e = ellipse(v(0.0, 0.0), v(10.0, 0.0), 5.0);
        assert_eq!(intersect(&e, &circle(v(0.0, 0.0), 7.0)).len(), 4);
        let dip = Shape::Bezier {
            p: [v(0.0, 12.0), v(10.0, -8.0), v(30.0, -8.0), v(40.0, 12.0)],
        };
        let h = intersect(&arch, &dip);
        assert_eq!(h.len(), 2);
        for hit in &h {
            let on = drift_core::shape::bezier_point(
                &[v(0.0, 12.0), v(10.0, -8.0), v(30.0, -8.0), v(40.0, 12.0)],
                hit.t[1],
            );
            assert!(near(on, hit.at));
        }
        // Curves meeting end to end touch at that end only.
        let next = Shape::Bezier {
            p: [v(40.0, 0.0), v(40.0, -20.0), v(80.0, -20.0), v(80.0, 0.0)],
        };
        let h = intersect(&arch, &next);
        assert_eq!(h.len(), 1);
        assert!(near(h[0].at, v(40.0, 0.0)));
        // The same curve twice: overlapping curves don't cut each other.
        assert!(intersect(&arch, &arch).is_empty());
        assert!(intersect(&e, &e).is_empty());
    }

    #[test]
    fn overlapping_curves_share_a_stretch() {
        let wall = line(v(10.0, 0.0), v(10.0, 10.0));
        // The same line, a longer one along it, and one that only pokes into it.
        assert!(overlapping(&wall, &wall));
        assert!(overlapping(&wall, &line(v(10.0, 20.0), v(10.0, -5.0))));
        assert!(overlapping(&wall, &line(v(10.0, 8.0), v(10.0, 30.0))));
        // End to end, parallel beside it, and crossing: not overlapping.
        assert!(!overlapping(&wall, &line(v(10.0, 10.0), v(10.0, 30.0))));
        assert!(!overlapping(&wall, &line(v(10.5, 0.0), v(10.5, 10.0))));
        assert!(!overlapping(&wall, &line(v(0.0, 5.0), v(20.0, 5.0))));
        // Arcs of one circle.
        let arc = |start: f64, sweep: f64| Shape::Arc {
            center: v(0.0, 0.0),
            radius: 5.0,
            start,
            sweep,
        };
        assert!(overlapping(&arc(0.0, PI), &arc(PI / 2.0, PI)));
        assert!(
            overlapping(&arc(0.0, PI), &arc(1.5 * PI, PI)),
            "wraps round"
        );
        assert!(
            !overlapping(&arc(0.0, PI), &arc(PI, PI / 2.0)),
            "end to end"
        );
        assert!(overlapping(&circle(v(0.0, 0.0), 5.0), &arc(1.0, 0.5)));
        assert!(!overlapping(
            &circle(v(0.0, 0.0), 5.0),
            &circle(v(0.0, 0.0), 6.0)
        ));
    }
}
