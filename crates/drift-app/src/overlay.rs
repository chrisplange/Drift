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
    /// The HUD's outline and its fields.
    pub label_border: Color32,
    pub field_bg: Color32,
    pub dark: bool,
}

impl OverlayColors {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                snap: Color32::from_rgb(0xff, 0x9f, 0x1c),
                window: Color32::from_rgb(0x2f, 0x9b, 0xff),
                crossing: Color32::from_rgb(0x4c, 0xc3, 0x6b),
                label_bg: Color32::from_rgba_unmultiplied(0x2a, 0x2d, 0x32, 0xf4),
                label_fg: Color32::from_rgb(0xe6, 0xe8, 0xeb),
                label_border: Color32::from_rgb(0x3b, 0x3f, 0x46),
                field_bg: Color32::from_rgb(0x16, 0x18, 0x1b),
                dark,
            }
        } else {
            Self {
                snap: Color32::from_rgb(0xe0, 0x6c, 0x00),
                window: Color32::from_rgb(0x0b, 0x6b, 0xde),
                crossing: Color32::from_rgb(0x1f, 0x9a, 0x45),
                label_bg: Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0xf4),
                label_fg: Color32::from_rgb(0x1d, 0x21, 0x26),
                label_border: Color32::from_rgb(0xd9, 0xdb, 0xdf),
                field_bg: Color32::from_rgb(0xf3, 0xf4, 0xf6),
                dark,
            }
        }
    }
}

