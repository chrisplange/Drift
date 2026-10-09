//! A spatial index over reference curves (underlays): curves that can be snapped to and drawn,
//! but aren't document entities. Snaps to them carry no entity, so tools place new points.

use std::f64::consts::FRAC_PI_2;

use drift_core::{Bounds, DVec2, Shape};
use rstar::{AABB, RTree, RTreeObject};

use crate::primitives::{angle_in_sweep, nearest_point};
use crate::snap::{Snap, SnapKind};

struct RefEntry {
    shape: Shape,
    envelope: AABB<[f64; 2]>,
}

impl RTreeObject for RefEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.envelope
    }
}

/// Reference curves in an R-tree.
#[derive(Default)]
pub struct RefIndex {
    tree: RTree<RefEntry>,
}

impl RefIndex {
    pub fn new(shapes: impl IntoIterator<Item = Shape>) -> Self {
        let entries: Vec<RefEntry> = shapes
            .into_iter()
            .map(|shape| {
                let b = shape.bounds();
                RefEntry {
                    shape,
                    envelope: AABB::from_corners(b.min.to_array(), b.max.to_array()),
                }
            })
            .collect();
        Self {
            tree: RTree::bulk_load(entries),
        }
    }

    pub fn len(&self) -> usize {
        self.tree.size()
    }

    pub fn is_empty(&self) -> bool {
        self.tree.size() == 0
    }

    /// The curves whose bounds meet `rect` (e.g. the visible part of the view).
    pub fn query(&self, rect: Bounds) -> impl Iterator<Item = &Shape> + '_ {
        let envelope = AABB::from_corners(rect.min.to_array(), rect.max.to_array());
        self.tree
            .locate_in_envelope_intersecting(envelope)
            .map(|e| &e.shape)
    }

    /// The best snap within `tolerance` of `p`: end points and centres, then midpoints and
    /// quadrants, then the nearest point on a curve.
    pub fn snap(&self, p: DVec2, tolerance: f64) -> Option<Snap> {
        let mut best: Option<(Snap, f64)> = None;
        let mut offer = |at: DVec2, kind: SnapKind| {
            let d = at.distance(p);
            if d > tolerance {
                return;
            }
            if best
                .as_ref()
                .is_none_or(|(b, bd)| (kind.priority(), d) < (b.kind.priority(), *bd))
            {
                best = Some((
                    Snap {
                        at,
                        kind,
                        entity: None,
                    },
                    d,
                ));
            }
        };
        let query = Bounds::from_center_half_size(p, DVec2::splat(tolerance));
        for shape in self.query(query) {
            match *shape {
                Shape::Point(at) => offer(at, SnapKind::Point),
                Shape::Line { a, b } => {
                    offer(a, SnapKind::Endpoint);
                    offer(b, SnapKind::Endpoint);
                    offer((a + b) * 0.5, SnapKind::Midpoint);
                }
                Shape::Circle { center, radius } => {
                    offer(center, SnapKind::Center);
                    for k in 0..4 {
                        let at = center + radius * DVec2::from_angle(f64::from(k) * FRAC_PI_2);
                        offer(at, SnapKind::Quadrant);
                    }
                }
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => {
                    offer(center, SnapKind::Center);
                    offer(
                        center + radius * DVec2::from_angle(start),
                        SnapKind::Endpoint,
                    );
                    offer(
                        center + radius * DVec2::from_angle(start + sweep),
                        SnapKind::Endpoint,
                    );
                    for k in 0..4 {
                        let angle = f64::from(k) * FRAC_PI_2;
                        if angle_in_sweep(angle, start, sweep) {
                            offer(
                                center + radius * DVec2::from_angle(angle),
                                SnapKind::Quadrant,
                            );
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
                    let at = |t: f64| drift_core::shape::ellipse_point(center, major, minor, t);
                    offer(center, SnapKind::Center);
                    if sweep < std::f64::consts::TAU {
                        offer(at(start), SnapKind::Endpoint);
                        offer(at(start + sweep), SnapKind::Endpoint);
                    }
                    for k in 0..4 {
                        let t = f64::from(k) * FRAC_PI_2;
                        if angle_in_sweep(t, start, sweep) {
                            offer(at(t), SnapKind::Quadrant);
                        }
                    }
                }
                Shape::Bezier { p } => {
                    offer(p[0], SnapKind::Endpoint);
                    offer(p[3], SnapKind::Endpoint);
                }
            }
            if !matches!(shape, Shape::Point(_)) {
                offer(nearest_point(shape, p), SnapKind::OnCurve);
            }
        }
        best.map(|(s, _)| s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn snaps_to_reference_ends_midpoints_centres_and_curves() {
        let index = RefIndex::new([
            Shape::Line {
                a: v(0.0, 0.0),
                b: v(10.0, 0.0),
            },
            Shape::Circle {
                center: v(30.0, 0.0),
                radius: 5.0,
            },
        ]);
        assert_eq!(index.len(), 2);
        let s = index.snap(v(0.3, 0.2), 1.0).unwrap();
        assert_eq!(
            (s.kind, s.at, s.entity),
            (SnapKind::Endpoint, v(0.0, 0.0), None)
        );
        let s = index.snap(v(5.2, 0.4), 1.0).unwrap();
        assert_eq!((s.kind, s.at), (SnapKind::Midpoint, v(5.0, 0.0)));
        let s = index.snap(v(30.2, 0.1), 1.0).unwrap();
        assert_eq!((s.kind, s.at), (SnapKind::Center, v(30.0, 0.0)));
        let s = index.snap(v(7.5, 0.3), 1.0).unwrap();
        assert_eq!((s.kind, s.at), (SnapKind::OnCurve, v(7.5, 0.0)));
        assert!(index.snap(v(50.0, 50.0), 1.0).is_none());
        let visible: Vec<_> = index
            .query(Bounds::from_center_half_size(v(30.0, 0.0), v(1.0, 1.0)))
            .collect();
        assert_eq!(visible.len(), 1);
    }
}
