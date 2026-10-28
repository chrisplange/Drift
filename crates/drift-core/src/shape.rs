use std::f64::consts::{FRAC_PI_2, PI, TAU};

use glam::DVec2;

use crate::Bounds;

/// How an entity is drawn. Construction and centre lines are excluded from profiles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineKind {
    #[default]
    Solid,
    Construction,
    Center,
}

/// A resolved geometric shape in world space, derived from document entities for rendering,
/// picking and snapping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Point(DVec2),
    Line {
        a: DVec2,
        b: DVec2,
    },
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// Counter-clockwise arc starting at angle `start` (radians) and sweeping `sweep` radians.
    Arc {
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// An ellipse, or the counter-clockwise part of one: the points
    /// `center + major·cos t + minor·perp(major̂)·sin t` for `t` from `start` over `sweep`
    /// (parametric angles; a whole ellipse sweeps `TAU`).
    Ellipse {
        center: DVec2,
        /// The first semi-axis, as a vector from the centre.
        major: DVec2,
        /// The length of the other semi-axis (at 90° counter-clockwise from `major`).
        minor: f64,
        start: f64,
        sweep: f64,
    },
    /// A cubic Bézier: from `p[0]` to `p[3]`, pulled towards the handles `p[1]` and `p[2]`.
    Bezier {
        p: [DVec2; 4],
    },
}

/// A point of a cubic Bézier at `t` in 0..1.
pub fn bezier_point(p: &[DVec2; 4], t: f64) -> DVec2 {
    let u = 1.0 - t;
    p[0] * (u * u * u) + p[1] * (3.0 * u * u * t) + p[2] * (3.0 * u * t * t) + p[3] * (t * t * t)
}

/// The first derivative of a cubic Bézier at `t`.
pub fn bezier_tangent(p: &[DVec2; 4], t: f64) -> DVec2 {
    let u = 1.0 - t;
    (p[1] - p[0]) * (3.0 * u * u) + (p[2] - p[1]) * (6.0 * u * t) + (p[3] - p[2]) * (3.0 * t * t)
}

/// Which control point sets a cubic Bézier's direction at its start (`at_end` false) or end:
/// that end's handle, or where the handle has no length (to within rounding), the next
/// control point along. The curve runs from it towards the end.
pub fn bezier_end_handle(p: &[DVec2; 4], at_end: bool) -> usize {
    let (anchor, order) = if at_end {
        (p[3], [2, 1, 0])
    } else {
        (p[0], [1, 2, 3])
    };
    let scale = p.iter().fold(1.0f64, |m, q| m.max(q.abs().max_element()));
    order
        .into_iter()
        .find(|&i| p[i].distance(anchor) > 1e-9 * scale)
        .unwrap_or(order[0])
}

/// The second derivative of a cubic Bézier at `t`.
pub fn bezier_second(p: &[DVec2; 4], t: f64) -> DVec2 {
    (p[2] - p[1] * 2.0 + p[0]) * (6.0 * (1.0 - t)) + (p[3] - p[2] * 2.0 + p[1]) * (6.0 * t)
}

/// Signed curvature (1 / radius, positive turning left) of a cubic Bézier at `t`.
pub fn bezier_curvature(p: &[DVec2; 4], t: f64) -> f64 {
    let d1 = bezier_tangent(p, t);
    let d2 = bezier_second(p, t);
    let len = d1.length();
    if len < 1e-12 {
        return 0.0;
    }
    d1.perp_dot(d2) / (len * len * len)
}

/// The most segments one curve is flattened into, however fine the tolerance.
pub const MAX_FLATTEN_SEGMENTS: usize = 4096;

/// The largest turn one segment of a flattened arc or ellipse takes (radians), so a curve
/// too small for the tolerance to matter still comes out round: 16 to a whole turn.
const MAX_FLATTEN_STEP: f64 = PI / 8.0;

