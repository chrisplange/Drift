//! Builds per-frame GPU data from world geometry.
//!
//! Everything is computed in `f64` relative to the camera and only converted to `f32` at the
//! end, in physical pixels relative to the canvas centre (y-down). The GPU never sees large
//! world coordinates, which keeps rendering exact at any zoom and any distance from the origin.

use std::f64::consts::TAU;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use drift_core::{DVec2, DofState, Emphasis, LineKind, LineType, Shape, SketchItem};

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

/// A filled-triangle vertex (solid fills and hatch backgrounds). Positions are physical
/// pixels relative to the canvas centre.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FillVertex {
    pub pos: [f32; 2],
    pub color: [u8; 4],
}

/// A picture to draw: RGBA pixels (sRGB, not premultiplied, rows from the top) on the
/// parallelogram with these world corners: top-left, top-right, bottom-right, bottom-left.
#[derive(Clone, Debug)]
pub struct CanvasImage {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub corners: [DVec2; 4],
    pub opacity: f32,
}

/// A vertex of a picture: where it is (physical pixels relative to the canvas centre), where
/// in the picture (0–1 across and down), and how opaque.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ImageVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub alpha: f32,
}

/// A picture as this frame draws it: its pixels, and triangles over the part that shows.
#[derive(Clone, Debug)]
pub struct FrameImage {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub vertices: Vec<ImageVertex>,
}

/// A triangulated area in world coordinates, with an sRGB colour and alpha.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FillMesh {
    pub vertices: Vec<DVec2>,
    /// Triangles, three indices each.
    pub indices: Vec<u32>,
    pub color: [u8; 4],
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
    /// Use imperial grid steps (fractions of an inch, inches, feet) instead of powers of ten.
    pub imperial_grid: bool,
    /// Draw every curve one device pixel wide, whatever its weight (Revit's thin lines).
    pub thin_lines: bool,
    /// Draw the grid lines.
    pub show_grid: bool,
    /// Draw the x and y axes through the origin.
    pub show_axes: bool,
}

/// Everything needed to draw one frame of the canvas.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub globals: Globals,
    /// Filled triangles, drawn under all curves.
    pub fills: Vec<FillVertex>,
    /// Pictures (underlays'), drawn over the fills and under all curves.
    pub images: Vec<FrameImage>,
    /// Curves drawn under all the others (hatch lines, underlays): arcs and lines are drawn
    /// in separate passes, so order within one list can't keep them below.
    pub under_segments: Vec<SegmentInstance>,
    pub under_arcs: Vec<ArcInstance>,
    pub segments: Vec<SegmentInstance>,
    pub arcs: Vec<ArcInstance>,
    /// Draw groups, bottom first: each a filled area and the curves drawn right over it
    /// (its hatch lines and outline), as ranges of `fills`, `arcs` and `segments`. Each is
    /// drawn over every group before it, so a fill covers the curves of the groups beneath.
    /// With no groups, every fill is drawn under every curve.
    pub groups: Vec<GroupRanges>,
}

/// What one draw group covers of a frame's `fills`, `arcs` and `segments`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupRanges {
    pub fills: std::ops::Range<u32>,
    pub arcs: std::ops::Range<u32>,
    pub segments: std::ops::Range<u32>,
}

/// A draw group to add to a frame: filled areas, and the curves drawn right over them.
#[derive(Clone, Copy, Debug)]
pub struct DrawGroup<'a> {
    pub fills: &'a [FillMesh],
    pub items: &'a [SketchItem],
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

/// Add items' curves and points to `frame`'s `arcs` and `segments`.
fn build_into(
    frame: &mut Frame,
    items: &[SketchItem],
    camera: &Camera,
    viewport: Viewport,
    palette: &Palette,
) {
    let mut builder = Builder {
        camera,
        viewport,
        palette,
        half_size: viewport.size_px * 0.5 + DVec2::splat(CULL_MARGIN_PX),
        frame: std::mem::take(frame),
    };
    for item in items {
        builder.add(item);
    }
    *frame = builder.frame;
}

