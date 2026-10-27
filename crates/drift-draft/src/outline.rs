//! Glyph outlines for sketch text: a line of text as closed contours of lines and cubic
//! Béziers, at a cap height, from its baseline start, turned by an angle. Quadratic glyph
//! curves (TrueType) are raised to cubics exactly.

use std::collections::HashMap;
use std::ops::Range;
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

/// Each character's advance in font units, and the font's cap height in the same units.
fn advance_units(text: &str, font: &TextFont) -> (Vec<f64>, f64) {
    let (data, index) = font_data(font);
    let Ok(f) = FontRef::from_index(&data, index) else {
        return (vec![0.0; text.chars().count()], 1.0);
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
    let units = text
        .chars()
        .map(|ch| {
            charmap
                .map(ch)
                .and_then(|g| advances.advance_width(g))
                .map_or(space, f64::from)
        })
        .collect();
    (units, cap)
}

/// How wide `text` is in `font` with capitals `height` tall (advance widths, so a trailing
/// space counts).
pub fn line_width(text: &str, font: &TextFont, height: f64) -> f64 {
    let (units, cap) = advance_units(text, font);
    units.iter().sum::<f64>() * height / cap
}

/// How far each character of `text` moves along the line (see [`line_width`]).
pub fn advances(text: &str, font: &TextFont, height: f64) -> Vec<f64> {
    let (units, cap) = advance_units(text, font);
    units.into_iter().map(|u| u * height / cap).collect()
}

/// The lines a text is set in, as byte ranges of it: where it breaks itself (`\n`), and,
/// with a `wrap` width, at the spaces that keep each line within it (the space at a break
/// belongs to neither line). A word longer than the width has a line to itself.
pub fn paragraph_spans(
    text: &str,
    font: &TextFont,
    height: f64,
    wrap: Option<f64>,
) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut from = 0;
    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        let next = from + raw.len() + 1;
        let Some(width) = wrap.filter(|w| *w > 0.0) else {
            spans.push(from..from + line.len());
            from = next;
            continue;
        };
        let (mut start, mut end, mut at) = (from, from, from);
        for word in line.split(' ') {
            let word_end = at + word.len();
            if end > start && line_width(&text[start..word_end], font, height) > width {
                spans.push(start..end);
                start = at;
            }
            end = word_end;
            at = word_end + 1;
        }
        spans.push(start..end);
        from = next;
    }
    spans
}

/// The lines a text is set in (see [`paragraph_spans`]).
pub fn paragraph_lines(text: &str, font: &TextFont, height: f64, wrap: Option<f64>) -> Vec<String> {
    paragraph_spans(text, font, height, wrap)
        .into_iter()
        .map(|span| text[span].to_owned())
        .collect()
}

