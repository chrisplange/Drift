//! Vector icons for tools and view toggles, drawn with the painter so they stay crisp at any
//! scale and follow the theme.

use std::f32::consts::{FRAC_PI_2, PI};

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::tools::ToolKind;

/// Icons for view toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Grid,
    Snap,
    Thin,
    Axes,
    Eye,
    Lock,
    Unlock,
    Print,
    /// Constraint status colours: a curve with a fixed and a free end.
    Status,
    /// End points always shown: a line with dots at its ends.
    Endpoints,
}

/// Maps a 20 × 20 design grid into `rect`.
struct Pen<'a> {
    painter: &'a egui::Painter,
    rect: Rect,
    stroke: Stroke,
    color: Color32,
}

impl Pen<'_> {
    fn p(&self, x: f32, y: f32) -> Pos2 {
        let s = self.rect.width().min(self.rect.height()) / 20.0;
        let o = self.rect.center() - vec2(10.0 * s, 10.0 * s);
        pos2(o.x + x * s, o.y + y * s)
    }

    fn s(&self, v: f32) -> f32 {
        self.rect.width().min(self.rect.height()) / 20.0 * v
    }

    fn line(&self, a: (f32, f32), b: (f32, f32)) {
        self.painter
            .line_segment([self.p(a.0, a.1), self.p(b.0, b.1)], self.stroke);
    }

    fn poly(&self, pts: &[(f32, f32)], closed: bool) {
        let pts: Vec<Pos2> = pts.iter().map(|(x, y)| self.p(*x, *y)).collect();
        self.painter.add(if closed {
            Shape::closed_line(pts, self.stroke)
        } else {
            Shape::line(pts, self.stroke)
        });
    }

    fn dot(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_filled(self.p(x, y), self.s(r), self.color);
    }

    fn circle(&self, x: f32, y: f32, r: f32) {
        self.painter
            .circle_stroke(self.p(x, y), self.s(r), self.stroke);
    }

    /// Arc around (x, y), screen angles (y down), from `a0` sweeping `sweep`.
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, sweep: f32) {
        let n = 24;
        let pts: Vec<Pos2> = (0..=n)
            .map(|k| {
                let a = a0 + sweep * k as f32 / n as f32;
                self.p(x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.painter.add(Shape::line(pts, self.stroke));
    }

    fn dashed(&self, a: (f32, f32), b: (f32, f32)) {
        let pts = [self.p(a.0, a.1), self.p(b.0, b.1)];
        self.painter.extend(Shape::dashed_line(
            &pts,
            self.stroke,
            self.s(2.2),
            self.s(1.8),
        ));
    }

    fn arrow_head(&self, tip: (f32, f32), dir: Vec2) {
        let d = dir.normalized();
        let n = d.rot90();
        let t = self.p(tip.0, tip.1);
        let back = t - d * self.s(3.2);
        self.painter.add(Shape::convex_polygon(
            vec![t, back + n * self.s(2.0), back - n * self.s(2.0)],
            self.color,
            Stroke::NONE,
        ));
    }
}

fn pen<'a>(painter: &'a egui::Painter, rect: Rect, color: Color32) -> Pen<'a> {
    let width = (rect.width() / 20.0 * 1.5).clamp(1.2, 2.2);
    Pen {
        painter,
        rect,
        stroke: Stroke::new(width, color),
        color,
    }
}