impl Frame {
    /// Add a draw group: filled areas, then curves over them (hatch lines, outlines), all
    /// drawn over every group added before.
    pub fn push_group(
        &mut self,
        meshes: &[FillMesh],
        items: &[SketchItem],
        camera: &Camera,
        viewport: Viewport,
        palette: &Palette,
    ) {
        let fills = self.fills.len() as u32;
        self.push_fills(meshes, camera, viewport);
        let arcs = self.arcs.len() as u32;
        let segments = self.segments.len() as u32;
        build_into(self, items, camera, viewport, palette);
        self.groups.push(GroupRanges {
            fills: fills..self.fills.len() as u32,
            arcs: arcs..self.arcs.len() as u32,
            segments: segments..self.segments.len() as u32,
        });
    }

    /// Add curves drawn under all the frame's others (after fills, before the sketch).
    pub fn push_under(
        &mut self,
        items: &[SketchItem],
        camera: &Camera,
        viewport: Viewport,
        palette: &Palette,
    ) {
        let under = build_frame(items, camera, viewport, palette);
        self.under_segments.extend(under.segments);
        self.under_arcs.extend(under.arcs);
    }

    /// Add pictures (drawn over the fills and under the curves, in the order given), each
    /// cut to the canvas as fills are, so far-off corners never reach the GPU.
    pub fn push_images(&mut self, images: &[CanvasImage], camera: &Camera, viewport: Viewport) {
        let ppp = viewport.pixels_per_point;
        let half = viewport.size_px * 0.5 + DVec2::splat(CULL_MARGIN_PX);
        for image in images {
            if image.width == 0 || image.height == 0 || image.opacity <= 0.0 {
                continue;
            }
            let px = image.corners.map(|p| camera.world_to_view(p) * ppp);
            // Where in the picture a point is: solve p = p0 + u·across + v·down.
            let (p0, across, down) = (px[0], px[1] - px[0], px[3] - px[0]);
            let det = across.perp_dot(down);
            if det.abs() < 1e-9 || !det.is_finite() {
                continue;
            }
            let uv = |p: DVec2| {
                let q = p - p0;
                [
                    (q.perp_dot(down) / det) as f32,
                    (across.perp_dot(q) / det) as f32,
                ]
            };
            let poly = clip_polygon(&px, half);
            if poly.len() < 3 {
                continue;
            }
            let mut vertices = Vec::with_capacity((poly.len() - 2) * 3);
            for k in 1..poly.len() - 1 {
                for p in [poly[0], poly[k], poly[k + 1]] {
                    vertices.push(ImageVertex {
                        pos: to_f32(p),
                        uv: uv(p),
                        alpha: image.opacity.clamp(0.0, 1.0),
                    });
                }
            }
            self.images.push(FrameImage {
                pixels: image.pixels.clone(),
                width: image.width,
                height: image.height,
                vertices,
            });
        }
    }

    /// Add filled areas (drawn under the curves, in the order given). World coordinates are
    /// made camera-relative in `f64` like everything else, and each triangle is clipped to the
    /// canvas there: one far off screen (or out at 1e150) would otherwise reach the GPU with
    /// vertices beyond `f32`, which can hang it.
    pub fn push_fills(&mut self, meshes: &[FillMesh], camera: &Camera, viewport: Viewport) {
        let ppp = viewport.pixels_per_point;
        let half = viewport.size_px * 0.5 + DVec2::splat(CULL_MARGIN_PX);
        for mesh in meshes {
            let px: Vec<DVec2> = mesh
                .vertices
                .iter()
                .map(|p| camera.world_to_view(*p) * ppp)
                .collect();
            for t in mesh.indices.as_chunks::<3>().0 {
                let Some(tri) = t
                    .iter()
                    .map(|&i| px.get(i as usize).copied())
                    .collect::<Option<Vec<DVec2>>>()
                else {
                    continue;
                };
                let inside = tri
                    .iter()
                    .all(|p| p.x.abs() <= half.x && p.y.abs() <= half.y);
                let poly = if inside {
                    tri
                } else {
                    clip_polygon(&tri, half)
                };
                // A fan over the (convex) clipped polygon.
                for k in 1..poly.len().saturating_sub(1) {
                    for p in [poly[0], poly[k], poly[k + 1]] {
                        self.fills.push(FillVertex {
                            pos: to_f32(p),
                            color: mesh.color,
                        });
                    }
                }
            }
        }
    }
}

