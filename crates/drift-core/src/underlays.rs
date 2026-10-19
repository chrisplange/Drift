//! Underlays: reference drawings (a PDF page or a DXF file) shown under the sketch to trace
//! over. Their curves can be snapped to but not edited; they're placed with a position, a
//! scale and a rotation, which calibration sets from two known points. A PDF page's
//! pictures (scans, photos, markup flattened into it) come with it, as images.

use std::sync::Arc;

use glam::DVec2;

use crate::Shape;
use crate::bounds::Bounds;
use crate::layers::Rgb;

/// Stable identifier of an underlay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnderlayId(pub u32);

impl std::fmt::Display for UnderlayId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "u{}", self.0)
    }
}

/// A curve of an underlay, in the underlay's own coordinates (PDF points, or mm for DXF).
#[derive(Clone, Debug, PartialEq)]
pub enum RefPrim {
    Line(DVec2, DVec2),
    /// Connected segments (curves from a PDF are flattened into these).
    Polyline(Vec<DVec2>),
    Circle {
        center: DVec2,
        radius: f64,
    },
    /// Counter-clockwise from `start` by `sweep` radians.
    Arc {
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

/// A picture on an underlay, placed in the underlay's own coordinates by its top-left
/// corner and the vectors along its top edge (`across`) and down its left edge (`down`).
#[derive(Clone, Debug)]
pub struct UnderlayImage {
    pub width: u32,
    pub height: u32,
    /// Pixels as RGBA, a byte a channel (not premultiplied), rows from the top. Shared, so
    /// undo records and copies don't duplicate them.
    pub rgba: Arc<Vec<u8>>,
    pub at: DVec2,
    pub across: DVec2,
    pub down: DVec2,
}

impl PartialEq for UnderlayImage {
    fn eq(&self, other: &Self) -> bool {
        (self.width, self.height, self.at, self.across, self.down)
            == (
                other.width,
                other.height,
                other.at,
                other.across,
                other.down,
            )
            && (Arc::ptr_eq(&self.rgba, &other.rgba) || self.rgba == other.rgba)
    }
}

impl UnderlayImage {
    /// Its corners in underlay coordinates: top-left, top-right, bottom-right, bottom-left.
    pub fn corners(&self) -> [DVec2; 4] {
        [
            self.at,
            self.at + self.across,
            self.at + self.across + self.down,
            self.at + self.down,
        ]
    }
}

/// A reference drawing placed in the sketch.
#[derive(Clone, Debug, PartialEq)]
pub struct Underlay {
    pub name: String,
    /// Where it came from, e.g. "plan.pdf, page 2".
    pub source: String,
    /// Shared, so undo records and copies don't duplicate large drawings.
    pub prims: Arc<Vec<RefPrim>>,
    /// Its pictures, drawn beneath its curves.
    pub images: Arc<Vec<UnderlayImage>>,
    /// Where the underlay's origin lands in the sketch.
    pub at: DVec2,
    /// Sketch units (mm) per underlay unit.
    pub scale: f64,
    /// Rotation in degrees, counter-clockwise, about `at`.
    pub angle: f64,
    /// `None` is a neutral grey.
    pub color: Option<Rgb>,
    /// How strongly it shows, 0–1.
    pub opacity: f64,
    pub visible: bool,
    /// Whether tools snap to its curves.
    pub snap: bool,
    /// Locked underlays can't be moved, scaled or calibrated.
    pub locked: bool,
}

/// Sketch units (mm) per PDF point.
pub const MM_PER_POINT: f64 = 25.4 / 72.0;

impl Underlay {
    pub fn new(name: impl Into<String>, source: impl Into<String>, prims: Vec<RefPrim>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            prims: Arc::new(prims),
            images: Arc::new(Vec::new()),
            at: DVec2::ZERO,
            scale: 1.0,
            angle: 0.0,
            color: None,
            opacity: 0.6,
            visible: true,
            snap: true,
            locked: false,
        }
    }

    fn rotor(&self) -> DVec2 {
        crate::blocks::exact_rotor(self.angle)
    }

    /// A point of the underlay in the sketch.
    pub fn to_world(&self, p: DVec2) -> DVec2 {
        self.at + self.rotor().rotate(p * self.scale)
    }

    /// A sketch point in the underlay's coordinates.
    pub fn to_local(&self, p: DVec2) -> DVec2 {
        let r = self.rotor();
        DVec2::new(r.x, -r.y).rotate(p - self.at) / self.scale
    }

    /// Its curves in the sketch, as shapes (polylines become their segments).
    pub fn shapes(&self) -> Vec<Shape> {
        let turn = self.angle.to_radians();
        let mut out = Vec::new();
        for prim in self.prims.iter() {
            match prim {
                RefPrim::Line(a, b) => out.push(Shape::Line {
                    a: self.to_world(*a),
                    b: self.to_world(*b),
                }),
                RefPrim::Polyline(points) => {
                    out.extend(points.windows(2).map(|w| Shape::Line {
                        a: self.to_world(w[0]),
                        b: self.to_world(w[1]),
                    }));
                }
                RefPrim::Circle { center, radius } => out.push(Shape::Circle {
                    center: self.to_world(*center),
                    radius: radius * self.scale,
                }),
                RefPrim::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => out.push(Shape::Arc {
                    center: self.to_world(*center),
                    radius: radius * self.scale,
                    start: start + turn,
                    sweep: *sweep,
                }),
            }
        }
        out
    }

    /// Bounds of its curves and images in its own coordinates.
    pub fn local_bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        for image in self.images.iter() {
            image.corners().into_iter().for_each(|p| b.include(p));
        }
        for prim in self.prims.iter() {
            match prim {
                RefPrim::Line(p, q) => {
                    b.include(*p);
                    b.include(*q);
                }
                RefPrim::Polyline(points) => points.iter().for_each(|p| b.include(*p)),
                RefPrim::Circle { center, radius } | RefPrim::Arc { center, radius, .. } => {
                    b.include(*center - DVec2::splat(*radius));
                    b.include(*center + DVec2::splat(*radius));
                }
            }
        }
        b
    }

