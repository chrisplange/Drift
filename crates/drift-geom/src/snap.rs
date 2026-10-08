use std::f64::consts::FRAC_PI_2;

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
    /// Grid intersection.
    Grid,
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
            SnapKind::Grid => "Grid",
        }
    }

    /// True if the snap target is an existing point entity, which tools can reuse.
    pub fn is_point(self) -> bool {
        matches!(
            self,
            SnapKind::Endpoint | SnapKind::Point | SnapKind::Center
        )
    }

    /// Lower is stronger: points beat derived points, which beat on-curve, which beats grid.
    pub fn priority(self) -> u8 {
        match self {
            SnapKind::Endpoint | SnapKind::Point | SnapKind::Center => 0,
            SnapKind::Midpoint | SnapKind::Quadrant => 1,
            SnapKind::OnCurve => 2,
            SnapKind::Grid => 3,
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
            }
            if !matches!(shape, Shape::Point(_)) {
                offer(Snap {
                    at: nearest_point(shape, p),
                    kind: SnapKind::OnCurve,
                    entity: Some(id),
                });
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
}
