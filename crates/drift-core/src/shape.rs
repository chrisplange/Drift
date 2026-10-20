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

/// A cubic Bézier as connected points: at least `min_segments`, more for long or bendy ones.
pub fn flatten_bezier(p: &[DVec2; 4], min_segments: usize) -> Vec<DVec2> {
    let hull = p[0].distance(p[1]) + p[1].distance(p[2]) + p[2].distance(p[3]);
    let chord = p[0].distance(p[3]);
    let bend = (hull - chord) / hull.max(1e-12);
    let n = (16.0 + 112.0 * bend.sqrt())
        .max(min_segments as f64)
        .min(256.0) as usize;
    (0..=n)
        .map(|i| bezier_point(p, i as f64 / n as f64))
        .collect()
}

/// A cubic Bézier as exactly `n` segments (for drawing, sized to the screen).
pub fn bezier_steps(p: &[DVec2; 4], n: usize) -> Vec<DVec2> {
    let n = n.max(1);
    (0..=n)
        .map(|i| bezier_point(p, i as f64 / n as f64))
        .collect()
}

/// An ellipse (or part) as exactly `n` segments (for drawing, sized to the screen).
pub fn ellipse_steps(
    center: DVec2,
    major: DVec2,
    minor: f64,
    start: f64,
    sweep: f64,
    n: usize,
) -> Vec<DVec2> {
    let n = n.max(1);
    (0..=n)
        .map(|i| ellipse_point(center, major, minor, start + sweep * i as f64 / n as f64))
        .collect()
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

/// An ellipse (or part of one) as connected points, at least `min_segments` segments and
/// finer for big ones.
pub fn flatten_ellipse(
    center: DVec2,
    major: DVec2,
    minor: f64,
    start: f64,
    sweep: f64,
    min_segments: usize,
) -> Vec<DVec2> {
    let n = ((sweep.abs() / TAU) * 128.0)
        .ceil()
        .max(min_segments as f64)
        .min(4096.0) as usize;
    (0..=n)
        .map(|i| ellipse_point(center, major, minor, start + sweep * i as f64 / n as f64))
        .collect()
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

impl Shape {
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

fn arc_bounds(center: DVec2, radius: f64, start: f64, sweep: f64) -> Bounds {
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
