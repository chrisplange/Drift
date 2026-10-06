//! Builds per-frame GPU data from world geometry.
//!
//! Everything is computed in `f64` relative to the camera and only converted to `f32` at the
//! end, in physical pixels relative to the canvas centre (y-down). The GPU never sees large
//! world coordinates, which keeps rendering exact at any zoom and any distance from the origin.

use std::f64::consts::TAU;

use bytemuck::{Pod, Zeroable};
use drift_core::{DVec2, LineKind, Shape, SketchItem};

use crate::{Camera, Palette};

/// Radii above this many pixels are flattened on the CPU (in `f64`) instead of rendered with the
/// arc SDF, whose `f32` precision degrades for huge radii.
const MAX_SDF_RADIUS_PX: f64 = 16_384.0;
/// Maximum deviation of a flattened arc from the true curve, in physical pixels.
const FLATTEN_TOLERANCE_PX: f64 = 0.1;
/// Extra pixels around the canvas for culling and clipping, so strokes aren't cut at the edge.
const CULL_MARGIN_PX: f64 = 8.0;
/// Minimum on-screen grid spacing, in logical points.
const MIN_GRID_SPACING_PT: f64 = 10.0;

/// Line segment instance. Positions are physical pixels relative to the canvas centre.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SegmentInstance {
    pub p0: [f32; 2],
    pub p1: [f32; 2],
    pub color: [u8; 4],
    pub half_width: f32,
    /// Dash pattern: `[dash, gap, dot, phase]` in pixels. `dash == 0` means solid.
    pub pattern: [f32; 4],
}

/// Circle / arc / point instance. Positions are physical pixels relative to the canvas centre.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ArcInstance {
    pub center: [f32; 2],
    /// `[radius, half_width, start_angle, sweep]`. Angles are world-space (counter-clockwise).
    pub params: [f32; 4],
    pub color: [u8; 4],
    pub pattern: [f32; 4],
}

/// Uniform block shared by all canvas pipelines. Laid out as `vec4`s for WebGL2 / std140.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Globals {
    /// `[width_px, height_px, srgb_target, 0]`
    pub viewport: [f32; 4],
    /// `[spacing_px, phase_x, phase_y, alpha]`
    pub grid_minor: [f32; 4],
    /// `[spacing_px, phase_x, phase_y, 0]`
    pub grid_major: [f32; 4],
    /// `[origin_x_px, origin_y_px, show_axes, 0]`
    pub axes: [f32; 4],
    pub background: [f32; 4],
    pub minor_color: [f32; 4],
    pub major_color: [f32; 4],
    pub axis_x_color: [f32; 4],
    pub axis_y_color: [f32; 4],
}

/// The canvas being drawn into.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    /// Canvas size in physical pixels (may be fractional).
    pub size_px: DVec2,
    pub pixels_per_point: f64,
    /// True if the render target is an sRGB format, so colours must be linearised.
    pub srgb_target: bool,
}

/// Everything needed to draw one frame of the canvas.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub globals: Globals,
    pub segments: Vec<SegmentInstance>,
    pub arcs: Vec<ArcInstance>,
}

/// Grid spacing currently shown, in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridInfo {
    pub minor: f64,
    pub major: f64,
}

struct Builder<'a> {
    camera: &'a Camera,
    viewport: Viewport,
    palette: &'a Palette,
    half_size: DVec2,
    frame: Frame,
}

/// Build the GPU data for one frame.
pub fn build_frame(
    items: &[SketchItem],
    camera: &Camera,
    viewport: Viewport,
    palette: &Palette,
) -> Frame {
    let mut builder = Builder {
        camera,
        viewport,
        palette,
        half_size: viewport.size_px * 0.5 + DVec2::splat(CULL_MARGIN_PX),
        frame: Frame::default(),
    };
    builder.frame.globals = builder.globals();
    for item in items {
        builder.add(item);
    }
    builder.frame
}

/// Grid spacing for a camera: the smallest power of ten that is at least
/// [`MIN_GRID_SPACING_PT`] on screen.
pub fn grid_info(camera: &Camera) -> GridInfo {
    let minor = 10f64.powf((MIN_GRID_SPACING_PT / camera.scale).log10().ceil());
    GridInfo {
        minor,
        major: minor * 10.0,
    }
}