/// Draw a tool's icon in `rect`.
pub fn tool(painter: &egui::Painter, rect: Rect, kind: ToolKind, color: Color32) {
    let p = pen(painter, rect, color);
    match kind {
        ToolKind::Select => {
            let pts = [
                (6.0, 3.0),
                (6.0, 16.5),
                (9.4, 13.4),
                (11.8, 18.0),
                (13.8, 17.0),
                (11.4, 12.4),
                (15.8, 12.0),
            ];
            let pts: Vec<Pos2> = pts.iter().map(|(x, y)| p.p(*x, *y)).collect();
            painter.add(Shape::convex_polygon(
                pts.clone(),
                Color32::TRANSPARENT,
                p.stroke,
            ));
        }
        ToolKind::Line => {
            p.line((4.0, 16.0), (16.0, 4.0));
            p.dot(4.0, 16.0, 1.8);
            p.dot(16.0, 4.0, 1.8);
        }
        ToolKind::Pen => {
            // A pen nib over a curve.
            p.poly(
                &[
                    (10.0, 2.5),
                    (14.5, 9.5),
                    (12.0, 15.0),
                    (8.0, 15.0),
                    (5.5, 9.5),
                ],
                true,
            );
            p.line((10.0, 2.5), (10.0, 9.0));
            p.dot(10.0, 10.0, 1.0);
            p.line((8.0, 17.5), (12.0, 17.5));
        }
        ToolKind::Ellipse => {
            let pts: Vec<(f32, f32)> = (0..32)
                .map(|k| {
                    let t = std::f32::consts::TAU * k as f32 / 32.0;
                    (10.0 + 7.5 * t.cos(), 10.0 + 4.6 * t.sin())
                })
                .collect();
            p.poly(&pts, true);
            p.dot(10.0, 10.0, 0.9);
        }
        ToolKind::EllipseArc => {
            let pts: Vec<(f32, f32)> = (0..=20)
                .map(|k| {
                    let t = std::f32::consts::PI * (1.0 + k as f32 / 20.0);
                    (10.0 + 7.5 * t.cos(), 11.5 + 4.6 * t.sin())
                })
                .collect();
            p.poly(&pts, false);
            p.dot(10.0, 11.5, 0.9);
        }
        ToolKind::Polygon => {
            let pts: Vec<(f32, f32)> = (0..6)
                .map(|k| {
                    let a = std::f32::consts::FRAC_PI_3 * k as f32;
                    (10.0 + 7.5 * a.cos(), 10.0 + 7.5 * a.sin())
                })
                .collect();
            p.poly(&pts, true);
        }
        ToolKind::Slot => {
            p.line((6.5, 6.0), (13.5, 6.0));
            p.line((6.5, 14.0), (13.5, 14.0));
            p.arc(6.5, 10.0, 4.0, PI / 2.0, PI);
            p.arc(13.5, 10.0, 4.0, -PI / 2.0, PI);
            p.dot(6.5, 10.0, 0.9);
            p.dot(13.5, 10.0, 0.9);
        }
        ToolKind::Rectangle => {
            p.poly(&[(3.5, 5.5), (16.5, 5.5), (16.5, 14.5), (3.5, 14.5)], true);
        }
        ToolKind::Circle => {
            p.circle(10.0, 10.0, 6.8);
            p.dot(10.0, 10.0, 1.3);
        }
        ToolKind::Arc => {
            p.arc(10.0, 13.0, 7.0, PI, PI);
            p.dot(10.0, 13.0, 1.3);
            p.dashed((10.0, 13.0), (15.0, 8.0));
        }
        ToolKind::ThreePointArc => {
            p.arc(10.0, 14.0, 7.0, PI, PI);
            p.dot(3.0, 14.0, 1.6);
            p.dot(10.0, 7.0, 1.6);
            p.dot(17.0, 14.0, 1.6);
        }
        ToolKind::TangentArc => {
            p.line((2.5, 14.0), (9.0, 14.0));
            p.arc(9.0, 7.5, 6.5, FRAC_PI_2, -PI);
            p.dot(9.0, 14.0, 1.4);
        }
        ToolKind::Point => {
            p.dot(10.0, 10.0, 2.4);
            p.line((10.0, 3.5), (10.0, 6.5));
            p.line((10.0, 13.5), (10.0, 16.5));
            p.line((3.5, 10.0), (6.5, 10.0));
            p.line((13.5, 10.0), (16.5, 10.0));
        }
        ToolKind::Dimension => {
            p.line((3.5, 5.0), (3.5, 15.0));
            p.line((16.5, 5.0), (16.5, 15.0));
            p.line((5.0, 10.0), (15.0, 10.0));
            p.arrow_head((4.0, 10.0), vec2(-1.0, 0.0));
            p.arrow_head((16.0, 10.0), vec2(1.0, 0.0));
        }
        ToolKind::Block => {
            // A chair seen from above, with its base point.
            p.poly(&[(5.0, 6.0), (15.0, 6.0), (15.0, 16.0), (5.0, 16.0)], true);
            p.line((4.0, 3.5), (16.0, 3.5));
            p.dot(10.0, 11.0, 1.4);
        }
        ToolKind::Ordinate => {
            p.line((3.5, 16.0), (17.0, 16.0));
            p.dot(3.5, 16.0, 1.6);
            for x in [3.5, 9.0, 15.0] {
                p.line((x, 3.0), (x, 12.5));
            }
            p.line((3.5, 3.0), (3.5, 6.0));
        }
        ToolKind::Note => {
            p.line((8.0, 5.0), (17.0, 5.0));
            p.line((8.0, 9.0), (17.0, 9.0));
            p.line((8.0, 13.0), (13.5, 13.0));
            p.line((3.0, 17.0), (6.5, 9.0));
            p.line((6.5, 9.0), (8.0, 9.0));
            p.arrow_head((2.8, 17.5), vec2(-0.4, 1.0));
        }
        ToolKind::Callout => {
            p.circle(6.5, 13.0, 4.0);
            p.line((9.3, 10.2), (13.0, 5.0));
            p.line((13.0, 5.0), (18.0, 5.0));
            p.arrow_head((9.0, 10.6), vec2(-0.6, 0.8));
            p.dot(6.5, 13.0, 1.0);
        }
        ToolKind::CentreMark => {
            p.circle(10.0, 10.0, 5.5);
            p.dashed((1.5, 10.0), (18.5, 10.0));
            p.dashed((10.0, 1.5), (10.0, 18.5));
        }
        ToolKind::Fill => {
            p.poly(&[(3.5, 4.5), (16.5, 4.5), (16.5, 15.5), (3.5, 15.5)], true);
            for k in 0..4 {
                let x = 3.5 + 3.4 * k as f32;
                let (x0, y0, x1, y1) = (
                    x,
                    15.5,
                    (x + 11.0).min(16.5),
                    15.5 - ((x + 11.0).min(16.5) - x),
                );
                p.line((x0, y0), (x1, y1));
            }
        }
        ToolKind::Trim => {
            p.line((6.0, 3.0), (6.0, 17.0));
            p.line((2.5, 10.0), (6.0, 10.0));
            p.dashed((6.0, 10.0), (17.5, 10.0));
            p.line((12.0, 6.0), (15.0, 8.5));
            p.line((12.0, 14.0), (15.0, 11.5));
        }
        ToolKind::Extend => {
            p.line((16.0, 3.0), (16.0, 17.0));
            p.line((3.0, 10.0), (8.0, 10.0));
            p.dashed((8.5, 10.0), (14.0, 10.0));
            p.arrow_head((15.5, 10.0), vec2(1.0, 0.0));
        }
        ToolKind::Split => {
            p.line((2.5, 13.0), (8.5, 9.0));
            p.line((11.5, 7.0), (17.5, 3.0));
            p.dot(10.0, 8.0, 1.8);
            p.line((8.0, 14.0), (12.0, 16.5));
        }
        ToolKind::Fillet => {
            p.line((4.0, 3.0), (4.0, 9.0));
            p.arc(11.0, 9.0, 7.0, PI, -FRAC_PI_2);
            p.line((11.0, 16.0), (17.0, 16.0));
            p.dashed((4.0, 12.0), (4.0, 16.0));
            p.dashed((4.0, 16.0), (8.0, 16.0));
        }
        ToolKind::Chamfer => {
            p.line((4.0, 3.0), (4.0, 10.0));
            p.line((4.0, 10.0), (10.0, 16.0));
            p.line((10.0, 16.0), (17.0, 16.0));
            p.dashed((4.0, 12.5), (4.0, 16.0));
            p.dashed((4.0, 16.0), (7.5, 16.0));
        }
        ToolKind::Offset => {
            p.arc(10.0, 16.0, 11.0, PI + 0.35, PI - 0.7);
            p.arc(10.0, 16.0, 6.0, PI + 0.35, PI - 0.7);
            p.arrow_head((10.0, 9.5), vec2(0.0, 1.0));
            p.line((10.0, 6.0), (10.0, 8.0));
        }
        ToolKind::Mirror => {
            p.dashed((10.0, 2.5), (10.0, 17.5));
            p.poly(&[(3.0, 15.0), (8.0, 15.0), (8.0, 6.0)], true);
            p.poly(&[(17.0, 15.0), (12.0, 15.0), (12.0, 6.0)], true);
        }
        ToolKind::Calibrate => {
            // A ruler.
            p.poly(&[(2.5, 7.0), (17.5, 7.0), (17.5, 13.0), (2.5, 13.0)], true);
            for (x, h) in [(5.5, 3.0), (8.5, 2.0), (11.5, 3.0), (14.5, 2.0)] {
                p.line((x, 7.0), (x, 7.0 + h));
            }
        }
        ToolKind::MoveUnderlay => {
            p.line((10.0, 2.5), (10.0, 17.5));
            p.line((2.5, 10.0), (17.5, 10.0));
            p.arrow_head((10.0, 2.0), vec2(0.0, -1.0));
            p.arrow_head((10.0, 18.0), vec2(0.0, 1.0));
            p.arrow_head((2.0, 10.0), vec2(-1.0, 0.0));
            p.arrow_head((18.0, 10.0), vec2(1.0, 0.0));
        }
    }
}