fn clamp_segments(n: f64) -> usize {
    if n.is_nan() {
        1
    } else {
        n.ceil().clamp(1.0, MAX_FLATTEN_SEGMENTS as f64) as usize
    }
}

/// How many segments keep an arc of `radius` over `sweep` within `tolerance` of its chords
/// (the chord error `r·(1 − cos(step/2))`).
pub fn arc_segments(radius: f64, sweep: f64, tolerance: f64) -> usize {
    let r = radius.abs();
    let step = if r > tolerance {
        2.0 * (1.0 - tolerance / r).acos()
    } else {
        MAX_FLATTEN_STEP
    };
    // A tolerance of zero or less, or none at all, flattens as finely as allowed.
    let step = if step > 0.0 {
        step.min(MAX_FLATTEN_STEP)
    } else {
        0.0
    };
    clamp_segments(sweep.abs() / step)
}

/// How many segments keep an ellipse (or part) within `tolerance` of its chords: as an arc
/// of its larger semi-axis, which bounds the chord error of an even step in its angle.
pub fn ellipse_segments(major: DVec2, minor: f64, sweep: f64, tolerance: f64) -> usize {
    arc_segments(major.length().max(minor.abs()), sweep, tolerance)
}

/// How many even steps in `t` keep a cubic Bézier within `tolerance` of its chords (Wang's
/// bound: `√(¾·M / tolerance)`, `M` the larger second difference of its control points).
pub fn bezier_segments(p: &[DVec2; 4], tolerance: f64) -> usize {
    let m = (p[0] - 2.0 * p[1] + p[2])
        .length()
        .max((p[1] - 2.0 * p[2] + p[3]).length());
    if m == 0.0 {
        return 1;
    }
    clamp_segments((0.75 * m / tolerance.max(0.0)).sqrt())
}

/// The part of a cubic Bézier between `t0` and `t1` (de Casteljau).
pub fn bezier_piece(p: &[DVec2; 4], t0: f64, t1: f64) -> [DVec2; 4] {
    // Split at t1 and keep the first part, then split that at t0 / t1.
    let split = |q: &[DVec2; 4], t: f64| -> ([DVec2; 4], [DVec2; 4]) {
        let lerp = |a: DVec2, b: DVec2| a + (b - a) * t;
        let (a, b, c) = (lerp(q[0], q[1]), lerp(q[1], q[2]), lerp(q[2], q[3]));
        let (d, e) = (lerp(a, b), lerp(b, c));
        let f = lerp(d, e);
        ([q[0], a, d, f], [f, e, c, q[3]])
    };
    let (head, _) = split(p, t1.clamp(0.0, 1.0));
    if t1 <= 1e-12 {
        return [p[0]; 4];
    }
    let (_, tail) = split(&head, (t0 / t1).clamp(0.0, 1.0));
    tail
}

fn bezier_bounds(p: &[DVec2; 4]) -> Bounds {
    let mut b = Bounds::from_point(p[0]);
    b.include(p[3]);
    // Where x' or y' is zero: a quadratic in t for each axis.
    for axis in 0..2 {
        let c = |v: DVec2| if axis == 0 { v.x } else { v.y };
        let (a0, a1, a2, a3) = (c(p[0]), c(p[1]), c(p[2]), c(p[3]));
        let qa = -a0 + 3.0 * a1 - 3.0 * a2 + a3;
        let qb = 2.0 * (a0 - 2.0 * a1 + a2);
        let qc = a1 - a0;
        let mut roots = Vec::new();
        if qa.abs() < 1e-12 {
            if qb.abs() > 1e-12 {
                roots.push(-qc / qb);
            }
        } else {
            let disc = qb * qb - 4.0 * qa * qc;
            if disc >= 0.0 {
                let r = disc.sqrt();
                roots.push((-qb + r) / (2.0 * qa));
                roots.push((-qb - r) / (2.0 * qa));
            }
        }
        for t in roots {
            if (0.0..=1.0).contains(&t) {
                b.include(bezier_point(p, t));
            }
        }
    }
    b
}

