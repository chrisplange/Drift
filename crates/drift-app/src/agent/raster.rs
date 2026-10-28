//! Drawing a page ([`Drawing`]) into pixels on the CPU, with tiny-skia.

use drift_core::{DVec2, Rgb, TextFont};
use drift_draft::page::{Align, Drawing, Pen, Prim};
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

/// A picture of a page: page millimetres (y up, from the lower left) onto pixels.
pub(super) struct Canvas {
    pub pixmap: Pixmap,
    /// Pixels per page millimetre.
    pub k: f64,
    page_height: f64,
}

fn color(c: Rgb, alpha: f64) -> Color {
    let [r, g, b] = c.0;
    Color::from_rgba8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn paint(c: Rgb, alpha: f64) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(color(c, alpha));
    p.anti_alias = true;
    p
}

impl Canvas {
    /// A white page of `width` × `height` pixels showing `page_size` mm.
    pub fn new(width: u32, height: u32, page_size: DVec2) -> Option<Self> {
        let mut pixmap = Pixmap::new(width, height)?;
        pixmap.fill(Color::WHITE);
        Some(Self {
            pixmap,
            k: f64::from(width) / page_size.x,
            page_height: page_size.y,
        })
    }

    fn px(&self, p: DVec2) -> (f32, f32) {
        (
            (p.x * self.k) as f32,
            ((self.page_height - p.y) * self.k) as f32,
        )
    }