/// Draw a view toggle's icon in `rect`.
pub fn glyph(painter: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    let p = pen(painter, rect, color);
    match glyph {
        Glyph::Grid => {
            for t in [5.0, 10.0, 15.0] {
                p.line((t, 3.0), (t, 17.0));
                p.line((3.0, t), (17.0, t));
            }
        }
        Glyph::Snap => {
            // A magnet.
            p.arc(10.0, 9.0, 5.5, 0.0, PI);
            p.line((4.5, 9.0), (4.5, 4.0));
            p.line((15.5, 9.0), (15.5, 4.0));
            p.line((2.8, 4.0), (6.2, 4.0));
            p.line((13.8, 4.0), (17.2, 4.0));
        }
        Glyph::Thin => {
            for (y, w) in [(5.0, 3.2), (10.0, 1.8), (15.0, 0.9)] {
                painter.line_segment(
                    [p.p(3.0, y), p.p(17.0, y)],
                    Stroke::new(p.s(w).max(1.0), color),
                );
            }
        }
        Glyph::Axes => {
            p.line((4.0, 16.0), (17.0, 16.0));
            p.line((4.0, 16.0), (4.0, 3.0));
            p.arrow_head((17.5, 16.0), vec2(1.0, 0.0));
            p.arrow_head((4.0, 2.5), vec2(0.0, -1.0));
        }
        Glyph::Eye => {
            p.arc(10.0, 17.0, 9.5, PI + 0.72, PI - 1.44);
            p.arc(10.0, 3.0, 9.5, 0.72, PI - 1.44);
            p.circle(10.0, 10.0, 2.6);
        }
        Glyph::Lock | Glyph::Unlock => {
            p.poly(&[(4.5, 9.5), (15.5, 9.5), (15.5, 17.0), (4.5, 17.0)], true);
            if glyph == Glyph::Lock {
                p.arc(10.0, 7.0, 3.6, PI, PI);
                p.line((6.4, 7.0), (6.4, 9.5));
                p.line((13.6, 7.0), (13.6, 9.5));
            } else {
                p.arc(13.6, 6.0, 3.6, PI, PI);
                p.line((10.0, 6.0), (10.0, 9.5));
            }
            p.dot(10.0, 13.2, 1.1);
        }
        Glyph::Endpoints => {
            p.line((5.0, 14.0), (15.0, 6.0));
            p.dot(5.0, 14.0, 2.2);
            p.dot(15.0, 6.0, 2.2);
        }
        Glyph::Status => {
            p.line((4.0, 15.0), (16.0, 5.0));
            p.dot(4.0, 15.0, 2.2);
            p.circle(16.0, 5.0, 2.2);
            p.line((2.0, 18.5), (6.0, 18.5));
        }
        Glyph::Print => {
            p.poly(&[(6.0, 7.5), (6.0, 3.0), (14.0, 3.0), (14.0, 7.5)], false);
            p.poly(
                &[
                    (6.0, 14.0),
                    (3.0, 14.0),
                    (3.0, 7.5),
                    (17.0, 7.5),
                    (17.0, 14.0),
                    (14.0, 14.0),
                ],
                false,
            );
            p.poly(
                &[(6.0, 11.5), (14.0, 11.5), (14.0, 17.0), (6.0, 17.0)],
                true,
            );
        }
    }
}