/// Imperial grid steps in inches: (minor, major).
const IMPERIAL_STEPS: [(f64, f64); 9] = [
    (1.0 / 64.0, 1.0 / 8.0),
    (1.0 / 32.0, 1.0 / 4.0),
    (1.0 / 16.0, 1.0),
    (1.0 / 8.0, 1.0),
    (1.0 / 4.0, 1.0),
    (1.0 / 2.0, 6.0),
    (1.0, 12.0),
    (6.0, 60.0),
    (12.0, 120.0),
];

/// Grid spacing for a camera: the smallest step that is at least [`MIN_GRID_SPACING_PT`] on
/// screen. Metric steps are powers of ten (major every 10); imperial steps follow
/// [`IMPERIAL_STEPS`] up to a foot, then powers of ten feet.
pub fn grid_info(camera: &Camera, imperial: bool) -> GridInfo {
    let min = MIN_GRID_SPACING_PT / camera.scale;
    if imperial {
        const INCH: f64 = 25.4;
        if let Some(&(minor, major)) = IMPERIAL_STEPS.iter().find(|(m, _)| m * INCH >= min) {
            return GridInfo {
                minor: minor * INCH,
                major: major * INCH,
            };
        }
        let feet = 10f64.powf((min / (12.0 * INCH)).log10().ceil());
        return GridInfo {
            minor: feet * 12.0 * INCH,
            major: feet * 120.0 * INCH,
        };
    }
    let minor = 10f64.powf(min.log10().ceil());
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
        let grid = grid_info(self.camera, self.viewport.imperial_grid);
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
            grid_minor: [
                minor_px as f32,
                minor_x,
                minor_y,
                if self.viewport.show_grid {
                    minor_alpha as f32
                } else {
                    0.0
                },
            ],
            grid_major: [
                (grid.major * scale_px) as f32,
                major_x,
                major_y,
                if self.viewport.show_grid { 1.0 } else { 0.0 },
            ],
            axes: [
                origin.x as f32,
                origin.y as f32,
                if self.viewport.show_axes { 1.0 } else { 0.0 },
                0.0,
            ],
            background: rgba_f32(palette.background),
            minor_color: rgba_f32(palette.grid_minor),
            major_color: rgba_f32(palette.grid_major),
            axis_x_color: rgba_f32(palette.axis_x),
            axis_y_color: rgba_f32(palette.axis_y),
        }
    }

    fn style(&self, item: &SketchItem) -> ([u8; 4], f64, [f64; 3]) {
        let ppp = self.ppp();
        let p = self.palette;
        let solid = match item.dof {
            DofState::Under => p.under,
            DofState::Over => p.over,
            DofState::Full | DofState::Unknown => p.solid,
        };
        let custom = item.color.map(|[r, g, b]| [r, g, b, 0xff]);
        // Weights show at about twice their printed size on a 96 dpi screen, so they read
        // clearly; thin lines draws everything one pixel wide.
        let weighted = if self.viewport.thin_lines {
            0.5 * ppp
        } else if item.weight > 0 {
            (f64::from(item.weight) / 100.0 * 96.0 / 25.4).max(0.8) * ppp
        } else {
            0.8 * ppp
        };
        let scale = (weighted / (0.8 * ppp)).max(1.0);
        let dashes = |d: [f64; 3]| [d[0] * ppp * scale, d[1] * ppp * scale, d[2] * ppp * scale];
        let (color, half_width, dash) = match item.kind {
            LineKind::Solid => (
                custom.unwrap_or(solid),
                weighted,
                match item.line_type {
                    LineType::Continuous => [0.0; 3],
                    LineType::Dashed => dashes([6.0, 3.0, 0.0]),
                    LineType::Hidden => dashes([3.0, 2.0, 0.0]),
                    LineType::Center => dashes([16.0, 3.0, 2.0]),
                    LineType::Phantom => dashes([16.0, 3.0, -2.0]),
                },
            ),
            LineKind::Construction => (
                custom.unwrap_or(p.construction),
                0.5 * ppp,
                [6.0 * ppp, 4.0 * ppp, 0.0],
            ),
            LineKind::Center => (
                custom.unwrap_or(p.center),
                0.5 * ppp,
                [16.0 * ppp, 3.0 * ppp, 2.0 * ppp],
            ),
        };
        // Conflicts show in red whatever the line style.
        let color = if item.dof == DofState::Over {
            p.over
        } else {
            color
        };
        match item.emphasis {
            Emphasis::Normal => (color, half_width, dash),
            Emphasis::Hovered => (p.hover, half_width + 0.5 * ppp, dash),
            Emphasis::Selected => (p.selected, half_width + 0.5 * ppp, dash),
            Emphasis::Preview => (p.preview, half_width, dash),
            Emphasis::Remove => (p.over, half_width + 0.7 * ppp, [5.0 * ppp, 3.0 * ppp, 0.0]),
        }
    }

    fn point_style(&self, item: &SketchItem) -> ([u8; 4], f64) {
        let ppp = self.ppp();
        let p = self.palette;
        let state = match item.dof {
            DofState::Under => Some(p.under),
            DofState::Over => Some(p.over),
            DofState::Full => Some(p.solid),
            DofState::Unknown => None,
        };
        let (color, radius) = if item.vertex {
            (state.unwrap_or(p.solid), 2.25 * ppp)
        } else {
            (state.unwrap_or(p.point), 2.75 * ppp)
        };
        match item.emphasis {
            Emphasis::Normal => (color, radius),
            Emphasis::Hovered => (p.point_hover, 4.0 * ppp),
            Emphasis::Selected => (p.selected, 3.5 * ppp),
            Emphasis::Preview => (p.preview, radius),
            Emphasis::Remove => (p.over, radius),
        }
    }

    fn add(&mut self, item: &SketchItem) {
        let (color, half_width, dash) = self.style(item);
        match item.shape {
            Shape::Point(p) => {
                let c = self.to_px(p);
                if self.visible(c, DVec2::ZERO) {
                    let (color, radius) = self.point_style(item);
                    if item.emphasis == Emphasis::Hovered {
                        // A ring around the lit-up point, so it reads at a glance.
                        let ppp = self.ppp();
                        self.frame.arcs.push(ArcInstance {
                            center: to_f32(c),
                            params: [
                                (radius + 2.5 * ppp) as f32,
                                (0.9 * ppp) as f32,
                                0.0,
                                TAU as f32,
                            ],
                            color: self.palette.selected,
                            pattern: [0.0; 4],
                        });
                    }
                    self.frame.arcs.push(ArcInstance {
                        center: to_f32(c),
                        params: [0.0, radius as f32, 0.0, TAU as f32],
                        color,
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
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                // Flattened in pixel space, finely enough to look smooth at this zoom.
                let b = item.shape.bounds();
                let (lo, hi) = (self.to_px(b.min), self.to_px(b.max));
                let mid = (lo + hi) * 0.5;
                if !self.visible(mid, (hi - lo).abs() * 0.5 + DVec2::splat(half_width)) {
                    return;
                }
                let span = (hi - lo).abs();
                let around = std::f64::consts::PI * (span.x + span.y) * 0.5 * (sweep / TAU);
                // About one segment per 4 px of curve: smooth, but no more than the screen needs.
                let n = (around / 4.0).clamp(12.0, 360.0) as usize;
                let pts = drift_core::shape::ellipse_steps(center, major, minor, start, sweep, n);
                let mut distance = 0.0;
                for w in pts.windows(2) {
                    let (a, b) = (self.to_px(w[0]), self.to_px(w[1]));
                    self.add_segment(a, b, color, half_width, dash, distance);
                    distance += a.distance(b);
                }
            }
            Shape::Bezier { p } => {
                let b = item.shape.bounds();
                let (lo, hi) = (self.to_px(b.min), self.to_px(b.max));
                let mid = (lo + hi) * 0.5;
                if !self.visible(mid, (hi - lo).abs() * 0.5 + DVec2::splat(half_width)) {
                    return;
                }
                // About one segment per 4 px of control polygon (which is longer than the curve).
                let polygon: f64 = p
                    .windows(2)
                    .map(|w| self.to_px(w[0]).distance(self.to_px(w[1])))
                    .sum();
                let n = (polygon / 4.0).clamp(4.0, 128.0) as usize;
                let pts = drift_core::shape::bezier_steps(&p, n);
                let mut distance = 0.0;
                for w in pts.windows(2) {
                    let (a, b) = (self.to_px(w[0]), self.to_px(w[1]));
                    self.add_segment(a, b, color, half_width, dash, distance);
                    distance += a.distance(b);
                }
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

/// Sutherland–Hodgman clip of a polygon against the box `[-half, half]`. Empty when nothing
/// of it is inside, or a corner isn't finite. A convex polygon stays convex; a concave one
/// may gain zero-width bridges along the box's edges, which fill nothing.
pub fn clip_polygon(poly: &[DVec2], half: DVec2) -> Vec<DVec2> {
    if poly.iter().any(|p| !p.is_finite()) {
        return Vec::new();
    }
    let mut out = poly.to_vec();
    // Each edge of the box as (axis, sign): keep `sign * p[axis] <= half[axis]`.
    for (axis, sign) in [(0, 1.0), (0, -1.0), (1, 1.0), (1, -1.0)] {
        let input = std::mem::take(&mut out);
        let Some(&last) = input.last() else {
            break;
        };
        let d = |p: DVec2| sign * p[axis] - half[axis];
        let mut prev = last;
        for &p in &input {
            let (dp, dq) = (d(prev), d(p));
            if dq <= 0.0 {
                if dp > 0.0 {
                    out.push(prev + (p - prev) * (dp / (dp - dq)));
                }
                out.push(p);
            } else if dp <= 0.0 {
                out.push(prev + (p - prev) * (dp / (dp - dq)));
            }
            prev = p;
        }
    }
    out
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
            imperial_grid: false,
            thin_lines: false,
            show_grid: true,
            show_axes: true,
        }
    }

    #[test]
    fn imperial_grid_uses_inch_fractions_and_feet() {
        for scale in [1e-4, 0.05, 0.37, 4.0, 123.0, 1e6] {
            let camera = Camera {
                center: DVec2::ZERO,
                scale,
            };
            let grid = grid_info(&camera, true);
            assert!(grid.minor * scale >= MIN_GRID_SPACING_PT - 1e-9 || grid.minor == 25.4 / 64.0);
            assert!(grid.major > grid.minor);
            // Every step is a whole number of 1/64 inches.
            let n = grid.minor / 25.4 * 64.0;
            assert!((n - n.round()).abs() < 1e-6, "{}", grid.minor);
        }
        let camera = Camera {
            center: DVec2::ZERO,
            scale: 1.0,
        };
        assert_eq!(grid_info(&camera, true).minor, 12.7); // 1/2"
    }

    /// A fill far bigger than `f32` reaches the GPU cut to the canvas, with finite corners
    /// (issue #82: a 1e150 fill hung the GPU).
    #[test]
    fn huge_fill_is_clipped_to_the_canvas() {
        let camera = Camera {
            center: DVec2::new(1e150 * 0.5, 0.0),
            scale: 1.0,
        };
        let mesh = FillMesh {
            vertices: vec![
                DVec2::ZERO,
                DVec2::new(1e150, 0.0),
                DVec2::new(1e150, 1e150),
                DVec2::new(0.0, 1e150),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            color: [255; 4],
        };
        let mut frame = Frame::default();
        frame.push_fills(&[mesh], &camera, viewport());
        assert!(!frame.fills.is_empty());
        assert_eq!(frame.fills.len() % 3, 0);
        for v in &frame.fills {
            assert!(
                v.pos[0].abs() <= 410.0 && v.pos[1].abs() <= 310.0,
                "{:?}",
                v.pos
            );
        }
        // Wholly off screen, or not finite: nothing.
        let mut frame = Frame::default();
        let far = FillMesh {
            vertices: vec![
                DVec2::splat(1e300),
                DVec2::new(f64::INFINITY, 0.0),
                DVec2::ZERO,
            ],
            indices: vec![0, 1, 2],
            color: [255; 4],
        };
        frame.push_fills(&[far], &camera, viewport());
        assert!(frame.fills.is_empty());
    }

    #[test]
    fn clip_triangle_inside_is_kept() {
        let tri = [DVec2::ZERO, DVec2::new(1.0, 0.0), DVec2::new(0.0, 1.0)];
        assert_eq!(clip_polygon(&tri, DVec2::splat(10.0)), tri.to_vec());
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
            let grid = grid_info(&camera, false);
            let on_screen = grid.minor * scale;
            assert!(on_screen >= MIN_GRID_SPACING_PT - 1e-9);
            assert!(on_screen < MIN_GRID_SPACING_PT * 10.0 + 1e-9);
            assert!((grid.minor.log10() - grid.minor.log10().round()).abs() < 1e-9);
        }
    }

    /// Stacked draw groups: each group covers just its own fills and curves, in order, and a
    /// frame built the plain way has none.
    #[test]
    fn draw_groups_cover_their_own_fills_and_curves() {
        let camera = Camera::default();
        let palette = Palette::light();
        let square = FillMesh {
            vertices: vec![
                DVec2::ZERO,
                DVec2::new(10.0, 0.0),
                DVec2::new(10.0, 10.0),
                DVec2::new(0.0, 10.0),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            color: [255; 4],
        };
        let line = SketchItem::new(Shape::Line {
            a: DVec2::ZERO,
            b: DVec2::new(10.0, 10.0),
        });
        let circle = SketchItem::new(Shape::Circle {
            center: DVec2::ZERO,
            radius: 5.0,
        });
        let mut frame = Frame::default();
        frame.push_group(
            std::slice::from_ref(&square),
            &[line, line],
            &camera,
            viewport(),
            &palette,
        );
        frame.push_group(&[], &[circle], &camera, viewport(), &palette);
        frame.push_group(&[square], &[], &camera, viewport(), &palette);
        let [a, b, c] = frame.groups.as_slice() else {
            panic!("three groups");
        };
        assert_eq!(
            (a.fills.clone(), a.arcs.clone(), a.segments.clone()),
            (0..6, 0..0, 0..2)
        );
        assert_eq!(
            (b.fills.clone(), b.arcs.clone(), b.segments.clone()),
            (6..6, 0..1, 2..2)
        );
        assert_eq!(
            (c.fills.clone(), c.arcs.clone(), c.segments.clone()),
            (6..12, 1..1, 2..2)
        );
        assert_eq!(frame.fills.len(), 12);
        assert!(
            build_frame(&[line], &camera, viewport(), &palette)
                .groups
                .is_empty()
        );
    }
}
