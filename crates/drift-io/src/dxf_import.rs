//! Reading ASCII DXF: geometry (lines, arcs, circles, ellipses, polylines, splines, solids,
//! leaders), text (TEXT, MTEXT, attributes), dimensions (as the geometry drawn for them),
//! hatches, blocks (nested, arrayed and unevenly scaled ones included), layers and line
//! types, and paper-space layouts with their viewports.
//!
//! The reader keeps model space and paper space apart: model-space entities become the
//! sketch; each layout becomes a sheet whose viewports are its views. What a Drift sketch
//! cannot hold is counted in [`DxfImport::skipped`] or explained in [`DxfImport::notes`]
//! rather than dropped silently.
// Reads files from anywhere: bad input is an error or is skipped, never a panic
// (tests/import_robustness.rs). Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::TAU;

use drift_core::{
    DVec2, Document, EntityId, Fill, FillKind, Hatch, Layer, LayerId, LineType, LineWeight,
    PaperSize, Rgb, Shape, Sheet, SketchText, TextAlign, TextFont, TitleBlock, View, Viewport,
};
use drift_draft::{PatLine, Pattern};

use crate::dxf::{aci_rgb, bulge_arc, parse_line_type, unit_scale};

/// How deep blocks may nest (a block that contains itself stops here).
const MAX_DEPTH: usize = 16;
/// The most copies an arrayed insert (MINSERT) is expanded to.
const MAX_ARRAY: usize = 10_000;

/// A curve read from a DXF file (world millimetres).
#[derive(Clone, Debug, PartialEq)]
pub struct DxfEntity {
    pub layer: String,
    pub shape: Shape,
    pub line_type: Option<LineType>,
    pub color: Option<Rgb>,
    pub weight: Option<LineWeight>,
}

/// Text of one or more lines (`\n` between them): its anchor on the first baseline (at the
/// start, middle or end of each line, per `align`), cap height, and angle in degrees.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfText {
    pub layer: String,
    pub text: String,
    pub at: DVec2,
    pub height: f64,
    pub angle: f64,
    pub color: Option<Rgb>,
    pub align: TextAlign,
    /// Baseline to baseline in cap heights, where the file says.
    pub spacing: Option<f64>,
    /// The width lines are wrapped to (mm), for paragraph text that has one.
    pub wrap: Option<f64>,
}

/// A hatch or solid fill: its boundary loops as curves.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfHatch {
    pub layer: String,
    pub loops: Vec<Vec<Shape>>,
    pub solid: bool,
    pub pattern: String,
    pub scale: f64,
    /// Degrees.
    pub angle: f64,
    /// The pattern's line families at scale 1 and angle 0, in millimetres (at real size:
    /// model-space hatches), as the file defines them; empty where it doesn't.
    pub definition: Vec<PatLine>,
    /// Where the pattern's first family passes through (mm).
    pub origin: DVec2,
    pub color: Option<Rgb>,
    /// A solid fill's opacity (0–1); `None` is Drift's default.
    pub opacity: Option<f64>,
    /// Points inside it, as the file gives them.
    pub seeds: Vec<DVec2>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DxfLayer {
    pub name: String,
    pub color: Option<Rgb>,
    pub line_type: LineType,
    pub weight: Option<LineWeight>,
    /// Neither off nor frozen.
    pub visible: bool,
    pub locked: bool,
    pub printable: bool,
}

/// A block definition read from a DXF file: its curves and text relative to its base point.
/// Blocks inside it are already expanded.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfBlock {
    /// What its inserts name it by. Unique in the import: a block placed in different
    /// "by block" colours, line types or weights is one block per look, and those have
    /// longer names.
    pub name: String,
    /// The name to show: the file's own, or a dynamic block's real name for its anonymous
    /// copies (`*U12`).
    pub label: String,
    pub entities: Vec<DxfEntity>,
    pub texts: Vec<DxfText>,
    /// Hatches and solid fills, in the block's coordinates.
    pub hatches: Vec<DxfHatch>,
}

/// A placed copy of a block (an INSERT scaled evenly; others are expanded into curves).
#[derive(Clone, Debug, PartialEq)]
pub struct DxfInsert {
    pub block: String,
    pub layer: String,
    pub at: DVec2,
    /// Degrees.
    pub angle: f64,
    pub scale: f64,
    pub mirror: bool,
}

/// A viewport of a layout: where it is on the paper, and what of the model it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfViewport {
    /// Its middle and size on the paper (mm).
    pub at: DVec2,
    pub size: DVec2,
    /// The model point at its middle (mm), and the scale 1 : n.
    pub center: DVec2,
    pub scale: f64,
}

/// A paper-space layout.
#[derive(Clone, Debug, PartialEq)]
pub struct DxfLayout {
    pub name: String,
    /// Paper width and height (mm), if the file says.
    pub paper: Option<DVec2>,
    pub views: Vec<DxfViewport>,
    /// Things drawn on the layout itself (title blocks, notes), which aren't imported.
    pub dropped: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DxfImport {
    pub entities: Vec<DxfEntity>,
    pub texts: Vec<DxfText>,
    pub hatches: Vec<DxfHatch>,
    pub layers: Vec<DxfLayer>,
    pub blocks: Vec<DxfBlock>,
    pub inserts: Vec<DxfInsert>,
    pub layouts: Vec<DxfLayout>,
    /// How many dimensions were read (as the lines, arrows and text drawn for them).
    pub dimensions: usize,
    /// Entity types that were skipped, with counts.
    pub skipped: BTreeMap<String, usize>,
    /// What was read approximately, in sentences for the user.
    pub notes: Vec<String>,
    /// Edges added only to bound a fill come in as construction (they don't print).
    pub fill_edges_construction: bool,
}

// ---------------------------------------------------------------------------------------------
// Transforms

/// An affine map: `p → a·p.x + b·p.y + t`.
#[derive(Clone, Copy, Debug)]
struct Xf {
    a: DVec2,
    b: DVec2,
    t: DVec2,
}

impl Xf {
    const IDENTITY: Xf = Xf {
        a: DVec2::X,
        b: DVec2::Y,
        t: DVec2::ZERO,
    };
    /// The mirror an entity drawn "from behind" (extrusion 0, 0, -1) is seen through.
    const FLIP: Xf = Xf {
        a: DVec2::NEG_X,
        b: DVec2::Y,
        t: DVec2::ZERO,
    };

    fn scale(k: f64) -> Xf {
        Xf {
            a: DVec2::X * k,
            b: DVec2::Y * k,
            t: DVec2::ZERO,
        }
    }

    fn translate(t: DVec2) -> Xf {
        Xf { t, ..Xf::IDENTITY }
    }

    fn point(&self, p: DVec2) -> DVec2 {
        self.a * p.x + self.b * p.y + self.t
    }

    /// A direction or offset (no translation).
    fn vector(&self, v: DVec2) -> DVec2 {
        self.a * v.x + self.b * v.y
    }

    /// `self` after `inner`.
    fn then(&self, inner: &Xf) -> Xf {
        Xf {
            a: self.vector(inner.a),
            b: self.vector(inner.b),
            t: self.point(inner.t),
        }
    }

    /// An insert: scale (per axis), turn by `degrees`, move to `at`.
    fn insert(at: DVec2, degrees: f64, sx: f64, sy: f64) -> Xf {
        let r = DVec2::from_angle(degrees.to_radians());
        Xf {
            a: r * sx,
            b: r.perp() * sy,
            t: at,
        }
    }

    fn det(&self) -> f64 {
        self.a.perp_dot(self.b)
    }

    /// Whether lengths grow the same in every direction (so circles stay circles).
    fn is_even(&self) -> bool {
        let (la, lb) = (self.a.length(), self.b.length());
        (la - lb).abs() <= 1e-9 * la.max(lb).max(1e-300)
            && self.a.dot(self.b).abs() <= 1e-9 * la * lb
    }

    fn shape(&self, s: &Shape) -> Shape {
        match *s {
            Shape::Point(p) => Shape::Point(self.point(p)),
            Shape::Line { a, b } => Shape::Line {
                a: self.point(a),
                b: self.point(b),
            },
            Shape::Bezier { p } => Shape::Bezier {
                p: p.map(|q| self.point(q)),
            },
            Shape::Circle { center, radius } => conic(
                self.point(center),
                self.a * radius,
                self.b * radius,
                0.0,
                TAU,
            ),
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => conic(
                self.point(center),
                self.a * radius,
                self.b * radius,
                start,
                sweep,
            ),
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                let other = major.normalize_or_zero().perp() * minor;
                conic(
                    self.point(center),
                    self.vector(major),
                    self.vector(other),
                    start,
                    sweep,
                )
            }
        }
    }
}

/// The curve `c + u·cos t + v·sin t` for `t` from `t0` over `sweep` (a whole turn or more is
/// the whole curve): a circle or arc where it is one, else an ellipse.
fn conic(c: DVec2, u: DVec2, v: DVec2, t0: f64, sweep: f64) -> Shape {
    let whole = sweep.abs() >= TAU - 1e-9;
    // Turn the parameter so the two vectors are the curve's own axes.
    let phi = 0.5 * (2.0 * u.dot(v)).atan2(u.length_squared() - v.length_squared());
    let (sin, cos) = phi.sin_cos();
    let (mut major, mut minor, mut shift) = (u * cos + v * sin, v * cos - u * sin, phi);
    if minor.length() > major.length() {
        (major, minor, shift) = (minor, -major, phi + TAU / 4.0);
    }
    // Drift's ellipses run counter-clockwise from the major axis.
    let reversed = major.perp_dot(minor) < 0.0;
    let (start, sweep) = if reversed {
        (-(t0 - shift + sweep), sweep)
    } else {
        (t0 - shift, sweep)
    };
    let (a, b) = (major.length(), minor.length());
    if (a - b).abs() <= 1e-9 * a.max(1e-300) {
        if whole {
            return Shape::Circle {
                center: c,
                radius: a,
            };
        }
        return Shape::Arc {
            center: c,
            radius: a,
            start: (major.y.atan2(major.x) + start).rem_euclid(TAU),
            sweep,
        };
    }
    Shape::Ellipse {
        center: c,
        major,
        minor: b,
        start: if whole { 0.0 } else { start.rem_euclid(TAU) },
        sweep: if whole { TAU } else { sweep },
    }
}

// ---------------------------------------------------------------------------------------------
// The file as records

/// One entity (or table entry, or object): its type, its group codes, and the entities that
/// belong to it (a polyline's vertices, an insert's attributes).
#[derive(Clone, Debug)]
struct Raw<'a> {
    kind: &'a str,
    g: Vec<(i32, &'a str)>,
    kids: Vec<Raw<'a>>,
}

impl<'a> Raw<'a> {
    fn f(&self, code: i32) -> Option<f64> {
        self.g
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
    }

    fn int(&self, code: i32) -> Option<i64> {
        self.f(code).map(|v| v as i64)
    }

    fn s(&self, code: i32) -> Option<&'a str> {
        self.g.iter().find(|(c, _)| *c == code).map(|(_, v)| *v)
    }

    fn p(&self, x: i32) -> Option<DVec2> {
        Some(DVec2::new(self.f(x)?, self.f(x + 10)?))
    }

    /// Drawn "from behind": its coordinates are mirrored left to right.
    fn flipped(&self) -> bool {
        self.f(230).is_some_and(|z| z < 0.0)
    }

    fn paper(&self) -> bool {
        self.int(67) == Some(1)
    }
}

/// The records from `i` up to (not including) the next record whose type is in `stops`.
fn records<'a>(pairs: &[(i32, &'a str)], i: &mut usize, stops: &[&str]) -> Vec<Raw<'a>> {
    let mut out: Vec<Raw<'a>> = Vec::new();
    // The record that is collecting vertices or attributes until SEQEND.
    let mut open: Option<usize> = None;
    while *i < pairs.len() {
        let (code, kind) = pairs[*i];
        if code != 0 {
            *i += 1;
            continue;
        }
        if stops.contains(&kind) {
            break;
        }
        *i += 1;
        let start = *i;
        while *i < pairs.len() && pairs[*i].0 != 0 {
            *i += 1;
        }
        let raw = Raw {
            kind,
            g: pairs[start..*i].to_vec(),
            kids: Vec::new(),
        };
        match (kind, open) {
            ("SEQEND", _) => open = None,
            ("VERTEX" | "ATTRIB", Some(parent)) => out[parent].kids.push(raw),
            _ => {
                let collects = kind == "POLYLINE" || (kind == "INSERT" && raw.int(66) == Some(1));
                out.push(raw);
                open = collects.then_some(out.len() - 1);
            }
        }
    }
    out
}

struct RawBlock<'a> {
    name: &'a str,
    base: DVec2,
    entities: Vec<Raw<'a>>,
}

struct RawLayout {
    name: String,
    /// The handle of its paper-space block record.
    record: String,
    paper: Option<DVec2>,
    margin: DVec2,
    /// Millimetres per paper unit.
    unit: f64,
    order: i64,
}

// ---------------------------------------------------------------------------------------------
// Text

/// A character from a DXF escape's hex digits.
fn hex_char(digits: &str) -> Option<char> {
    u32::from_str_radix(digits, 16)
        .ok()
        .and_then(char::from_u32)
}