/// A square icon button: an accent-filled tile when `selected`, a soft hover otherwise.
/// `label` names it for accessibility (screen readers and UI tests).
/// A full-width toolbar row: icon, name and (dimmed, at the right) shortcut key.
pub fn tool_row(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    key: Option<&str>,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let size = vec2(ui.available_width(), 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let dark = ui.visuals().dark_mode;
    let accent = crate::theme::accent(dark);
    let (bg, fg, weak) = if selected {
        (Some(accent), Color32::WHITE, Color32::from_white_alpha(170))
    } else if response.is_pointer_button_down_on() {
        (
            Some(ui.visuals().widgets.active.bg_fill),
            ui.visuals().strong_text_color(),
            ui.visuals().weak_text_color(),
        )
    } else if response.hovered() {
        (
            Some(ui.visuals().widgets.hovered.bg_fill),
            ui.visuals().strong_text_color(),
            ui.visuals().weak_text_color(),
        )
    } else {
        (
            None,
            ui.visuals().text_color(),
            ui.visuals().weak_text_color(),
        )
    };
    let painter = ui.painter();
    if let Some(bg) = bg {
        painter.rect_filled(rect, 6.0, bg);
    }
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + 17.0, rect.center().y),
        Vec2::splat(18.0),
    );
    draw(painter, icon, fg);
    // Text is centred on its ink (cap height), not its line box, so capitals and the key
    // letter sit on the row's middle.
    let centred = |g: &std::sync::Arc<egui::Galley>| rect.center().y - g.mesh_bounds.center().y;
    let mut right = rect.right() - 9.0;
    if let Some(key) = key {
        let g = painter.layout_no_wrap(key.to_owned(), egui::FontId::monospace(10.5), weak);
        right -= g.size().x;
        let y = centred(&g);
        painter.galley(egui::pos2(right, y), g, weak);
        right -= 6.0;
    }
    let left = rect.left() + 34.0;
    let mut job = egui::text::LayoutJob::single_section(
        label.to_uppercase(),
        egui::TextFormat {
            font_id: egui::FontId::proportional(10.5),
            color: fg,
            extra_letter_spacing: 0.8,
            ..Default::default()
        },
    );
    job.wrap.max_width = (right - left).max(10.0);
    job.wrap.max_rows = 1;
    let g = painter.layout_job(job);
    let y = centred(&g);
    painter.galley(egui::pos2(left, y), g, fg);
    response
}

pub fn button(
    ui: &mut egui::Ui,
    size: f32,
    selected: bool,
    label: &str,
    draw: impl FnOnce(&egui::Painter, Rect, Color32),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let dark = ui.visuals().dark_mode;
    let accent = crate::theme::accent(dark);
    let (bg, fg) = if selected {
        (Some(accent), Color32::WHITE)
    } else if response.is_pointer_button_down_on() {
        (
            Some(ui.visuals().widgets.active.bg_fill),
            ui.visuals().strong_text_color(),
        )
    } else if response.hovered() {
        (
            Some(ui.visuals().widgets.hovered.bg_fill),
            ui.visuals().strong_text_color(),
        )
    } else {
        (None, ui.visuals().text_color())
    };
    if let Some(bg) = bg {
        ui.painter().rect_filled(rect, 7.0, bg);
    }
    let inner = rect.shrink(size * 0.2);
    draw(ui.painter(), inner, fg);
    response
}
