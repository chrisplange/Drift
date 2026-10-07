//! Screen-space overlays drawn with egui on top of the GPU canvas: snap markers, the
//! selection box and cursor labels. Geometry itself is always drawn by the GPU renderer.

use drift_core::{Bounds, DVec2};
use drift_geom::{Snap, SnapKind};
use drift_render::Camera;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, vec2};

/// Converts world positions to screen positions for one canvas rectangle.
pub struct ScreenMap {
    pub rect: Rect,
    pub camera: Camera,
}

impl ScreenMap {
    pub fn to_screen(&self, p: DVec2) -> Pos2 {
        let v = self.camera.world_to_view(p);
        self.rect.center() + vec2(v.x as f32, v.y as f32)
    }

    pub fn to_world(&self, pos: Pos2) -> DVec2 {
        let v = pos - self.rect.center();
        self.camera
            .view_to_world(DVec2::new(f64::from(v.x), f64::from(v.y)))
    }
}

pub struct OverlayColors {
    pub snap: Color32,
    pub window: Color32,
    pub crossing: Color32,
    pub label_bg: Color32,
    pub label_fg: Color32,
}

impl OverlayColors {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                snap: Color32::from_rgb(0xff, 0x9f, 0x1c),
                window: Color32::from_rgb(0x2f, 0x9b, 0xff),
                crossing: Color32::from_rgb(0x4c, 0xc3, 0x6b),
                label_bg: Color32::from_rgba_unmultiplied(0x10, 0x12, 0x15, 0xe0),
                label_fg: Color32::from_rgb(0xe6, 0xe8, 0xeb),
            }
        } else {
            Self {
                snap: Color32::from_rgb(0xe0, 0x6c, 0x00),
                window: Color32::from_rgb(0x0b, 0x6b, 0xde),
                crossing: Color32::from_rgb(0x1f, 0x9a, 0x45),
                label_bg: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0xe8),
                label_fg: Color32::from_rgb(0x1d, 0x21, 0x26),
            }
        }
    }
}