/// A point of an ellipse at parametric angle `t`.
pub fn ellipse_point(center: DVec2, major: DVec2, minor: f64, t: f64) -> DVec2 {
    let n = major.perp().normalize_or_zero() * minor;
    center + major * t.cos() + n * t.sin()
}

/// Cubic Béziers along an ellipse (or part of one) from parametric angle `start` over
/// `sweep`, each spanning at most `max_step` radians. A circle is the ellipse with `major`
/// of length `minor` along +x.
pub fn ellipse_cubics(
    center: DVec2,
    major: DVec2,
    minor: f64,
    start: f64,
    sweep: f64,
    max_step: f64,
) -> Vec<[DVec2; 4]> {
    let pieces = ((sweep.abs() / max_step).ceil() as usize).max(1);
    let step = sweep / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let n = major.perp().normalize_or_zero() * minor;
    let at = |t: f64| center + major * t.cos() + n * t.sin();
    let tangent = |t: f64| n * t.cos() - major * t.sin();
    (0..pieces)
        .map(|i| {
            let t0 = start + step * i as f64;
            let t1 = t0 + step;
            [
                at(t0),
                at(t0) + tangent(t0) * k,
                at(t1) - tangent(t1) * k,
                at(t1),
            ]
        })
        .collect()
}

/// Cubic Béziers along a circular arc, each spanning at most `max_step` radians.
pub fn arc_cubics(
    center: DVec2,
    radius: f64,
    start: f64,
    sweep: f64,
    max_step: f64,
) -> Vec<[DVec2; 4]> {
    ellipse_cubics(center, DVec2::X * radius, radius, start, sweep, max_step)
}

/// The points an ellipse entity is made of: its centre, the ends of its two semi-axes, and
/// (for part of one) its start and end.
pub fn ellipse_handles(
    center: DVec2,
    major: DVec2,
    minor: f64,
    start: f64,
    sweep: f64,
) -> ([DVec2; 3], Option<[DVec2; 2]>) {
    let axes = [
        center,
        center + major,
        center + major.perp().normalize_or_zero() * minor,
    ];
    let ends = (sweep < TAU - 1e-9).then(|| {
        [
            ellipse_point(center, major, minor, start),
            ellipse_point(center, major, minor, start + sweep),
        ]
    });
    (axes, ends)
}

/// The parametric angle of the point of an ellipse in the direction of `p` from its centre.
pub fn ellipse_angle(center: DVec2, major: DVec2, minor: f64, p: DVec2) -> f64 {
    let a = major.length();
    let u = major.normalize_or_zero();
    let d = p - center;
    let (x, y) = (d.dot(u), d.dot(u.perp()));
    (y / minor.max(1e-300)).atan2(x / a.max(1e-300))
}

/// The circle through three points: centre and radius, or `None` when they are (nearly)
/// collinear. Worked relative to `a`, so it holds far from the origin.
pub fn circle_through(a: DVec2, b: DVec2, c: DVec2) -> Option<(DVec2, f64)> {
    let (u, v) = (b - a, c - a);
    let d = 2.0 * u.perp_dot(v);
    if d == 0.0 || d.abs() <= 1e-12 * u.length() * v.length() {
        return None;
    }
    let (uu, vv) = (u.length_squared(), v.length_squared());
    let center = a + DVec2::new(v.y * uu - u.y * vv, u.x * vv - v.x * uu) / d;
    Some((center, center.distance(a)))
}

impl Shape {
    /// The shape as connected points, each chord within `tolerance` of the curve (see
    /// [`crate::tolerance`] for the named ones): a line's two ends, a point alone, a circle
    /// closed (its last point is its first). A Bézier ends exactly on its end points.
    pub fn flatten(&self, tolerance: f64) -> Vec<DVec2> {
        let mut out = Vec::new();
        self.flatten_into(tolerance, &mut out);
        out
    }