impl Builder<'_> {
    fn ppp(&self) -> f64 {
        self.viewport.pixels_per_point
    }

    fn to_px(&self, p: DVec2) -> DVec2 {
        self.camera.world_to_view(p) * self.ppp()
    }

    fn scale_px(&self) -> f64 {
        self.camera.scale * self.ppp()
    }

    fn globals(&self) -> Globals {
        let palette = self.palette;
        let scale_px = self.scale_px();
        let grid = grid_info(self.camera);
        let center = self.camera.center;
        let phase = |spacing: f64| {
            [
                ((-center.x).rem_euclid(spacing) * scale_px) as f32,
                (center.y.rem_euclid(spacing) * scale_px) as f32,
            ]
        };
        let [minor_x, minor_y] = phase(grid.minor);
        let [major_x, major_y] = phase(grid.major);
        let min_px = MIN_GRID_SPACING_PT * self.ppp();
        let minor_px = grid.minor * scale_px;
        let minor_alpha = ((minor_px - min_px) / (3.0 * min_px)).clamp(0.0, 1.0);
        let origin = self
            .to_px(DVec2::ZERO)
            .clamp(DVec2::splat(-1e7), DVec2::splat(1e7));

        Globals {
            viewport: [
                self.viewport.size_px.x as f32,
                self.viewport.size_px.y as f32,
                if self.viewport.srgb_target { 1.0 } else { 0.0 },
                0.0,
            ],
            grid_minor: [minor_px as f32, minor_x, minor_y, minor_alpha as f32],
            grid_major: [(grid.major * scale_px) as f32, major_x, major_y, 0.0],
            axes: [origin.x as f32, origin.y as f32, 1.0, 0.0],
            background: rgba_f32(palette.background),
            minor_color: rgba_f32(palette.grid_minor),
            major_color: rgba_f32(palette.grid_major),
            axis_x_color: rgba_f32(palette.axis_x),
            axis_y_color: rgba_f32(palette.axis_y),
        }
    }

    fn style(&self, kind: LineKind) -> ([u8; 4], f64, [f64; 3]) {
        let ppp = self.ppp();
        let p = self.palette;
        match kind {
            LineKind::Solid => (p.solid, 0.8 * ppp, [0.0; 3]),
            LineKind::Construction => (p.construction, 0.5 * ppp, [6.0 * ppp, 4.0 * ppp, 0.0]),
            LineKind::Center => (p.center, 0.5 * ppp, [16.0 * ppp, 3.0 * ppp, 2.0 * ppp]),
        }
    }

    fn add(&mut self, item: &SketchItem) {
        let (color, half_width, dash) = self.style(item.kind);
        match item.shape {
            Shape::Point(p) => {
                let c = self.to_px(p);
                if self.visible(c, DVec2::ZERO) {
                    let radius = 2.5 * self.ppp();
                    self.frame.arcs.push(ArcInstance {
                        center: to_f32(c),
                        params: [0.0, radius as f32, 0.0, TAU as f32],
                        color: self.palette.point,
                        pattern: [0.0; 4],
                    });
                }
            }
            Shape::Line { a, b } => {
                self.add_segment(self.to_px(a), self.to_px(b), color, half_width, dash, 0.0);
            }
            Shape::Circle { center, radius } => {
                self.add_arc(center, radius, 0.0, TAU, color, half_width, dash);
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let (start, sweep) = if sweep < 0.0 {
                    (start + sweep, -sweep)
                } else {
                    (start, sweep)
                };
                self.add_arc(
                    center,
                    radius,
                    start,
                    sweep.min(TAU),
                    color,
                    half_width,
                    dash,
                );
            }
        }
    }

    fn visible(&self, center: DVec2, half_extent: DVec2) -> bool {
        let lo = center - half_extent;
        let hi = center + half_extent;
        hi.x >= -self.half_size.x
            && lo.x <= self.half_size.x
            && hi.y >= -self.half_size.y
            && lo.y <= self.half_size.y
    }

    /// Add a segment in pixel space, clipped to the canvas. `distance` is how far along the
    /// original curve `a` lies, so dash patterns stay continuous across clipping and flattening.
    fn add_segment(
        &mut self,
        a: DVec2,
        b: DVec2,
        color: [u8; 4],
        half_width: f64,
        dash: [f64; 3],
        distance: f64,
    ) {
        let Some((t0, t1)) = clip_segment(a, b, self.half_size) else {
            return;
        };
        let d = b - a;
        let p0 = a + d * t0;
        let p1 = a + d * t1;
        self.frame.segments.push(SegmentInstance {
            p0: to_f32(p0),
            p1: to_f32(p1),
            color,
            half_width: half_width as f32,
            pattern: pattern(dash, distance + d.length() * t0),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn add_arc(
        &mut self,
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
        color: [u8; 4],
        half_width: f64,
        dash: [f64; 3],
    ) {
        let c = self.to_px(center);
        let r = radius * self.scale_px();
        if !self.visible(c, DVec2::splat(r + half_width)) {
            return;
        }
        if r <= MAX_SDF_RADIUS_PX {
            self.frame.arcs.push(ArcInstance {
                center: to_f32(c),
                params: [
                    r as f32,
                    half_width as f32,
                    start.rem_euclid(TAU) as f32,
                    sweep as f32,
                ],
                color,
                pattern: pattern(dash, 0.0),
            });
            return;
        }

        // Huge radius: only a sliver is on screen. Find the angular window that can intersect
        // the canvas and flatten just that part, in f64.
        let view_radius = self.half_size.length();
        let dist = c.length();
        if (dist - r).abs() > view_radius {
            return;
        }
        let cos_half =
            ((dist * dist + r * r - view_radius * view_radius) / (2.0 * dist * r)).clamp(-1.0, 1.0);
        let half_window = cos_half.acos();
        // Direction from the arc centre to the canvas centre. The pixel-space vector is `-c`;
        // flipping y gives world orientation (y-up).
        let towards_view = c.y.atan2(-c.x);
        let window_start = (towards_view - half_window - start).rem_euclid(TAU);
        let window_len = 2.0 * half_window;

        let step = 2.0 * (1.0 - FLATTEN_TOLERANCE_PX / r).clamp(-1.0, 1.0).acos();
        for offset in [0.0, -TAU] {
            let lo = (window_start + offset).max(0.0);
            let hi = (window_start + offset + window_len).min(sweep);
            if hi <= lo {
                continue;
            }
            let n = ((hi - lo) / step).ceil().clamp(1.0, 4096.0) as usize;
            let at = |t: f64| {
                let angle = start + t;
                c + r * DVec2::new(angle.cos(), -angle.sin())
            };
            let mut prev = at(lo);
            for i in 1..=n {
                let t = lo + (hi - lo) * i as f64 / n as f64;
                let next = at(t);
                let t_prev = lo + (hi - lo) * (i - 1) as f64 / n as f64;
                self.add_segment(prev, next, color, half_width, dash, t_prev * r);
                prev = next;
            }
        }
    }
}

/// Liang–Barsky clip of segment `a`→`b` against the box `[-half, half]`.
/// Returns the parameter range that remains visible.
fn clip_segment(a: DVec2, b: DVec2, half: DVec2) -> Option<(f64, f64)> {
    let d = b - a;
    let mut t0: f64 = 0.0;
    let mut t1: f64 = 1.0;
    for (p, q) in [
        (-d.x, a.x + half.x),
        (d.x, half.x - a.x),
        (-d.y, a.y + half.y),
        (d.y, half.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
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
    (t0 <= t1).then_some((t0, t1))
}

fn pattern(dash: [f64; 3], distance: f64) -> [f32; 4] {
    let [long, gap, dot] = dash;
    if long <= 0.0 {
        return [0.0; 4];
    }
    let period = if dot > 0.0 {
        long + 2.0 * gap + dot
    } else {
        long + gap
    };
    [
        long as f32,
        gap as f32,
        dot as f32,
        distance.rem_euclid(period) as f32,
    ]
}

fn to_f32(p: DVec2) -> [f32; 2] {
    [p.x as f32, p.y as f32]
}

fn rgba_f32(c: [u8; 4]) -> [f32; 4] {
    c.map(|v| f32::from(v) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Viewport {
        Viewport {
            size_px: DVec2::new(800.0, 600.0),
            pixels_per_point: 1.0,
            srgb_target: false,
        }
    }

    #[test]
    fn clip_keeps_inside_segment() {
        let half = DVec2::new(10.0, 10.0);
        assert_eq!(
            clip_segment(DVec2::new(-1.0, 0.0), DVec2::new(1.0, 0.0), half),
            Some((0.0, 1.0))
        );
    }

    #[test]
    fn clip_trims_long_segment() {
        let half = DVec2::new(10.0, 10.0);
        let (t0, t1) = clip_segment(DVec2::new(-30.0, 0.0), DVec2::new(30.0, 0.0), half).unwrap();
        assert!((t0 - 20.0 / 60.0).abs() < 1e-12);
        assert!((t1 - 40.0 / 60.0).abs() < 1e-12);
    }

    #[test]
    fn clip_rejects_outside_segment() {
        let half = DVec2::new(10.0, 10.0);
        assert_eq!(
            clip_segment(DVec2::new(-30.0, 20.0), DVec2::new(30.0, 20.0), half),
            None
        );
    }

    #[test]
    fn far_from_origin_geometry_is_exact_in_pixels() {
        // A 1 µm line a kilometre from the origin, zoomed in so it spans 100 px.
        let a = DVec2::new(1.0e6, 1.0e6);
        let b = a + DVec2::new(0.001, 0.0);
        let camera = Camera {
            center: (a + b) * 0.5,
            scale: 100_000.0,
        };
        let items = [SketchItem::new(Shape::Line { a, b })];
        let frame = build_frame(&items, &camera, viewport(), &Palette::dark());
        let seg = frame.segments[0];
        assert!((seg.p0[0] + 50.0).abs() < 1e-3, "{:?}", seg.p0);
        assert!((seg.p1[0] - 50.0).abs() < 1e-3, "{:?}", seg.p1);
    }

    #[test]
    fn huge_circle_is_flattened_and_stays_on_the_curve() {
        // Zoomed far into the top of a 1 m circle: radius is ~1e8 px.
        let radius = 1000.0;
        let camera = Camera {
            center: DVec2::new(0.0, radius),
            scale: 100_000.0,
        };
        let items = [SketchItem::new(Shape::Circle {
            center: DVec2::ZERO,
            radius,
        })];
        let frame = build_frame(&items, &camera, viewport(), &Palette::dark());
        assert!(frame.arcs.is_empty());
        assert!(!frame.segments.is_empty());
        let r_px = radius * camera.scale;
        for seg in &frame.segments {
            for p in [seg.p0, seg.p1] {
                // The circle passes through the canvas centre; in pixel space its centre is at
                // (0, r_px) (y-down), so every vertex must lie on that circle.
                let d = DVec2::new(f64::from(p[0]), f64::from(p[1]) - r_px).length();
                assert!((d - r_px).abs() < 0.5, "off curve by {}", d - r_px);
            }
        }
        // Covers the full canvas width.
        let min_x = frame
            .segments
            .iter()
            .map(|s| s.p0[0].min(s.p1[0]))
            .fold(f32::MAX, f32::min);
        let max_x = frame
            .segments
            .iter()
            .map(|s| s.p0[0].max(s.p1[0]))
            .fold(f32::MIN, f32::max);
        assert!(min_x <= -400.0 && max_x >= 400.0, "{min_x}..{max_x}");
    }

    #[test]
    fn offscreen_shapes_are_culled() {
        let camera = Camera::default();
        let items = [
            SketchItem::new(Shape::Circle {
                center: DVec2::new(1.0e4, 0.0),
                radius: 1.0,
            }),
            SketchItem::new(Shape::Line {
                a: DVec2::new(1.0e4, 0.0),
                b: DVec2::new(1.0e4, 10.0),
            }),
        ];
        let frame = build_frame(&items, &camera, viewport(), &Palette::dark());
        assert!(frame.arcs.is_empty() && frame.segments.is_empty());
    }

    #[test]
    fn grid_spacing_is_power_of_ten_and_large_enough() {
        for scale in [1e-4, 0.37, 4.0, 123.0, 1e6] {
            let camera = Camera {
                center: DVec2::ZERO,
                scale,
            };
            let grid = grid_info(&camera);
            let on_screen = grid.minor * scale;
            assert!(on_screen >= MIN_GRID_SPACING_PT - 1e-9);
            assert!(on_screen < MIN_GRID_SPACING_PT * 10.0 + 1e-9);
            assert!((grid.minor.log10() - grid.minor.log10().round()).abs() < 1e-9);
        }
    }
}
