//! Exact 2D geometry for Drift: nearest points, intersections, hit testing, snapping and a
//! spatial index over document entities. All computation is `f64`.

mod index;
pub mod intersect;
pub mod offset;
mod primitives;
mod reference;
pub mod regions;
mod snap;
pub mod walls;

pub use index::{BoxIndex, PointRole, SceneIndex};
pub use primitives::{
    angle_in_sweep, circle_segment_intersections, distance, ellipse_segment_intersections,
    nearest_on_bezier, nearest_on_ellipse, nearest_point, param_of, shape_intersects_rect,
    shape_within_rect,
};
pub use reference::RefIndex;
pub use snap::{Snap, SnapKind, SnapOptions, perpendicular_feet, tangent_points};

/// Whether two shapes are the same to the bit (`-0.0` and `0.0` differ; so do NaNs), as
/// a cache of something found from shapes needs to know.
pub fn same_shape(a: &drift_core::Shape, b: &drift_core::Shape) -> bool {
    use drift_core::{DVec2, Shape};
    fn bits(s: &Shape) -> [u64; 9] {
        let p = |v: DVec2| [v.x.to_bits(), v.y.to_bits()];
        let f = f64::to_bits;
        match *s {
            Shape::Point(q) => [0, p(q)[0], p(q)[1], 0, 0, 0, 0, 0, 0],
            Shape::Line { a, b } => [1, p(a)[0], p(a)[1], p(b)[0], p(b)[1], 0, 0, 0, 0],
            Shape::Circle { center, radius } => {
                [2, p(center)[0], p(center)[1], f(radius), 0, 0, 0, 0, 0]
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => [
                3,
                p(center)[0],
                p(center)[1],
                f(radius),
                f(start),
                f(sweep),
                0,
                0,
                0,
            ],
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => [
                4,
                p(center)[0],
                p(center)[1],
                p(major)[0],
                p(major)[1],
                f(minor),
                f(start),
                f(sweep),
                0,
            ],
            Shape::Bezier { p: q } => [
                5,
                p(q[0])[0],
                p(q[0])[1],
                p(q[1])[0],
                p(q[1])[1],
                p(q[2])[0],
                p(q[2])[1],
                p(q[3])[0],
                p(q[3])[1],
            ],
        }
    }
    bits(a) == bits(b)
}