/// The word around the gap before character `index`: the characters it starts and ends
/// before. Between words, the gap itself.
pub fn word_range(text: &str, index: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let index = index.min(chars.len());
    let part = |c: &char| c.is_alphanumeric() || *c == '_' || *c == '\'';
    let before = chars[..index].iter().rev().take_while(|c| part(c)).count();
    let after = chars[index..].iter().take_while(|c| part(c)).count();
    (index - before, index + after)
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

/// How far a caret and a selection reach below and above the baseline, in cap heights.
const CARET: (f64, f64) = (0.28, 1.14);

impl Paragraph<'_> {
    fn spans(&self) -> Vec<Range<usize>> {
        paragraph_spans(self.text, self.font, self.height, self.wrap)
    }

    /// From along the first baseline and up from it, to the drawing.
    fn out(&self, local: DVec2) -> DVec2 {
        self.at + DVec2::from_angle(self.angle.to_radians()).rotate(local)
    }

    /// Where line `k` starts, along and up from the anchor.
    fn start(&self, k: usize, span: &Range<usize>) -> DVec2 {
        let line = &self.text[span.clone()];
        DVec2::new(
            -self.align.factor() * line_width(line.trim_end(), self.font, self.height),
            -(k as f64) * self.spacing * self.height,
        )
    }

    /// The byte a character index is at.
    fn byte(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(b, _)| b)
    }

    fn chars_before(&self, byte: usize) -> usize {
        self.text[..byte].chars().count()
    }

    /// The line the gap before byte `at` is on.
    fn line_of(spans: &[Range<usize>], at: usize) -> usize {
        spans
            .iter()
            .rposition(|s| s.start <= at)
            .unwrap_or_default()
    }

    /// The gap before character `index`: its line, and where it is along and up from the
    /// anchor.
    fn locate(&self, index: usize) -> (usize, DVec2) {
        let spans = self.spans();
        let at = self.byte(index);
        let k = Self::line_of(&spans, at);
        let span = &spans[k];
        let along = line_width(
            &self.text[span.start..at.clamp(span.start, span.end)],
            self.font,
            self.height,
        );
        (k, self.start(k, span) + DVec2::new(along, 0.0))
    }

    /// The gap on line `k` nearest `x` (along from the anchor), as a character index.
    fn gap_on(&self, k: usize, span: &Range<usize>, x: f64) -> usize {
        let line = &self.text[span.clone()];
        let mut from = self.start(k, span).x;
        let mut gap = 0;
        for advance in advances(line, self.font, self.height) {
            if x < from + advance / 2.0 {
                break;
            }
            from += advance;
            gap += 1;
        }
        self.chars_before(span.start) + gap
    }

    /// The caret before character `index`: its foot and its head, a little below the
    /// baseline to a little above the capitals.
    pub fn caret(&self, index: usize) -> [DVec2; 2] {
        let (_, base) = self.locate(index);
        let up = DVec2::new(0.0, self.height);
        [self.out(base - up * CARET.0), self.out(base + up * CARET.1)]
    }

    /// The gap between characters nearest `p`, as a character index.
    pub fn index_at(&self, p: DVec2) -> usize {
        let local = DVec2::from_angle(-self.angle.to_radians()).rotate(p - self.at);
        let spans = self.spans();
        let pitch = (self.spacing * self.height).abs().max(1e-9);
        // Lines are picked by their middles: half a cap height above each baseline.
        let k = ((self.height / 2.0 - local.y) / pitch)
            .round()
            .clamp(0.0, (spans.len() - 1) as f64) as usize;
        self.gap_on(k, &spans[k], local.x)
    }

    /// The gap `by` lines below (above, if negative) the one before `index`, as near under
    /// it as the line allows. Off the top is the text's start, off the bottom its end.
    pub fn line_step(&self, index: usize, by: isize) -> usize {
        let spans = self.spans();
        let (k, at) = self.locate(index);
        match k.checked_add_signed(by) {
            None => 0,
            Some(to) if to >= spans.len() => self.text.chars().count(),
            Some(to) => self.gap_on(to, &spans[to], at.x),
        }
    }

    /// The first and last gaps of the line the gap before `index` is on.
    pub fn line_ends(&self, index: usize) -> (usize, usize) {
        let spans = self.spans();
        let span = &spans[Self::line_of(&spans, self.byte(index))];
        (self.chars_before(span.start), self.chars_before(span.end))
    }

    /// The boxes behind the characters from gap `from` to gap `to`, one per line (corners
    /// in order). A line break inside the range shows as a little extra at the line's end.
    pub fn selection(&self, from: usize, to: usize) -> Vec<[DVec2; 4]> {
        let (from, to) = (self.byte(from.min(to)), self.byte(from.max(to)));
        if from == to {
            return Vec::new();
        }
        let width = |a: usize, b: usize| line_width(&self.text[a..b], self.font, self.height);
        let (low, high) = (self.height * CARET.0, self.height * CARET.1);
        self.spans()
            .iter()
            .enumerate()
            .filter(|(_, span)| from <= span.end && to >= span.start)
            .filter_map(|(k, span)| {
                let (a, b) = (from.max(span.start), to.min(span.end));
                let start = self.start(k, span);
                let x0 = start.x + width(span.start, a);
                let mut x1 = start.x + width(span.start, b);
                if to > span.end {
                    x1 += 0.3 * self.height;
                }
                (x1 > x0).then(|| {
                    [
                        self.out(DVec2::new(x0, start.y - low)),
                        self.out(DVec2::new(x1, start.y - low)),
                        self.out(DVec2::new(x1, start.y + high)),
                        self.out(DVec2::new(x0, start.y + high)),
                    ]
                })
            })
            .collect()
    }

    /// Each line with where its baseline starts.
    pub fn lines(&self) -> Vec<(String, DVec2)> {
        self.spans()
            .iter()
            .enumerate()
            .map(|(k, span)| {
                (
                    self.text[span.clone()].to_owned(),
                    self.out(self.start(k, span)),
                )
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

/// The size a font is set at (its em) for capitals `height` tall.
pub fn em_size(font: &TextFont, height: f64) -> f64 {
    let (data, index) = font_data(font);
    let Ok(f) = FontRef::from_index(&data, index) else {
        return height / 0.7;
    };
    let metrics = f.metrics(Size::unscaled(), LocationRef::default());
    let em = f64::from(metrics.units_per_em.max(1));
    let cap = metrics
        .cap_height
        .map(f64::from)
        .filter(|c| *c > 0.0)
        .unwrap_or(0.7 * em);
    height * em / cap
}

/// Glyph contours as closed rings of points (curves flattened), for filling.
pub fn contour_rings(contours: &[Vec<Shape>]) -> Vec<Vec<DVec2>> {
    contours
        .iter()
        .map(|contour| {
            let mut ring = Vec::new();
            for piece in contour {
                match *piece {
                    Shape::Line { a, b } => {
                        if ring.is_empty() {
                            ring.push(a);
                        }
                        ring.push(b);
                    }
                    Shape::Bezier { p } => {
                        if ring.is_empty() {
                            ring.push(p[0]);
                        }
                        for k in 1..=8 {
                            let t = f64::from(k) / 8.0;
                            ring.push(drift_core::shape::bezier_point(&p, t));
                        }
                    }
                    _ => {}
                }
            }
            ring
        })
        .filter(|r| r.len() >= 3)
        .collect()
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

    #[test]
    fn carets_and_selections_follow_the_lines() {
        use drift_core::TextAlign;
        let font = TextFont::Sans;
        let w = |t: &str| line_width(t, &font, 10.0);
        let mut para = Paragraph {
            text: "HO HO\nH\n\nOH",
            font: &font,
            height: 10.0,
            at: DVec2::new(100.0, 50.0),
            angle: 0.0,
            align: TextAlign::Left,
            spacing: 1.5,
            wrap: None,
        };
        // A caret stands on its line's baseline, as far along as the letters before it.
        let foot = |p: &Paragraph<'_>, i| {
            let [a, b] = p.caret(i);
            a + (b - a) * CARET.0 / (CARET.0 + CARET.1)
        };
        assert!(foot(&para, 0).distance(DVec2::new(100.0, 50.0)) < 1e-9);
        assert!(foot(&para, 2).distance(DVec2::new(100.0 + w("HO"), 50.0)) < 1e-9);
        assert!(foot(&para, 5).distance(DVec2::new(100.0 + w("HO HO"), 50.0)) < 1e-9);
        // After the break it is at the start of the next line; an empty line has one too.
        assert!(foot(&para, 6).distance(DVec2::new(100.0, 35.0)) < 1e-9);
        assert!(foot(&para, 8).distance(DVec2::new(100.0, 20.0)) < 1e-9);
        assert!(foot(&para, 11).distance(DVec2::new(100.0 + w("OH"), 5.0)) < 1e-9);
        let [a, b] = para.caret(0);
        assert!(
            (b - a).x.abs() < 1e-9 && (b - a).y > 10.0,
            "upright, taller than a capital"
        );

        // A point finds the nearest gap on the nearest line; and every gap finds itself.
        assert_eq!(para.index_at(DVec2::new(100.0 + w("H") * 0.4, 55.0)), 0);
        assert_eq!(para.index_at(DVec2::new(100.0 + w("H") * 0.6, 55.0)), 1);
        assert_eq!(
            para.index_at(DVec2::new(500.0, 55.0)),
            5,
            "past the end of the line"
        );
        assert_eq!(
            para.index_at(DVec2::new(0.0, 38.0)),
            6,
            "before the second line"
        );
        assert_eq!(para.index_at(DVec2::new(130.0, 24.0)), 8, "the empty line");
        assert_eq!(
            para.index_at(DVec2::new(500.0, -900.0)),
            11,
            "below everything"
        );
        assert_eq!(para.index_at(DVec2::new(0.0, 900.0)), 0, "above everything");
        for i in 0..=11 {
            assert_eq!(para.index_at(foot(&para, i) + DVec2::new(0.0, 4.0)), i);
        }

        // Up and down keep the place along the line as well as the lines allow.
        assert_eq!(para.line_step(4, 1), 7, "the second line is shorter");
        assert_eq!(para.line_step(1, 1), 7);
        assert_eq!(para.line_step(0, 1), 6);
        assert_eq!(para.line_step(7, -1), 1);
        assert_eq!(para.line_step(3, -1), 0, "off the top: the start");
        assert_eq!(para.line_step(9, 1), 11, "off the bottom: the end");
        assert_eq!(para.line_step(7, 2), 9 + usize::from(w("H") > w("O") / 2.0));
        assert_eq!(para.line_ends(3), (0, 5));
        assert_eq!(para.line_ends(6), (6, 7));
        assert_eq!(para.line_ends(8), (8, 8));
        assert_eq!(para.line_ends(11), (9, 11));

        // A selection is a box per line it touches; a break inside it shows at the line end.
        assert!(para.selection(3, 3).is_empty());
        let one = para.selection(5, 3);
        assert_eq!(one.len(), 1);
        assert!(one[0][0].distance(DVec2::new(100.0 + w("HO "), 50.0 - 2.8)) < 1e-9);
        assert!(one[0][2].distance(DVec2::new(100.0 + w("HO HO"), 50.0 + 11.4)) < 1e-9);
        let all = para.selection(0, 11);
        assert_eq!(all.len(), 4, "the empty line shows its break");
        assert!((all[0][1].x - (100.0 + w("HO HO") + 3.0)).abs() < 1e-9);
        assert!((all[2][1].x - all[2][0].x - 3.0).abs() < 1e-9);
        assert!(
            (all[3][1].x - (100.0 + w("OH"))).abs() < 1e-9,
            "nothing after the end"
        );
        assert_eq!(para.selection(5, 6).len(), 1, "just the break");

        // Centred lines: carets and picks move with them.
        para.align = TextAlign::Center;
        assert!(foot(&para, 6).distance(DVec2::new(100.0 - w("H") / 2.0, 35.0)) < 1e-9);
        assert_eq!(para.index_at(DVec2::new(100.0 + w("H"), 38.0)), 7);
        assert_eq!(
            para.index_at(DVec2::new(100.0 - w("HO HO") / 2.0 - 1.0, 55.0)),
            0
        );
        // Turned a quarter turn, everything turns about the anchor.
        para.align = TextAlign::Left;
        para.angle = 90.0;
        assert!(foot(&para, 2).distance(DVec2::new(100.0, 50.0 + w("HO"))) < 1e-9);
        assert!(foot(&para, 6).distance(DVec2::new(115.0, 50.0)) < 1e-9);
        assert_eq!(para.index_at(DVec2::new(96.0, 50.0 + w("HO") + 0.1)), 2);
        assert_eq!(para.index_at(DVec2::new(111.0, 49.0)), 6);
        let [a, b] = para.caret(0);
        assert!((b - a).y.abs() < 1e-9 && (b - a).x < -10.0);

        // Wrapped: the space at a break belongs to the line before it, and a caret after it
        // starts the next line.
        let text = "HO HO HO";
        let wrapped = Paragraph {
            text,
            wrap: Some(w("HO HO") + 0.01),
            angle: 0.0,
            ..para
        };
        assert_eq!(
            paragraph_spans(text, &font, 10.0, wrapped.wrap),
            [0..5, 6..8]
        );
        assert!(foot(&wrapped, 5).distance(DVec2::new(100.0 + w("HO HO"), 50.0)) < 1e-9);
        assert!(foot(&wrapped, 6).distance(DVec2::new(100.0, 35.0)) < 1e-9);
        assert_eq!(wrapped.line_ends(7), (6, 8));
        assert_eq!(wrapped.line_step(1, 1), 7);
        // Leading and doubled spaces are kept as typed.
        assert_eq!(
            paragraph_lines(" A  B", &font, 10.0, Some(1000.0)),
            [" A  B"]
        );

        // Words.
        assert_eq!(word_range("see detail 4a.", 6), (4, 10));
        assert_eq!(word_range("see detail 4a.", 4), (4, 10));
        assert_eq!(word_range("see detail 4a.", 10), (4, 10));
        assert_eq!(word_range("see detail 4a.", 14), (14, 14));
        assert_eq!(word_range("a  b", 2), (2, 2));
        assert_eq!(word_range("Ø12", 99), (0, 3));
    }
}
