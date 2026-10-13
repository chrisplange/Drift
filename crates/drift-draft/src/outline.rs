//! Glyph outlines for sketch text: a line of text as closed contours of lines and cubic
//! Béziers, at a cap height, from its baseline start, turned by an angle. Quadratic glyph
//! curves (TrueType) are raised to cubics exactly.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use drift_core::{DVec2, Shape, TextFont};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};

/// A system font's file and the face's index in it; `None` once it is known to be missing.
type Loaded = Option<(Arc<Vec<u8>>, u32)>;

/// System fonts made known with [`register_font`], by family name.
fn registry() -> &'static RwLock<HashMap<String, Loaded>> {
    static FONTS: OnceLock<RwLock<HashMap<String, Loaded>>> = OnceLock::new();
    FONTS.get_or_init(Default::default)
}

/// Make a system font's file known for outlining text (`None`: it isn't installed here, so
/// text in it falls back to the built-in sans).
pub fn register_font(family: &str, font: Option<(Vec<u8>, u32)>) {
    // A file that doesn't read as a font counts as missing.
    let font = font
        .filter(|(data, index)| FontRef::from_index(data, *index).is_ok())
        .map(|(data, index)| (Arc::new(data), index));
    if let Ok(mut fonts) = registry().write() {
        fonts.insert(family.to_owned(), font);
    }
}

/// Whether a system font is known: `None` if it hasn't been looked for yet, else whether it
/// was found.
pub fn font_known(family: &str) -> Option<bool> {
    registry()
        .read()
        .ok()?
        .get(family)
        .map(|font| font.is_some())
}

/// The bytes of a built-in font (the sans, or the monospaced one).
pub fn builtin_font(mono: bool) -> Arc<Vec<u8>> {
    font_data(if mono {
        &TextFont::Mono
    } else {
        &TextFont::Sans
    })
    .0
}

/// The font's file and face index: a built-in one, or a registered system font (the built-in
/// sans where that isn't installed).
fn font_data(font: &TextFont) -> (Arc<Vec<u8>>, u32) {
    static BUILT_IN: OnceLock<[Arc<Vec<u8>>; 2]> = OnceLock::new();
    let built_in = BUILT_IN.get_or_init(|| {
        [
            Arc::new(epaint_default_fonts::UBUNTU_LIGHT.to_vec()),
            Arc::new(epaint_default_fonts::HACK_REGULAR.to_vec()),
        ]
    });
    match font {
        TextFont::Sans => (built_in[0].clone(), 0),
        TextFont::Mono => (built_in[1].clone(), 0),
        TextFont::System(family) => registry()
            .read()
            .ok()
            .and_then(|fonts| fonts.get(family).cloned().flatten())
            .unwrap_or_else(|| (built_in[0].clone(), 0)),
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
/// The most characters of a text that are outlined (a line of sketch text, not a document):
/// each one is tens of curves, redrawn every frame.
pub const MAX_OUTLINE_CHARS: usize = 2000;

pub fn text_contours(
    text: &str,
    font: &TextFont,
    height: f64,
    at: DVec2,
    angle: f64,
) -> Vec<Vec<Shape>> {
    let (data, index) = font_data(font);
    contours_in(&data, index, text, height, at, angle, 0.0)
}

/// How wide `text` is in `font` with capitals `height` tall (advance widths, so a trailing
/// space counts).
pub fn line_width(text: &str, font: &TextFont, height: f64) -> f64 {
    let (data, index) = font_data(font);
    let Ok(f) = FontRef::from_index(&data, index) else {
        return 0.0;
    };
    let metrics = f.metrics(Size::unscaled(), LocationRef::default());
    let em = f64::from(metrics.units_per_em.max(1));
    let cap = metrics
        .cap_height
        .map(f64::from)
        .filter(|c| *c > 0.0)
        .unwrap_or(0.7 * em);
    let charmap = f.charmap();
    let advances = f.glyph_metrics(Size::unscaled(), LocationRef::default());
    let space = charmap
        .map(' ')
        .and_then(|g| advances.advance_width(g))
        .map_or(0.3 * em, f64::from);
    let units: f64 = text
        .chars()
        .map(|ch| {
            charmap
                .map(ch)
                .and_then(|g| advances.advance_width(g))
                .map_or(space, f64::from)
        })
        .sum();
    units * height / cap
}

/// The lines a text is set in: where it breaks itself (`\n`), and, with a `wrap` width, at
/// the spaces that keep each line within it. A word longer than the width has a line to
/// itself.
pub fn paragraph_lines(text: &str, font: &TextFont, height: f64, wrap: Option<f64>) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        let Some(width) = wrap.filter(|w| *w > 0.0) else {
            lines.push(line.to_owned());
            continue;
        };
        let mut current = String::new();
        for word in line.split(' ') {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!("{current} {word}")
            };
            if !current.is_empty() && line_width(&candidate, font, height) > width {
                lines.push(std::mem::take(&mut current));
                current = word.to_owned();
            } else {
                current = candidate;
            }
        }
        lines.push(current);
    }
    lines
}

/// A text of one or more lines.
#[derive(Clone, Copy, Debug)]
pub struct Paragraph<'a> {
    pub text: &'a str,
    pub font: &'a TextFont,
    /// Cap height.
    pub height: f64,
    /// The anchor: on the first baseline, at each line's start, middle or end per `align`.
    pub at: DVec2,
    /// Degrees counter-clockwise.
    pub angle: f64,
    pub align: drift_core::TextAlign,
    /// Baseline to baseline, in cap heights.
    pub spacing: f64,
    pub wrap: Option<f64>,
}