    /// The length of the curve: exact for lines, circles and arcs, from chords within
    /// [`MEASURE_MM`](crate::tolerance::MEASURE_MM) for ellipses and Béziers; 0 for a point.
    pub fn length(&self) -> f64 {
        match *self {
            Shape::Point(_) => 0.0,
            Shape::Line { a, b } => a.distance(b),
            Shape::Circle { radius, .. } => TAU * radius.abs(),
            Shape::Arc { radius, sweep, .. } => (radius * sweep).abs(),
            Shape::Ellipse { .. } | Shape::Bezier { .. } => self
                .flatten(crate::tolerance::MEASURE_MM)
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .sum(),
        }
    }

    /// [`flatten`](Self::flatten), appended to `out`.
    pub fn flatten_into(&self, tolerance: f64, out: &mut Vec<DVec2>) {
        let along = |out: &mut Vec<DVec2>, n: usize, at: &dyn Fn(f64) -> DVec2| {
            out.extend((0..=n).map(|i| at(i as f64 / n as f64)));
        };
        match *self {
            Shape::Point(p) => out.push(p),
            Shape::Line { a, b } => out.extend([a, b]),
            Shape::Circle { center, radius } => {
                let n = arc_segments(radius, TAU, tolerance);
                along(out, n, &|f| center + DVec2::from_angle(TAU * f) * radius);
                // Closed exactly.
                if let Some(last) = out.last_mut() {
                    *last = center + DVec2::X * radius;
                }
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = arc_segments(radius, sweep, tolerance);
                along(out, n, &|f| {
                    center + DVec2::from_angle(start + sweep * f) * radius
                });
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                let n = ellipse_segments(major, minor, sweep, tolerance);
                along(out, n, &|f| {
                    ellipse_point(center, major, minor, start + sweep * f)
                });
            }
            Shape::Bezier { p } => {
                let n = bezier_segments(&p, tolerance);
                along(out, n, &|t| bezier_point(&p, t));
            }
        }
    }

    /// The point at parameter `t`: a fraction along a line (its ends exactly at 0 and 1), an
    /// angle from the start of an arc or ellipse, an angle from +x on a circle, a Bézier's
    /// parameter.
    pub fn point_at(&self, t: f64) -> DVec2 {
        match *self {
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
            } => ellipse_point(center, major, minor, start + t),
            Shape::Bezier { p } => bezier_point(&p, t),
            Shape::Point(p) => p,
        }
    }

    pub fn bounds(&self) -> Bounds {
        match *self {
            Shape::Point(p) => Bounds::from_point(p),
            Shape::Line { a, b } => {
                let mut bounds = Bounds::from_point(a);
                bounds.include(b);
                bounds
            }
            Shape::Circle { center, radius } => {
                Bounds::from_center_half_size(center, DVec2::splat(radius))
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => arc_bounds(center, radius, start, sweep),
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => ellipse_bounds(center, major, minor, start, sweep),
            Shape::Bezier { p } => bezier_bounds(&p),
        }
    }

    /// The curve as cubic Béziers from its start to its end, round arcs and ellipses in
    /// pieces of at most `max_step` radians (a line has its handles at its thirds). A point
    /// has none.
    pub fn cubics(&self, max_step: f64) -> Vec<[DVec2; 4]> {
        match *self {
            Shape::Point(_) => Vec::new(),
            Shape::Line { a, b } => vec![[a, a + (b - a) / 3.0, a + (b - a) * (2.0 / 3.0), b]],
            Shape::Circle { center, radius } => arc_cubics(center, radius, 0.0, TAU, max_step),
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => arc_cubics(center, radius, start, sweep, max_step),
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => ellipse_cubics(center, major, minor, start, sweep, max_step),
            Shape::Bezier { p } => vec![p],
        }
    }

    /// Whether the curve ends where it starts (a whole circle or ellipse).
    pub fn is_closed(&self) -> bool {
        match *self {
            Shape::Circle { .. } => true,
            Shape::Ellipse { sweep, .. } => sweep.abs() >= TAU - 1e-12,
            _ => false,
        }
    }
}

