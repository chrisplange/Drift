//! Glyph outlines for sketch text: a line of text as closed contours of lines and cubic
//! Béziers, at a cap height, from its baseline start, turned by an angle. Quadratic glyph
//! curves (TrueType) are raised to cubics exactly.

use drift_core::{DVec2, Shape, TextFont};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

fn font_bytes(font: TextFont) -> &'static [u8] {
    match font {
        TextFont::Sans => epaint_default_fonts::UBUNTU_LIGHT,
        TextFont::Mono => epaint_default_fonts::HACK_REGULAR,
    }
}

/// Collects one glyph's contours in font units, offset along the baseline.
struct Pen {
    offset: f64,
    contours: Vec<Vec<Shape>>,
    current: Vec<Shape>,
    start: DVec2,
    at: DVec2,
}

impl Pen {
    fn p(&self, x: f32, y: f32) -> DVec2 {
        DVec2::new(f64::from(x) + self.offset, f64::from(y))
    }

    fn finish(&mut self) {
        if self.at.distance(self.start) > 1e-9 && !self.current.is_empty() {
            self.current.push(Shape::Line {
                a: self.at,
                b: self.start,
            });
        }
        if !self.current.is_empty() {
            self.contours.push(std::mem::take(&mut self.current));
        }
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        self.start = self.p(x, y);
        self.at = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let b = self.p(x, y);
        if b.distance(self.at) > 1e-9 {
            self.current.push(Shape::Line { a: self.at, b });
        }
        self.at = b;
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        // The same curve as a cubic: handles two thirds of the way to the control point.
        let (a, c, b) = (self.at, self.p(cx, cy), self.p(x, y));
        self.current.push(Shape::Bezier {
            p: [a, a + (c - a) * (2.0 / 3.0), b + (c - b) * (2.0 / 3.0), b],
        });
        self.at = b;
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p = [self.at, self.p(cx0, cy0), self.p(cx1, cy1), self.p(x, y)];
        self.current.push(Shape::Bezier { p });
        self.at = p[3];
    }

    fn close(&mut self) {
        self.finish();
    }
}

/// The contours of `text` in `font` with capitals `height` tall, starting at `at` on the
/// baseline and turned `angle` degrees counter-clockwise. Characters the font lacks are
/// skipped (with a space's width).
pub fn text_contours(
    text: &str,
    font: TextFont,
    height: f64,
    at: DVec2,
    angle: f64,
) -> Vec<Vec<Shape>> {
    let Ok(f) = FontRef::new(font_bytes(font)) else {
        return Vec::new();
    };
    let metrics = f.metrics(Size::unscaled(), LocationRef::default());
    let em = f64::from(metrics.units_per_em.max(1));
    let cap = metrics
        .cap_height
        .map(f64::from)
        .filter(|c| *c > 0.0)
        .unwrap_or(0.7 * em);
    let scale = height / cap;
    let charmap = f.charmap();
    let glyphs = f.outline_glyphs();
    let advances = f.glyph_metrics(Size::unscaled(), LocationRef::default());
    let space = charmap
        .map(' ')
        .and_then(|g| advances.advance_width(g))
        .map_or(0.3 * em, f64::from);
    let mut pen = Pen {
        offset: 0.0,
        contours: Vec::new(),
        current: Vec::new(),
        start: DVec2::ZERO,
        at: DVec2::ZERO,
    };
    for ch in text.chars() {
        let Some(gid) = charmap.map(ch) else {
            pen.offset += space;
            continue;
        };
        if let Some(glyph) = glyphs.get(gid) {
            let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
            let _ = glyph.draw(settings, &mut pen);
            pen.finish();
        }
        pen.offset += advances.advance_width(gid).map_or(space, f64::from);
    }
    let rotor = DVec2::from_angle(angle.to_radians());
    let place = |p: DVec2| at + rotor.rotate(p * scale);
    pen.contours
        .into_iter()
        .map(|contour| {
            contour
                .into_iter()
                .map(|s| match s {
                    Shape::Line { a, b } => Shape::Line {
                        a: place(a),
                        b: place(b),
                    },
                    Shape::Bezier { p } => Shape::Bezier { p: p.map(place) },
                    other => other,
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_have_closed_outlines_at_the_cap_height() {
        let contours = text_contours("HO", TextFont::Sans, 10.0, DVec2::ZERO, 0.0);
        // H is one contour, O two (outside and counter).
        assert_eq!(contours.len(), 3);
        for c in &contours {
            let start = match c[0] {
                Shape::Line { a, .. } => a,
                Shape::Bezier { p } => p[0],
                _ => unreachable!(),
            };
            let end = match c[c.len() - 1] {
                Shape::Line { b, .. } => b,
                Shape::Bezier { p } => p[3],
                _ => unreachable!(),
            };
            assert!(start.distance(end) < 1e-9, "closed");
        }
        // The H stands on the baseline and is the cap height tall.
        let bounds: drift_core::Bounds = contours[0].iter().map(Shape::bounds).collect();
        assert!(
            bounds.min.y.abs() < 0.05 && (bounds.max.y - 10.0).abs() < 0.05,
            "{bounds:?}"
        );
        // Turned 90°: it rises along +y from the start.
        let turned = text_contours("H", TextFont::Mono, 10.0, DVec2::new(5.0, 5.0), 90.0);
        let b: drift_core::Bounds = turned.iter().flatten().map(Shape::bounds).collect();
        assert!(b.max.x <= 5.0 + 1e-9 && b.min.y >= 5.0 - 1e-9, "{b:?}");
    }
}
