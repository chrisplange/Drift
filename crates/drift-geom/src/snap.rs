use std::f64::consts::{FRAC_PI_2, PI};

use drift_core::{DVec2, EntityId, Shape};

use crate::index::{PointRole, SceneIndex};
use crate::primitives::{angle_in_sweep, nearest_point};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    /// End of a line or arc. `entity` is the point.
    Endpoint,
    /// A standalone sketch point. `entity` is the point.
    Point,
    /// Centre of a circle or arc. `entity` is the centre point.
    Center,
    /// Midpoint of a line. `entity` is the line.
    Midpoint,
    /// 0°/90°/180°/270° point of a circle or arc. `entity` is the curve.
    Quadrant,
    /// Nearest point on a curve. `entity` is the curve.
    OnCurve,
    /// The sketch's origin, (0, 0). No entity.
    Origin,
    /// Grid intersection.
    Grid,
    /// Foot of the perpendicular from the segment's start onto a line (within it), or the
    /// point along the radius on a circle or arc. `entity` is the curve.
    Perpendicular,
    /// Where a line from the segment's start touches a circle or arc. `entity` is the curve.
    Tangent,
}

impl SnapKind {
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Endpoint => "Endpoint",
            SnapKind::Point => "Point",
            SnapKind::Center => "Center",
            SnapKind::Midpoint => "Midpoint",
            SnapKind::Quadrant => "Quadrant",
            SnapKind::OnCurve => "On curve",
            SnapKind::Origin => "Origin",
            SnapKind::Grid => "Grid",
            SnapKind::Perpendicular => "Perpendicular",
            SnapKind::Tangent => "Tangent",
        }
    }

    /// True if the snap target is an existing point entity, which tools can reuse.
    pub fn is_point(self) -> bool {
        matches!(
            self,
            SnapKind::Endpoint | SnapKind::Point | SnapKind::Center
        )
    }

    /// Lower is stronger: points beat perpendicular / tangent points, which beat other derived
    /// points, which beat on-curve, which beats grid.
    pub fn priority(self) -> u8 {
        match self {
            SnapKind::Endpoint | SnapKind::Point | SnapKind::Center => 0,
            SnapKind::Perpendicular | SnapKind::Tangent => 1,
            SnapKind::Midpoint | SnapKind::Quadrant | SnapKind::Origin => 2,
            SnapKind::OnCurve => 3,
            SnapKind::Grid => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub at: DVec2,
    pub kind: SnapKind,
    pub entity: Option<EntityId>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapOptions {
    /// Capture radius in world units (usually a fixed number of screen pixels / zoom).
    pub tolerance: f64,
    /// Grid spacing to snap to when nothing else is in range, if grid snapping is on.
    pub grid: Option<f64>,
    /// Where the segment being drawn starts, for perpendicular and tangent snaps (`None`: off).
    pub from: Option<DVec2>,
}

/// Feet of perpendiculars from `from` onto a curve: on a line, where the perpendicular meets
/// it (only within the line); on a circle or arc, both points along the radius through `from`
/// (those within the arc).
pub fn perpendicular_feet(shape: &Shape, from: DVec2) -> Vec<DVec2> {
    match *shape {
        Shape::Line { a, b } => {
            let d = b - a;
            let len2 = d.length_squared();
            if len2 == 0.0 {
                return Vec::new();
            }
            let t = (from - a).dot(d) / len2;
            if (0.0..=1.0).contains(&t) {
                vec![a + d * t]
            } else {
                Vec::new()
            }
        }
        Shape::Circle { center, radius } => radial_points(center, radius, from, None),
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => radial_points(center, radius, from, Some((start, sweep))),
        _ => Vec::new(),
    }
}

/// Points where lines from `from` touch a circle or arc (none from inside the circle).
pub fn tangent_points(shape: &Shape, from: DVec2) -> Vec<DVec2> {
    let (center, radius, sweep) = match *shape {
        Shape::Circle { center, radius } => (center, radius, None),
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => (center, radius, Some((start, sweep))),
        _ => return Vec::new(),
    };
    let d = from - center;
    let dist = d.length();
    if radius <= 0.0 || dist <= radius * (1.0 + 1e-12) {
        return Vec::new();
    }
    let toward = d.y.atan2(d.x);
    let spread = (radius / dist).acos();
    on_circle(center, radius, [toward - spread, toward + spread], sweep)
}

/// The two points of a circle on the line through its centre and `from`.
fn radial_points(center: DVec2, radius: f64, from: DVec2, sweep: Option<(f64, f64)>) -> Vec<DVec2> {
    let d = from - center;
    if d.length_squared() == 0.0 || radius <= 0.0 {
        return Vec::new();
    }
    let toward = d.y.atan2(d.x);
    on_circle(center, radius, [toward, toward + PI], sweep)
}

/// The points of a circle at `angles`, keeping those within an arc's sweep (if any).
fn on_circle(
    center: DVec2,
    radius: f64,
    angles: [f64; 2],
    sweep: Option<(f64, f64)>,
) -> Vec<DVec2> {
    angles
        .into_iter()
        .filter(|t| sweep.is_none_or(|(start, sweep)| angle_in_sweep(*t, start, sweep)))
        .map(|t| center + radius * DVec2::from_angle(t))
        .collect()
}

impl SceneIndex {
    /// Find the best snap target near `p`. Entities for which `exclude` returns true are
    /// ignored (e.g. the geometry being dragged).
    pub fn snap(
        &self,
        p: DVec2,
        options: SnapOptions,
        exclude: impl Fn(EntityId) -> bool,
    ) -> Option<Snap> {
        let tol = options.tolerance;
        let mut best: Option<(Snap, f64)> = None;
        let mut offer = |snap: Snap| {
            let d = snap.at.distance(p);
            if d > tol {
                return;
            }
            let better = match &best {
                None => true,
                Some((b, bd)) => (snap.kind.priority(), d) < (b.kind.priority(), *bd),
            };
            if better {
                best = Some((snap, d));
            }
        };

        // Derived points (centres, quadrants) can be up to a radius away from the curve, so
        // query the index by bounding box rather than by distance to the curve.
        let query = drift_core::Bounds::from_center_half_size(p, DVec2::splat(tol));
        for id in self.query(query) {
            if exclude(id) {
                continue;
            }
            let Some(shape) = self.shape(id) else {
                continue;
            };
            match *shape {
                Shape::Point(at) => {
                    let kind = match self.role(id) {
                        Some(PointRole::Endpoint) => SnapKind::Endpoint,
                        Some(PointRole::Center) => SnapKind::Center,
                        // Handles shape curves; they aren't places to snap to.
                        Some(PointRole::Handle) => continue,
                        _ => SnapKind::Point,
                    };
                    offer(Snap {
                        at,
                        kind,
                        entity: Some(id),
                    });
                }
                Shape::Line { a, b } => {
                    offer(Snap {
                        at: (a + b) * 0.5,
                        kind: SnapKind::Midpoint,
                        entity: Some(id),
                    });
                }
                Shape::Circle { center, radius } => {
                    for k in 0..4 {
                        offer(Snap {
                            at: center + radius * DVec2::from_angle(f64::from(k) * FRAC_PI_2),
                            kind: SnapKind::Quadrant,
                            entity: Some(id),
                        });
                    }
                }
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    for k in 0..4 {
                        let angle = f64::from(k) * FRAC_PI_2;
                        if angle_in_sweep(angle, start, sweep) {
                            offer(Snap {
                                at: center + radius * DVec2::from_angle(angle),
                                kind: SnapKind::Quadrant,
                                entity: Some(id),
                            });
                        }
                    }
                }
                Shape::Ellipse {
                    center,
                    major,
                    minor,
                    start,
                    sweep,
                } => {
                    // The ends of both axes (the two given ones are points already).
                    for k in 0..4 {
                        let t = f64::from(k) * FRAC_PI_2;
                        if angle_in_sweep(t, start, sweep) {
                            offer(Snap {
                                at: drift_core::shape::ellipse_point(center, major, minor, t),
                                kind: SnapKind::Quadrant,
                                entity: Some(id),
                            });
                        }
                    }
                }
                // Curves snap to their anchors (points) and along themselves.
                Shape::Bezier { .. } => {}
            }
            if !matches!(shape, Shape::Point(_)) {
                offer(Snap {
                    at: nearest_point(shape, p),
                    kind: SnapKind::OnCurve,
                    entity: Some(id),
                });
            }
            // Perpendicular and tangent points, seen from the segment's start (none so close
            // to it that the segment would vanish).
            if let Some(from) = options.from {
                let far = |q: &DVec2| q.distance(from) > tol;
                for at in perpendicular_feet(shape, from).into_iter().filter(far) {
                    offer(Snap {
                        at,
                        kind: SnapKind::Perpendicular,
                        entity: Some(id),
                    });
                }
                for at in tangent_points(shape, from).into_iter().filter(far) {
                    offer(Snap {
                        at,
                        kind: SnapKind::Tangent,
                        entity: Some(id),
                    });
                }
            }
        }

        if let Some((snap, _)) = best {
            return Some(snap);
        }
        options.grid.filter(|g| *g > 0.0).map(|g| Snap {
            at: (p / g).round() * g,
            kind: SnapKind::Grid,
            entity: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use drift_core::Document;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn options(tolerance: f64) -> SnapOptions {
        SnapOptions {
            tolerance,
            grid: None,
            from: None,
        }
    }

    fn fixture() -> (Document, EntityId, EntityId, EntityId) {
        let mut doc = Document::new();
        let a = doc.add_point(v(0.0, 0.0));
        let b = doc.add_point(v(10.0, 0.0));
        let line = doc.add_line(a, b).unwrap();
        let c = doc.add_point(v(20.0, 0.0));
        doc.add_circle(c, 5.0).unwrap();
        (doc, a, line, c)
    }

    #[test]
    fn snaps_to_endpoints_midpoints_and_centres() {
        let (doc, a, line, c) = fixture();
        let index = SceneIndex::build(&doc);

        let s = index.snap(v(0.4, 0.3), options(1.0), |_| false).unwrap();
        assert_eq!(
            (s.kind, s.entity, s.at),
            (SnapKind::Endpoint, Some(a), v(0.0, 0.0))
        );

        let s = index.snap(v(5.2, 0.3), options(1.0), |_| false).unwrap();
        assert_eq!(
            (s.kind, s.entity, s.at),
            (SnapKind::Midpoint, Some(line), v(5.0, 0.0))
        );

        let s = index.snap(v(20.3, -0.2), options(1.0), |_| false).unwrap();
        assert_eq!((s.kind, s.entity), (SnapKind::Center, Some(c)));

        let s = index.snap(v(25.2, 0.4), options(1.0), |_| false).unwrap();
        assert_eq!(s.kind, SnapKind::Quadrant);
        assert!((s.at - v(25.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn falls_back_to_curve_then_grid() {
        let (doc, ..) = fixture();
        let index = SceneIndex::build(&doc);
        let s = index.snap(v(2.5, 0.4), options(0.5), |_| false).unwrap();
        assert_eq!((s.kind, s.at), (SnapKind::OnCurve, v(2.5, 0.0)));

        assert_eq!(index.snap(v(50.0, 50.0), options(0.5), |_| false), None);
        let grid = SnapOptions {
            tolerance: 0.5,
            grid: Some(10.0),
            from: None,
        };
        let s = index.snap(v(47.0, 52.0), grid, |_| false).unwrap();
        assert_eq!((s.kind, s.at), (SnapKind::Grid, v(50.0, 50.0)));
    }

    #[test]
    fn excluded_entities_are_ignored() {
        let (doc, a, line, _) = fixture();
        let index = SceneIndex::build(&doc);
        let s = index.snap(v(0.4, 0.3), options(1.0), |id| id == a || id == line);
        assert_eq!(s, None);
    }

    fn upper_half_arc() -> Shape {
        Shape::Arc {
            center: v(0.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: PI,
        }
    }

    #[test]
    fn perpendicular_feet_on_lines_circles_and_arcs() {
        let line = Shape::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        };
        assert_eq!(perpendicular_feet(&line, v(3.0, 4.0)), vec![v(3.0, 0.0)]);
        // Beyond the line's ends: none (the extension doesn't count).
        assert!(perpendicular_feet(&line, v(12.0, 4.0)).is_empty());

        let circle = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 5.0,
        };
        let feet = perpendicular_feet(&circle, v(10.0, 0.0));
        assert_eq!(feet.len(), 2);
        assert!(feet[0].distance(v(5.0, 0.0)) < 1e-12);
        assert!(feet[1].distance(v(-5.0, 0.0)) < 1e-12);
        // From the centre every direction is radial: no foot.
        assert!(perpendicular_feet(&circle, v(0.0, 0.0)).is_empty());

        // Only the far foot of a point below the upper half lies on it.
        let feet = perpendicular_feet(&upper_half_arc(), v(0.0, -10.0));
        assert_eq!(feet.len(), 1);
        assert!(feet[0].distance(v(0.0, 5.0)) < 1e-12);
    }

    #[test]
    fn tangent_points_from_outside() {
        let circle = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 5.0,
        };
        let from = v(10.0, 0.0);
        let points = tangent_points(&circle, from);
        assert_eq!(points.len(), 2);
        for q in &points {
            // On the circle, and the radius there is square to the line from `from`.
            assert!((q.length() - 5.0).abs() < 1e-12);
            assert!(q.dot(from - *q).abs() < 1e-9);
        }
        // From inside or on the circle: none.
        assert!(tangent_points(&circle, v(1.0, 1.0)).is_empty());
        assert!(tangent_points(&circle, v(5.0, 0.0)).is_empty());

        // The upper half only keeps the upper tangent point.
        let points = tangent_points(&upper_half_arc(), from);
        assert_eq!(points.len(), 1);
        assert!(points[0].y > 0.0);
    }

    #[test]
    fn snaps_perpendicular_and_tangent_from_the_segment_start() {
        let (doc, _, line, _) = fixture();
        let index = SceneIndex::build(&doc);
        let from = |at| SnapOptions {
            tolerance: 0.5,
            grid: None,
            from: Some(at),
        };
        // Square onto the line, from above it.
        let s = index
            .snap(v(3.2, 0.3), from(v(3.0, 6.0)), |_| false)
            .unwrap();
        assert_eq!(
            (s.kind, s.entity, s.at),
            (SnapKind::Perpendicular, Some(line), v(3.0, 0.0))
        );
        // Without a start point the same spot is only on the curve.
        let s = index.snap(v(3.2, 0.3), options(0.5), |_| false).unwrap();
        assert_eq!(s.kind, SnapKind::OnCurve);

        // Tangent to the circle (centre (20, 0), radius 5) from (20, 15): the touching points
        // are 25 / 15 above the centre.
        let touch_y: f64 = 25.0 / 15.0;
        let touch = v(20.0 + (25.0 - touch_y * touch_y).sqrt(), touch_y);
        let s = index
            .snap(touch + v(0.2, 0.1), from(v(20.0, 15.0)), |_| false)
            .unwrap();
        assert_eq!(s.kind, SnapKind::Tangent);
        assert!(s.at.distance(touch) < 1e-9);

        // An end point still wins over a perpendicular foot at the same spot.
        let s = index
            .snap(v(0.1, 0.1), from(v(0.0, 6.0)), |_| false)
            .unwrap();
        assert_eq!(s.kind, SnapKind::Endpoint);
        // A foot at the segment's own start is no target.
        let s = index
            .snap(v(3.1, 0.1), from(v(3.0, 0.0)), |_| false)
            .unwrap();
        assert_eq!(s.kind, SnapKind::OnCurve);
    }
}