/// Draw the glyph for a snap target: square for points, circle for centres, triangle for
/// midpoints, diamond for quadrants, cross for on-curve, plus for grid.
pub fn snap_marker(painter: &egui::Painter, map: &ScreenMap, snap: &Snap, color: Color32) {
    let c = map.to_screen(snap.at);
    let s = 5.0;
    let stroke = Stroke::new(1.5, color);
    match snap.kind {
        SnapKind::Endpoint | SnapKind::Point => {
            painter.rect_stroke(
                Rect::from_center_size(c, vec2(2.0 * s, 2.0 * s)),
                0.0,
                stroke,
                StrokeKind::Middle,
            );
        }
        SnapKind::Center => {
            painter.circle_stroke(c, s, stroke);
            painter.line_segment([c - vec2(s + 3.0, 0.0), c + vec2(s + 3.0, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, s + 3.0), c + vec2(0.0, s + 3.0)], stroke);
        }
        SnapKind::Midpoint => {
            let pts = vec![
                c + vec2(0.0, -s - 1.0),
                c + vec2(s + 1.0, s),
                c + vec2(-s - 1.0, s),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
        SnapKind::Quadrant => {
            let pts = vec![
                c + vec2(0.0, -s - 1.0),
                c + vec2(s + 1.0, 0.0),
                c + vec2(0.0, s + 1.0),
                c + vec2(-s - 1.0, 0.0),
            ];
            painter.add(egui::Shape::closed_line(pts, stroke));
        }
        SnapKind::OnCurve => {
            painter.line_segment([c + vec2(-s, -s), c + vec2(s, s)], stroke);
            painter.line_segment([c + vec2(-s, s), c + vec2(s, -s)], stroke);
        }
        SnapKind::Grid => {
            painter.line_segment([c - vec2(s, 0.0), c + vec2(s, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, s), c + vec2(0.0, s)], stroke);
        }
    }
}

/// Window selection draws a solid blue box; crossing selection a dashed green one.
pub fn selection_box(
    painter: &egui::Painter,
    map: &ScreenMap,
    rect: &Bounds,
    crossing: bool,
    colors: &OverlayColors,
) {
    let r = Rect::from_two_pos(map.to_screen(rect.min), map.to_screen(rect.max));
    let color = if crossing {
        colors.crossing
    } else {
        colors.window
    };
    painter.rect_filled(r, 0.0, color.gamma_multiply(0.12));
    let stroke = Stroke::new(1.0, color);
    if crossing {
        let path = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        painter.extend(egui::Shape::dashed_line(&path, stroke, 6.0, 4.0));
    } else {
        painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
    }
}

/// A modal move: a ring at the base point and a dashed line to where it has moved.
pub fn move_guide(
    painter: &egui::Painter,
    map: &ScreenMap,
    base: DVec2,
    to: Option<DVec2>,
    colors: &OverlayColors,
) {
    let b = map.to_screen(base);
    let stroke = Stroke::new(1.5, colors.snap);
    painter.circle_stroke(b, 6.0, stroke);
    painter.circle_filled(b, 2.0, colors.snap);
    if let Some(to) = to {
        let t = map.to_screen(to);
        painter.extend(egui::Shape::dashed_line(&[b, t], stroke, 5.0, 3.0));
        painter.circle_filled(t, 3.0, colors.snap);
    }
}

/// Fill a region (outline and holes) with a translucent colour.
pub fn region_fill(
    painter: &egui::Painter,
    map: &ScreenMap,
    region: &drift_geom::regions::Region,
    color: Color32,
) {
    use lyon_tessellation::geometry_builder::simple_builder;
    use lyon_tessellation::math::point;
    use lyon_tessellation::path::Path;
    use lyon_tessellation::{FillOptions, FillTessellator, VertexBuffers};

    // Flatten arcs finely enough on screen (about 2 px of sagitta at most).
    let max_angle = 0.15;
    let mut builder = Path::builder();
    for lp in std::iter::once(&region.outer).chain(&region.holes) {
        let poly = lp.polygon(max_angle);
        let Some(first) = poly.first() else {
            continue;
        };
        let to = |p: DVec2| {
            let s = map.to_screen(p);
            point(s.x, s.y)
        };
        builder.begin(to(*first));
        for p in &poly[1..] {
            builder.line_to(to(*p));
        }
        builder.close();
    }
    let path = builder.build();
    let mut buffers: VertexBuffers<lyon_tessellation::math::Point, u16> = VertexBuffers::new();
    let ok = FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::default().with_fill_rule(lyon_tessellation::FillRule::EvenOdd),
            &mut simple_builder(&mut buffers),
        )
        .is_ok();
    if !ok {
        return;
    }
    let mut mesh = egui::Mesh::default();
    for v in &buffers.vertices {
        mesh.colored_vertex(egui::pos2(v.x, v.y), color);
    }
    mesh.indices = buffers.indices.iter().map(|i| u32::from(*i)).collect();
    painter.add(egui::Shape::mesh(mesh));
}

/// A small text label with a background, offset from the cursor.
pub fn cursor_label(painter: &egui::Painter, at: Pos2, lines: &[String], colors: &OverlayColors) {
    if lines.is_empty() {
        return;
    }
    let text = lines.join("\n");
    let font = FontId::monospace(11.0);
    let galley = painter.layout_no_wrap(text, font, colors.label_fg);
    let pos = at + vec2(16.0, 16.0);
    let bg = Rect::from_min_size(pos, galley.size()).expand(3.0);
    painter.rect_filled(bg, 3.0, colors.label_bg);
    painter.galley(pos, galley, colors.label_fg);
}

/// Centered hint text, wrapped to the canvas width, e.g. for an empty document.
pub fn canvas_hint(
    painter: &egui::Painter,
    rect: Rect,
    title: &str,
    lines: &[&str],
    color: Color32,
) {
    let wrap = (rect.width() - 48.0).clamp(80.0, 420.0);
    let title = painter.layout(
        title.to_owned(),
        FontId::new(20.0, crate::theme::bold()),
        color,
        wrap,
    );
    let body: Vec<_> = lines
        .iter()
        .map(|l| painter.layout((*l).to_owned(), FontId::proportional(13.5), color, wrap))
        .collect();
    let height = title.size().y + 10.0 + body.iter().map(|g| g.size().y + 4.0).sum::<f32>();
    let mut y = rect.center().y - height / 2.0;
    painter.galley(
        egui::pos2(rect.center().x - title.size().x / 2.0, y),
        title.clone(),
        color,
    );
    y += title.size().y + 10.0;
    for g in body {
        painter.galley(
            egui::pos2(rect.center().x - g.size().x / 2.0, y),
            g.clone(),
            color,
        );
        y += g.size().y + 4.0;
    }
}

/// A transient message at the bottom of the canvas (macOS-style toast), fading out as
/// `fade` goes from 1 to 0.
pub fn toast(painter: &egui::Painter, rect: Rect, text: &str, error: bool, dark: bool, fade: f32) {
    let fg = if dark {
        Color32::from_rgb(0xee, 0xee, 0xf0)
    } else {
        Color32::from_rgb(0x1d, 0x1d, 0x1f)
    };
    let bg = if dark {
        Color32::from_rgb(0x2e, 0x30, 0x36)
    } else {
        Color32::WHITE
    };
    let wrap = (rect.width() * 0.7).clamp(120.0, 560.0);
    let galley = painter.layout(
        text.to_owned(),
        FontId::proportional(13.0),
        fg.gamma_multiply(fade),
        wrap,
    );
    let size = galley.size() + vec2(44.0, 18.0);
    let at = egui::pos2(
        rect.center().x - size.x / 2.0,
        rect.bottom() - size.y - 24.0,
    );
    let r = Rect::from_min_size(at, size);
    painter.add(egui::Shape::Rect(egui::epaint::RectShape::filled(
        r.translate(vec2(0.0, 3.0)),
        12.0,
        Color32::from_black_alpha((50.0 * fade) as u8),
    )));
    painter.rect_filled(r, 12.0, bg.gamma_multiply(fade));
    painter.rect_stroke(
        r,
        12.0,
        Stroke::new(
            1.0,
            Color32::from_gray(if dark { 0x44 } else { 0xd8 }).gamma_multiply(fade),
        ),
        StrokeKind::Inside,
    );
    let dot = if error {
        Color32::from_rgb(0xff, 0x45, 0x3a)
    } else {
        crate::theme::accent(dark)
    };
    painter.circle_filled(
        r.left_center() + vec2(16.0, 0.0),
        4.0,
        dot.gamma_multiply(fade),
    );
    painter.galley(
        r.left_top() + vec2(28.0, 9.0),
        galley,
        fg.gamma_multiply(fade),
    );
}