/// Draw the glyph for a snap target: square for points, circle for centres, triangle for
/// midpoints, diamond for quadrants, cross for on-curve, plus for grid, ⊥ for perpendicular and
/// a circle on a line for tangent.
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
        SnapKind::Origin => {
            // A ring round the crossing of the axes.
            painter.circle_stroke(c, s + 2.0, stroke);
            painter.circle_filled(c, 2.0, color);
        }
        SnapKind::Grid => {
            painter.line_segment([c - vec2(s, 0.0), c + vec2(s, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, s), c + vec2(0.0, s)], stroke);
        }
        SnapKind::Perpendicular => {
            // ⊥, standing on the snap point, with the square-corner mark.
            painter.line_segment([c - vec2(s + 1.0, 0.0), c + vec2(s + 1.0, 0.0)], stroke);
            painter.line_segment([c, c - vec2(0.0, 2.0 * s)], stroke);
            let corner = vec![
                c + vec2(0.0, -0.5 * s - 1.0),
                c + vec2(0.5 * s + 1.0, -0.5 * s - 1.0),
                c + vec2(0.5 * s + 1.0, 0.0),
            ];
            painter.add(egui::Shape::line(corner, stroke));
        }
        SnapKind::Tangent => {
            // A circle touching a line at the snap point.
            painter.circle_stroke(c + vec2(0.0, s), s, stroke);
            painter.line_segment([c - vec2(s + 3.0, 0.0), c + vec2(s + 3.0, 0.0)], stroke);
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

/// A rotate in progress: the pivot, the reference ray, the current ray and the arc between.
pub fn rotate_guide(
    painter: &egui::Painter,
    map: &ScreenMap,
    pivot: DVec2,
    from: DVec2,
    degrees: Option<f64>,
    colors: &OverlayColors,
) {
    let c = map.to_screen(pivot);
    let stroke = Stroke::new(1.2, colors.snap);
    painter.circle_stroke(c, 5.0, stroke);
    painter.line_segment([c - vec2(8.0, 0.0), c + vec2(8.0, 0.0)], stroke);
    painter.line_segment([c - vec2(0.0, 8.0), c + vec2(0.0, 8.0)], stroke);
    let Some(deg) = degrees else { return };
    let r = 46.0;
    let v = map.to_screen(from) - c;
    let a0 = v.y.atan2(v.x);
    let dir = |a: f32| vec2(a.cos(), a.sin());
    // Screen angles turn clockwise, so a counter-clockwise turn is negative. Whole turns
    // (a typed 3600°, say) add nothing to the picture, and must not add points to it.
    let sweep = -((deg % 360.0).to_radians() as f32);
    painter.extend(egui::Shape::dashed_line(
        &[c, c + dir(a0) * (r + 14.0)],
        Stroke::new(1.0, colors.snap.gamma_multiply(0.7)),
        4.0,
        3.0,
    ));
    painter.line_segment([c, c + dir(a0 + sweep) * (r + 14.0)], stroke);
    let n = (sweep.abs() / 0.08).ceil().clamp(2.0, 128.0) as usize;
    let arc: Vec<Pos2> = (0..=n)
        .map(|k| c + dir(a0 + sweep * k as f32 / n as f32) * r)
        .collect();
    painter.add(egui::Shape::line(arc, stroke));
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
    // Each ring is cut to the canvas (and a little round it) in f64 before it is made f32:
    // a region far off screen, or out at 1e150, would otherwise reach the GPU with infinite
    // vertices.
    let half = DVec2::new(f64::from(map.rect.width()), f64::from(map.rect.height())) * 0.5
        + DVec2::splat(8.0);
    let centre = map.rect.center();
    let mut builder = Path::builder();
    for lp in std::iter::once(&region.outer).chain(&region.holes) {
        let poly: Vec<DVec2> = lp
            .polygon(max_angle)
            .into_iter()
            .map(|p| map.camera.world_to_view(p))
            .collect();
        let poly = drift_render::clip_polygon(&poly, half);
        let Some(first) = poly.first() else {
            continue;
        };
        let to = |v: DVec2| point(centre.x + v.x as f32, centre.y + v.y as f32);
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
    let info = CursorInfo {
        notes: lines.to_vec(),
        ..Default::default()
    };
    cursor_hud(painter, at, &info, colors);
}

/// A value being typed beside the cursor: its name, the text so far, and a note (what an
/// expression comes to, or why it can't be used).
pub struct HudField {
    pub name: String,
    pub value: String,
    pub note: String,
    pub active: bool,
    pub error: bool,
}

/// What the cursor HUD shows: what the pointer snapped to, the tool's measurement, and the
/// values being typed (as AutoCAD's dynamic input).
#[derive(Default)]
pub struct CursorInfo {
    pub snap: Option<String>,
    pub notes: Vec<String>,
    pub fields: Vec<HudField>,
}

/// The cursor HUD: a small card beside the pointer in the theme's colours, flipped to the
/// pointer's other side near the canvas's edges.
pub fn cursor_hud(painter: &egui::Painter, at: Pos2, info: &CursorInfo, colors: &OverlayColors) {
    if info.snap.is_none() && info.notes.is_empty() && info.fields.is_empty() {
        return;
    }
    let size = crate::theme::size::LABEL;
    let fg = colors.label_fg;
    let weak = fg.gamma_multiply(0.62);
    let accent = crate::theme::accent(colors.dark);
    let error = Color32::from_rgb(0xe5, 0x48, 0x4d);
    let pad = vec2(8.0, 6.0);
    let line_h = 18.0;
    let field_h = 20.0;
    // Lay out every line first, to size the card.
    let snap = info.snap.as_ref().map(|s| {
        painter.layout_no_wrap(
            s.clone(),
            FontId::new(size, crate::theme::bold()),
            colors.snap,
        )
    });
    let notes: Vec<_> = info
        .notes
        .iter()
        .map(|n| painter.layout_no_wrap(n.clone(), FontId::proportional(size), fg))
        .collect();
    let fields: Vec<_> = info
        .fields
        .iter()
        .map(|f| {
            let name = painter.layout_no_wrap(f.name.clone(), FontId::proportional(size), weak);
            let value = painter.layout_no_wrap(f.value.clone(), FontId::proportional(size), fg);
            let note = painter.layout_no_wrap(
                f.note.clone(),
                FontId::proportional(crate::theme::size::CAPTION),
                if f.error { error } else { weak },
            );
            (name, value, note)
        })
        .collect();
    let name_w = fields
        .iter()
        .map(|(n, _, _)| n.size().x)
        .fold(0.0_f32, f32::max);
    let box_w = fields
        .iter()
        .map(|(_, v, _)| v.size().x + 16.0)
        .fold(64.0_f32, f32::max);
    let fields_w = fields
        .iter()
        .map(|(_, _, n)| {
            name_w
                + 8.0
                + box_w
                + if n.size().x > 0.0 {
                    8.0 + n.size().x
                } else {
                    0.0
                }
        })
        .fold(0.0_f32, f32::max);
    let width = snap
        .iter()
        .chain(notes.iter())
        .map(|g| g.size().x)
        .fold(fields_w, f32::max);
    let height = snap.as_ref().map_or(0.0, |_| line_h)
        + notes.len() as f32 * line_h
        + fields.len() as f32 * (field_h + 4.0);
    let card = vec2(width, height) + pad * 2.0;
    // Below and right of the pointer, unless that runs off the canvas.
    let clip = painter.clip_rect();
    let mut min = at + vec2(18.0, 18.0);
    if min.x + card.x > clip.right() - 4.0 {
        min.x = at.x - 18.0 - card.x;
    }
    if min.y + card.y > clip.bottom() - 4.0 {
        min.y = at.y - 18.0 - card.y;
    }
    let r = Rect::from_min_size(min, card);
    painter.rect_filled(r, crate::theme::radius::MENU, colors.label_bg);
    painter.rect_stroke(
        r,
        crate::theme::radius::MENU,
        Stroke::new(1.0, colors.label_border),
        StrokeKind::Inside,
    );
    let mut y = r.top() + pad.y;
    let x = r.left() + pad.x;
    let mid = |g: &std::sync::Arc<egui::Galley>, y: f32, h: f32| y + (h - g.size().y) / 2.0;
    if let Some(g) = snap {
        painter.galley(egui::pos2(x, mid(&g, y, line_h)), g, colors.snap);
        y += line_h;
    }
    for g in notes {
        painter.galley(egui::pos2(x, mid(&g, y, line_h)), g, fg);
        y += line_h;
    }
    for ((name, value, note), f) in fields.into_iter().zip(&info.fields) {
        painter.galley(egui::pos2(x, mid(&name, y, field_h)), name, weak);
        let b = Rect::from_min_size(egui::pos2(x + name_w + 8.0, y), vec2(box_w, field_h));
        painter.rect_filled(b, crate::theme::radius::CONTROL, colors.field_bg);
        let edge = if f.active {
            Stroke::new(1.5, accent)
        } else {
            Stroke::new(1.0, colors.label_border)
        };
        painter.rect_stroke(b, crate::theme::radius::CONTROL, edge, StrokeKind::Inside);
        let vx = b.left() + 8.0;
        let caret_x = vx + value.size().x + 1.0;
        painter.galley(egui::pos2(vx, mid(&value, y, field_h)), value, fg);
        if f.active {
            painter.line_segment(
                [
                    egui::pos2(caret_x, b.top() + 4.0),
                    egui::pos2(caret_x, b.bottom() - 4.0),
                ],
                Stroke::new(1.0, fg),
            );
        }
        if note.size().x > 0.0 {
            let c = if f.error { error } else { weak };
            painter.galley(egui::pos2(b.right() + 8.0, mid(&note, y, field_h)), note, c);
        }
        y += field_h + 4.0;
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

/// A small curved arrow: what dragging here does is turn.
pub fn turn_glyph(painter: &egui::Painter, at: Pos2, color: Color32) {
    let r = 6.0;
    let stroke = Stroke::new(1.5, color);
    let arc: Vec<Pos2> = (0..=12)
        .map(|i| {
            let a = std::f32::consts::PI * (0.15 + 1.2 * i as f32 / 12.0);
            at + vec2(a.cos(), -a.sin()) * r
        })
        .collect();
    let tip = arc[arc.len() - 1];
    painter.add(egui::Shape::line(arc, stroke));
    painter.line_segment([tip, tip + vec2(-3.5, -1.0)], stroke);
    painter.line_segment([tip, tip + vec2(0.5, -3.5)], stroke);
}
