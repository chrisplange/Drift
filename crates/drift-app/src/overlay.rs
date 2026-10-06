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
pub fn canvas_hint(painter: &egui::Painter, rect: Rect, text: &str, color: Color32) {
    let wrap = (rect.width() - 32.0).max(80.0);
    let galley = painter.layout(text.to_owned(), FontId::proportional(15.0), color, wrap);
    let pos = rect.center() - galley.size() / 2.0;
    painter.galley(pos, galley, color);
}