impl Paragraph<'_> {
    /// Each line with where its baseline starts.
    pub fn lines(&self) -> Vec<(String, DVec2)> {
        let rotor = DVec2::from_angle(self.angle.to_radians());
        paragraph_lines(self.text, self.font, self.height, self.wrap)
            .into_iter()
            .enumerate()
            .map(|(k, line)| {
                let across =
                    -self.align.factor() * line_width(line.trim_end(), self.font, self.height);
                let down = -(k as f64) * self.spacing * self.height;
                let start = self.at + rotor.rotate(DVec2::new(across, down));
                (line, start)
            })
            .collect()
    }

    /// The outlines of every line (see [`text_contours`]).
    pub fn contours(&self) -> Vec<Vec<Shape>> {
        self.lines()
            .into_iter()
            .flat_map(|(line, start)| {
                text_contours(&line, self.font, self.height, start, self.angle)
            })
            .collect()
    }
}

/// The contours of `text` in a font file's face (see [`text_contours`]), with `tracking` (a
/// fraction of the font size) added after each letter.
pub fn contours_in(
    data: &[u8],
    index: u32,
    text: &str,
    height: f64,
    at: DVec2,
    angle: f64,
    tracking: f64,
) -> Vec<Vec<Shape>> {
    let text: String = text.chars().take(MAX_OUTLINE_CHARS).collect();
    let text = text.as_str();
    let Ok(f) = FontRef::from_index(data, index) else {
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
            pen.offset += space + tracking * em;
            continue;
        };
        if let Some(glyph) = glyphs.get(gid) {
            let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
            let _ = glyph.draw(settings, &mut pen);
            pen.finish();
        }
        pen.offset += advances.advance_width(gid).map_or(space, f64::from) + tracking * em;
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
        let contours = text_contours("HO", &TextFont::Sans, 10.0, DVec2::ZERO, 0.0);
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
        let turned = text_contours("H", &TextFont::Mono, 10.0, DVec2::new(5.0, 5.0), 90.0);
        let b: drift_core::Bounds = turned.iter().flatten().map(Shape::bounds).collect();
        assert!(b.max.x <= 5.0 + 1e-9 && b.min.y >= 5.0 - 1e-9, "{b:?}");
    }

    #[test]
    fn paragraphs_break_align_and_wrap() {
        use drift_core::TextAlign;
        let font = TextFont::Sans;
        let para = |text: &'static str, align, wrap| Paragraph {
            text,
            font: &TextFont::Sans,
            height: 10.0,
            at: DVec2::new(100.0, 50.0),
            angle: 0.0,
            align,
            spacing: 1.5,
            wrap,
        };
        // Two lines: the second a line spacing below the first, both from the anchor.
        let left = para("HO\nH", TextAlign::Left, None).lines();
        assert_eq!(left.len(), 2);
        assert_eq!(left[0], ("HO".to_owned(), DVec2::new(100.0, 50.0)));
        assert_eq!(left[1], ("H".to_owned(), DVec2::new(100.0, 35.0)));
        // Centred and right-aligned lines sit about, and end at, the anchor.
        let (wide, narrow) = (line_width("HO", &font, 10.0), line_width("H", &font, 10.0));
        assert!(wide > narrow && narrow > 5.0);
        let centre = para("HO\nH", TextAlign::Center, None).lines();
        assert!((centre[0].1.x - (100.0 - wide / 2.0)).abs() < 1e-9);
        assert!((centre[1].1.x - (100.0 - narrow / 2.0)).abs() < 1e-9);
        let right = para("HO\nH", TextAlign::Right, None).lines();
        assert!((right[1].1.x - (100.0 - narrow)).abs() < 1e-9);
        // The outlines are where the lines are: the second line's are a spacing lower.
        let contours = para("H\nH", TextAlign::Left, None).contours();
        let ys: Vec<f64> = contours
            .iter()
            .flatten()
            .map(|s| s.bounds().min.y)
            .collect();
        let (top, bottom) = (
            ys.iter().copied().fold(f64::MIN, f64::max),
            ys.iter().copied().fold(f64::MAX, f64::min),
        );
        assert!(
            (bottom - 35.0).abs() < 1e-6 && top >= 50.0 - 1e-6,
            "{bottom} {top}"
        );
        // Turned a quarter turn, lines stack to the right (down, turned).
        let mut turned = para("H\nH", TextAlign::Left, None);
        turned.angle = 90.0;
        assert!(turned.lines()[1].1.distance(DVec2::new(115.0, 50.0)) < 1e-9);

        // Wrapping: lines break at spaces to stay within the width; the text's own breaks
        // stay; a word wider than the width has a line to itself.
        let text = "HO HO HO HO\nHOHOHOHOHO H";
        let width = line_width("HO HO", &font, 10.0) + 0.01;
        let lines = paragraph_lines(text, &font, 10.0, Some(width));
        assert_eq!(lines, ["HO HO", "HO HO", "HOHOHOHOHO", "H"]);
        assert!(
            lines[..2]
                .iter()
                .all(|l| line_width(l, &font, 10.0) <= width)
        );
        assert_eq!(paragraph_lines(text, &font, 10.0, None).len(), 2);
        // A blank line is a line.
        assert_eq!(paragraph_lines("A\n\nB", &font, 10.0, None), ["A", "", "B"]);
    }
}
