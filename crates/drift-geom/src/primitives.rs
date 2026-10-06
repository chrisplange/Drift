use std::f64::consts::TAU;

use drift_core::{Bounds, DVec2, Shape};

/// Angular slack when deciding whether an angle lies within an arc's sweep.
const ANGLE_EPSILON: f64 = 1e-12;

/// True if `angle` lies on the counter-clockwise sweep starting at `start`.
pub fn angle_in_sweep(angle: f64, start: f64, sweep: f64) -> bool {
    sweep >= TAU || (angle - start).rem_euclid(TAU) <= sweep + ANGLE_EPSILON
}

/// The point on `shape` closest to `p`.
pub fn nearest_point(shape: &Shape, p: DVec2) -> DVec2 {
    match *shape {
        Shape::Point(at) => at,
        Shape::Line { a, b } => {
            let d = b - a;
            let len2 = d.length_squared();
            if len2 == 0.0 {
                return a;
            }
            let t = ((p - a).dot(d) / len2).clamp(0.0, 1.0);
            a + d * t
        }
        Shape::Circle { center, radius } => on_circle(center, radius, p),
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let rel = p - center;
            let angle = rel.y.atan2(rel.x);
            if rel != DVec2::ZERO && angle_in_sweep(angle, start, sweep) {
                on_circle(center, radius, p)
            } else {
                let s = center + radius * DVec2::from_angle(start);
                let e = center + radius * DVec2::from_angle(start + sweep);
                if p.distance_squared(s) <= p.distance_squared(e) {
                    s
                } else {
                    e
                }
            }
        }
    }
}

fn on_circle(center: DVec2, radius: f64, p: DVec2) -> DVec2 {
    let rel = p - center;
    if rel == DVec2::ZERO {
        center + DVec2::new(radius, 0.0)
    } else {
        center + rel.normalize() * radius
    }
}

/// Distance from `p` to the nearest point of `shape`.
pub fn distance(shape: &Shape, p: DVec2) -> f64 {
    nearest_point(shape, p).distance(p)
}

/// Intersections of the full circle with the segment `a`→`b`.
pub fn circle_segment_intersections(center: DVec2, radius: f64, a: DVec2, b: DVec2) -> Vec<DVec2> {
    let d = b - a;
    let f = a - center;
    let qa = d.dot(d);
    if qa == 0.0 {
        return Vec::new();
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.dot(f) - radius * radius;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return Vec::new();
    }
    let root = disc.sqrt();
    let mut out = Vec::with_capacity(2);
    for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
        if (0.0..=1.0).contains(&t) && !out.iter().any(|q: &DVec2| *q == a + d * t) {
            out.push(a + d * t);
        }
    }
    out
}

fn rect_edges(rect: &Bounds) -> [(DVec2, DVec2); 4] {
    let (lo, hi) = (rect.min, rect.max);
    let c = [lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
    [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])]
}

fn point_in_rect(p: DVec2, rect: &Bounds) -> bool {
    p.x >= rect.min.x && p.x <= rect.max.x && p.y >= rect.min.y && p.y <= rect.max.y
}

/// Window selection: the whole shape lies inside `rect`.
pub fn shape_within_rect(shape: &Shape, rect: &Bounds) -> bool {
    let b = shape.bounds();
    b.min.x >= rect.min.x && b.max.x <= rect.max.x && b.min.y >= rect.min.y && b.max.y <= rect.max.y
}

/// Crossing selection: any part of the shape lies inside `rect` or touches its boundary.
pub fn shape_intersects_rect(shape: &Shape, rect: &Bounds) -> bool {
    if !shape.bounds().intersects(rect) {
        return false;
    }
    if shape_within_rect(shape, rect) {
        return true;
    }
    match *shape {
        Shape::Point(p) => point_in_rect(p, rect),
        Shape::Line { a, b } => {
            point_in_rect(a, rect) || point_in_rect(b, rect) || segment_crosses_rect(a, b, rect)
        }
        Shape::Circle { center, radius } => rect_edges(rect)
            .iter()
            .any(|&(a, b)| !circle_segment_intersections(center, radius, a, b).is_empty()),
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let s = center + radius * DVec2::from_angle(start);
            let e = center + radius * DVec2::from_angle(start + sweep);
            point_in_rect(s, rect)
                || point_in_rect(e, rect)
                || rect_edges(rect).iter().any(|&(a, b)| {
                    circle_segment_intersections(center, radius, a, b)
                        .iter()
                        .any(|q| {
                            let rel = *q - center;
                            angle_in_sweep(rel.y.atan2(rel.x), start, sweep)
                        })
                })
        }
    }
}