fn ellipse_bounds(center: DVec2, major: DVec2, minor: f64, start: f64, sweep: f64) -> Bounds {
    let at = |t: f64| ellipse_point(center, major, minor, t);
    let mut bounds = Bounds::from_point(at(start));
    bounds.include(at(start + sweep));
    // Where x and y peak: dx/dt = 0 and dy/dt = 0, each twice a turn.
    let (a, phi) = (major.length(), major.y.atan2(major.x));
    let tx = (-minor * phi.sin()).atan2(a * phi.cos());
    let ty = (minor * phi.cos()).atan2(a * phi.sin());
    for t0 in [tx, tx + PI, ty, ty + PI] {
        let off = (t0 - start).rem_euclid(TAU);
        if sweep >= TAU || off <= sweep {
            bounds.include(at(start + off));
        }
    }
    bounds
}

/// The box round an arc: its ends and every axis extreme inside the sweep.
pub fn arc_bounds(center: DVec2, radius: f64, start: f64, sweep: f64) -> Bounds {
    if sweep.abs() >= TAU {
        return Bounds::from_center_half_size(center, DVec2::splat(radius));
    }
    let (start, sweep) = if sweep < 0.0 {
        (start + sweep, -sweep)
    } else {
        (start, sweep)
    };
    let at = |angle: f64| center + radius * DVec2::from_angle(angle);
    let mut bounds = Bounds::from_point(at(start));
    bounds.include(at(start + sweep));
    // Include every axis extreme (multiple of 90°) that lies inside the sweep.
    let mut k = (start / FRAC_PI_2).ceil();
    while k * FRAC_PI_2 <= start + sweep {
        bounds.include(at(k * FRAC_PI_2));
        k += 1.0;
    }
    bounds
}

/// Interaction state that changes how an item is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Emphasis {
    #[default]
    Normal,
    Hovered,
    Selected,
    /// Geometry a tool is about to create.
    Preview,
    /// Geometry a tool is about to remove (e.g. a piece being trimmed).
    Remove,
}

/// How completely an entity is determined by constraints, for colouring (SolidWorks style:
/// blue = can still move, black/white = fully defined, red = conflicting).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DofState {
    /// Not analysed; drawn in the normal colour.
    #[default]
    Unknown,
    Under,
    Full,
    Over,
}

/// A shape plus its display properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SketchItem {
    pub shape: Shape,
    pub kind: LineKind,
    pub emphasis: Emphasis,
    /// For points: drawn as a small endpoint marker of a curve rather than a sketch point.
    pub vertex: bool,
    pub dof: DofState,
    /// Colour override (sRGB); `None` uses the state / theme colour.
    pub color: Option<[u8; 3]>,
    /// Line weight in hundredths of a mm on paper (0 = default thin line).
    pub weight: u16,
    pub line_type: crate::LineType,
}

impl SketchItem {
    pub fn new(shape: Shape) -> Self {
        Self {
            shape,
            kind: LineKind::Solid,
            emphasis: Emphasis::Normal,
            vertex: false,
            dof: DofState::Unknown,
            color: None,
            weight: 0,
            line_type: crate::LineType::Continuous,
        }
    }

    /// Set colour, weight and line type.
    pub fn with_look(
        mut self,
        color: Option<[u8; 3]>,
        weight: u16,
        line_type: crate::LineType,
    ) -> Self {
        self.color = color;
        self.weight = weight;
        self.line_type = line_type;
        self
    }