    /// A picture (RGBA, rows from the top) with its top-left corner at `at` and its top and
    /// left edges along `across` and `down` (page mm).
    pub fn image(
        &mut self,
        image: &drift_core::UnderlayImage,
        at: DVec2,
        across: DVec2,
        down: DVec2,
        opacity: f64,
    ) {
        let premultiplied: Vec<u8> = image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|c| {
                let a = u16::from(c[3]);
                let m = |v: u8| ((u16::from(v) * a + 127) / 255) as u8;
                [m(c[0]), m(c[1]), m(c[2]), c[3]]
            })
            .collect();
        let Some(size) = tiny_skia::IntSize::from_wh(image.width, image.height) else {
            return;
        };
        let Some(picture) = Pixmap::from_vec(premultiplied, size) else {
            return;
        };
        let o = self.px(at);
        let (a, d) = (self.px(at + across), self.px(at + down));
        let (w, h) = (image.width as f32, image.height as f32);
        let t = Transform::from_row(
            (a.0 - o.0) / w,
            (a.1 - o.1) / w,
            (d.0 - o.0) / h,
            (d.1 - o.1) / h,
            o.0,
            o.1,
        );
        let paint = tiny_skia::PixmapPaint {
            opacity: opacity.clamp(0.0, 1.0) as f32,
            quality: tiny_skia::FilterQuality::Bilinear,
            ..Default::default()
        };
        self.pixmap
            .draw_pixmap(0, 0, picture.as_ref(), &paint, t, None);
    }

    fn path(&self, pts: &[DVec2], closed: bool) -> Option<tiny_skia::Path> {
        let mut b = PathBuilder::new();
        let (x, y) = self.px(*pts.first()?);
        b.move_to(x, y);
        for p in &pts[1..] {
            let (x, y) = self.px(*p);
            b.line_to(x, y);
        }
        if closed {
            b.close();
        }
        b.finish()
    }

    /// A polyline `width` pixels wide, dashed in pixels when `dash` is given.
    pub fn stroke(
        &mut self,
        pts: &[DVec2],
        closed: bool,
        c: Rgb,
        width: f32,
        dash: Option<Vec<f32>>,
    ) {
        let Some(path) = self.path(pts, closed) else {
            return;
        };
        let stroke = Stroke {
            width: width.max(0.5),
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            dash: dash.and_then(|d| StrokeDash::new(d, 0.0)),
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &paint(c, 1.0), &stroke, Transform::identity(), None);
    }

    /// Rings filled even–odd (holes inside outlines).
    pub fn fill(&mut self, rings: &[Vec<DVec2>], c: Rgb, alpha: f64, rule: FillRule) {
        let mut b = PathBuilder::new();
        for ring in rings.iter().filter(|r| r.len() >= 3) {
            let (x, y) = self.px(ring[0]);
            b.move_to(x, y);
            for p in &ring[1..] {
                let (x, y) = self.px(*p);
                b.line_to(x, y);
            }
            b.close();
        }
        if let Some(path) = b.finish() {
            self.pixmap
                .fill_path(&path, &paint(c, alpha), rule, Transform::identity(), None);
        }
    }

    /// A round dot `radius` pixels across.
    pub fn dot(&mut self, at: DVec2, radius: f32, c: Rgb) {
        let (x, y) = self.px(at);
        if let Some(path) = PathBuilder::from_circle(x, y, radius) {
            self.pixmap.fill_path(
                &path,
                &paint(c, 1.0),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// A line of text `height` page mm tall (capitals) from `at` on its baseline, turned by
    /// `angle` radians; with a white `halo` (pixels) behind it to read over linework.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &mut self,
        text: &str,
        font: &TextFont,
        at: DVec2,
        height: f64,
        angle: f64,
        align: Align,
        c: Rgb,
        halo: f32,
    ) {
        let width = drift_draft::outline::line_width(text, font, height);
        let shift = match align {
            Align::Left => 0.0,
            Align::Center => width / 2.0,
            Align::Right => width,
        };
        let start = at - DVec2::from_angle(angle) * shift;
        let contours =
            drift_draft::outline::text_contours(text, font, height, start, angle.to_degrees());
        let rings = drift_draft::outline::contour_rings(&contours);
        if halo > 0.0 {
            for ring in &rings {
                self.stroke(ring, true, Rgb([255, 255, 255]), halo, None);
            }
        }
        self.fill(&rings, c, 1.0, FillRule::Winding);
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        self.pixmap.encode_png().map_err(|e| e.to_string())
    }
}

/// Points along a primitive's curve, in page mm.
fn prim_polyline(prim: &Prim) -> Option<(Vec<DVec2>, bool)> {
    let tol = drift_core::tolerance::FLATTEN_MM;
    Some(match prim {
        Prim::Line(a, b) => (vec![*a, *b], false),
        Prim::Polyline { points, closed } => (points.clone(), *closed),
        Prim::Circle { center, radius } => (
            drift_core::Shape::Circle {
                center: *center,
                radius: *radius,
            }
            .flatten(tol),
            true,
        ),
        Prim::Arc { .. } | Prim::Ellipse { .. } | Prim::Bezier { .. } => {
            (drift_draft::page::prim_points(prim), false)
        }
        _ => return None,
    })
}

/// Line widths (page mm) and dash patterns into pixels.
fn pen_px(canvas: &Canvas, pen: &Pen, line_type_scale: f64) -> (f32, Option<Vec<f32>>) {
    let width = ((pen.width * canvas.k) as f32).max(1.0);
    let pattern = drift_draft::page::dash_pattern(pen.line_type);
    let dash = (!pattern.is_empty()).then(|| {
        pattern
            .iter()
            .map(|d| ((d.abs() * canvas.k * line_type_scale) as f32).max(1.0))
            .collect()
    });
    (width, dash)
}

/// Draw everything on a page.
/// Draw a page, with `between` drawn over its filled areas and under everything else (as the
/// canvas draws underlays).
pub(super) fn draw_over(canvas: &mut Canvas, d: &Drawing, between: impl FnOnce(&mut Canvas)) {
    let d = d.exploded();
    let lts = if d.line_type_scale > 0.0 {
        d.line_type_scale
    } else {
        1.0
    };
    for it in &d.items {
        if let Prim::Fill {
            rings, mask: false, ..
        } = &it.prim
        {
            canvas.fill(rings, it.pen.color, 1.0, FillRule::EvenOdd);
        }
    }
    between(canvas);
    for it in &d.items {
        match &it.prim {
            Prim::Fill { mask: false, .. } => {}
            Prim::Fill { rings, mask, .. } => {
                let c = if *mask {
                    Rgb([255, 255, 255])
                } else {
                    it.pen.color
                };
                canvas.fill(rings, c, 1.0, FillRule::EvenOdd);
            }
            Prim::Text {
                at,
                height,
                angle,
                align,
                text,
                font,
                ..
            } => {
                let font = font.clone().unwrap_or(TextFont::Sans);
                for (i, line) in text.lines().enumerate() {
                    let down = DVec2::from_angle(*angle - std::f64::consts::FRAC_PI_2)
                        * (i as f64 * height * drift_core::SketchText::SPACING);
                    canvas.text(
                        line,
                        &font,
                        *at + down,
                        *height,
                        *angle,
                        *align,
                        it.pen.color,
                        0.0,
                    );
                }
            }
            Prim::Insert { .. } => {}
            prim => {
                if let Some((pts, closed)) = prim_polyline(prim) {
                    let (w, dash) = pen_px(canvas, &it.pen, lts);
                    canvas.stroke(&pts, closed, it.pen.color, w, dash);
                }
            }
        }
    }
}
