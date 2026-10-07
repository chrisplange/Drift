//! Exact 2D geometry for Drift: nearest points, intersections, hit testing, snapping and a
//! spatial index over document entities. All computation is `f64`.

mod index;
pub mod intersect;
mod primitives;
pub mod regions;
mod snap;

pub use index::{PointRole, SceneIndex};
pub use primitives::{
    angle_in_sweep, circle_segment_intersections, distance, nearest_point, shape_intersects_rect,
    shape_within_rect,
};
pub use snap::{Snap, SnapKind, SnapOptions};