    pub fn with_kind(mut self, kind: LineKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_emphasis(mut self, emphasis: Emphasis) -> Self {
        self.emphasis = emphasis;
        self
    }

    pub fn with_dof(mut self, dof: DofState) -> Self {
        self.dof = dof;
        self
    }

    pub fn as_vertex(mut self) -> Self {
        self.vertex = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: DVec2, b: DVec2) {
        assert!((a - b).length() < 1e-12, "{a} != {b}");
    }

    #[test]
    fn a_lines_ends_are_exact_at_0_and_1() {
        let (a, b) = (DVec2::new(0.1, 0.2), DVec2::new(1e6 + 0.3, -7.7));
        let line = Shape::Line { a, b };
        assert_eq!(line.point_at(0.0), a);
        assert_eq!(line.point_at(1.0), b);
        assert_close(line.point_at(0.5), (a + b) * 0.5);
    }

    #[test]
    fn flattening_keeps_to_its_tolerance_and_lengths_are_exact_where_they_can_be() {
        let arc = Shape::Arc {
            center: DVec2::new(1e5, -2e5),
            radius: 100.0,
            start: 0.3,
            sweep: 2.0,
        };
        for tol in [0.1, 0.005, 1e-4] {
            let pts = arc.flatten(tol);
            // Every chord's middle is within the tolerance of the arc, and not much inside it.
            let sag = pts
                .windows(2)
                .map(|w| 100.0 - ((w[0] + w[1]) / 2.0).distance(DVec2::new(1e5, -2e5)))
                .fold(0.0, f64::max);
            assert!(sag <= tol && sag > tol / 4.0, "{tol}: {sag}");
        }
        // A Bézier's chords too (Wang's bound): against points between the steps.
        let p = [
            DVec2::new(0.0, 0.0),
            DVec2::new(30.0, 60.0),
            DVec2::new(70.0, -40.0),
            DVec2::new(100.0, 10.0),
        ];
        let b = Shape::Bezier { p };
        let pts = b.flatten(0.005);
        let n = pts.len() - 1;
        for k in 0..n {
            let mid = bezier_point(&p, (k as f64 + 0.5) / n as f64);
            let (a, c) = (pts[k], pts[k + 1]);
            let d = (c - a).perp_dot(mid - a).abs() / a.distance(c);
            assert!(d <= 0.005, "{k}: {d}");
        }
        assert_eq!((pts[0], pts[n]), (p[0], p[3]));
        // Lengths: exact for arcs, a Bézier's to well under a thousandth.
        assert_eq!(arc.length(), 200.0);
        let fine: f64 = (0..=20000)
            .map(|k| bezier_point(&p, k as f64 / 20000.0))
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| w[0].distance(w[1]))
            .sum();
        assert!((b.length() - fine).abs() < 1e-4, "{} vs {fine}", b.length());
    }

    #[test]
    fn circle_through_holds_far_from_the_origin_and_refuses_a_line() {
        let far = DVec2::new(1e6, -1e6);
        let (c, r) = circle_through(
            far + DVec2::new(1.0, 0.0),
            far + DVec2::new(0.0, 1.0),
            far + DVec2::new(-1.0, 0.0),
        )
        .expect("a circle");
        assert!(c.distance(far) < 1e-6, "{c}");
        assert!((r - 1.0).abs() < 1e-6, "{r}");
        assert!(circle_through(far, far + DVec2::X, far + 2.0 * DVec2::X).is_none());
    }

    #[test]
    fn quarter_arc_bounds() {
        let b = arc_bounds(DVec2::ZERO, 2.0, 0.0, FRAC_PI_2);
        assert_close(b.min, DVec2::new(0.0, 0.0));
        assert_close(b.max, DVec2::new(2.0, 2.0));
    }

    #[test]
    fn arc_crossing_quadrant_includes_extreme() {
        // From 45° to 135° passes through the top of the circle.
        let b = arc_bounds(DVec2::ZERO, 1.0, FRAC_PI_2 / 2.0, FRAC_PI_2);
        assert!((b.max.y - 1.0).abs() < 1e-12);
        assert!((b.min.y - 0.5f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn negative_sweep_matches_positive() {
        let a = arc_bounds(DVec2::ZERO, 1.0, 1.0, -2.0);
        let b = arc_bounds(DVec2::ZERO, 1.0, -1.0, 2.0);
        assert_close(a.min, b.min);
        assert_close(a.max, b.max);
    }
}