/// Plain text from a TEXT / ATTRIB string: `%%c`, `%%d`, `%%p` and `\U+XXXX` decoded, the
/// underline and overline toggles dropped.
fn plain_text(raw: &str) -> String {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(c) = rest.chars().next() {
        if let Some(tail) = rest.strip_prefix("%%") {
            let mut chars = tail.chars();
            match chars.next().map(|c| c.to_ascii_lowercase()) {
                Some('c') => out.push('Ø'),
                Some('d') => out.push('°'),
                Some('p') => out.push('±'),
                Some('%') => out.push('%'),
                Some('u' | 'o' | 'k') => {}
                Some(d) if d.is_ascii_digit() => {
                    // %%nnn: a character by its code.
                    let digits: String = tail.chars().take(3).collect();
                    if digits.len() == 3
                        && let Some(ch) = digits.parse::<u32>().ok().and_then(char::from_u32)
                    {
                        out.push(ch);
                        rest = &tail[3..];
                        continue;
                    }
                    out.push_str("%%");
                    rest = tail;
                    continue;
                }
                _ => {
                    out.push_str("%%");
                    rest = tail;
                    continue;
                }
            }
            rest = chars.as_str();
            continue;
        }
        if let Some(tail) = rest.strip_prefix("\\U+")
            && tail.len() >= 4
            && tail.is_char_boundary(4)
            && let Some(ch) = hex_char(&tail[..4])
        {
            out.push(ch);
            rest = &tail[4..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// The lines of an MTEXT string, without its formatting codes.
fn mtext_lines(raw: &str) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut chars = raw.chars().peekable();
    let push = |lines: &mut Vec<String>, c: char| {
        if let Some(line) = lines.last_mut() {
            line.push(c);
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('P' | 'N' | 'X') => lines.push(String::new()),
                Some('~') => push(&mut lines, ' '),
                Some(c @ ('\\' | '{' | '}')) => push(&mut lines, c),
                // Toggles: underline, overline, strike-through.
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                Some('U') if chars.peek() == Some(&'+') => {
                    chars.next();
                    let digits: String = chars.by_ref().take(4).collect();
                    if let Some(ch) = hex_char(&digits) {
                        push(&mut lines, ch);
                    }
                }
                // A fraction or tolerance stack: its parts, with a slash.
                Some('S') => {
                    for d in chars.by_ref() {
                        match d {
                            ';' => break,
                            '^' | '#' => push(&mut lines, '/'),
                            d => push(&mut lines, d),
                        }
                    }
                }
                // Font, height, colour, tracking, width, alignment, paragraph: up to the
                // next semicolon.
                Some(
                    'f' | 'F' | 'H' | 'h' | 'C' | 'c' | 'T' | 't' | 'Q' | 'q' | 'W' | 'w' | 'A'
                    | 'a' | 'p' | 'M',
                ) => {
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                Some(other) => push(&mut lines, other),
                None => {}
            },
            '{' | '}' => {}
            '^' => match chars.peek() {
                Some('I') => {
                    chars.next();
                    push(&mut lines, ' ');
                }
                Some('J') => {
                    chars.next();
                    lines.push(String::new());
                }
                Some('M') => {
                    chars.next();
                }
                _ => push(&mut lines, '^'),
            },
            c => push(&mut lines, c),
        }
    }
    lines.iter().map(|l| plain_text(l)).collect()
}

/// How wide `text` is at a cap height (in the built-in sans, which is what it is shown in).
#[cfg(test)]
fn text_width(text: &str, height: f64) -> f64 {
    if text.trim().is_empty() {
        return 0.0;
    }
    drift_draft::outline::text_contours(text, &TextFont::Sans, height, DVec2::ZERO, 0.0)
        .iter()
        .flatten()
        .map(|s| s.bounds().max.x)
        .fold(0.0, f64::max)
}

// ---------------------------------------------------------------------------------------------
// Splines

type H = [f64; 3];

fn lerp(a: H, b: H, t: f64) -> H {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn project(h: H) -> DVec2 {
    let w = if h[2].abs() > 1e-12 { h[2] } else { 1.0 };
    DVec2::new(h[0] / w, h[1] / w)
}

/// A Bézier segment (weighted control points, any degree) at `t`.
fn de_casteljau(points: &[H], t: f64) -> DVec2 {
    let mut work = points.to_vec();
    for level in 1..work.len() {
        for i in 0..work.len() - level {
            work[i] = lerp(work[i], work[i + 1], t);
        }
    }
    project(work[0])
}

/// The cubic through four points at t = 0, 1/3, 2/3, 1.
fn cubic_through(p0: DVec2, q1: DVec2, q2: DVec2, p3: DVec2) -> Shape {
    let c1 = (-5.0 * p0 + 18.0 * q1 - 9.0 * q2 + 2.0 * p3) / 6.0;
    let c2 = (2.0 * p0 - 9.0 * q1 + 18.0 * q2 - 5.0 * p3) / 6.0;
    Shape::Bezier {
        p: [p0, c1, c2, p3],
    }
}

/// A B-spline (degree, knots, control points, weights) as lines and cubic Béziers. Degrees 1
/// to 3 without weights are exact; weighted and higher-degree spans are fitted. Returns
/// whether anything was fitted.
fn bspline(degree: usize, knots: &[f64], points: &[DVec2], weights: &[f64]) -> (Vec<Shape>, bool) {
    let n = points.len();
    if degree == 0 || n <= degree {
        return (Vec::new(), false);
    }
    let weighted = weights.len() == n
        && weights
            .iter()
            .any(|w| (w - weights[0]).abs() > 1e-9 * weights[0].abs().max(1e-12));
    let mut pts: Vec<H> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let w = if weighted { weights[i] } else { 1.0 };
            [p.x * w, p.y * w, w]
        })
        .collect();
    // Knots that don't fit the points are replaced by a clamped, even set.
    let mut u: Vec<f64> = if knots.len() == n + degree + 1 && knots.windows(2).all(|w| w[0] <= w[1])
    {
        knots.to_vec()
    } else {
        let spans = (n - degree) as f64;
        (0..n + degree + 1)
            .map(|i| {
                if i <= degree {
                    0.0
                } else if i >= n {
                    spans
                } else {
                    (i - degree) as f64
                }
            })
            .collect()
    };
    let (lo, hi) = (u[degree], u[n]);
    if hi <= lo || !(hi - lo).is_finite() {
        return (Vec::new(), false);
    }
    let eps = 1e-9 * (hi - lo);
    // Every knot of the curve's range to full multiplicity: the control points are then
    // those of its Bézier spans, end to end.
    let mut breaks: Vec<f64> = Vec::new();
    for k in u
        .iter()
        .copied()
        .filter(|k| *k >= lo - eps && *k <= hi + eps)
    {
        if breaks.last().is_none_or(|b| (k - b).abs() > eps) {
            breaks.push(k);
        }
    }
    for &k in &breaks {
        loop {
            let have = u.iter().filter(|x| (**x - k).abs() <= eps).count();
            if have >= degree {
                break;
            }
            // The span holding k (the last one, for the range's end).
            let Some(span) = (0..u.len() - 1).rev().find(|&i| {
                u[i] <= k + eps && (k < u[i + 1] - eps || (k - hi).abs() <= eps && u[i] < u[i + 1])
            }) else {
                break;
            };
            if span < degree {
                break;
            }
            let mut next: Vec<H> = Vec::with_capacity(pts.len() + 1);
            for i in 0..=pts.len() {
                if i + degree <= span {
                    next.push(pts[i]);
                } else if i > span {
                    next.push(pts[i - 1]);
                } else {
                    let den = u[i + degree] - u[i];
                    let a = if den.abs() > 0.0 {
                        (k - u[i]) / den
                    } else {
                        0.0
                    };
                    next.push(lerp(pts[i - 1], pts[i], a.clamp(0.0, 1.0)));
                }
            }
            pts = next;
            u.insert(span + 1, k);
        }
    }
    let Some(last_lo) = u.iter().rposition(|x| (*x - lo).abs() <= eps) else {
        return (Vec::new(), false);
    };
    if last_lo < degree {
        return (Vec::new(), false);
    }
    let first = last_lo - degree;
    let mut out = Vec::new();
    let mut fitted = false;
    for span in 0..breaks.len().saturating_sub(1) {
        let from = first + span * degree;
        let Some(seg) = pts.get(from..=from + degree) else {
            break;
        };
        let (a, b) = (project(seg[0]), project(seg[degree]));
        let shape = match (degree, weighted) {
            (1, _) => Shape::Line { a, b },
            (2, false) => {
                let c = project(seg[1]);
                Shape::Bezier {
                    p: [a, a + (c - a) * (2.0 / 3.0), b + (c - b) * (2.0 / 3.0), b],
                }
            }
            (3, false) => Shape::Bezier {
                p: [a, project(seg[1]), project(seg[2]), b],
            },
            _ => {
                // Fitted in quarters: a quarter circle comes out within a few millionths
                // of its radius.
                fitted = true;
                const PIECES: usize = 4;
                for k in 0..PIECES - 1 {
                    let t = |j: usize| (k as f64 + j as f64 / 3.0) / PIECES as f64;
                    out.push(cubic_through(
                        de_casteljau(seg, t(0)),
                        de_casteljau(seg, t(1)),
                        de_casteljau(seg, t(2)),
                        de_casteljau(seg, t(3)),
                    ));
                }
                let t = |j: usize| ((PIECES - 1) as f64 + j as f64 / 3.0) / PIECES as f64;
                cubic_through(
                    de_casteljau(seg, t(0)),
                    de_casteljau(seg, t(1)),
                    de_casteljau(seg, t(2)),
                    b,
                )
            }
        };
        let degenerate = match shape {
            Shape::Line { a, b } => a.distance(b) <= 1e-12,
            Shape::Bezier { p } => p.iter().all(|q| q.distance(p[0]) <= 1e-12),
            _ => false,
        };
        if !degenerate {
            out.push(shape);
        }
    }
    (out, fitted)
}

/// A smooth curve through `fit` points (a spline stored without control points).
fn through_points(fit: &[DVec2], closed: bool) -> Vec<Shape> {
    let n = fit.len();
    if n < 2 {
        return Vec::new();
    }
    if n == 2 {
        return vec![Shape::Line {
            a: fit[0],
            b: fit[1],
        }];
    }
    let at = |i: i64| -> DVec2 {
        if closed {
            fit[i.rem_euclid(n as i64) as usize]
        } else {
            fit[i.clamp(0, n as i64 - 1) as usize]
        }
    };
    let spans = if closed { n } else { n - 1 };
    (0..spans as i64)
        .map(|i| {
            let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            Shape::Bezier {
                p: [p1, p1 + (p2 - p0) / 6.0, p2 - (p3 - p1) / 6.0, p2],
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Hatch boundaries

/// A hatch's boundary loops, read in order from its group codes (file units).
fn hatch_loops(g: &[(i32, &str)], fitted: &mut bool) -> Vec<Vec<Shape>> {
    let num = |v: &str| {
        v.parse::<f64>()
            .ok()
            .filter(|x| x.is_finite())
            .unwrap_or(0.0)
    };
    let mut loops = Vec::new();
    let Some(mut i) = g.iter().position(|(c, _)| *c == 91) else {
        return loops;
    };
    let count = num(g[i].1) as usize;
    i += 1;
    // The next pair with this code at or after `i`, before the next loop starts.
    let take = |i: &mut usize, code: i32| -> Option<f64> {
        let (c, v) = g.get(*i)?;
        (*c == code).then(|| {
            *i += 1;
            num(v)
        })
    };
    for _ in 0..count.min(100_000) {
        // Skip to this loop's type flag.
        while i < g.len() && g[i].0 != 92 {
            if matches!(g[i].0, 75 | 76 | 98) {
                return loops;
            }
            i += 1;
        }
        let Some(flag) = take(&mut i, 92) else {
            break;
        };
        let mut shapes: Vec<Shape> = Vec::new();
        if (flag as i64) & 2 != 0 {
            // A polyline: has-bulge, closed, count, then the vertices.
            let has_bulge = take(&mut i, 72).unwrap_or(0.0) != 0.0;
            let closed = take(&mut i, 73).unwrap_or(1.0) != 0.0;
            let n = take(&mut i, 93).unwrap_or(0.0) as usize;
            let mut verts: Vec<(DVec2, f64)> = Vec::new();
            for _ in 0..n.min(1_000_000) {
                let (Some(x), Some(y)) = (take(&mut i, 10), take(&mut i, 20)) else {
                    break;
                };
                let bulge = if has_bulge {
                    take(&mut i, 42).unwrap_or(0.0)
                } else {
                    0.0
                };
                verts.push((DVec2::new(x, y), bulge));
            }
            polyline(&verts, closed, &mut |s| shapes.push(s));
        } else {
            let edges = take(&mut i, 93).unwrap_or(0.0) as usize;
            for _ in 0..edges.min(1_000_000) {
                let Some(kind) = take(&mut i, 72) else {
                    break;
                };
                match kind as i64 {
                    1 => {
                        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                            take(&mut i, 10),
                            take(&mut i, 20),
                            take(&mut i, 11),
                            take(&mut i, 21),
                        ) else {
                            break;
                        };
                        shapes.push(Shape::Line {
                            a: DVec2::new(x0, y0),
                            b: DVec2::new(x1, y1),
                        });
                    }
                    2 => {
                        let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) = (
                            take(&mut i, 10),
                            take(&mut i, 20),
                            take(&mut i, 40),
                            take(&mut i, 50),
                            take(&mut i, 51),
                        ) else {
                            break;
                        };
                        let ccw = take(&mut i, 73).unwrap_or(1.0) != 0.0;
                        // A clockwise arc's angles are measured clockwise.
                        let (a0, a1) = if ccw { (a0, a1) } else { (-a1, -a0) };
                        let start = a0.to_radians();
                        let mut sweep = (a1 - a0).to_radians().rem_euclid(TAU);
                        if sweep < 1e-9 {
                            sweep = TAU;
                        }
                        shapes.push(conic(
                            DVec2::new(x, y),
                            DVec2::X * r,
                            DVec2::Y * r,
                            start,
                            sweep,
                        ));
                    }
                    3 => {
                        let (Some(x), Some(y), Some(mx), Some(my), Some(ratio), Some(a0), Some(a1)) = (
                            take(&mut i, 10),
                            take(&mut i, 20),
                            take(&mut i, 11),
                            take(&mut i, 21),
                            take(&mut i, 40),
                            take(&mut i, 50),
                            take(&mut i, 51),
                        ) else {
                            break;
                        };
                        let ccw = take(&mut i, 73).unwrap_or(1.0) != 0.0;
                        let (a0, a1) = if ccw { (a0, a1) } else { (-a1, -a0) };
                        let major = DVec2::new(mx, my);
                        let mut sweep = (a1 - a0).to_radians().rem_euclid(TAU);
                        if sweep < 1e-9 {
                            sweep = TAU;
                        }
                        shapes.push(conic(
                            DVec2::new(x, y),
                            major,
                            major.perp() * ratio,
                            a0.to_radians(),
                            sweep,
                        ));
                    }
                    4 => {
                        let degree = take(&mut i, 94).unwrap_or(3.0) as usize;
                        let _rational = take(&mut i, 73);
                        let _periodic = take(&mut i, 74);
                        let nk = take(&mut i, 95).unwrap_or(0.0) as usize;
                        let nc = take(&mut i, 96).unwrap_or(0.0) as usize;
                        let mut knots = Vec::new();
                        for _ in 0..nk.min(1_000_000) {
                            match take(&mut i, 40) {
                                Some(k) => knots.push(k),
                                None => break,
                            }
                        }
                        let (mut points, mut weights) = (Vec::new(), Vec::new());
                        for _ in 0..nc.min(1_000_000) {
                            let (Some(x), Some(y)) = (take(&mut i, 10), take(&mut i, 20)) else {
                                break;
                            };
                            points.push(DVec2::new(x, y));
                            if let Some(w) = take(&mut i, 42) {
                                weights.push(w);
                            }
                        }
                        let nf = take(&mut i, 97).unwrap_or(0.0) as usize;
                        let mut fit = Vec::new();
                        for _ in 0..nf.min(1_000_000) {
                            let (Some(x), Some(y)) = (take(&mut i, 11), take(&mut i, 21)) else {
                                break;
                            };
                            fit.push(DVec2::new(x, y));
                        }
                        // Start and end tangents, if given.
                        for code in [12, 22, 13, 23] {
                            take(&mut i, code);
                        }
                        if points.is_empty() {
                            shapes.extend(through_points(&fit, false));
                        } else {
                            let (curves, approx) = bspline(degree, &knots, &points, &weights);
                            *fitted |= approx;
                            shapes.extend(curves);
                        }
                    }
                    _ => break,
                }
            }
        }
        // The objects the boundary was made from.
        if let Some(sources) = take(&mut i, 97) {
            for _ in 0..sources as usize {
                if take(&mut i, 330).is_none() {
                    break;
                }
            }
        }
        if !shapes.is_empty() {
            loops.push(shapes);
        }
    }
    loops
}

pub(crate) fn polyline(verts: &[(DVec2, f64)], closed: bool, push: &mut impl FnMut(Shape)) {
    let n = verts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    for k in 0..segments {
        let (a, bulge) = verts[k];
        let b = verts[(k + 1) % n].0;
        if a.distance(b) <= 1e-12 {
            continue;
        }
        push(if bulge.abs() > 1e-12 {
            bulge_arc(a, b, bulge)
        } else {
            Shape::Line { a, b }
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Line types, colours and weights

/// The nearest of Drift's line types to a pattern of dashes (positive), gaps (negative) and
/// dots (zero), in millimetres.
fn classify_line_type(name: &str, pattern: &[f64]) -> LineType {
    let known = parse_line_type(name);
    if known != LineType::Continuous || pattern.is_empty() {
        return known;
    }
    // The marks are the dashes and dots (gaps are negative), as PDF / SVG dashes are read.
    LineType::of_marks(pattern)
}

/// An entity's own colour: a true colour (420), else an indexed one (62). `None` for
/// "by layer" and "by block".
fn own_color(raw: &Raw) -> Option<Rgb> {
    if let Some(rgb) = raw.int(420) {
        let v = rgb as u32;
        return Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]));
    }
    raw.int(62)
        .filter(|c| (1..=255).contains(c))
        .and_then(aci_rgb)
}

/// `v` without the round-off its turning and scaling left: a value within a billionth of
/// a millionth (of a degree, or of a millimetre) is that millionth, so 44.99999999999999°
/// is 45° on every platform, and in the Inspector.
fn tidy(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if (v - r).abs() <= 1e-9 * v.abs().max(1.0) {
        r + 0.0
    } else {
        v
    }
}

/// An angle in degrees in 0..360, tidied.
fn tidy_degrees(a: f64) -> f64 {
    let a = tidy(a.rem_euclid(360.0));
    if a >= 360.0 { a - 360.0 } else { a }
}

/// A family with its numbers tidied.
fn tidy_family(l: &mut PatLine) {
    l.angle = tidy_degrees(l.angle);
    l.origin = DVec2::new(tidy(l.origin.x), tidy(l.origin.y));
    l.delta = DVec2::new(tidy(l.delta.x), tidy(l.delta.y));
    for d in &mut l.dashes {
        *d = tidy(*d);
    }
}

/// The opacity (0–1) a transparency (group 440, or a layer's AcCmTransparency) gives:
/// `0x02000000` plus an alpha from 0 (clear) to 255 (opaque). `None` for "by layer" and "by
/// block".
fn opacity_of(value: i64) -> Option<f64> {
    (value & 0x0200_0000 != 0).then(|| (value & 0xFF) as f64 / 255.0)
}

/// A gradient fill's first colour (true colour 421, else an indexed one 63, after 453).
fn gradient_color(g: &[(i32, &str)]) -> Option<Rgb> {
    let start = g.iter().position(|(c, _)| *c == 453)?;
    for (code, v) in &g[start..] {
        match code {
            421 => {
                let v = v.parse::<i64>().ok()? as u32;
                return Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]));
            }
            63 => return v.parse::<i64>().ok().and_then(aci_rgb),
            _ => {}
        }
    }
    None
}

/// A hatch's pattern line families as the file defines them (groups 53, 43, 44, 45, 46, 79,
/// 49 after 78), taken back to scale 1 and angle 0 and into millimetres: the file stores
/// them scaled by `scale`, turned by `angle` (degrees), with each family's step (45, 46) in
/// drawing coordinates. Also where the first family passes through, which is where the
/// pattern starts (mm).
fn pattern_definition(g: &[(i32, &str)], scale: f64, angle: f64, mm: f64) -> (Vec<PatLine>, DVec2) {
    let num = |v: &str| v.parse::<f64>().ok().filter(|x| x.is_finite());
    let Some(start) = g.iter().position(|(c, _)| *c == 78) else {
        return (Vec::new(), DVec2::ZERO);
    };
    let count = num(g[start].1).unwrap_or(0.0).clamp(0.0, 1000.0) as usize;
    let k = if scale.abs() > 1e-12 {
        scale.abs()
    } else {
        1.0
    };
    let mut lines = Vec::new();
    let mut i = start + 1;
    // The next value with `code` at `i`, if it is there.
    let take = |i: &mut usize, code: i32| -> Option<f64> {
        let (c, v) = g.get(*i)?;
        (*c == code).then(|| {
            *i += 1;
            num(v).unwrap_or(0.0)
        })
    };
    let mut first: Option<DVec2> = None;
    for _ in 0..count {
        let (Some(a), Some(bx), Some(by), Some(ox), Some(oy)) = (
            take(&mut i, 53),
            take(&mut i, 43),
            take(&mut i, 44),
            take(&mut i, 45),
            take(&mut i, 46),
        ) else {
            break;
        };
        let n = take(&mut i, 79).unwrap_or(0.0).clamp(0.0, 100.0) as usize;
        let mut dashes = Vec::with_capacity(n);
        for _ in 0..n {
            match take(&mut i, 49) {
                Some(d) => dashes.push(d / k * mm),
                None => break,
            }
        }
        let base = DVec2::new(bx, by);
        let at = *first.get_or_insert(base);
        // The step, in the family's own frame (along it, across it).
        let step = DVec2::from_angle(-a.to_radians()).rotate(DVec2::new(ox, oy));
        lines.push(PatLine {
            angle: tidy_degrees(a - angle),
            // From the first family's point, unturned and unscaled.
            origin: DVec2::from_angle(-angle.to_radians()).rotate(base - at) / k * mm,
            delta: step / k * mm,
            dashes,
        });
    }
    // A family that never steps across would be one endless line drawn over and over.
    if lines.iter().any(|l| l.delta.y.abs() < 1e-9) {
        return (Vec::new(), DVec2::ZERO);
    }
    (lines, first.unwrap_or(DVec2::ZERO) * mm)
}

/// A line weight from group 370 (hundredths of a millimetre; negative values mean "by
/// layer", "by block" or "default").
fn own_weight(raw: &Raw) -> Option<LineWeight> {
    raw.int(370)
        .filter(|w| *w >= 0)
        .map(|w| LineWeight::nearest_cad(w as f64 / 100.0))
}

// ---------------------------------------------------------------------------------------------
// Converting records

/// What an entity inside a block takes from the insert that places it.
#[derive(Clone, Debug, Default)]
struct Inherit {
    /// The insert's layer: for entities on layer 0.
    layer: Option<String>,
    /// The insert's colour: for entities coloured "by block".
    color: Option<Rgb>,
    /// The insert's line type and weight: for entities whose are "by block".
    line_type: Option<LineType>,
    weight: Option<LineWeight>,
    /// In a block kept for copies: an entity on a layer of its own (not 0) shows that
    /// layer's look where it is "by layer", as AutoCAD draws it, since a block's items have
    /// no layer in Drift (left "by layer", they would take the copy's).
    own_layers: bool,
}

impl Inherit {
    /// A look for "by block" entities to take, besides the default one.
    fn gives_a_look(&self) -> bool {
        self.color.is_some() || self.line_type.is_some() || self.weight.is_some()
    }
}

/// Whether an entity's colour, line type or weight is "by block".
fn by_block(raw: &Raw) -> bool {
    (raw.int(62) == Some(0) && raw.int(420).is_none())
        || raw.s(6).is_some_and(|t| t.eq_ignore_ascii_case("BYBLOCK"))
        || raw.int(370) == Some(-2)
}

/// A block kept for inserts: the block, and the look its "by block" entities take.
struct Kept {
    block: String,
    inherit: Inherit,
}

#[derive(Default)]
struct Sink {
    entities: Vec<DxfEntity>,
    texts: Vec<DxfText>,
    hatches: Vec<DxfHatch>,
    inserts: Vec<DxfInsert>,
}

struct Reader<'a> {
    /// Millimetres per drawing unit.
    scale: f64,
    blocks: HashMap<String, RawBlock<'a>>,
    line_types: HashMap<String, LineType>,
    skipped: BTreeMap<String, usize>,
    notes: BTreeSet<&'static str>,
    dimensions: usize,
    /// Blocks placed by inserts that were kept as inserts, by the name the inserts use.
    used: BTreeMap<String, Kept>,
    /// Whether a block has anything "by block" in it (or in blocks it places "by block").
    by_block: HashMap<String, bool>,
    /// Dynamic blocks' anonymous copies (`*U12`, upper case) and the dynamic block's name.
    true_names: HashMap<String, String>,
    /// External references, by block name (upper case): their name and file.
    xrefs: HashMap<String, (String, String)>,
    /// The external references something places.
    xrefs_used: BTreeSet<String>,
    /// Layers' transparency as opacity, by name (upper case).
    layer_opacity: HashMap<String, f64>,
    /// Layers' colour, line type and weight, by name (upper case): for entities in kept
    /// blocks on layers of their own (see [`Inherit::own_layers`]). Colour 7 is black,
    /// which Drift shows as the theme's foreground and exports as 7.
    layer_looks: HashMap<String, (Rgb, LineType, Option<LineWeight>)>,
    /// Hatch patterns the file names without defining them are AutoCAD's own: acad.pat (in
    /// inches) for imperial drawings, else acadiso.pat (in millimetres).
    imperial: bool,
}

impl<'a> Reader<'a> {
    fn skip(&mut self, kind: &str) {
        *self.skipped.entry(kind.to_owned()).or_insert(0) += 1;
    }

    fn line_type(&self, raw: &Raw) -> Option<LineType> {
        let name = raw.s(6)?;
        if name.eq_ignore_ascii_case("BYLAYER") || name.eq_ignore_ascii_case("BYBLOCK") {
            return None;
        }
        Some(
            self.line_types
                .get(&name.to_ascii_uppercase())
                .copied()
                .unwrap_or_else(|| parse_line_type(name)),
        )
    }

    /// Convert one record into `sink`. `xf` maps the record's own coordinates (file units)
    /// to the sketch (mm). Inserts stay inserts only where `keep_inserts` allows and they
    /// can be (see [`DxfInsert`]); otherwise their block's content is expanded here.
    fn emit(
        &mut self,
        raw: &Raw<'a>,
        xf: &Xf,
        inherit: &Inherit,
        depth: usize,
        keep_inserts: bool,
        sink: &mut Sink,
    ) {
        let own_layer = raw.s(8).unwrap_or("0");
        let layer = match (&inherit.layer, own_layer) {
            (Some(outer), "0") => outer.clone(),
            _ => own_layer.to_owned(),
        };
        // "By layer" in a kept block, on a layer other than 0: that layer's look.
        let layer_look = if inherit.own_layers && layer != "0" {
            self.layer_looks.get(&layer.to_ascii_uppercase()).copied()
        } else {
            None
        };
        // Colour 0 is "by block": the insert's.
        let color = if raw.int(62) == Some(0) && raw.int(420).is_none() {
            inherit.color
        } else {
            own_color(raw).or(layer_look.map(|l| l.0))
        };
        let line_type = if raw.s(6).is_some_and(|t| t.eq_ignore_ascii_case("BYBLOCK")) {
            inherit.line_type
        } else {
            self.line_type(raw).or(layer_look.map(|l| l.1))
        };
        let weight = if raw.int(370) == Some(-2) {
            inherit.weight
        } else {
            own_weight(raw).or(layer_look.and_then(|l| l.2))
        };
        // Entities drawn in their own plane are mirrored when that plane faces away.
        let local = |flips: bool| if flips { xf.then(&Xf::FLIP) } else { *xf };
        let push = |sink: &mut Sink, shape: Shape| {
            sink.entities.push(DxfEntity {
                layer: layer.clone(),
                shape,
                line_type,
                color,
                weight,
            });
        };
        match raw.kind {
            "LINE" => {
                if let (Some(a), Some(b)) = (raw.p(10), raw.p(11))
                    && a != b
                {
                    push(sink, xf.shape(&Shape::Line { a, b }));
                }
            }
            "POINT" => {
                if let Some(a) = raw.p(10) {
                    push(sink, Shape::Point(xf.point(a)));
                }
            }
            "CIRCLE" => {
                if let (Some(center), Some(radius)) = (raw.p(10), raw.f(40))
                    && radius > 0.0
                {
                    push(
                        sink,
                        local(raw.flipped()).shape(&Shape::Circle { center, radius }),
                    );
                }
            }
            "ARC" => {
                if let (Some(center), Some(radius), Some(a0), Some(a1)) =
                    (raw.p(10), raw.f(40), raw.f(50), raw.f(51))
                    && radius > 0.0
                {
                    let start = a0.to_radians();
                    let mut sweep = (a1.to_radians() - start).rem_euclid(TAU);
                    if sweep < 1e-12 {
                        sweep = TAU;
                    }
                    push(
                        sink,
                        local(raw.flipped()).shape(&Shape::Arc {
                            center,
                            radius,
                            start,
                            sweep,
                        }),
                    );
                }
            }
            "ELLIPSE" => {
                // Centre, the major axis end (relative to it), the minor / major ratio and
                // parametric start and end angles (radians). In world coordinates; its
                // normal says which way the angles run.
                if let (Some(c), Some(major), Some(ratio)) = (raw.p(10), raw.p(11), raw.f(40))
                    && major.length() > 0.0
                    && ratio > 0.0
                {
                    let t0 = raw.f(41).unwrap_or(0.0);
                    let t1 = raw.f(42).unwrap_or(TAU);
                    // A whole one may come a hair short of a turn, or as 0 to 0.
                    let mut sweep = (t1 - t0).rem_euclid(TAU);
                    if !(1e-5..=TAU - 1e-5).contains(&sweep) {
                        sweep = TAU;
                    }
                    let side = if raw.flipped() { -1.0 } else { 1.0 };
                    push(
                        sink,
                        conic(
                            xf.point(c),
                            xf.vector(major),
                            xf.vector(major.perp() * ratio * side),
                            t0,
                            sweep,
                        ),
                    );
                }
            }
            "LWPOLYLINE" => {
                let closed = raw.int(70).is_some_and(|v| v & 1 == 1);
                // Vertices: each 10 / 20, optionally followed by a 42 bulge.
                let mut verts: Vec<(DVec2, f64)> = Vec::new();
                let mut x = None;
                for (code, v) in &raw.g {
                    let v: f64 = v.parse().unwrap_or(0.0);
                    match code {
                        10 => x = Some(v),
                        20 => {
                            if let Some(x) = x.take() {
                                verts.push((DVec2::new(x, v), 0.0));
                            }
                        }
                        42 => {
                            if let Some(last) = verts.last_mut() {
                                last.1 = v;
                            }
                        }
                        _ => {}
                    }
                }
                let m = local(raw.flipped());
                polyline(&verts, closed, &mut |s| push(sink, m.shape(&s)));
            }
            "POLYLINE" => {
                let flags = raw.int(70).unwrap_or(0);
                // Meshes and polyface meshes aren't curves.
                if flags & (16 | 64) != 0 {
                    self.skip("POLYLINE (mesh)");
                    return;
                }
                let verts: Vec<(DVec2, f64)> = raw
                    .kids
                    .iter()
                    .filter(|v| v.kind == "VERTEX")
                    // Spline-frame control points of a fitted polyline are not on it.
                    .filter(|v| v.int(70).unwrap_or(0) & 16 == 0)
                    .filter_map(|v| Some((v.p(10)?, v.f(42).unwrap_or(0.0))))
                    .collect();
                // A 3D polyline is in world coordinates; a 2D one in its own plane.
                let m = local(raw.flipped() && flags & 8 == 0);
                polyline(&verts, flags & 1 == 1, &mut |s| push(sink, m.shape(&s)));
            }
            "SPLINE" => {
                let degree = raw.int(71).unwrap_or(3).clamp(1, 32) as usize;
                let closed = raw.int(70).unwrap_or(0) & 1 == 1;
                let mut knots = Vec::new();
                let mut weights = Vec::new();
                let (mut points, mut fit) = (Vec::new(), Vec::new());
                let (mut x, mut fx) = (None, None);
                for (code, v) in &raw.g {
                    let v: f64 = v.parse().unwrap_or(0.0);
                    match code {
                        40 => knots.push(v),
                        41 => weights.push(v),
                        10 => x = Some(v),
                        20 => {
                            if let Some(x) = x.take() {
                                points.push(DVec2::new(x, v));
                            }
                        }
                        11 => fx = Some(v),
                        21 => {
                            if let Some(x) = fx.take() {
                                fit.push(DVec2::new(x, v));
                            }
                        }
                        _ => {}
                    }
                }
                let curves = if points.len() > degree {
                    let (curves, fitted) = bspline(degree, &knots, &points, &weights);
                    if fitted {
                        self.notes.insert(
                            "Some splines (weighted, or above degree 3) were fitted with cubic curves.",
                        );
                    }
                    curves
                } else {
                    through_points(&fit, closed)
                };
                if curves.is_empty() {
                    self.skip("SPLINE (no usable points)");
                }
                for s in curves {
                    push(sink, xf.shape(&s));
                }
            }
            "SOLID" | "TRACE" | "3DFACE" => {
                // Corners 1, 2, 4, 3 go round a SOLID or TRACE; a 3DFACE's are in order.
                let corners: Vec<DVec2> =
                    [10, 11, 12, 13].iter().filter_map(|c| raw.p(*c)).collect();
                let order: &[usize] = match (raw.kind, corners.len()) {
                    (_, 3) => &[0, 1, 2],
                    ("3DFACE", 4) => &[0, 1, 2, 3],
                    (_, 4) => &[0, 1, 3, 2],
                    _ => &[],
                };
                let m = local(raw.kind != "3DFACE" && raw.flipped());
                let ring: Vec<DVec2> = order.iter().map(|k| corners[*k]).collect();
                let area = (0..ring.len())
                    .map(|k| ring[k].perp_dot(ring[(k + 1) % ring.len()]))
                    .sum::<f64>()
                    .abs()
                    / 2.0;
                let edges: Vec<Shape> = (0..ring.len())
                    .filter_map(|k| {
                        let (a, b) = (ring[k], ring[(k + 1) % ring.len()]);
                        (a.distance(b) > 1e-12).then(|| m.shape(&Shape::Line { a, b }))
                    })
                    .collect();
                let size = ring.iter().map(|p| p.distance(ring[0])).fold(0.0, f64::max);
                // A SOLID or TRACE is a filled area with no outline of its own; a 3DFACE (or
                // one with no area) is its edges.
                if raw.kind != "3DFACE" && area > 1e-9 * size * size && edges.len() >= 3 {
                    sink.hatches.push(DxfHatch {
                        layer,
                        loops: vec![edges],
                        solid: true,
                        pattern: "SOLID".into(),
                        scale: 1.0,
                        angle: 0.0,
                        definition: Vec::new(),
                        origin: DVec2::ZERO,
                        color,
                        opacity: Some(self.opacity(raw, own_layer)),
                        seeds: Vec::new(),
                    });
                } else {
                    for e in edges {
                        push(sink, e);
                    }
                }
            }
            "LEADER" => {
                let mut verts = Vec::new();
                let mut x = None;
                for (code, v) in &raw.g {
                    let v: f64 = v.parse().unwrap_or(0.0);
                    match code {
                        10 => x = Some(v),
                        20 => {
                            if let Some(x) = x.take() {
                                verts.push((DVec2::new(x, v), 0.0));
                            }
                        }
                        _ => {}
                    }
                }
                polyline(&verts, false, &mut |s| push(sink, xf.shape(&s)));
                self.notes
                    .insert("Leaders were read as plain lines, without their arrowheads.");
            }
            "TEXT" | "ATTRIB" | "ATTDEF" => {
                // An attribute's definition is only drawn where it is a constant.
                if raw.kind == "ATTDEF" && raw.int(70).unwrap_or(0) & 2 == 0 {
                    return;
                }
                // Invisible attributes.
                if raw.kind != "TEXT" && raw.int(70).unwrap_or(0) & 1 == 1 {
                    return;
                }
                let (Some(text), Some(height)) = (raw.s(1), raw.f(40)) else {
                    return;
                };
                let text = plain_text(text);
                if text.trim().is_empty() || height <= 0.0 {
                    return;
                }
                let m = local(raw.flipped());
                let h_align = raw.int(72).unwrap_or(0);
                // TEXT's vertical alignment is group 73; an attribute's is 74.
                let v_align = raw
                    .int(if raw.kind == "TEXT" { 73 } else { 74 })
                    .unwrap_or(0);
                let angle = raw.f(50).unwrap_or(0.0);
                let dir = DVec2::from_angle(angle.to_radians());
                // Aligned any way but left / baseline, it hangs from its second point.
                // ("Aligned" and "fit" run between two points: from the first.)
                let anchored = (h_align != 0 || v_align != 0) && !matches!(h_align, 3 | 5);
                let mut at = if anchored {
                    raw.p(11).or(raw.p(10))
                } else {
                    raw.p(10)
                }
                .unwrap_or(DVec2::ZERO);
                let align = match h_align {
                    1 | 4 => TextAlign::Center,
                    2 => TextAlign::Right,
                    _ => TextAlign::Left,
                };
                match (h_align, v_align) {
                    (4, _) | (_, 2) => at -= dir.perp() * (height / 2.0),
                    (_, 3) => at -= dir.perp() * height,
                    _ => {}
                }
                self.push_text(
                    sink,
                    &m,
                    DxfText {
                        layer,
                        text,
                        at,
                        height,
                        angle,
                        color,
                        align,
                        spacing: None,
                        wrap: None,
                    },
                );
            }
            "MTEXT" => {
                let Some(height) = raw.f(40).filter(|h| *h > 0.0) else {
                    return;
                };
                // Long text comes in chunks: the 3s, then the 1.
                let mut whole = String::new();
                for (code, v) in &raw.g {
                    if *code == 3 || *code == 1 {
                        whole.push_str(v);
                    }
                }
                let mut lines = mtext_lines(&whole);
                while lines.last().is_some_and(|l| l.trim().is_empty()) {
                    lines.pop();
                }
                if lines.iter().all(|l| l.trim().is_empty()) {
                    return;
                }
                let text = lines.join("\n");
                let Some(at) = raw.p(10) else {
                    return;
                };
                // Its direction: an x axis vector, else an angle.
                let angle = match raw.p(11) {
                    Some(d) if d.length() > 0.0 => d.y.atan2(d.x).to_degrees(),
                    _ => raw.f(50).unwrap_or(0.0),
                };
                let dir = DVec2::from_angle(angle.to_radians());
                let attach = raw.int(71).unwrap_or(1).clamp(1, 9);
                let spacing = 5.0 / 3.0 * raw.f(44).unwrap_or(1.0).clamp(0.25, 4.0);
                let wrap = raw.f(41).filter(|w| *w > 0.0);
                let align = match (attach - 1) % 3 {
                    0 => TextAlign::Left,
                    1 => TextAlign::Center,
                    _ => TextAlign::Right,
                };
                // How many lines it makes at its width, to hang the block from its top,
                // middle or bottom.
                let count =
                    drift_draft::outline::paragraph_lines(&text, &TextFont::Sans, height, wrap)
                        .len()
                        .max(1) as f64;
                let block = height + (count - 1.0) * spacing * height;
                let top = match (attach - 1) / 3 {
                    0 => 0.0,
                    1 => block / 2.0,
                    _ => block,
                };
                // With a width, the lines sit in a box that the attachment point is on the
                // left, middle or right of: the same side the anchor is on.
                let first_baseline = at + dir.perp() * (top - height);
                let m = local(raw.flipped());
                self.push_text(
                    sink,
                    &m,
                    DxfText {
                        layer,
                        text,
                        at: first_baseline,
                        height,
                        angle,
                        color,
                        align,
                        spacing: Some(spacing),
                        wrap,
                    },
                );
            }
            "HATCH" => {
                let mut fitted = false;
                let m = local(raw.flipped());
                let loops: Vec<Vec<Shape>> = hatch_loops(&raw.g, &mut fitted)
                    .into_iter()
                    .map(|l| l.iter().map(|s| m.shape(s)).collect())
                    .collect();
                if loops.is_empty() {
                    self.skip("HATCH (no boundary)");
                    return;
                }
                let gradient = raw.int(450) == Some(1);
                if gradient {
                    self.notes
                        .insert("Gradient fills were read as solid fills in their first colour.");
                }
                let solid = raw.int(70) == Some(1) || gradient;
                // Seed points come after the pattern, as 98 then 10 / 20 pairs.
                let mut seeds = Vec::new();
                if let Some(start) = raw.g.iter().position(|(c, _)| *c == 98) {
                    let mut x = None;
                    for (code, v) in &raw.g[start..] {
                        let v: f64 = v.parse().unwrap_or(0.0);
                        match code {
                            10 => x = Some(v),
                            20 => {
                                if let Some(x) = x.take() {
                                    seeds.push(m.point(DVec2::new(x, v)));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let pattern = raw.s(2).unwrap_or("").to_owned();
                let scale = raw.f(41).filter(|s| *s > 0.0).unwrap_or(1.0);
                let angle = raw.f(52).unwrap_or(0.0);
                let (mut definition, mut origin) = if solid {
                    (Vec::new(), DVec2::ZERO)
                } else {
                    pattern_definition(&raw.g, scale, angle, 1.0)
                };
                // A pattern named but not defined: AutoCAD's own, in its library's unit.
                if definition.is_empty()
                    && !solid
                    && let Some(known) = drift_draft::find(&pattern)
                {
                    let unit = if self.imperial {
                        drift_core::units::MM_PER_INCH
                    } else {
                        1.0
                    };
                    definition = known
                        .lines
                        .iter()
                        .map(|l| PatLine {
                            angle: l.angle,
                            origin: l.origin / unit,
                            delta: l.delta / unit,
                            dashes: l.dashes.iter().map(|d| d / unit).collect(),
                        })
                        .collect();
                    origin = DVec2::ZERO;
                }
                // Into the sketch: millimetres, and turned (or mirrored) as the hatch is. A
                // mirrored pattern's families run the other way and step the other side.
                let mm = m.det().abs().sqrt();
                let mirrored = m.det() < 0.0;
                for l in &mut definition {
                    l.origin *= mm;
                    l.delta *= mm;
                    for d in &mut l.dashes {
                        *d *= mm;
                    }
                    if mirrored {
                        l.angle = -l.angle;
                        l.origin.y = -l.origin.y;
                        l.delta.y = -l.delta.y;
                    }
                    tidy_family(l);
                }
                let along = m.vector(DVec2::from_angle(angle.to_radians()));
                sink.hatches.push(DxfHatch {
                    layer,
                    loops,
                    solid,
                    pattern,
                    scale,
                    angle: tidy_degrees(along.y.atan2(along.x).to_degrees()),
                    definition,
                    origin: {
                        let o = m.point(origin);
                        DVec2::new(tidy(o.x), tidy(o.y))
                    },
                    color: if gradient {
                        gradient_color(&raw.g).or(color)
                    } else {
                        color
                    },
                    opacity: solid.then(|| self.opacity(raw, own_layer)),
                    seeds,
                });
            }
            "DIMENSION" => {
                // A dimension carries a block of what is drawn for it.
                let name = raw.s(2).unwrap_or("");
                let Some(block) = self.blocks.get(&name.to_ascii_uppercase()) else {
                    self.skip("DIMENSION (no drawn geometry)");
                    return;
                };
                if depth >= MAX_DEPTH {
                    return;
                }
                let inner = xf.then(&Xf::translate(-block.base));
                let entities = block.entities.clone();
                let inherit = Inherit {
                    layer: Some(layer),
                    color,
                    line_type,
                    weight,
                    own_layers: inherit.own_layers,
                };
                for child in &entities {
                    self.emit(child, &inner, &inherit, depth + 1, false, sink);
                }
                self.dimensions += 1;
            }
            "INSERT" => {
                let Some(name) = raw.s(2) else {
                    return;
                };
                let key = name.to_ascii_uppercase();
                if self.xrefs.contains_key(&key) {
                    self.xrefs_used.insert(key);
                    return;
                }
                let Some(block) = self.blocks.get(&key) else {
                    self.skip("INSERT (block not in the file)");
                    return;
                };
                if depth >= MAX_DEPTH {
                    self.notes
                        .insert("Blocks nested more than 16 deep were cut off there.");
                    return;
                }
                let at = raw.p(10).unwrap_or(DVec2::ZERO);
                // Each scale is 1 where it isn't given: AutoCAD, and so every DWG, leaves a 1
                // out (a door mirrored as -1, 1 has no 42).
                let sx = raw.f(41).unwrap_or(1.0);
                let sy = raw.f(42).unwrap_or(1.0);
                let angle = raw.f(50).unwrap_or(0.0);
                let (cols, rows) = (
                    raw.int(70).unwrap_or(1).max(1) as usize,
                    raw.int(71).unwrap_or(1).max(1) as usize,
                );
                let (dc, dr) = (raw.f(44).unwrap_or(0.0), raw.f(45).unwrap_or(0.0));
                let base = block.base;
                let own = local(raw.flipped());
                let even = (sx.abs() - sy.abs()).abs() <= 1e-9 * sx.abs().max(sy.abs())
                    && sx != 0.0
                    // Only a mirror across the block's own y axis fits an insert.
                    && sy > 0.0;
                let plain = keep_inserts
                    && even
                    && cols * rows == 1
                    && !raw.flipped()
                    && xf.is_even()
                    && xf.det() > 0.0;
                if plain {
                    // Where the block's base point lands, and how it is turned.
                    let place = own.then(&Xf::insert(at, angle, sx, sy));
                    let turned = place.a.y.atan2(place.a.x).to_degrees()
                        - if sx < 0.0 { 180.0 } else { 0.0 };
                    // "By block" entities take this copy's look: a block placed in a look of
                    // its own is kept once per look.
                    let look = Inherit {
                        layer: None,
                        color,
                        line_type,
                        weight,
                        own_layers: false,
                    };
                    let variant = if look.gives_a_look() && self.uses_by_block(&key, 0) {
                        format!(
                            "{name} [{:?} {:?} {:?}]",
                            look.color.map(|c| c.0),
                            look.line_type,
                            look.weight.map(|w| w.0)
                        )
                    } else {
                        name.to_owned()
                    };
                    self.used.entry(variant.clone()).or_insert(Kept {
                        block: key,
                        inherit: if variant == name {
                            Inherit::default()
                        } else {
                            look
                        },
                    });
                    sink.inserts.push(DxfInsert {
                        block: variant,
                        layer: layer.clone(),
                        at: place.t,
                        angle: turned.rem_euclid(360.0),
                        // The block's contents are already in millimetres, so the file's
                        // unit is taken back out of the copy's own scale.
                        scale: place.b.length() / self.scale,
                        mirror: sx < 0.0,
                    });
                } else {
                    if cols * rows > MAX_ARRAY {
                        self.skip("INSERT (array too large)");
                        return;
                    }
                    let entities = block.entities.clone();
                    let inherit = Inherit {
                        layer: Some(layer.clone()),
                        color: color.or(inherit.color),
                        line_type,
                        weight,
                        own_layers: inherit.own_layers,
                    };
                    let turn = Xf::insert(DVec2::ZERO, angle, 1.0, 1.0);
                    for row in 0..rows {
                        for col in 0..cols {
                            let offset = turn.vector(DVec2::new(col as f64 * dc, row as f64 * dr));
                            let place = own
                                .then(&Xf::insert(at + offset, angle, sx, sy))
                                .then(&Xf::translate(-base));
                            for child in &entities {
                                self.emit(child, &place, &inherit, depth + 1, false, sink);
                            }
                        }
                    }
                }
                // Attribute values travel with the insert, in the same coordinates as it.
                let inherit = Inherit {
                    layer: Some(layer),
                    color,
                    line_type,
                    weight,
                    own_layers: inherit.own_layers,
                };
                for attrib in raw.kids.iter().filter(|k| k.kind == "ATTRIB") {
                    self.emit(attrib, xf, &inherit, depth + 1, false, sink);
                }
            }
            // Never drawn, or drawn elsewhere.
            "VIEWPORT" | "ATTDEF_END" | "SEQEND" | "VERTEX" => {}
            other => self.skip(other),
        }
    }

    /// A fill's opacity: its own transparency, else its layer's, else opaque.
    fn opacity(&self, raw: &Raw, layer: &str) -> f64 {
        raw.int(440)
            .and_then(opacity_of)
            .or_else(|| self.layer_opacity.get(&layer.to_ascii_uppercase()).copied())
            .unwrap_or(1.0)
    }

    /// Whether block `key` (upper case) has an entity whose colour, line type or weight is
    /// "by block", directly or in a block it places "by block".
    fn uses_by_block(&mut self, key: &str, depth: usize) -> bool {
        if let Some(known) = self.by_block.get(key) {
            return *known;
        }
        // A block that places itself stops here.
        self.by_block.insert(key.to_owned(), false);
        let Some(block) = self.blocks.get(key) else {
            return false;
        };
        let mut nested = Vec::new();
        let mut found = false;
        for raw in &block.entities {
            if raw.kind == "INSERT" {
                if by_block(raw)
                    && let Some(name) = raw.s(2)
                {
                    nested.push(name.to_ascii_uppercase());
                }
            } else if by_block(raw) {
                found = true;
                break;
            }
        }
        if !found && depth < MAX_DEPTH {
            found = nested.iter().any(|n| self.uses_by_block(n, depth + 1));
        }
        self.by_block.insert(key.to_owned(), found);
        found
    }

    /// Add text given in a record's own coordinates and units.
    fn push_text(&mut self, sink: &mut Sink, m: &Xf, text: DxfText) {
        let dir = DVec2::from_angle(text.angle.to_radians());
        let along = m.vector(dir);
        let up = m.vector(dir.perp());
        let scale = up.length();
        let height = text.height * scale;
        if !(height > 0.0 && height.is_finite()) {
            return;
        }
        if m.det() < 0.0 {
            self.notes
                .insert("Mirrored text was read the right way round, at its mirrored position.");
        }
        sink.texts.push(DxfText {
            at: m.point(text.at),
            height,
            angle: along.y.atan2(along.x).to_degrees(),
            wrap: text.wrap.map(|w| w * along.length()),
            ..text
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Reading a file

/// DXF text from a file's bytes: UTF-8 where it is, else Windows Latin (what older DXF files
/// are written in).
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes
            .iter()
            .map(|b| match *b {
                0x80 => '€',
                0x85 => '…',
                0x91 | 0x92 => '\'',
                0x93 | 0x94 => '"',
                0x96 | 0x97 => '-',
                b => char::from(b),
            })
            .collect(),
    }
}

/// A shape `k` times larger about the origin.
fn scale_shape(shape: &Shape, k: f64) -> Shape {
    match *shape {
        Shape::Point(p) => Shape::Point(p * k),
        Shape::Line { a, b } => Shape::Line { a: a * k, b: b * k },
        Shape::Circle { center, radius } => Shape::Circle {
            center: center * k,
            radius: radius * k,
        },
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => Shape::Arc {
            center: center * k,
            radius: radius * k,
            start,
            sweep,
        },
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => Shape::Ellipse {
            center: center * k,
            major: major * k,
            minor: minor * k,
            start,
            sweep,
        },
        Shape::Bezier { p } => Shape::Bezier {
            p: p.map(|q| q * k),
        },
    }
}

impl DxfText {
    fn scaled(mut self, k: f64) -> Self {
        self.at *= k;
        self.height *= k;
        self.wrap = self.wrap.map(|w| w * k);
        self
    }
}

impl DxfEntity {
    fn scaled(mut self, k: f64) -> Self {
        self.shape = scale_shape(&self.shape, k);
        self
    }
}

impl DxfHatch {
    fn scale_by(&mut self, k: f64) {
        for edge in self.loops.iter_mut().flatten() {
            *edge = scale_shape(edge, k);
        }
        for seed in &mut self.seeds {
            *seed *= k;
        }
        self.origin *= k;
        for l in &mut self.definition {
            l.origin *= k;
            l.delta *= k;
            for d in &mut l.dashes {
                *d *= k;
            }
        }
    }
}

impl DxfImport {
    /// The drawing as [`read_at`] reads it at 1 : `k` times what it was read at, without
    /// reading the file again. (Block copies keep their own scales, as there.)
    pub fn scaled(mut self, k: f64) -> Self {
        self.entities = self.entities.into_iter().map(|e| e.scaled(k)).collect();
        self.texts = self.texts.into_iter().map(|t| t.scaled(k)).collect();
        for hatch in &mut self.hatches {
            hatch.scale_by(k);
        }
        for block in &mut self.blocks {
            block.entities = std::mem::take(&mut block.entities)
                .into_iter()
                .map(|e| e.scaled(k))
                .collect();
            block.texts = std::mem::take(&mut block.texts)
                .into_iter()
                .map(|t| t.scaled(k))
                .collect();
            for hatch in &mut block.hatches {
                hatch.scale_by(k);
            }
        }
        for insert in &mut self.inserts {
            insert.at *= k;
        }
        for view in self.layouts.iter_mut().flat_map(|l| &mut l.views) {
            view.center *= k;
            view.scale *= k;
        }
        self
    }
}

/// Read an ASCII DXF file. Files that don't say their units (`$INSUNITS` missing or 0) are
/// read in `default_mm_per_unit` (the document's display unit).
pub fn read(text: &str, default_mm_per_unit: f64) -> Result<DxfImport, String> {
    read_at(text, default_mm_per_unit, 1.0)
}

/// Read an ASCII DXF file as [`read`] does, drawn at 1 : `drawn_at`: a drawing made at
/// paper size comes in that many times larger.
pub fn read_at(text: &str, default_mm_per_unit: f64, drawn_at: f64) -> Result<DxfImport, String> {
    if text.starts_with("AutoCAD Binary DXF") {
        return Err(
            "this is a binary DXF file; save it as ASCII DXF (the usual kind) and try again".into(),
        );
    }
    if text.starts_with("AC10") && !text[..text.len().min(64)].contains('\n') {
        return Err("this is a DWG file, not DXF; save it as DXF from your CAD program".into());
    }
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    if lines.len() < 2 {
        return Err("empty file".into());
    }
    let mut pairs: Vec<(i32, &str)> = Vec::with_capacity(lines.len() / 2);
    for chunk in lines.chunks(2) {
        let [code, value] = chunk else {
            break;
        };
        // A blank line at the end of a file isn't a group code.
        if code.is_empty() && pairs.last().is_some_and(|p| *p == (0, "EOF")) {
            break;
        }
        let code: i32 = code
            .parse()
            .map_err(|_| format!("not an ASCII DXF file (bad group code {code:?})"))?;
        pairs.push((code, value));
    }

    let mut scale = default_mm_per_unit;
    let (mut insunits, mut measurement): (Option<i64>, Option<i64>) = (None, None);
    let mut layer_opacity: HashMap<String, f64> = HashMap::new();
    let mut layers: Vec<DxfLayer> = Vec::new();
    let mut line_types: HashMap<String, LineType> = HashMap::new();
    let mut raw_line_types: Vec<(String, Vec<f64>)> = Vec::new();
    let mut custom_types = 0usize;
    // Each layer's line type, by the layer's name.
    let mut layer_line_types: HashMap<String, String> = HashMap::new();
    // Block records by handle, for naming layouts.
    let mut records_by_handle: HashMap<String, String> = HashMap::new();
    // The same, in the file's own case, for naming dynamic blocks.
    let mut record_names: HashMap<String, String> = HashMap::new();
    // A dynamic block's anonymous copy (upper case), and the handle of the dynamic block.
    let mut copy_of: Vec<(String, String)> = Vec::new();
    let mut xrefs: HashMap<String, (String, String)> = HashMap::new();
    let mut blocks: HashMap<String, RawBlock> = HashMap::new();
    let mut model: Vec<Raw> = Vec::new();
    // Paper space: the active layout's entities, and the other layouts' by block name.
    let mut papers: Vec<(String, Vec<Raw>)> = Vec::new();
    let mut raw_layouts: Vec<RawLayout> = Vec::new();

    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        i += 1;
        if code != 0 || value != "SECTION" {
            continue;
        }
        let section = pairs.get(i).map_or("", |p| p.1);
        i += 1;
        match section {
            "HEADER" => {
                while i < pairs.len() && pairs[i] != (0, "ENDSEC") {
                    if pairs[i] == (9, "$INSUNITS")
                        && let Some((70, v)) = pairs.get(i + 1).copied()
                    {
                        let code = v.parse().unwrap_or(0);
                        scale = unit_scale(code).unwrap_or(default_mm_per_unit);
                        insunits = Some(code);
                    }
                    if pairs[i] == (9, "$MEASUREMENT")
                        && let Some((70, v)) = pairs.get(i + 1).copied()
                    {
                        measurement = v.parse::<i64>().ok();
                    }
                    i += 1;
                }
            }
            "TABLES" => {
                for entry in records(&pairs, &mut i, &["ENDSEC"]) {
                    match entry.kind {
                        "LTYPE" => {
                            if let Some(name) = entry.s(2) {
                                let pattern: Vec<f64> = entry
                                    .g
                                    .iter()
                                    .filter(|(c, _)| *c == 49)
                                    .filter_map(|(_, v)| v.parse().ok())
                                    .collect();
                                raw_line_types.push((name.to_owned(), pattern));
                            }
                        }
                        "LAYER" => {
                            if let Some(name) = entry.s(2) {
                                let flags = entry.int(70).unwrap_or(0);
                                let index = entry.int(62).unwrap_or(7);
                                let color = match entry.int(420) {
                                    Some(rgb) => {
                                        let v = rgb as u32;
                                        Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]))
                                    }
                                    None => aci_rgb(index.abs()),
                                };
                                layers.push(DxfLayer {
                                    name: name.to_owned(),
                                    color,
                                    // Resolved once every line type is known.
                                    line_type: LineType::Continuous,
                                    weight: own_weight(&entry),
                                    // A negative colour is "off"; bit 1 is "frozen".
                                    visible: index >= 0 && flags & 1 == 0,
                                    locked: flags & 4 != 0,
                                    printable: entry.int(290) != Some(0),
                                });
                                if let Some(lt) = entry.s(6) {
                                    layer_line_types
                                        .insert(name.to_ascii_uppercase(), lt.to_ascii_uppercase());
                                }
                                // Its transparency, in extended data.
                                if let Some(at) = entry
                                    .g
                                    .iter()
                                    .position(|p| *p == (1001, "AcCmTransparency"))
                                    && let Some(alpha) = entry.g[at + 1..]
                                        .iter()
                                        .take_while(|(c, _)| *c != 1001)
                                        .find(|(c, _)| *c == 1071)
                                        .and_then(|(_, v)| v.parse::<i64>().ok())
                                        .and_then(opacity_of)
                                {
                                    layer_opacity.insert(name.to_ascii_uppercase(), alpha);
                                }
                            }
                        }
                        "BLOCK_RECORD" => {
                            if let (Some(handle), Some(name)) = (entry.s(5), entry.s(2)) {
                                records_by_handle
                                    .insert(handle.to_ascii_uppercase(), name.to_ascii_uppercase());
                                record_names.insert(handle.to_ascii_uppercase(), name.to_owned());
                                // An anonymous copy of a dynamic block points at the dynamic
                                // block's record (extended data AcDbBlockRepBTag, then 1005).
                                if let Some(at) = entry
                                    .g
                                    .iter()
                                    .position(|p| *p == (1001, "AcDbBlockRepBTag"))
                                    && let Some((_, of)) = entry.g[at + 1..]
                                        .iter()
                                        .take_while(|(c, _)| *c != 1001)
                                        .find(|(c, _)| *c == 1005)
                                {
                                    copy_of
                                        .push((name.to_ascii_uppercase(), of.to_ascii_uppercase()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "BLOCKS" => {
                while i < pairs.len() && pairs[i] != (0, "ENDSEC") {
                    if pairs[i] != (0, "BLOCK") {
                        i += 1;
                        continue;
                    }
                    let head = records(&pairs, &mut i, &["ENDSEC", "ENDBLK"]);
                    let mut head = head.into_iter();
                    let Some(block) = head.next() else {
                        continue;
                    };
                    let name = block.s(2).unwrap_or("");
                    let entities: Vec<Raw> = head.collect();
                    let upper = name.to_ascii_uppercase();
                    // An external reference (4) or overlay (8): its drawing is another file.
                    if block.int(70).is_some_and(|f| f & (4 | 8) != 0) && !name.is_empty() {
                        let path = block.s(1).unwrap_or("").to_owned();
                        xrefs.insert(upper, (name.to_owned(), path));
                    } else if upper.starts_with("*PAPER_SPACE") {
                        if !entities.is_empty() {
                            papers.push((upper, entities));
                        }
                    } else if !upper.starts_with("*MODEL_SPACE") && !name.is_empty() {
                        blocks.insert(
                            upper,
                            RawBlock {
                                name,
                                base: block.p(10).unwrap_or(DVec2::ZERO),
                                entities,
                            },
                        );
                    } else if upper.starts_with("*MODEL_SPACE") {
                        // Some writers put the model in its block rather than ENTITIES.
                        model.extend(entities);
                    }
                }
            }
            "ENTITIES" => {
                let mut active = Vec::new();
                for raw in records(&pairs, &mut i, &["ENDSEC"]) {
                    if raw.paper() {
                        active.push(raw);
                    } else {
                        model.push(raw);
                    }
                }
                if !active.is_empty() {
                    papers.push(("*PAPER_SPACE".to_owned(), active));
                }
            }
            "OBJECTS" => {
                for object in records(&pairs, &mut i, &["ENDSEC"]) {
                    if object.kind != "LAYOUT" {
                        continue;
                    }
                    // Plot settings first, then the layout's own groups.
                    let split = object
                        .g
                        .iter()
                        .position(|(c, v)| *c == 100 && *v == "AcDbLayout")
                        .unwrap_or(object.g.len());
                    let plot = Raw {
                        kind: "PLOT",
                        g: object.g[..split].to_vec(),
                        kids: Vec::new(),
                    };
                    let own = Raw {
                        kind: "LAYOUT",
                        g: object.g[split..].to_vec(),
                        kids: Vec::new(),
                    };
                    let Some(name) = own.s(1) else {
                        continue;
                    };
                    let record = own
                        .g
                        .iter()
                        .rev()
                        .find(|(c, _)| *c == 330)
                        .map_or(String::new(), |(_, v)| v.to_ascii_uppercase());
                    let size = match (plot.f(44), plot.f(45)) {
                        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => {
                            // Turned a quarter turn, the paper is the other way up.
                            Some(if plot.int(73).unwrap_or(0) % 2 == 1 {
                                DVec2::new(h, w)
                            } else {
                                DVec2::new(w, h)
                            })
                        }
                        _ => None,
                    };
                    raw_layouts.push(RawLayout {
                        name: name.to_owned(),
                        record,
                        paper: size,
                        margin: DVec2::new(plot.f(40).unwrap_or(0.0), plot.f(41).unwrap_or(0.0)),
                        unit: if plot.int(72) == Some(0) {
                            drift_core::units::MM_PER_INCH
                        } else {
                            1.0
                        },
                        order: own.int(71).unwrap_or(0),
                    });
                }
            }
            _ => {}
        }
    }

    // Line types: by name where the name is a known one, else by the look of the pattern.
    let mm = scale;
    for (name, pattern) in &raw_line_types {
        let pattern: Vec<f64> = pattern.iter().map(|d| d * mm).collect();
        let kind = classify_line_type(name, &pattern);
        let upper = name.to_ascii_uppercase();
        let standard = ["CONTINUOUS", "BYLAYER", "BYBLOCK"].contains(&upper.as_str())
            || parse_line_type(name) != LineType::Continuous;
        if !standard && !pattern.is_empty() {
            custom_types += 1;
        }
        line_types.insert(upper, kind);
    }
    for layer in &mut layers {
        let upper = layer.name.to_ascii_uppercase();
        if let Some(lt) = layer_line_types.get(&upper) {
            layer.line_type = line_types
                .get(lt)
                .copied()
                .unwrap_or_else(|| parse_line_type(lt));
        }
    }

    let layer_looks = layers
        .iter()
        .map(|l| {
            (
                l.name.to_ascii_uppercase(),
                (l.color.unwrap_or(Rgb([0, 0, 0])), l.line_type, l.weight),
            )
        })
        .collect();
    let true_names = copy_of
        .into_iter()
        .filter_map(|(copy, of)| Some((copy, record_names.get(&of)?.clone())))
        .collect();
    let mut reader = Reader {
        scale: scale * drawn_at,
        blocks,
        line_types,
        skipped: BTreeMap::new(),
        notes: BTreeSet::new(),
        dimensions: 0,
        used: BTreeMap::new(),
        by_block: HashMap::new(),
        true_names,
        xrefs,
        xrefs_used: BTreeSet::new(),
        layer_opacity,
        layer_looks,
        // Without $MEASUREMENT, inches and feet drawings are imperial.
        imperial: match measurement {
            Some(m) => m == 0,
            None => matches!(insunits, Some(1 | 2 | 3 | 8 | 9 | 10)),
        },
    };
    let world = Xf::scale(reader.scale);
    let mut sink = Sink::default();
    for raw in &model {
        reader.emit(raw, &world, &Inherit::default(), 0, true, &mut sink);
    }

    // The blocks the kept inserts place, with what is nested in them expanded.
    let mut out_blocks = Vec::new();
    let kept = std::mem::take(&mut reader.used);
    for (name, kept) in &kept {
        let Some(block) = reader.blocks.get(&kept.block) else {
            continue;
        };
        let label = reader
            .true_names
            .get(&kept.block)
            .cloned()
            .unwrap_or_else(|| block.name.to_owned());
        let (base, entities) = (block.base, block.entities.clone());
        let place = world.then(&Xf::translate(-base));
        let inherit = Inherit {
            own_layers: true,
            ..kept.inherit.clone()
        };
        let mut inner = Sink::default();
        for raw in &entities {
            reader.emit(raw, &place, &inherit, 1, false, &mut inner);
        }
        out_blocks.push(DxfBlock {
            name: name.clone(),
            label,
            entities: inner.entities,
            texts: inner.texts,
            hatches: inner.hatches,
        });
    }
    let mut xref_note = None;
    if !reader.xrefs_used.is_empty() {
        let list: Vec<String> = reader
            .xrefs_used
            .iter()
            .filter_map(|k| reader.xrefs.get(k))
            .take(6)
            .map(|(name, path)| {
                if path.is_empty() {
                    name.clone()
                } else {
                    format!("{name} ({path})")
                }
            })
            .collect();
        xref_note = Some(format!(
            "External reference(s) weren't brought in, as their drawings are other files: {}. \
             Bind them in AutoCAD (XREF, then Bind) and import again, or import those files \
             on their own.",
            list.join(", ")
        ));
    }

    // Layouts: their viewports become views; what is drawn on the paper itself is counted.
    let mut layouts = Vec::new();
    raw_layouts.sort_by_key(|l| l.order);
    for (k, (container, entities)) in papers.iter().enumerate() {
        let info = raw_layouts.iter().find(|l| {
            reader_record(&records_by_handle, &l.record).is_some_and(|name| *name == *container)
        });
        if info.is_some_and(|l| l.name.eq_ignore_ascii_case("Model")) {
            continue;
        }
        let unit = info.map_or(1.0, |l| l.unit);
        let margin = info.map_or(DVec2::ZERO, |l| l.margin);
        let mut views = Vec::new();
        let mut dropped = 0;
        for raw in entities {
            if raw.kind != "VIEWPORT" {
                dropped += 1;
                continue;
            }
            // Viewport 1 is the paper itself; a status of 0 is switched off.
            if raw.int(69) == Some(1) || raw.int(68) == Some(0) {
                continue;
            }
            let (Some(at), Some(w), Some(h), Some(view_h)) =
                (raw.p(10), raw.f(40), raw.f(41), raw.f(45))
            else {
                continue;
            };
            if !(w > 0.0 && h > 0.0 && view_h > 0.0) {
                continue;
            }
            let size = DVec2::new(w, h) * unit;
            views.push(DxfViewport {
                at: at * unit + margin,
                size,
                center: raw.p(12).unwrap_or(DVec2::ZERO) * reader.scale,
                scale: (view_h * reader.scale) / size.y,
            });
        }
        if views.is_empty() && dropped == 0 {
            continue;
        }
        layouts.push(DxfLayout {
            name: info.map_or_else(|| format!("Layout {}", k + 1), |l| l.name.clone()),
            paper: info.and_then(|l| l.paper),
            views,
            dropped,
        });
    }

    let mut notes: Vec<String> = reader.notes.iter().map(|n| (*n).to_owned()).collect();
    notes.extend(xref_note);
    if custom_types > 0 {
        notes.push(format!(
            "{custom_types} custom line type(s) were matched to the nearest of Drift's five \
             (continuous, dashed, hidden, centre, phantom)."
        ));
    }
    if reader.dimensions > 0 {
        notes.push(format!(
            "{} dimension(s) were read as the lines, arrows and text drawn for them: they look \
             the same but no longer measure.",
            reader.dimensions
        ));
    }
    let dropped: usize = layouts.iter().map(|l| l.dropped).sum();
    if dropped > 0 {
        notes.push(format!(
            "{dropped} object(s) drawn on layouts (title blocks and notes on the paper) were \
             left out: a Drift sheet has its own title block."
        ));
    }
    Ok(DxfImport {
        entities: sink.entities,
        texts: sink.texts,
        hatches: sink.hatches,
        layers,
        blocks: out_blocks,
        inserts: sink.inserts,
        layouts,
        dimensions: reader.dimensions,
        skipped: reader.skipped,
        notes,
        fill_edges_construction: false,
    })
}

/// The block record a handle names.
fn reader_record<'m>(records: &'m HashMap<String, String>, handle: &str) -> Option<&'m String> {
    records.get(handle).filter(|_| !handle.is_empty())
}

// ---------------------------------------------------------------------------------------------
// Into a document

/// What an import added.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub curves: usize,
    pub layers: usize,
    /// Blocks created, and copies placed.
    pub blocks: usize,
    pub copies: usize,
    pub texts: usize,
    pub fills: usize,
    pub sheets: usize,
    pub dimensions: usize,
    pub skipped: BTreeMap<String, usize>,
    pub notes: Vec<String>,
}

/// Why a curve whose coordinates overflow (or aren't numbers) is left out.
const TOO_LARGE: &str = "curves too large to draw";

/// The largest coordinate an imported curve may have, in mm. Stray geometry far off (1e12
/// and more) still comes in and is reported; near f64's limit the drawing's own sums and
/// squares overflow and come out as NaN.
const MAX_COORD: f64 = 1e30;

/// Every point `add_to_document` makes for `shape` is a number within [`MAX_COORD`] (a NaN
/// or infinite one would make a drawing that doesn't save).
fn finite(shape: &Shape) -> bool {
    let ok = |p: DVec2| p.abs().max_element() <= MAX_COORD;
    match *shape {
        Shape::Point(p) => ok(p),
        Shape::Line { a, b } => ok(a) && ok(b),
        Shape::Circle { center, radius } => ok(center) && radius.abs() <= MAX_COORD,
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            ok(center)
                && ok(center + DVec2::from_angle(start) * radius)
                && ok(center + DVec2::from_angle(start + sweep) * radius)
        }
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => {
            let (axes, ends) =
                drift_core::shape::ellipse_handles(center, major, minor, start, sweep);
            axes.into_iter().chain(ends.into_iter().flatten()).all(ok)
        }
        Shape::Bezier { p } => p.into_iter().all(ok),
    }
}

/// Points along a curve, for testing what is inside a hatch.
fn flatten(shape: &Shape, out: &mut Vec<DVec2>) {
    match *shape {
        Shape::Point(_) => {}
        Shape::Line { a, b } => out.extend([a, b]),
        Shape::Circle { center, radius } => {
            out.extend((0..=48).map(|k| center + DVec2::from_angle(k as f64 / 48.0 * TAU) * radius))
        }
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => out.extend(
            (0..=24).map(|k| center + DVec2::from_angle(start + sweep * k as f64 / 24.0) * radius),
        ),
        Shape::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => out.extend(drift_core::shape::flatten_ellipse(
            center, major, minor, start, sweep, 48,
        )),
        Shape::Bezier { p } => out.extend(drift_core::shape::flatten_bezier(&p, 16)),
    }
}

/// A point inside a hatch: one of the file's seed points if it is inside, else the middle
/// of the widest stretch of a line across it. Also the index of its largest loop.
fn interior(hatch: &DxfHatch) -> Option<(DVec2, usize)> {
    // Each loop as a closed run of points, by joining its curves end to end.
    let polygons: Vec<Vec<DVec2>> = hatch
        .loops
        .iter()
        .map(|shapes| {
            let mut points: Vec<DVec2> = Vec::new();
            for s in shapes {
                let mut piece = Vec::new();
                flatten(s, &mut piece);
                // Curves of a loop may be stored either way round.
                if let (Some(last), Some(first), Some(end)) =
                    (points.last(), piece.first(), piece.last())
                    && last.distance(*end) < last.distance(*first)
                {
                    piece.reverse();
                }
                points.extend(piece);
            }
            points
        })
        .collect();
    let area = |p: &[DVec2]| drift_core::polygon::area(p).abs();
    let outer =
        (0..polygons.len()).max_by(|a, b| area(&polygons[*a]).total_cmp(&area(&polygons[*b])))?;
    // Crossings of a horizontal line with every loop: inside is between odd and even ones.
    let crossings = |y: f64| -> Vec<f64> {
        let mut xs = Vec::new();
        for p in &polygons {
            for i in 0..p.len() {
                let (a, b) = (p[i], p[(i + 1) % p.len()]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        xs
    };
    let inside = |q: DVec2| crossings(q.y).iter().filter(|x| **x < q.x).count() % 2 == 1;
    if let Some(seed) = hatch.seeds.iter().copied().find(|s| inside(*s)) {
        return Some((seed, outer));
    }
    let (lo, hi) = polygons[outer]
        .iter()
        .fold((f64::MAX, f64::MIN), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    let mut best: Option<(f64, DVec2)> = None;
    for k in 1..=7 {
        let y = lo + (hi - lo) * (k as f64 / 8.0 + 0.013);
        for pair in crossings(y).as_chunks::<2>().0 {
            let width = pair[1] - pair[0];
            if best.is_none_or(|(w, _)| width > w) {
                best = Some((width, DVec2::new((pair[0] + pair[1]) / 2.0, y)));
            }
        }
    }
    best.filter(|(w, _)| *w > 0.0).map(|(_, p)| (p, outer))
}

/// `k` when the families `def` are `lib`'s at `k` times the size (taken from `lib`'s first
/// family's point, as `def` is), else `None`.
fn same_pattern(def: &[PatLine], lib: &[PatLine]) -> Option<f64> {
    let (first, lib_first) = (def.first()?, lib.first()?);
    if def.len() != lib.len() || lib_first.delta.y.abs() < 1e-12 {
        return None;
    }
    let k = tidy(first.delta.y / lib_first.delta.y);
    if !(k.is_finite() && k > 0.0) {
        return None;
    }
    let size = lib.iter().map(|l| l.delta.length()).fold(0.0, f64::max) * k;
    let near = |a: f64, b: f64| (a - b).abs() <= 1e-6 * size.max(1e-9);
    let close = |a: DVec2, b: DVec2| near(a.x, b.x) && near(a.y, b.y);
    let same = def.iter().zip(lib).all(|(d, l)| {
        let turn = (d.angle - l.angle).rem_euclid(360.0);
        (turn < 1e-6 || turn > 360.0 - 1e-6)
            && close(d.origin, (l.origin - lib_first.origin) * k)
            && close(d.delta, l.delta * k)
            && d.dashes.len() == l.dashes.len()
            && d.dashes.iter().zip(&l.dashes).all(|(a, b)| near(*a, b * k))
    });
    same.then_some(k)
}

/// The hatch patterns an import uses: Drift's own where the file's pattern is one of them,
/// else the file's own definitions, added to the drawing's imported patterns.
struct Patterns {
    custom: Vec<Pattern>,
    changed: bool,
    /// Patterns the file names but doesn't define, which Drift doesn't have either.
    unknown: BTreeSet<String>,
}

impl Patterns {
    fn new(doc: &Document) -> Self {
        Self {
            custom: drift_draft::parse_pat(doc.custom_patterns()).unwrap_or_default(),
            changed: false,
            unknown: BTreeSet::new(),
        }
    }

    /// What fills `h`.
    fn kind(&mut self, h: &DxfHatch) -> FillKind {
        if h.solid {
            return FillKind::Solid;
        }
        if h.definition.is_empty() {
            // Neither defined in the file nor one of Drift's.
            self.unknown.insert(h.pattern.clone());
            let mut hatch = Hatch::new("ANSI31");
            hatch.scale = h.scale;
            hatch.angle = h.angle;
            return FillKind::Hatch(hatch);
        }
        let mut hatch = Hatch::new(h.pattern.clone());
        hatch.scale = h.scale;
        hatch.angle = h.angle;
        hatch.origin = h.origin;
        // The file's sizes are real sizes.
        hatch.model = true;
        if let Some(known) = drift_draft::find(&h.pattern)
            && let Some(k) = same_pattern(&h.definition, &known.lines)
        {
            hatch.pattern = known.name.clone();
            hatch.scale = tidy(h.scale * k);
            // Drift's pattern starts its first family where the file's does.
            let first = known.lines.first().map_or(DVec2::ZERO, |l| l.origin);
            let o = h.origin - DVec2::from_angle(h.angle.to_radians()).rotate(first * hatch.scale);
            hatch.origin = DVec2::new(tidy(o.x), tidy(o.y));
            return FillKind::Hatch(hatch);
        }
        // The file's own pattern, under its name (or a name of its own where Drift or the
        // drawing has a different pattern by that name).
        let base = if h.pattern.trim().is_empty() {
            "DXF pattern".to_owned()
        } else if drift_draft::find(&h.pattern).is_some() {
            format!("{} (from file)", h.pattern)
        } else {
            h.pattern.clone()
        };
        let mut name = base.clone();
        let mut n = 2;
        loop {
            match self
                .custom
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&name))
            {
                Some(p)
                    if same_pattern(&h.definition, &p.lines)
                        .is_some_and(|k| (k - 1.0).abs() < 1e-6) =>
                {
                    break;
                }
                Some(_) => {
                    name = format!("{base} ({n})");
                    n += 1;
                }
                None => {
                    self.custom.push(Pattern {
                        name: name.clone(),
                        description: "From a DXF or DWG file".into(),
                        model: true,
                        lines: h.definition.clone(),
                    });
                    self.changed = true;
                    break;
                }
            }
        }
        hatch.pattern = name;
        FillKind::Hatch(hatch)
    }

    /// Store the patterns added in the drawing, and say what couldn't be matched.
    fn finish(self, doc: &mut Document, report: &mut ImportReport) {
        if self.changed {
            doc.set_custom_patterns(drift_draft::to_pat(&self.custom));
        }
        if !self.unknown.is_empty() {
            let names: Vec<&str> = self.unknown.iter().map(String::as_str).take(6).collect();
            report.notes.push(format!(
                "Hatch pattern(s) the file names but doesn't define, and Drift doesn't have \
                 ({}), were drawn as ANSI31: import the .pat file that defines them and pick \
                 it in the fill's inspector.",
                names.join(", ")
            ));
        }
    }
}

/// Add an import to a document (inside the caller's transaction): curves on layers of the
/// same names, text, hatches as fills on their boundaries, blocks and their copies, and
/// layouts as sheets. Ends that coincide (within 1e-6 of the drawing's size) share one point,
/// so the imported outlines stay connected.
pub fn add_to_document(doc: &mut Document, import: &DxfImport) -> ImportReport {
    let mut report = ImportReport {
        skipped: import.skipped.clone(),
        notes: import.notes.clone(),
        dimensions: import.dimensions,
        ..Default::default()
    };
    // The drawing's size, for the tolerance ends are joined within. A stray object far off
    // (as files from other programs often have) isn't part of it: counted in, it would join
    // every end of the real drawing to its neighbours. (Unless everything is far off.)
    let hatch_shapes = import.hatches.iter().flat_map(|h| h.loops.iter().flatten());
    let shapes: Vec<&Shape> = import
        .entities
        .iter()
        .map(|e| &e.shape)
        .chain(hatch_shapes)
        .collect();
    let extent_of = |skip_far: bool| {
        let mut extent = drift_core::Bounds::EMPTY;
        for b in shapes.iter().map(|s| s.bounds()) {
            if !(skip_far && b.is_far_off()) {
                extent.include(b.min);
                extent.include(b.max);
            }
        }
        extent
    };
    let mut extent = extent_of(true);
    if extent.is_empty() {
        extent = extent_of(false);
    }
    let tol = if extent.is_empty() {
        1e-9
    } else {
        (extent.max - extent.min).max_element().max(1.0) * 1e-6
    };
    // Layers by name ("0" is the default layer).
    let mut layer_ids: BTreeMap<String, LayerId> = doc
        .layers()
        .map(|(id, l)| (l.name.to_ascii_uppercase(), id))
        .collect();
    let mut layer_of = |doc: &mut Document, name: &str, report: &mut ImportReport| -> LayerId {
        let key = name.to_ascii_uppercase();
        if let Some(id) = layer_ids.get(&key) {
            return *id;
        }
        let info = import
            .layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name));
        let mut layer = Layer::new(name);
        layer.color = info.and_then(|l| l.color);
        layer.line_type = info.map_or(LineType::Continuous, |l| l.line_type);
        layer.weight = info.and_then(|l| l.weight).unwrap_or_default();
        layer.visible = info.is_none_or(|l| l.visible);
        layer.locked = info.is_some_and(|l| l.locked);
        layer.printable = info.is_none_or(|l| l.printable);
        let id = doc.add_layer(layer);
        layer_ids.insert(key, id);
        report.layers += 1;
        id
    };
    // Shared points, found through a grid of cells `tol` wide.
    let mut points: BTreeMap<(i64, i64), Vec<(DVec2, EntityId)>> = BTreeMap::new();
    let mut point = |doc: &mut Document, p: DVec2| -> EntityId {
        let cell = ((p.x / tol).round() as i64, (p.y / tol).round() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) =
                    points.get(&(cell.0.saturating_add(dx), cell.1.saturating_add(dy)))
                    && let Some((_, id)) = list.iter().find(|(q, _)| q.distance(p) <= tol)
                {
                    return *id;
                }
            }
        }
        let id = doc.add_point(p);
        points.entry(cell).or_default().push((p, id));
        id
    };
    let mut add_shape = |doc: &mut Document, shape: &Shape| -> Option<EntityId> {
        if !finite(shape) {
            return None;
        }
        match *shape {
            Shape::Line { a, b } => {
                let (pa, pb) = (point(doc, a), point(doc, b));
                if pa == pb {
                    return None;
                }
                doc.add_line(pa, pb).ok()
            }
            Shape::Circle { center, radius } => {
                let c = point(doc, center);
                doc.add_circle(c, radius).ok()
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let c = point(doc, center);
                let s = point(doc, center + DVec2::from_angle(start) * radius);
                let e2 = point(doc, center + DVec2::from_angle(start + sweep) * radius);
                if s == e2 {
                    doc.add_circle(c, radius).ok()
                } else {
                    doc.add_arc(c, s, e2).ok()
                }
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                let (axes, ends) =
                    drift_core::shape::ellipse_handles(center, major, minor, start, sweep);
                let [c, m, n] = axes.map(|q| point(doc, q));
                match ends {
                    Some([s, e2]) => {
                        let (s, e2) = (point(doc, s), point(doc, e2));
                        doc.add_ellipse_arc(c, m, n, s, e2).ok()
                    }
                    None => doc.add_ellipse(c, m, n).ok(),
                }
            }
            Shape::Bezier { p } => {
                let [a, c1, c2, b] = p.map(|q| point(doc, q));
                doc.add_bezier(a, c1, c2, b).ok()
            }
            Shape::Point(p) => Some(point(doc, p)),
        }
    };
    // Curves already added, by what they are, so a hatch finds the outline it was drawn on
    // instead of doubling it.
    let key_of = |shape: &Shape| -> Option<(u8, [i64; 6])> {
        let q = |v: f64| (v / tol).round() as i64;
        Some(match *shape {
            Shape::Line { a, b } => {
                let (a, b) = if (a.x, a.y) <= (b.x, b.y) {
                    (a, b)
                } else {
                    (b, a)
                };
                (0, [q(a.x), q(a.y), q(b.x), q(b.y), 0, 0])
            }
            Shape::Circle { center, radius } => (1, [q(center.x), q(center.y), q(radius), 0, 0, 0]),
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let (s, e) = (
                    center + DVec2::from_angle(start) * radius,
                    center + DVec2::from_angle(start + sweep) * radius,
                );
                (
                    2,
                    [q(center.x), q(center.y), q(s.x), q(s.y), q(e.x), q(e.y)],
                )
            }
            // A Bézier either way round (its middle handles say which way it bends).
            Shape::Bezier { p } => {
                let p = if (p[0].x, p[0].y) <= (p[3].x, p[3].y) {
                    p
                } else {
                    [p[3], p[2], p[1], p[0]]
                };
                (
                    3,
                    [
                        q(p[0].x),
                        q(p[0].y),
                        q(p[1].x),
                        q(p[1].y),
                        q(p[3].x),
                        q(p[3].y),
                    ],
                )
            }
            _ => return None,
        })
    };
    let mut added: HashMap<(u8, [i64; 6]), EntityId> = HashMap::new();

    for e in &import.entities {
        let layer = layer_of(doc, &e.layer, &mut report);
        if !finite(&e.shape) {
            *report.skipped.entry(TOO_LARGE.into()).or_insert(0) += 1;
            continue;
        }
        let Some(id) = add_shape(doc, &e.shape) else {
            continue;
        };
        if let Some(key) = key_of(&e.shape) {
            added.entry(key).or_insert(id);
        }
        let _ = doc.set_entity_layer(id, layer);
        if e.color.is_some() || e.line_type.is_some() || e.weight.is_some() {
            let mut look = doc.get(id).map(|x| x.appearance).unwrap_or_default();
            look.color = e.color.or(look.color);
            look.line_type = e.line_type.or(look.line_type);
            look.weight = e.weight.or(look.weight);
            let _ = doc.set_appearance(id, look);
        }
        report.curves += 1;
    }

    let add_text = |doc: &mut Document, t: &DxfText, layer: LayerId| {
        let mut text = SketchText::new(t.text.clone(), t.at, t.height);
        text.angle = t.angle.rem_euclid(360.0);
        text.align = t.align;
        text.spacing = t.spacing.unwrap_or(SketchText::SPACING);
        text.wrap = t.wrap;
        let id = doc.add_text(text.clone());
        text.layer = layer;
        text.appearance.color = t.color;
        doc.set_text(id, text);
    };
    for t in &import.texts {
        let layer = layer_of(doc, &t.layer, &mut report);
        add_text(doc, t, layer);
        report.texts += 1;
    }

    // Hatches: their boundaries (shared with the drawing's own curves where they coincide),
    // then a fill on the outer one.
    let mut patterns = Patterns::new(doc);
    for h in &import.hatches {
        let layer = layer_of(doc, &h.layer, &mut report);
        let Some((seed, outer)) = interior(h) else {
            *report.skipped.entry("HATCH (no area)".into()).or_insert(0) += 1;
            continue;
        };
        let mut boundary = Vec::new();
        for (k, shapes) in h.loops.iter().enumerate() {
            for shape in shapes {
                let key = key_of(shape);
                let id = match key.and_then(|k| added.get(&k).copied()) {
                    Some(id) => Some(id),
                    None => {
                        let id = add_shape(doc, shape);
                        if let Some(id) = id {
                            let _ = doc.set_entity_layer(id, layer);
                            if import.fill_edges_construction {
                                doc.set_style(id, drift_core::LineKind::Construction);
                            }
                            if let Some(key) = key {
                                added.insert(key, id);
                            }
                            report.curves += 1;
                        }
                        id
                    }
                };
                if k == outer {
                    boundary.extend(id);
                }
            }
        }
        if boundary.is_empty() {
            continue;
        }
        let kind = patterns.kind(h);
        let mut fill = Fill::new(boundary, seed, kind);
        fill.color = h.color;
        if let Some(opacity) = h.opacity {
            fill.opacity = opacity.clamp(0.0, 1.0);
        }
        let id = doc.add_fill(fill.clone());
        if let Some(mut placed) = doc.fill(id).cloned() {
            placed.layer = layer;
            doc.set_fill(id, placed);
        }
        report.fills += 1;
    }

    // Copies of blocks: each block is created once, then placed. A block the drawing already
    // has by that name is used only if it is the same; otherwise the new one gets a name of
    // its own ("Door (2)"). A block's text is added with each copy (Drift's blocks hold
    // curves only).
    let mut block_text = false;
    let mut made: HashMap<&str, drift_core::BlockId> = HashMap::new();
    for insert in &import.inserts {
        let def = import.blocks.iter().find(|b| b.name == insert.block);
        let block = match made.get(insert.block.as_str()) {
            Some(b) => *b,
            None => {
                let Some(def) = def else {
                    continue;
                };
                let mut items: Vec<drift_core::BlockItem> = def
                    .entities
                    .iter()
                    .filter(|e| !matches!(e.shape, Shape::Point(_)))
                    .map(|e| drift_core::BlockItem {
                        shape: e.shape,
                        style: drift_core::LineKind::Solid,
                        appearance: drift_core::Appearance {
                            color: e.color,
                            weight: e.weight,
                            line_type: e.line_type,
                        },
                    })
                    .collect();
                // Its hatches fill the area of its curves around their seeds: their
                // boundaries join the curves where they aren't curves already (as
                // construction, if the import says fill edges are).
                let mut fills = Vec::new();
                for h in &def.hatches {
                    let Some((seed, _)) = interior(h) else {
                        continue;
                    };
                    for shape in h.loops.iter().flatten().filter(|s| finite(s)) {
                        let key = key_of(shape);
                        let known = items.iter().any(|it| {
                            it.shape == *shape || (key.is_some() && key_of(&it.shape) == key)
                        });
                        if !known {
                            items.push(drift_core::BlockItem {
                                shape: *shape,
                                style: if import.fill_edges_construction {
                                    drift_core::LineKind::Construction
                                } else {
                                    drift_core::LineKind::Solid
                                },
                                appearance: drift_core::Appearance::default(),
                            });
                        }
                    }
                    let mut fill = Fill::new(Vec::new(), seed, patterns.kind(h));
                    fill.color = h.color;
                    if let Some(opacity) = h.opacity {
                        fill.opacity = opacity.clamp(0.0, 1.0);
                    }
                    fills.push(fill);
                }
                let label = if def.label.is_empty() {
                    &def.name
                } else {
                    &def.label
                };
                let same = doc.block_named(label).filter(|b| {
                    doc.block(*b)
                        .is_some_and(|b| b.items == items && b.fills == fills)
                });
                let id = match same {
                    Some(b) => b,
                    None => {
                        let mut name = label.clone();
                        let mut n = 2;
                        while doc.block_named(&name).is_some() {
                            name = format!("{label} ({n})");
                            n += 1;
                        }
                        report.blocks += 1;
                        report.fills += fills.len();
                        doc.add_block(drift_core::Block {
                            name,
                            items,
                            fills,
                            profile: false,
                        })
                    }
                };
                made.insert(insert.block.as_str(), id);
                id
            }
        };
        let layer = layer_of(doc, &insert.layer, &mut report);
        let mut copy = drift_core::Instance::new(block, insert.at);
        copy.angle = insert.angle;
        copy.scale = insert.scale;
        copy.mirror = insert.mirror;
        if let Ok(id) = doc.add_instance(copy) {
            copy.layer = layer;
            let _ = doc.set_instance(id, copy);
            report.copies += 1;
        }
        for t in def.iter().flat_map(|d| d.texts.iter()) {
            let placed = DxfText {
                at: copy.apply(t.at),
                height: t.height * insert.scale,
                angle: t.angle + insert.angle,
                wrap: t.wrap.map(|w| w * insert.scale),
                ..t.clone()
            };
            let text_layer = if t.layer == "0" {
                layer
            } else {
                layer_of(doc, &t.layer, &mut report)
            };
            add_text(doc, &placed, text_layer);
            report.texts += 1;
            block_text = true;
        }
    }
    patterns.finish(doc, &mut report);
    if block_text {
        report.notes.push(
            "Text inside blocks was added beside each copy as ordinary text: it doesn't move \
             with the copy."
                .into(),
        );
    }

    // Layouts become sheets; their viewports, views.
    for layout in &import.layouts {
        let mut sheet = Sheet::new(layout.name.clone());
        // Its own title block was drawn on the paper, which isn't imported.
        sheet.title_block = TitleBlock::Border;
        if let Some(size) = layout.paper {
            let (long, short) = (size.x.max(size.y), size.x.min(size.y));
            sheet.portrait = size.y > size.x;
            sheet.paper = PaperSize::STANDARD
                .into_iter()
                .find(|p| {
                    let (l, s) = p.sides();
                    (l - long).abs() <= 1.5 && (s - short).abs() <= 1.5
                })
                .unwrap_or(PaperSize::Custom(long, short));
        }
        // Each viewport becomes a view of the model, named after its layout.
        let several = layout.views.len() > 1;
        for (i, v) in layout
            .views
            .iter()
            .filter(|v| v.scale.is_finite() && v.scale > 0.0)
            .enumerate()
        {
            let name = if several {
                format!("{} {}", layout.name, i + 1)
            } else {
                layout.name.clone()
            };
            let half = v.size * v.scale * 0.5;
            let crop = drift_core::Bounds {
                min: v.center - half,
                max: v.center + half,
            };
            let id = doc.add_view(View::new(name, crop, v.scale));
            sheet.viewports.push(Viewport::new(id, v.at));
        }
        doc.add_sheet(sheet);
        report.sheets += 1;
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DXF file from group codes and values.
    fn dxf(pairs: &[(i32, &str)]) -> String {
        pairs
            .iter()
            .map(|(code, value)| format!("{code:>3}\n{value}\n"))
            .collect()
    }

    /// A file with just these entities in model space.
    fn entities(body: &[(i32, &str)]) -> String {
        let mut all = vec![(0, "SECTION"), (2, "ENTITIES")];
        all.extend_from_slice(body);
        all.extend_from_slice(&[(0, "ENDSEC"), (0, "EOF")]);
        dxf(&all)
    }

    fn close(a: DVec2, b: DVec2) -> bool {
        a.distance(b) < 1e-9
    }

    /// Reading a file at 1 : 50 and scaling a full-size read by 50 give the same drawing
    /// (to rounding): every number in it agrees, and everything else is the same.
    fn assert_scaled_read_matches(file: &str) {
        let numbers = |import: &DxfImport| {
            let text = format!("{import:?}");
            let values: Vec<f64> = text
                .split(|c: char| !(c.is_ascii_digit() || "-.eE".contains(c)))
                .filter(|piece| piece.chars().any(|c| c.is_ascii_digit()))
                .filter_map(|piece| piece.parse().ok())
                .collect();
            let skeleton: String = text
                .chars()
                .filter(|c| !(c.is_ascii_digit() || "-.eE".contains(*c)))
                .collect();
            (skeleton, values)
        };
        let direct = read_at(file, 1.0, 50.0).unwrap();
        let scaled = read(file, 1.0).unwrap().scaled(50.0);
        let (a, b) = (numbers(&direct), numbers(&scaled));
        assert_eq!(a.0, b.0);
        assert_eq!(a.1.len(), b.1.len());
        for (x, y) in a.1.iter().zip(&b.1) {
            assert!((x - y).abs() <= 1e-9 * x.abs().max(1.0), "{x} vs {y}");
        }
    }

    fn at(shape: &Shape, t: f64) -> DVec2 {
        match *shape {
            Shape::Bezier { p } => {
                let u = 1.0 - t;
                p[0] * (u * u * u)
                    + p[1] * (3.0 * u * u * t)
                    + p[2] * (3.0 * u * t * t)
                    + p[3] * (t * t * t)
            }
            Shape::Line { a, b } => a + (b - a) * t,
            _ => panic!("{shape:?}"),
        }
    }

    #[test]
    fn a_drawing_drawn_at_a_scale_comes_in_that_much_larger() {
        let file = dxf(&[
            (0, "SECTION"),
            (2, "HEADER"),
            (9, "$INSUNITS"),
            (70, "4"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "LINE"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (11, "100"),
            (21, "0"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let length = |import: &DxfImport| match import.entities[0].shape {
            Shape::Line { a, b } => a.distance(b),
            ref other => panic!("{other:?}"),
        };
        assert!((length(&read(&file, 1.0).unwrap()) - 100.0).abs() < 1e-9);
        // In millimetres, so the unit for unitless files doesn't matter; 1 : 50 does.
        assert!((length(&read_at(&file, 25.4, 50.0).unwrap()) - 5000.0).abs() < 1e-9);
    }

    #[test]
    fn splines_become_cubic_curves() {
        // One clamped cubic span: exactly its control points.
        let one = entities(&[
            (0, "SPLINE"),
            (8, "CURVES"),
            (70, "8"),
            (71, "3"),
            (72, "8"),
            (73, "4"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "20"),
            (10, "20"),
            (20, "20"),
            (10, "30"),
            (20, "0"),
        ]);
        let read_one = read(&one, 1.0).unwrap();
        assert_eq!(read_one.entities.len(), 1);
        assert_eq!(read_one.entities[0].layer, "CURVES");
        let Shape::Bezier { p } = read_one.entities[0].shape else {
            panic!("a curve");
        };
        let want = [(0.0, 0.0), (10.0, 20.0), (20.0, 20.0), (30.0, 0.0)];
        assert!(
            p.iter()
                .zip(want)
                .all(|(p, w)| close(*p, DVec2::new(w.0, w.1)))
        );
        assert!(read_one.notes.is_empty() && read_one.skipped.is_empty());

        // Two spans (one inside knot): two curves that meet, smoothly, and end on the ends.
        let two = entities(&[
            (0, "SPLINE"),
            (71, "3"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "1"),
            (40, "2"),
            (40, "2"),
            (40, "2"),
            (40, "2"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "20"),
            (10, "20"),
            (20, "20"),
            (10, "30"),
            (20, "0"),
            (10, "40"),
            (20, "10"),
        ]);
        let curves: Vec<Shape> = read(&two, 1.0)
            .unwrap()
            .entities
            .into_iter()
            .map(|e| e.shape)
            .collect();
        assert_eq!(curves.len(), 2);
        assert!(close(at(&curves[0], 0.0), DVec2::ZERO));
        assert!(close(at(&curves[1], 1.0), DVec2::new(40.0, 10.0)));
        assert!(close(at(&curves[0], 1.0), at(&curves[1], 0.0)));
        let (Shape::Bezier { p: a }, Shape::Bezier { p: b }) = (curves[0], curves[1]) else {
            panic!("curves");
        };
        // Tangent across the join: the handles either side are in line.
        assert!((a[3] - a[2]).perp_dot(b[1] - b[0]).abs() < 1e-9);
        // The join is the B-spline's own point at the inside knot (worked by hand).
        assert!(close(a[3], DVec2::new(20.0, 15.0)), "{:?}", a[3]);

        // A quadratic is raised to a cubic exactly: its middle is where the parabola's is.
        let quad = entities(&[
            (0, "SPLINE"),
            (71, "2"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "10"),
            (10, "20"),
            (20, "0"),
        ]);
        let q = read(&quad, 1.0).unwrap();
        assert!(close(at(&q.entities[0].shape, 0.5), DVec2::new(10.0, 5.0)));

        // A weighted spline (a quarter circle as a rational quadratic) is fitted, and says so.
        let w = std::f64::consts::FRAC_1_SQRT_2.to_string();
        let arc = entities(&[
            (0, "SPLINE"),
            (70, "4"),
            (71, "2"),
            (40, "0"),
            (40, "0"),
            (40, "0"),
            (40, "1"),
            (40, "1"),
            (40, "1"),
            (41, "1"),
            (41, &w),
            (41, "1"),
            (10, "10"),
            (20, "0"),
            (10, "10"),
            (20, "10"),
            (10, "0"),
            (20, "10"),
        ]);
        let r = read(&arc, 1.0).unwrap();
        assert_eq!(r.entities.len(), 4, "fitted in quarters");
        for piece in &r.entities {
            for t in [0.0, 0.2, 0.5, 0.8, 1.0] {
                let radius = at(&piece.shape, t).length();
                assert!((radius - 10.0).abs() < 1e-3, "{radius}");
            }
        }
        assert!(close(at(&r.entities[0].shape, 0.0), DVec2::new(10.0, 0.0)));
        assert!(close(at(&r.entities[3].shape, 1.0), DVec2::new(0.0, 10.0)));
        assert!(r.notes.iter().any(|n| n.contains("fitted")));

        // Stored as fit points only: a curve through every one of them.
        let fit = entities(&[
            (0, "SPLINE"),
            (71, "3"),
            (11, "0"),
            (21, "0"),
            (11, "10"),
            (21, "5"),
            (11, "20"),
            (21, "0"),
            (11, "30"),
            (21, "5"),
        ]);
        let f = read(&fit, 1.0).unwrap();
        assert_eq!(f.entities.len(), 3);
        assert!(close(at(&f.entities[1].shape, 0.0), DVec2::new(10.0, 5.0)));
        assert!(close(at(&f.entities[2].shape, 1.0), DVec2::new(30.0, 5.0)));
    }

    #[test]
    fn text_and_paragraph_text_are_read() {
        let file = entities(&[
            (0, "TEXT"),
            (8, "NOTES"),
            (10, "5"),
            (20, "7"),
            (40, "2.5"),
            (1, "%%c12 %%p0.1 45%%d"),
            (62, "1"),
            // Right-aligned on its second point, turned a quarter turn.
            (0, "TEXT"),
            (10, "0"),
            (20, "0"),
            (11, "50"),
            (21, "20"),
            (40, "3"),
            (1, "END"),
            (50, "90"),
            (72, "2"),
            // Middle-centred paragraph text with formatting, in two chunks and two lines.
            (0, "MTEXT"),
            (8, "NOTES"),
            (10, "100"),
            (20, "50"),
            (40, "4"),
            (71, "5"),
            (3, "{\\fArial|b1|i0;\\H1.5x;GENERAL }"),
            (1, "NOTES\\P\\LAll\\l dims \\S1^2; \\U+00D810"),
            (0, "TEXT"),
            (10, "0"),
            (20, "0"),
            (40, "2"),
            (1, "   "),
        ]);
        let back = read(&file, 1.0).unwrap();
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
        let texts = &back.texts;
        assert_eq!(
            texts.len(),
            3,
            "blank text is left out; the paragraph is one text"
        );
        assert_eq!(texts[0].text, "Ø12 ±0.1 45°");
        assert_eq!(
            (texts[0].at, texts[0].height, texts[0].layer.as_str()),
            (DVec2::new(5.0, 7.0), 2.5, "NOTES")
        );
        assert_eq!(
            (texts[0].color, texts[0].align),
            (Some(Rgb([255, 0, 0])), TextAlign::Left)
        );
        // Right-aligned and vertical: it keeps its alignment, anchored on its second point.
        assert!((texts[1].angle - 90.0).abs() < 1e-9);
        assert_eq!(
            (texts[1].at, texts[1].align),
            (DVec2::new(50.0, 20.0), TextAlign::Right)
        );
        assert!(text_width("END", 3.0) > 4.0);
        // The paragraph: one text of two lines, codes gone, the stack as a fraction, the
        // escape decoded.
        let para = &texts[2];
        assert_eq!(para.text, "GENERAL NOTES\nAll dims 1/2 Ø10");
        assert_eq!(para.align, TextAlign::Center);
        assert!((para.spacing.unwrap() - 5.0 / 3.0).abs() < 1e-12);
        assert_eq!(para.wrap, None);
        // Centred on (100, 50) both ways: the anchor is on its centre line, and the two
        // baselines straddle the middle.
        assert_eq!(para.at.x, 100.0);
        let second = para.at.y - para.spacing.unwrap() * para.height;
        assert!(((para.at.y + para.height + second) / 2.0 - 50.0).abs() < 1e-9);

        // With a width, long lines wrap, and the block hangs from its top left corner.
        let wide = entities(&[
            (0, "MTEXT"),
            (10, "0"),
            (20, "100"),
            (40, "2"),
            (41, "30"),
            (71, "1"),
            (1, "one two three four five six seven eight nine ten"),
        ]);
        let wrapped = read(&wide, 1.0).unwrap();
        let w = &wrapped.texts[0];
        assert_eq!((w.wrap, w.align), (Some(30.0), TextAlign::Left));
        assert_eq!(
            w.at,
            DVec2::new(0.0, 98.0),
            "the first baseline, a cap height down"
        );
        let lines =
            drift_draft::outline::paragraph_lines(&w.text, &TextFont::Sans, w.height, w.wrap);
        assert!(lines.len() >= 2, "{lines:?}");
        assert!(lines.iter().all(|l| text_width(l, 2.0) <= 30.0));
        assert_eq!(lines.join(" "), w.text);

        // Into a document: text on its layer, in its colour.
        let mut doc = Document::new();
        doc.begin();
        let report = add_to_document(&mut doc, &back);
        doc.commit("Import");
        assert_eq!(report.texts, 3);
        let para = doc
            .texts()
            .map(|(_, t)| t.clone())
            .find(|t| t.text.contains('\n'))
            .unwrap();
        assert_eq!((para.align, para.wrap), (TextAlign::Center, None));
        assert!((para.spacing - 5.0 / 3.0).abs() < 1e-12);
        let first = doc
            .texts()
            .map(|(_, t)| t.clone())
            .find(|t| t.text.starts_with('Ø'))
            .unwrap();
        assert_eq!(doc.layer(first.layer).unwrap().name, "NOTES");
        assert_eq!(first.appearance.color, Some(Rgb([255, 0, 0])));
    }

    #[test]
    fn dimensions_come_in_as_they_are_drawn() {
        let file = dxf(&[
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (2, "*D1"),
            (10, "0"),
            (20, "0"),
            (0, "LINE"),
            (8, "0"),
            (10, "0"),
            (20, "10"),
            (11, "50"),
            (21, "10"),
            (0, "LINE"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (11, "0"),
            (21, "12"),
            (0, "SOLID"),
            (8, "0"),
            (10, "0"),
            (20, "10"),
            (11, "3"),
            (21, "10.5"),
            (12, "3"),
            (22, "9.5"),
            (13, "3"),
            (23, "9.5"),
            (0, "MTEXT"),
            (8, "0"),
            (10, "25"),
            (20, "11"),
            (40, "2.5"),
            (71, "8"),
            (1, "50"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "DIMENSION"),
            (8, "DIMS"),
            (2, "*D1"),
            (10, "50"),
            (20, "10"),
            (70, "32"),
            (62, "3"),
            (0, "DIMENSION"),
            (8, "DIMS"),
            (2, "*D9"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let back = read(&file, 25.4).unwrap();
        assert_eq!(back.dimensions, 1);
        assert_eq!(back.skipped.get("DIMENSION (no drawn geometry)"), Some(&1));
        // Two lines, and the arrowhead (a SOLID) as a solid fill, all on the dimension's
        // layer and in its colour ("by block" inside it); the number as text.
        assert_eq!(back.entities.len(), 2);
        assert!(back.entities.iter().all(|e| e.layer == "DIMS"));
        assert_eq!(back.hatches.len(), 1);
        let arrow = &back.hatches[0];
        assert!(arrow.solid && arrow.loops[0].len() == 3 && arrow.layer == "DIMS");
        assert_eq!(arrow.color, back.entities[0].color);
        assert_eq!(back.texts.len(), 1);
        assert_eq!(
            (back.texts[0].text.as_str(), back.texts[0].layer.as_str()),
            ("50", "DIMS")
        );
        // No unit in the file: read in the default (inches here).
        assert!(back.entities.iter().any(|e| e.shape
            == Shape::Line {
                a: DVec2::new(0.0, 254.0),
                b: DVec2::new(1270.0, 254.0)
            }));
        assert!(back.notes.iter().any(|n| n.contains("1 dimension")));
        // Nothing of the dimension's block is left as a block.
        assert!(back.blocks.is_empty() && back.inserts.is_empty());
    }

    #[test]
    fn round_off_from_turning_a_hatch_is_tidied() {
        // What macOS's atan2 made of a 45° hatch, and its neighbours.
        assert_eq!(tidy_degrees(44.99999999999999), 45.0);
        assert_eq!(tidy_degrees(359.99999999999994), 0.0);
        assert_eq!(tidy_degrees(-1e-15), 0.0);
        assert_eq!(tidy(1.9999999999999998), 2.0);
        assert_eq!(tidy(3.175000000000001), 3.175);
        // Real values are left alone.
        assert_eq!(tidy(44.9999), 44.9999);
        assert_eq!(tidy(0.3333333333), 0.3333333333);
        assert_eq!(tidy(12345.678901), 12345.678901);
    }

    #[test]
    fn hatches_become_fills_on_their_boundaries() {
        let file = entities(&[
            // The outline the hatch was drawn on.
            (0, "LWPOLYLINE"),
            (8, "WALLS"),
            (90, "4"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (10, "40"),
            (20, "0"),
            (10, "40"),
            (20, "30"),
            (10, "0"),
            (20, "30"),
            // A hatch on it, with a round hole given as two arc edges.
            (0, "HATCH"),
            (8, "HATCH"),
            (2, "ANSI31"),
            (70, "0"),
            (71, "1"),
            (91, "2"),
            (92, "3"),
            (72, "0"),
            (73, "1"),
            (93, "4"),
            (10, "0"),
            (20, "0"),
            (10, "40"),
            (20, "0"),
            (10, "40"),
            (20, "30"),
            (10, "0"),
            (20, "30"),
            (97, "0"),
            (92, "16"),
            (93, "2"),
            (72, "2"),
            (10, "20"),
            (20, "15"),
            (40, "5"),
            (50, "0"),
            (51, "180"),
            (73, "1"),
            (72, "2"),
            (10, "20"),
            (20, "15"),
            (40, "5"),
            (50, "180"),
            (51, "360"),
            (73, "1"),
            (97, "0"),
            (75, "0"),
            (76, "1"),
            (52, "45"),
            (41, "2"),
            (77, "0"),
            (78, "1"),
            // ANSI31 as AutoCAD writes it at scale 2, turned 45°: its lines (45° in the
            // pattern) at 90°, 6.35 apart, the step in drawing coordinates.
            (53, "90"),
            (43, "0"),
            (44, "0"),
            (45, "-6.35"),
            (46, "0"),
            (79, "0"),
            (98, "1"),
            (10, "5"),
            (20, "5"),
            // A solid fill of a triangle given as line edges, with nothing drawn under it.
            (0, "HATCH"),
            (8, "HATCH"),
            (2, "SOLID"),
            (70, "1"),
            (91, "1"),
            (92, "1"),
            (93, "3"),
            (72, "1"),
            (10, "60"),
            (20, "0"),
            (11, "80"),
            (21, "0"),
            (72, "1"),
            (10, "80"),
            (20, "0"),
            (11, "70"),
            (21, "20"),
            (72, "1"),
            (10, "70"),
            (20, "20"),
            (11, "60"),
            (21, "0"),
            (97, "0"),
            (75, "0"),
            (76, "1"),
            (98, "0"),
        ]);
        let back = read(&file, 1.0).unwrap();
        assert!(back.skipped.is_empty(), "{:?}", back.skipped);
        assert_eq!(back.hatches.len(), 2);
        let h = &back.hatches[0];
        assert_eq!((h.pattern.as_str(), h.solid), ("ANSI31", false));
        assert!((h.scale - 2.0).abs() < 1e-9 && (h.angle - 45.0).abs() < 1e-9);
        assert_eq!(h.loops.len(), 2);
        assert_eq!((h.loops[0].len(), h.loops[1].len()), (4, 2));
        assert!(matches!(h.loops[1][0], Shape::Arc { radius, .. } if (radius - 5.0).abs() < 1e-9));
        assert_eq!(h.seeds, vec![DVec2::new(5.0, 5.0)]);
        assert!(back.hatches[1].solid && back.hatches[1].loops[0].len() == 3);

        let mut doc = Document::new();
        doc.begin();
        let report = add_to_document(&mut doc, &back);
        doc.commit("Import");
        assert_eq!(report.fills, 2);
        // The square is there once (the hatch shares it), plus the hole's two arcs and the
        // triangle's three lines.
        let lines = doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, drift_core::EntityKind::Line { .. }))
            .count();
        let arcs = doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, drift_core::EntityKind::Arc { .. }))
            .count();
        assert_eq!((lines, arcs), (7, 2));
        let fills: Vec<Fill> = doc.fills().map(|(_, f)| f.clone()).collect();
        let hatch = fills
            .iter()
            .find(|f| matches!(f.kind, FillKind::Hatch(_)))
            .unwrap();
        assert_eq!(hatch.boundary.len(), 4, "the square's four sides");
        assert_eq!(hatch.seed, DVec2::new(5.0, 5.0));
        assert_eq!(doc.layer(hatch.layer).unwrap().name, "HATCH");
        let FillKind::Hatch(pattern) = &hatch.kind else {
            unreachable!()
        };
        assert_eq!(pattern.pattern, "ANSI31");
        assert!((pattern.scale - 2.0).abs() < 1e-9 && (pattern.angle - 45.0).abs() < 1e-9);
        // The solid one found a point inside its triangle by itself.
        let solid = fills.iter().find(|f| f.kind == FillKind::Solid).unwrap();
        assert!(
            solid.seed.x > 60.0 && solid.seed.x < 80.0 && solid.seed.y > 0.0 && solid.seed.y < 20.0
        );
        assert_scaled_read_matches(&file);
    }

    #[test]
    fn blocks_nest_stretch_repeat_and_carry_attributes() {
        let file = dxf(&[
            (0, "SECTION"),
            (2, "BLOCKS"),
            // A bolt: a unit circle, with a constant label and a tag to fill in.
            (0, "BLOCK"),
            (2, "BOLT"),
            (10, "0"),
            (20, "0"),
            (0, "CIRCLE"),
            (8, "0"),
            (10, "0"),
            (20, "0"),
            (40, "1"),
            (62, "0"),
            (0, "ATTDEF"),
            (8, "0"),
            (10, "0"),
            (20, "2"),
            (40, "1"),
            (1, "M8"),
            (2, "SIZE"),
            (70, "2"),
            (0, "ATTDEF"),
            (8, "0"),
            (10, "0"),
            (20, "-3"),
            (40, "1"),
            (1, "?"),
            (2, "MARK"),
            (70, "0"),
            (0, "ENDBLK"),
            // A plate holding two bolts, its base point at (10, 0).
            (0, "BLOCK"),
            (2, "PLATE"),
            (10, "10"),
            (20, "0"),
            (0, "LINE"),
            (8, "0"),
            (10, "10"),
            (20, "0"),
            (11, "30"),
            (21, "0"),
            (0, "INSERT"),
            (8, "0"),
            (2, "BOLT"),
            (10, "15"),
            (20, "0"),
            (0, "INSERT"),
            (8, "0"),
            (2, "BOLT"),
            (10, "25"),
            (20, "0"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            // An ordinary copy, with its attribute's value.
            (0, "INSERT"),
            (8, "PARTS"),
            (2, "BOLT"),
            (10, "100"),
            (20, "50"),
            (66, "1"),
            (62, "5"),
            (0, "ATTRIB"),
            (8, "PARTS"),
            (10, "100"),
            (20, "47"),
            (40, "1"),
            (1, "B-7"),
            (2, "MARK"),
            (70, "0"),
            (0, "SEQEND"),
            // A nested block, placed evenly: it stays a block, its bolts expanded inside it.
            (0, "INSERT"),
            (8, "PARTS"),
            (2, "PLATE"),
            (10, "0"),
            (20, "100"),
            (50, "90"),
            // Stretched: twice as wide as tall. Its circle is an ellipse.
            (0, "INSERT"),
            (8, "PARTS"),
            (2, "BOLT"),
            (10, "200"),
            (20, "0"),
            (41, "2"),
            (42, "1"),
            // An array: three columns, two rows.
            (0, "INSERT"),
            (8, "PARTS"),
            (2, "BOLT"),
            (10, "300"),
            (20, "0"),
            (70, "3"),
            (71, "2"),
            (44, "10"),
            (45, "20"),
            (0, "INSERT"),
            (8, "PARTS"),
            (2, "MISSING"),
            (10, "0"),
            (20, "0"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let back = read(&file, 1.0).unwrap();
        assert_eq!(back.skipped.get("INSERT (block not in the file)"), Some(&1));
        assert_eq!(back.skipped.len(), 1, "{:?}", back.skipped);
        // Kept as copies: the plain bolt and the plate.
        assert_eq!(back.inserts.len(), 2);
        assert_eq!(back.inserts[0].at, DVec2::new(100.0, 50.0));
        // Its circle is "by block" and the copy is blue: the block it places is the bolt in
        // blue, shown by the bolt's own name.
        let bolt = back
            .blocks
            .iter()
            .find(|b| b.name == back.inserts[0].block)
            .unwrap();
        assert_eq!(bolt.label, "BOLT");
        assert_eq!(bolt.entities[0].color, Some(Rgb([0, 0, 255])));
        assert_eq!(back.inserts[1].block, "PLATE");
        assert!((back.inserts[1].angle - 90.0).abs() < 1e-9);
        // The plate's definition: its line from the base point, and both bolts' circles
        // (and their constant labels), relative to it.
        let plate = back.blocks.iter().find(|b| b.name == "PLATE").unwrap();
        assert_eq!(plate.entities.len(), 3);
        assert_eq!(
            plate.entities[0].shape,
            Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(20.0, 0.0)
            }
        );
        assert!(plate.entities.iter().any(|e| e.shape
            == Shape::Circle {
                center: DVec2::new(15.0, 0.0),
                radius: 1.0
            }));
        assert_eq!(plate.texts.len(), 2);
        assert!(plate.texts.iter().all(|t| t.text == "M8"));
        // The attribute's value is text where the file put it; the constant comes with the
        // bolt's definition; the tag's "?" placeholder is never drawn.
        assert!(
            back.texts
                .iter()
                .any(|t| t.text == "B-7" && t.at == DVec2::new(100.0, 47.0))
        );
        assert!(back.texts.iter().all(|t| t.text != "?"));
        // The stretched copy: an ellipse 2 by 1 on the insert's layer.
        let stretched: Vec<&DxfEntity> = back
            .entities
            .iter()
            .filter(|e| matches!(e.shape, Shape::Ellipse { .. }))
            .collect();
        assert_eq!(stretched.len(), 1);
        let Shape::Ellipse {
            center,
            major,
            minor,
            sweep,
            ..
        } = stretched[0].shape
        else {
            unreachable!()
        };
        assert!(
            close(center, DVec2::new(200.0, 0.0))
                && (major.length() - 2.0).abs() < 1e-9
                && (minor - 1.0).abs() < 1e-9
        );
        assert_eq!(sweep, TAU);
        assert_eq!(stretched[0].layer, "PARTS");
        // The array: six circles, ten apart across and twenty up.
        let circles: Vec<DVec2> = back
            .entities
            .iter()
            .filter_map(|e| match e.shape {
                Shape::Circle { center, .. } if center.x >= 300.0 => Some(center),
                _ => None,
            })
            .collect();
        assert_eq!(circles.len(), 6);
        assert!(
            circles.contains(&DVec2::new(320.0, 20.0)) && circles.contains(&DVec2::new(300.0, 0.0))
        );

        let mut doc = Document::new();
        doc.begin();
        let report = add_to_document(&mut doc, &back);
        doc.commit("Import");
        assert_eq!((report.blocks, report.copies), (2, 2));
        // B-7, the stretched bolt's and the array's M8s (7), the plain bolt's M8, and the
        // plate's two.
        assert_eq!(report.texts, 1 + 7 + 1 + 2);
        // The plate was turned a quarter turn about its base point: its bolts' labels too.
        let turned = doc
            .texts()
            .map(|(_, t)| t.clone())
            .filter(|t| (t.angle - 90.0).abs() < 1e-9)
            .count();
        assert_eq!(turned, 2);
        assert_scaled_read_matches(&file);
    }

    #[test]
    fn mirrored_planes_and_colours_are_understood() {
        // Drawn "from behind" (extrusion 0, 0, -1): x runs the other way, and so do angles.
        let file = entities(&[
            (0, "ARC"),
            (10, "-10"),
            (20, "0"),
            (40, "5"),
            (50, "0"),
            (51, "90"),
            (230, "-1"),
            (0, "CIRCLE"),
            (10, "-30"),
            (20, "5"),
            (40, "2"),
            (230, "-1"),
            (62, "30"),
            (0, "LINE"),
            (10, "1"),
            (20, "2"),
            (11, "3"),
            (21, "4"),
            (420, "16711935"),
            (370, "50"),
            (0, "ELLIPSE"),
            (10, "0"),
            (20, "0"),
            (11, "10"),
            (21, "0"),
            (40, "0.5"),
            (41, "0"),
            (42, "1.5707963267948966"),
            (230, "-1"),
        ]);
        let back = read(&file, 1.0).unwrap();
        let Shape::Arc {
            center,
            start,
            sweep,
            ..
        } = back.entities[0].shape
        else {
            panic!("{:?}", back.entities[0].shape);
        };
        // From (15, 0) round to (10, 5) seen from behind is (5, 0) to (10, 5) from in front:
        // counter-clockwise from 90 to 180 degrees about (10, 0).
        assert!(close(center, DVec2::new(10.0, 0.0)));
        assert!((start - TAU / 4.0).abs() < 1e-9 && (sweep - TAU / 4.0).abs() < 1e-9);
        assert_eq!(
            back.entities[1].shape,
            Shape::Circle {
                center: DVec2::new(30.0, 5.0),
                radius: 2.0
            }
        );
        assert_eq!(back.entities[1].color, Some(Rgb([255, 127, 0])));
        assert_eq!(
            back.entities[2].color,
            Some(Rgb([255, 0, 255])),
            "a true colour"
        );
        assert_eq!(back.entities[2].weight, Some(LineWeight(50)));
        // The ellipse's quarter runs the other way round: from its major axis down.
        let Shape::Ellipse { start, sweep, .. } = back.entities[3].shape else {
            panic!("an ellipse arc");
        };
        assert!((sweep - TAU / 4.0).abs() < 1e-9);
        assert!((start - 3.0 * TAU / 4.0).abs() < 1e-9, "{start}");
        // The whole colour index is known, not a handful of it.
        assert_eq!(aci_rgb(1), Some(Rgb([255, 0, 0])));
        assert_eq!(aci_rgb(90), Some(Rgb([0, 255, 0])));
        assert_eq!(aci_rgb(170), Some(Rgb([0, 0, 255])));
        assert_eq!(aci_rgb(7), None);
        assert!((10..=255).all(|i| aci_rgb(i).is_some()));
    }

    #[test]
    fn copies_in_an_inch_drawing_are_scaled_by_the_unit_once() {
        // A drawing in inches: a 10" square block placed at twice its size, and once at
        // its own size. The block's contents come in millimetres, so the copies' scales
        // stay the file's (#83: they came in 25.4 times too large).
        let file = dxf(&[
            (0, "SECTION"),
            (2, "HEADER"),
            (9, "$INSUNITS"),
            (70, "1"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (2, "TAG"),
            (10, "0"),
            (20, "0"),
            (0, "LWPOLYLINE"),
            (8, "0"),
            (90, "4"),
            (70, "1"),
            (10, "0"),
            (20, "0"),
            (10, "10"),
            (20, "0"),
            (10, "10"),
            (20, "10"),
            (10, "0"),
            (20, "10"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "INSERT"),
            (8, "0"),
            (2, "TAG"),
            (10, "100"),
            (20, "0"),
            (41, "2"),
            (42, "2"),
            (50, "90"),
            (0, "INSERT"),
            (8, "0"),
            (2, "TAG"),
            (10, "0"),
            (20, "0"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let back = read(&file, 1.0).unwrap();
        let tag = back.blocks.iter().find(|b| b.name == "TAG").unwrap();
        assert!(tag.entities.iter().any(|e| e.shape
            == Shape::Line {
                a: DVec2::ZERO,
                b: DVec2::new(254.0, 0.0)
            }));
        assert_eq!(back.inserts.len(), 2);
        let (big, own) = (&back.inserts[0], &back.inserts[1]);
        assert!(close(big.at, DVec2::new(2540.0, 0.0)));
        assert!((big.scale - 2.0).abs() < 1e-9, "{}", big.scale);
        assert!((big.angle - 90.0).abs() < 1e-9);
        assert!((own.scale - 1.0).abs() < 1e-9, "{}", own.scale);

        // Placed in a drawing, the bigger copy is 20" (508 mm) square.
        let mut doc = Document::new();
        add_to_document(&mut doc, &back);
        let copy = doc
            .instances()
            .find(|(_, i)| (i.scale - 2.0).abs() < 1e-9)
            .map(|(_, i)| *i)
            .unwrap();
        let far = copy.apply(DVec2::new(254.0, 254.0));
        assert!(close(far, DVec2::new(2540.0 - 508.0, 508.0)), "{far}");
    }

    #[test]
    fn paper_space_is_kept_out_of_the_model_and_layouts_become_sheets() {
        let file = dxf(&[
            (0, "SECTION"),
            (2, "HEADER"),
            (9, "$INSUNITS"),
            (70, "4"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "TABLES"),
            (0, "TABLE"),
            (2, "LTYPE"),
            (0, "LTYPE"),
            (2, "CONTINUOUS"),
            (73, "0"),
            (40, "0"),
            (0, "LTYPE"),
            (2, "FENCELINE1"),
            (73, "4"),
            (40, "20"),
            (49, "12"),
            (49, "-3"),
            (49, "0"),
            (49, "-3"),
            (0, "LTYPE"),
            (2, "ACAD_ISO02W100"),
            (73, "2"),
            (40, "15"),
            (49, "12"),
            (49, "-3"),
            (0, "LTYPE"),
            (2, "TINY"),
            (73, "2"),
            (40, "3"),
            (49, "2"),
            (49, "-1"),
            (0, "ENDTAB"),
            (0, "TABLE"),
            (2, "LAYER"),
            (0, "LAYER"),
            (2, "FENCE"),
            (70, "4"),
            (62, "-3"),
            (6, "FENCELINE1"),
            (370, "35"),
            (290, "0"),
            (0, "ENDTAB"),
            (0, "TABLE"),
            (2, "BLOCK_RECORD"),
            (0, "BLOCK_RECORD"),
            (5, "1F"),
            (2, "*Model_Space"),
            (0, "BLOCK_RECORD"),
            (5, "1B"),
            (2, "*Paper_Space"),
            (0, "BLOCK_RECORD"),
            (5, "2A"),
            (2, "*Paper_Space0"),
            (0, "ENDTAB"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "BLOCKS"),
            (0, "BLOCK"),
            (2, "*Model_Space"),
            (0, "ENDBLK"),
            (0, "BLOCK"),
            (2, "*Paper_Space"),
            (0, "ENDBLK"),
            // A second layout's content lives in its block.
            (0, "BLOCK"),
            (2, "*Paper_Space0"),
            (0, "VIEWPORT"),
            (67, "1"),
            (10, "100"),
            (20, "100"),
            (40, "300"),
            (41, "200"),
            (69, "1"),
            (68, "1"),
            (0, "VIEWPORT"),
            (67, "1"),
            (10, "5"),
            (20, "4"),
            (40, "6"),
            (41, "4"),
            (69, "2"),
            (68, "2"),
            (12, "0"),
            (22, "0"),
            (45, "5080"),
            (0, "ENDBLK"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "ENTITIES"),
            (0, "LINE"),
            (8, "FENCE"),
            (10, "0"),
            (20, "0"),
            (11, "1000"),
            (21, "0"),
            (0, "LINE"),
            (8, "0"),
            (6, "ACAD_ISO02W100"),
            (10, "0"),
            (20, "5"),
            (11, "9"),
            (21, "5"),
            (0, "LINE"),
            (8, "0"),
            (6, "TINY"),
            (10, "0"),
            (20, "6"),
            (11, "9"),
            (21, "6"),
            // The active layout: its title block, and a viewport at 1:50.
            (0, "LINE"),
            (67, "1"),
            (8, "TITLE"),
            (10, "0"),
            (20, "0"),
            (11, "400"),
            (21, "0"),
            (0, "TEXT"),
            (67, "1"),
            (8, "TITLE"),
            (10, "300"),
            (20, "10"),
            (40, "5"),
            (1, "PLAN"),
            (0, "VIEWPORT"),
            (67, "1"),
            (10, "210"),
            (20, "148"),
            (40, "420"),
            (41, "297"),
            (69, "1"),
            (68, "1"),
            (0, "VIEWPORT"),
            (67, "1"),
            (10, "200"),
            (20, "150"),
            (40, "300"),
            (41, "200"),
            (69, "2"),
            (68, "2"),
            (12, "5000"),
            (22, "2500"),
            (45, "10000"),
            (0, "ENDSEC"),
            (0, "SECTION"),
            (2, "OBJECTS"),
            (0, "LAYOUT"),
            (5, "59"),
            (330, "1A"),
            (100, "AcDbPlotSettings"),
            (1, ""),
            (2, "none"),
            (40, "5"),
            (41, "5"),
            (42, "5"),
            (43, "5"),
            (44, "297"),
            (45, "420"),
            (72, "1"),
            (73, "1"),
            (100, "AcDbLayout"),
            (1, "Ground floor"),
            (70, "1"),
            (71, "1"),
            (330, "1B"),
            (0, "LAYOUT"),
            (5, "5A"),
            (330, "1A"),
            (100, "AcDbPlotSettings"),
            (1, ""),
            (44, "215.9"),
            (45, "279.4"),
            (72, "0"),
            (73, "1"),
            (100, "AcDbLayout"),
            (1, "Details"),
            (70, "1"),
            (71, "2"),
            (330, "2A"),
            (0, "LAYOUT"),
            (5, "5B"),
            (330, "1A"),
            (100, "AcDbPlotSettings"),
            (1, ""),
            (100, "AcDbLayout"),
            (1, "Model"),
            (70, "1"),
            (71, "0"),
            (330, "1F"),
            (0, "ENDSEC"),
            (0, "EOF"),
        ]);
        let back = read(&file, 25.4).unwrap();
        // Only the three model-space lines are in the model.
        assert_eq!(back.entities.len(), 3);
        assert!(back.texts.is_empty());
        // A custom line type takes the nearest of Drift's: a long dash and a dot is a centre
        // line; an ISO dash is dashed; a short dash is hidden.
        let fence = &back.layers[0];
        assert_eq!(fence.line_type, LineType::Center);
        assert_eq!(back.entities[1].line_type, Some(LineType::Dashed));
        assert_eq!(back.entities[2].line_type, Some(LineType::Hidden));
        assert!(
            back.notes.iter().any(|n| n.contains("3 custom line type")),
            "{:?}",
            back.notes
        );
        // The layer: off (a negative colour), locked, not plotted, 0.35 wide.
        assert!(!fence.visible && fence.locked && !fence.printable);
        assert_eq!(
            (fence.color, fence.weight),
            (Some(Rgb([0, 255, 0])), Some(LineWeight(35)))
        );

        // Two layouts, named, in tab order.
        assert_eq!(back.layouts.len(), 2);
        let ground = back
            .layouts
            .iter()
            .find(|l| l.name == "Ground floor")
            .unwrap();
        // Portrait A3 media turned a quarter turn: landscape.
        assert_eq!(ground.paper, Some(DVec2::new(420.0, 297.0)));
        assert_eq!(ground.dropped, 2, "the title block's line and text");
        assert_eq!(
            ground.views.len(),
            1,
            "the paper's own viewport isn't a view"
        );
        let v = &ground.views[0];
        // Inside the 5 mm margin; 10 m of model in 200 mm of paper is 1:50.
        assert_eq!(
            (v.at, v.size),
            (DVec2::new(205.0, 155.0), DVec2::new(300.0, 200.0))
        );
        assert_eq!((v.center, v.scale), (DVec2::new(5000.0, 2500.0), 50.0));
        // The other layout is in inches: a 6 by 4 inch window, 5.08 m tall in the model.
        let details = back.layouts.iter().find(|l| l.name == "Details").unwrap();
        let v = &details.views[0];
        assert!(close(v.size, DVec2::new(152.4, 101.6)) && (v.scale - 50.0).abs() < 1e-9);
        assert!(
            back.notes
                .iter()
                .any(|n| n.contains("2 object(s) drawn on layouts"))
        );

        let mut doc = Document::new();
        doc.begin();
        let report = add_to_document(&mut doc, &back);
        doc.commit("Import");
        assert_eq!(report.sheets, 2);
        let sheet = doc
            .sheets()
            .map(|(_, s)| s.clone())
            .find(|s| s.name == "Ground floor")
            .unwrap();
        assert_eq!((sheet.paper, sheet.portrait), (PaperSize::A3, false));
        assert_eq!(sheet.title_block, TitleBlock::Border);
        let views = doc.placed_views(&sheet);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].scale, 50.0);
        assert_eq!(views[0].frame.size, DVec2::new(300.0, 200.0));
        assert_eq!(views[0].name, "Ground floor");
        let fence = doc
            .layers()
            .map(|(_, l)| l.clone())
            .find(|l| l.name == "FENCE")
            .unwrap();
        assert!(
            !fence.visible && fence.locked && !fence.printable && fence.weight == LineWeight(35)
        );
    }

    #[test]
    fn files_that_are_not_ascii_dxf_say_what_they_are() {
        assert!(
            read("AutoCAD Binary DXF\r\n\u{1a}\0", 1.0)
                .unwrap_err()
                .contains("binary")
        );
        assert!(read("AC1027\0\0\0\0", 1.0).unwrap_err().contains("DWG"));
        assert!(
            read("hello\nworld\n", 1.0)
                .unwrap_err()
                .contains("not an ASCII DXF")
        );
        // Older files are in Windows Latin, not UTF-8.
        let latin = b"  0\nSECTION\n  2\nENTITIES\n  0\nTEXT\n 10\n0\n 20\n0\n 40\n2\n  1\nCaf\xe9 \xd810\n  0\nENDSEC\n  0\nEOF\n";
        let back = read(&decode(latin), 1.0).unwrap();
        assert_eq!(back.texts[0].text, "Café Ø10");
        // A file cut off before its end still gives what it has.
        let cut = "  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n 10\n0\n 20\n0\n 11\n5\n 21\n5\n  0\nCIRCLE\n 10\n1";
        assert_eq!(read(cut, 1.0).unwrap().entities.len(), 1);
    }
}

#[cfg(test)]
mod probe {
    /// Read the DXF named by `DRIFT_DXF` and print what came in and what didn't: a way to
    /// try a file from a real project without opening the app.
    /// `DRIFT_DXF=path cargo test -p drift-io probe -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn read_the_file_named_by_drift_dxf() {
        let path = std::env::var("DRIFT_DXF").expect("set DRIFT_DXF to a file");
        let bytes = std::fs::read(&path).expect("readable");
        let import = super::read(&super::decode(&bytes), 1.0).expect("a DXF file");
        let mut doc = drift_core::Document::new();
        doc.begin();
        let report = super::add_to_document(&mut doc, &import);
        doc.commit("Import");
        println!(
            "{path}\n  curves {}  texts {}  fills {}  blocks {}  copies {}  dimensions {}  layers {}  sheets {}",
            report.curves,
            report.texts,
            report.fills,
            report.blocks,
            report.copies,
            report.dimensions,
            report.layers,
            report.sheets
        );
        for (kind, n) in &report.skipped {
            println!("  skipped {n} x {kind}");
        }
        for note in &report.notes {
            println!("  note: {note}");
        }
    }
}