/// Liang–Barsky: does segment `a`→`b` pass through the rectangle?
fn segment_crosses_rect(a: DVec2, b: DVec2, rect: &Bounds) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-d.x, a.x - rect.min.x),
        (d.x, rect.max.x - a.x),
        (-d.y, a.y - rect.min.y),
        (d.y, rect.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    t0 <= t1
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, PI};

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
        Bounds {
            min: v(x0, y0),
            max: v(x1, y1),
        }
    }

    #[test]
    fn nearest_on_segment_clamps_to_ends() {
        let line = Shape::Line {
            a: v(0.0, 0.0),
            b: v(10.0, 0.0),
        };
        assert_eq!(nearest_point(&line, v(3.0, 4.0)), v(3.0, 0.0));
        assert_eq!(nearest_point(&line, v(-5.0, 1.0)), v(0.0, 0.0));
        assert_eq!(distance(&line, v(13.0, 4.0)), 5.0);
    }

    #[test]
    fn nearest_on_arc_respects_sweep() {
        // Upper half circle of radius 2.
        let arc = Shape::Arc {
            center: v(0.0, 0.0),
            radius: 2.0,
            start: 0.0,
            sweep: PI,
        };
        assert!((nearest_point(&arc, v(0.0, 5.0)) - v(0.0, 2.0)).length() < 1e-12);
        // Below the arc: the nearest point is an end.
        assert!((nearest_point(&arc, v(1.5, -3.0)) - v(2.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn circle_segment_intersections_are_exact() {
        let hits = circle_segment_intersections(v(0.0, 0.0), 5.0, v(-10.0, 3.0), v(10.0, 3.0));
        assert_eq!(hits.len(), 2);
        assert!((hits[0] - v(-4.0, 3.0)).length() < 1e-12);
        assert!((hits[1] - v(4.0, 3.0)).length() < 1e-12);
        assert!(
            circle_segment_intersections(v(0.0, 0.0), 5.0, v(-1.0, 0.0), v(1.0, 0.0)).is_empty()
        );
    }

    #[test]
    fn window_and_crossing_selection() {
        let r = rect(0.0, 0.0, 10.0, 10.0);
        let inside = Shape::Line {
            a: v(1.0, 1.0),
            b: v(9.0, 9.0),
        };
        let crossing = Shape::Line {
            a: v(-5.0, 5.0),
            b: v(15.0, 5.0),
        };
        let diagonal_miss = Shape::Line {
            a: v(-5.0, 6.0),
            b: v(6.0, 17.0),
        }; // bounding box overlaps the rect but the line does not
        assert!(shape_within_rect(&inside, &r) && shape_intersects_rect(&inside, &r));
        assert!(!shape_within_rect(&crossing, &r) && shape_intersects_rect(&crossing, &r));
        assert!(!shape_intersects_rect(&diagonal_miss, &r));
    }

    #[test]
    fn circle_around_rect_does_not_cross_it() {
        let r = rect(-1.0, -1.0, 1.0, 1.0);
        let big = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 5.0,
        };
        let through = Shape::Circle {
            center: v(0.0, 0.0),
            radius: 1.2,
        };
        assert!(!shape_intersects_rect(&big, &r));
        assert!(shape_intersects_rect(&through, &r));
    }

    #[test]
    fn arc_crossing_checks_the_swept_part_only() {
        let r = rect(-1.0, 4.0, 1.0, 6.0); // straddles the top of a radius-5 circle
        let upper = Shape::Arc {
            center: v(0.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: PI,
        };
        let lower = Shape::Arc {
            center: v(0.0, 0.0),
            radius: 5.0,
            start: PI,
            sweep: PI,
        };
        assert!(shape_intersects_rect(&upper, &r));
        assert!(!shape_intersects_rect(&lower, &r));
        assert!(angle_in_sweep(FRAC_PI_2, 0.0, PI));
        assert!(!angle_in_sweep(-FRAC_PI_2, 0.0, PI));
    }
}