    /// Bounds in the sketch.
    pub fn bounds(&self) -> Bounds {
        let local = self.local_bounds();
        let mut b = Bounds::EMPTY;
        if local.is_empty() {
            return b;
        }
        for corner in [
            local.min,
            local.max,
            DVec2::new(local.min.x, local.max.y),
            DVec2::new(local.max.x, local.min.y),
        ] {
            b.include(self.to_world(corner));
        }
        b
    }

    /// Rescale so the sketch distance between the underlay points under `a` and `b` becomes
    /// `distance`, keeping the point under `a` where it is.
    pub fn calibrate(&mut self, a: DVec2, b: DVec2, distance: f64) -> Result<(), &'static str> {
        let measured = a.distance(b);
        if measured <= 1e-9 || measured.is_nan() {
            return Err("Pick two different points");
        }
        if !(distance > 0.0 && distance.is_finite()) {
            return Err("The distance must be more than zero");
        }
        let anchor = self.to_local(a);
        self.scale *= distance / measured;
        // Keep the first point fixed: move the origin so `anchor` maps back onto `a`.
        self.at += a - self.to_world(anchor);
        Ok(())
    }

    /// The number of curves (segments of polylines counted one by one).
    pub fn curve_count(&self) -> usize {
        self.prims
            .iter()
            .map(|p| match p {
                RefPrim::Polyline(points) => points.len().saturating_sub(1),
                _ => 1,
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn placement_round_trips_and_calibration_keeps_the_first_point() {
        let mut u = Underlay::new(
            "Plan",
            "plan.pdf",
            vec![RefPrim::Polyline(vec![
                v(0.0, 0.0),
                v(100.0, 0.0),
                v(100.0, 50.0),
            ])],
        );
        u.at = v(10.0, 20.0);
        u.scale = MM_PER_POINT;
        u.angle = 30.0;
        let p = v(12.5, -4.0);
        assert!(u.to_local(u.to_world(p)).distance(p) < 1e-9);
        assert_eq!(u.curve_count(), 2);
        assert_eq!(u.shapes().len(), 2);
        // The 100-unit wall is really 5000 mm.
        let (a, b) = (u.to_world(v(0.0, 0.0)), u.to_world(v(100.0, 0.0)));
        u.calibrate(a, b, 5000.0).unwrap();
        assert!(u.to_world(v(0.0, 0.0)).distance(a) < 1e-9);
        assert!((u.to_world(v(100.0, 0.0)).distance(a) - 5000.0).abs() < 1e-6);
        assert!(u.calibrate(a, a, 10.0).is_err());
        assert!(u.calibrate(a, b, 0.0).is_err());
    }

    #[test]
    fn arcs_turn_with_the_underlay() {
        let mut u = Underlay::new(
            "Door",
            "door.dxf",
            vec![RefPrim::Arc {
                center: DVec2::ZERO,
                radius: 1.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            }],
        );
        u.angle = 90.0;
        u.scale = 2.0;
        let Shape::Arc { radius, start, .. } = u.shapes()[0] else {
            panic!()
        };
        assert_eq!(radius, 2.0);
        assert!((start - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        let b = u.bounds();
        assert!(b.min.x <= -2.0 + 1e-9 && b.max.x >= 2.0 - 1e-9);
    }
}
