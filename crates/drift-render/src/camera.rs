use drift_core::{Bounds, DVec2};

/// Maps between world space (f64, y-up, millimetres) and view space.
///
/// View space is measured in logical points relative to the centre of the canvas, y-down.
/// All maths here is `f64`; precision is only reduced to `f32` after geometry has been made
/// relative to the camera, so there is no jitter at large coordinates or deep zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// World point shown at the centre of the canvas.
    pub center: DVec2,
    /// Logical points per world unit.
    pub scale: f64,
}

impl Camera {
    pub const MIN_SCALE: f64 = 1e-6;
    pub const MAX_SCALE: f64 = 1e9;

    pub fn world_to_view(&self, p: DVec2) -> DVec2 {
        let d = (p - self.center) * self.scale;
        DVec2::new(d.x, -d.y)
    }

    pub fn view_to_world(&self, v: DVec2) -> DVec2 {
        self.center + DVec2::new(v.x, -v.y) / self.scale
    }

    /// Pan by a view-space delta (e.g. a mouse drag), so content follows the pointer.
    pub fn pan(&mut self, view_delta: DVec2) {
        self.center -= DVec2::new(view_delta.x, -view_delta.y) / self.scale;
    }

    /// Multiply the scale by `factor`, keeping the world point under `view_anchor` fixed.
    pub fn zoom_at(&mut self, view_anchor: DVec2, factor: f64) {
        let anchor_world = self.view_to_world(view_anchor);
        self.scale = (self.scale * factor).clamp(Self::MIN_SCALE, Self::MAX_SCALE);
        self.center = anchor_world - DVec2::new(view_anchor.x, -view_anchor.y) / self.scale;
    }

    /// Frame `bounds` inside a view of `view_size` points, leaving `margin` points on each side.
    pub fn fit(&mut self, bounds: Bounds, view_size: DVec2, margin: f64) {
        if bounds.is_empty() {
            return;
        }
        let available = (view_size - DVec2::splat(2.0 * margin)).max(DVec2::splat(1.0));
        let size = bounds.size().max(DVec2::splat(1e-9));
        let scale = (available.x / size.x).min(available.y / size.y);
        self.center = bounds.center();
        self.scale = scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE);
    }

    /// The world-space rectangle visible in a view of `view_size` points.
    pub fn visible_world(&self, view_size: DVec2) -> Bounds {
        Bounds::from_center_half_size(self.center, view_size * 0.5 / self.scale)
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: DVec2::ZERO,
            scale: 4.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_at_large_coordinates() {
        let cam = Camera {
            center: DVec2::new(1.0e6, -2.5e6),
            scale: 1.0e5,
        };
        let p = DVec2::new(1.0e6 + 0.001, -2.5e6 - 0.002);
        let back = cam.view_to_world(cam.world_to_view(p));
        assert!((back - p).length() < 1e-9);
    }

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut cam = Camera::default();
        let anchor = DVec2::new(120.0, -45.0);
        let before = cam.view_to_world(anchor);
        cam.zoom_at(anchor, 3.7);
        let after = cam.view_to_world(anchor);
        assert!((before - after).length() < 1e-12);
    }

    #[test]
    fn pan_follows_pointer() {
        let mut cam = Camera::default();
        let p = DVec2::new(10.0, 20.0);
        let v0 = cam.world_to_view(p);
        cam.pan(DVec2::new(15.0, -7.0));
        let v1 = cam.world_to_view(p);
        assert!((v1 - v0 - DVec2::new(15.0, -7.0)).length() < 1e-12);
    }

    #[test]
    fn fit_contains_bounds() {
        let mut cam = Camera::default();
        let bounds = Bounds {
            min: DVec2::new(-10.0, 0.0),
            max: DVec2::new(110.0, 80.0),
        };
        let view = DVec2::new(800.0, 600.0);
        cam.fit(bounds, view, 20.0);
        let visible = cam.visible_world(view);
        assert!(visible.min.x <= bounds.min.x && visible.max.x >= bounds.max.x);
        assert!(visible.min.y <= bounds.min.y && visible.max.y >= bounds.max.y);
    }
}
