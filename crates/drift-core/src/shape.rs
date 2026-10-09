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
}

/// A point of an ellipse at parametric angle `t`.
pub fn ellipse_point(center: DVec2, major: DVec2, minor: f64, t: f64) -> DVec2 {
    let n = major.perp().normalize_or_zero() * minor;
    center + major * t.cos() + n * t.sin()
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
        .max(min_segments as f64) as usize;
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
