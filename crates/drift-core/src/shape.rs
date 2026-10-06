use std::f64::consts::{FRAC_PI_2, TAU};

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

/// A primitive sketch shape in world space.
///
/// This is the pre-solver geometry representation used by the M0 renderer. In M1 it is
/// replaced by parameter-backed entities, and shapes become derived output of the solver.
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
        }
    }
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

/// A shape plus its display properties.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SketchItem {
    pub shape: Shape,
    pub kind: LineKind,
}

impl SketchItem {
    pub fn new(shape: Shape) -> Self {
        Self {
            shape,
            kind: LineKind::Solid,
        }
    }

    pub fn with_kind(mut self, kind: LineKind) -> Self {
        self.kind = kind;
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
