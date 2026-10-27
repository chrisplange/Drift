//! A logo for a sheet's title block, read from an SVG file into filled and stroked outlines.
//!
//! This reads the SVG that logos are made of: paths and the basic shapes, flat fills and
//! strokes, groups and transforms. Gradients, text, images, clipping and style sheets are not
//! read: a gradient fill is drawn black, and the rest is left out. (Convert text to outlines
//! before saving the SVG, as is usual for logos.)

use drift_core::{Bounds, DVec2, Rgb};
use lyon_tessellation::geometry_builder::simple_builder;
use lyon_tessellation::math::point;
use lyon_tessellation::path::Path;
use lyon_tessellation::{FillOptions, FillRule, FillTessellator, VertexBuffers};

use crate::page::{Drawing, Pen, Prim};

/// The largest SVG read (bytes), and the most points a logo may flatten to.
pub const MAX_BYTES: usize = 1024 * 1024;
const MAX_POINTS: usize = 200_000;

/// A filled area of a logo: its outlines, and the same area as triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct LogoFill {
    pub rings: Vec<Vec<DVec2>>,
    pub triangles: Vec<[DVec2; 3]>,
    pub color: Rgb,
}

/// A stroked line of a logo.
#[derive(Clone, Debug, PartialEq)]
pub struct LogoStroke {
    pub points: Vec<DVec2>,
    pub closed: bool,
    pub color: Rgb,
    /// In the logo's own units.
    pub width: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LogoShape {
    Fill(LogoFill),
    Stroke(LogoStroke),
}

/// A logo, in its own units with y down (as SVG has it), from (0, 0) to `size`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Logo {
    pub size: DVec2,
    pub shapes: Vec<LogoShape>,
}

/// A 2 × 3 affine transform: `(a c e; b d f)`. `then` applies its argument second, so
/// an outer transform goes `inner.then(&outer)`.
use drift_core::Affine2 as Affine;

/// Numbers in an attribute, separated by spaces or commas (signs and exponents included).
fn numbers(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        if bytes[i] == b'-' || bytes[i] == b'+' {
            i += 1;
        }
        let mut digits = false;
        let mut dot = false;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || (bytes[i] == b'.' && !dot)) {
            dot |= bytes[i] == b'.';
            digits |= bytes[i].is_ascii_digit();
            i += 1;
        }
        if digits {
            if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
                let mark = i;
                i += 1;
                if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
                    i += 1;
                }
                if i < bytes.len() && bytes[i].is_ascii_digit() {
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = mark;
                }
            }
            if let Ok(v) = text[start..i].parse::<f64>() {
                out.push(v);
            }
        } else {
            i = start + 1;
        }
    }
    out
}

/// A length attribute as a number (its unit, if any, is dropped).
fn length(text: &str) -> Option<f64> {
    numbers(text).first().copied()
}

fn transform(text: &str) -> Affine {
    let mut out = Affine::IDENTITY;
    let mut rest = text;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let n = numbers(&rest[open + 1..open + close]);
        let get = |k: usize, default: f64| n.get(k).copied().unwrap_or(default);
        let step = match name {
            "matrix" if n.len() >= 6 => Affine([n[0], n[1], n[2], n[3], n[4], n[5]]),
            "translate" => Affine([1.0, 0.0, 0.0, 1.0, get(0, 0.0), get(1, 0.0)]),
            "scale" => {
                let sx = get(0, 1.0);
                Affine([sx, 0.0, 0.0, get(1, sx), 0.0, 0.0])
            }
            "rotate" => {
                let (s, c) = get(0, 0.0).to_radians().sin_cos();
                let turn = Affine([c, s, -s, c, 0.0, 0.0]);
                let (cx, cy) = (get(1, 0.0), get(2, 0.0));
                Affine([1.0, 0.0, 0.0, 1.0, -cx, -cy])
                    .then(&turn)
                    .then(&Affine([1.0, 0.0, 0.0, 1.0, cx, cy]))
            }
            "skewX" => Affine([1.0, 0.0, get(0, 0.0).to_radians().tan(), 1.0, 0.0, 0.0]),
            "skewY" => Affine([1.0, get(0, 0.0).to_radians().tan(), 0.0, 1.0, 0.0, 0.0]),
            _ => Affine::IDENTITY,
        };
        out = step.then(&out);
        rest = &rest[open + close + 1..];
    }
    out
}

/// A paint: `None` for `none`, else a colour (gradients and patterns come out black).
fn paint(text: &str) -> Option<Rgb> {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "none" || lower == "transparent" {
        return None;
    }
    if let Some(hex) = lower.strip_prefix('#') {
        let digit = |k: usize| u8::from_str_radix(&hex[k..k + 1], 16).ok();
        let pair = |k: usize| u8::from_str_radix(&hex[k..k + 2], 16).ok();
        if !hex.is_ascii() {
            return Some(Rgb([0, 0, 0]));
        }
        return Some(match hex.len() {
            3 | 4 => Rgb([
                digit(0).unwrap_or(0) * 17,
                digit(1).unwrap_or(0) * 17,
                digit(2).unwrap_or(0) * 17,
            ]),
            6 | 8 => Rgb([
                pair(0).unwrap_or(0),
                pair(2).unwrap_or(0),
                pair(4).unwrap_or(0),
            ]),
            _ => Rgb([0, 0, 0]),
        });
    }
    if lower.starts_with("rgb") {
        let n = numbers(&lower);
        let percent = lower.contains('%');
        let channel = |k: usize| {
            let v = n.get(k).copied().unwrap_or(0.0);
            (if percent { v * 2.55 } else { v })
                .clamp(0.0, 255.0)
                .round() as u8
        };
        return Some(Rgb([channel(0), channel(1), channel(2)]));
    }
    Some(Rgb(match lower.as_str() {
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "navy" => [0, 0, 128],
        "teal" => [0, 128, 128],
        "maroon" => [128, 0, 0],
        "silver" => [192, 192, 192],
        "gray" | "grey" => [128, 128, 128],
        // Black, currentColor, gradients and anything unknown.
        _ => [0, 0, 0],
    }))
}

/// How an element is painted, inherited down the tree.
#[derive(Clone, Copy, Debug)]
struct Style {
    fill: Option<Rgb>,
    stroke: Option<Rgb>,
    stroke_width: f64,
    even_odd: bool,
}

impl Style {
    fn of(node: roxmltree::Node, parent: Style) -> Style {
        let mut s = parent;
        let mut set = |name: &str, value: &str| match name {
            "fill" => s.fill = paint(value),
            "stroke" => s.stroke = paint(value),
            "stroke-width" => s.stroke_width = length(value).unwrap_or(s.stroke_width),
            "fill-rule" => s.even_odd = value.trim() == "evenodd",
            _ => {}
        };
        for name in ["fill", "stroke", "stroke-width", "fill-rule"] {
            if let Some(v) = node.attribute(name) {
                set(name, v);
            }
        }
        // An inline style wins over the attributes.
        if let Some(style) = node.attribute("style") {
            for part in style.split(';') {
                if let Some((name, value)) = part.split_once(':') {
                    set(name.trim(), value.trim());
                }
            }
        }
        s
    }
}

/// One outline of a shape, in the element's own coordinates.
#[derive(Clone, Debug, Default)]
struct Sub {
    points: Vec<DVec2>,
    closed: bool,
}

fn cubic(out: &mut Vec<DVec2>, p0: DVec2, p1: DVec2, p2: DVec2, p3: DVec2) {
    const STEPS: usize = 16;
    for k in 1..=STEPS {
        let t = k as f64 / STEPS as f64;
        let u = 1.0 - t;
        out.push(
            p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t),
        );
    }
}

/// An elliptical arc from `from` to `to`, as SVG's path data gives it.
fn arc(
    out: &mut Vec<DVec2>,
    from: DVec2,
    r: DVec2,
    turn: f64,
    large: bool,
    sweep: bool,
    to: DVec2,
) {
    let (mut rx, mut ry) = (r.x.abs(), r.y.abs());
    if rx == 0.0 || ry == 0.0 || from == to {
        out.push(to);
        return;
    }
    let (sin, cos) = turn.to_radians().sin_cos();
    let half = (from - to) * 0.5;
    let p = DVec2::new(cos * half.x + sin * half.y, -sin * half.x + cos * half.y);
    // Radii too small to reach are grown until they do.
    let reach = (p.x * p.x) / (rx * rx) + (p.y * p.y) / (ry * ry);
    if reach > 1.0 {
        rx *= reach.sqrt();
        ry *= reach.sqrt();
    }
    let num = (rx * rx * ry * ry - rx * rx * p.y * p.y - ry * ry * p.x * p.x).max(0.0);
    let den = rx * rx * p.y * p.y + ry * ry * p.x * p.x;
    let k =
        if den == 0.0 { 0.0 } else { (num / den).sqrt() } * if large == sweep { -1.0 } else { 1.0 };
    let c = DVec2::new(k * rx * p.y / ry, -k * ry * p.x / rx);
    let mid = (from + to) * 0.5;
    let centre = DVec2::new(cos * c.x - sin * c.y + mid.x, sin * c.x + cos * c.y + mid.y);
    let angle = |v: DVec2| v.y.atan2(v.x);
    let a0 = angle(DVec2::new((p.x - c.x) / rx, (p.y - c.y) / ry));
    let a1 = angle(DVec2::new((-p.x - c.x) / rx, (-p.y - c.y) / ry));
    let mut delta = a1 - a0;
    if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    } else if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    }
    let steps = ((delta.abs() / std::f64::consts::TAU) * 48.0)
        .ceil()
        .max(2.0) as usize;
    for i in 1..=steps {
        let a = a0 + delta * i as f64 / steps as f64;
        let e = DVec2::new(rx * a.cos(), ry * a.sin());
        out.push(DVec2::new(cos * e.x - sin * e.y, sin * e.x + cos * e.y) + centre);
    }
    // Land exactly on the end.
    if let Some(last) = out.last_mut() {
        *last = to;
    }
}

/// The outlines of an SVG path's `d`.
fn path_data(d: &str) -> Vec<Sub> {
    let mut subs: Vec<Sub> = Vec::new();
    let mut at = DVec2::ZERO;
    let mut start = DVec2::ZERO;
    // The last control point, for the smooth commands.
    let mut handle: Option<(char, DVec2)> = None;
    let mut rest = d;
    let mut command = ' ';
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        let Some(first) = rest.chars().next() else {
            break;
        };
        if first.is_ascii_alphabetic() {
            command = first;
            rest = &rest[1..];
        } else if command == ' ' {
            break;
        }
        // The arguments up to the next command letter.
        let end = rest
            .char_indices()
            .find(|(_, c)| c.is_ascii_alphabetic() && *c != 'e' && *c != 'E')
            .map_or(rest.len(), |(i, _)| i);
        let args = numbers(&rest[..end]);
        rest = &rest[end..];
        let relative = command.is_ascii_lowercase();
        let upper = command.to_ascii_uppercase();
        let per = match upper {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'A' => 7,
            _ => 0,
        };
        if upper == 'Z' {
            if let Some(sub) = subs.last_mut() {
                sub.closed = true;
            }
            at = start;
            handle = None;
            continue;
        }
        if per == 0 {
            continue;
        }
        for (i, a) in args.chunks_exact(per).enumerate() {
            let base = if relative { at } else { DVec2::ZERO };
            let pt = |k: usize| base + DVec2::new(a[k], a[k + 1]);
            let mut next_handle = None;
            let line = |subs: &mut Vec<Sub>, p: DVec2| {
                if let Some(sub) = subs.last_mut() {
                    sub.points.push(p);
                }
            };
            match upper {
                // After the first pair, a move's further pairs are lines.
                'M' if i == 0 => {
                    at = pt(0);
                    start = at;
                    subs.push(Sub {
                        points: vec![at],
                        closed: false,
                    });
                }
                'M' | 'L' => {
                    at = pt(0);
                    line(&mut subs, at);
                }
                'H' => {
                    at.x = if relative { at.x + a[0] } else { a[0] };
                    line(&mut subs, at);
                }
                'V' => {
                    at.y = if relative { at.y + a[0] } else { a[0] };
                    line(&mut subs, at);
                }
                'C' | 'S' => {
                    let (c1, c2, end) = if upper == 'C' {
                        (pt(0), pt(2), pt(4))
                    } else {
                        let c1 = match handle {
                            Some(('C', h)) => at * 2.0 - h,
                            _ => at,
                        };
                        (c1, pt(0), pt(2))
                    };
                    if let Some(sub) = subs.last_mut() {
                        cubic(&mut sub.points, at, c1, c2, end);
                    }
                    next_handle = Some(('C', c2));
                    at = end;
                }
                'Q' | 'T' => {
                    let (c, end) = if upper == 'Q' {
                        (pt(0), pt(2))
                    } else {
                        let c = match handle {
                            Some(('Q', h)) => at * 2.0 - h,
                            _ => at,
                        };
                        (c, pt(0))
                    };
                    // A quadratic as the cubic it is.
                    let (c1, c2) = (at + (c - at) * (2.0 / 3.0), end + (c - end) * (2.0 / 3.0));
                    if let Some(sub) = subs.last_mut() {
                        cubic(&mut sub.points, at, c1, c2, end);
                    }
                    next_handle = Some(('Q', c));
                    at = end;
                }
                'A' => {
                    let end = base + DVec2::new(a[5], a[6]);
                    if let Some(sub) = subs.last_mut() {
                        arc(
                            &mut sub.points,
                            at,
                            DVec2::new(a[0], a[1]),
                            a[2],
                            a[3] != 0.0,
                            a[4] != 0.0,
                            end,
                        );
                    }
                    at = end;
                }
                _ => {}
            }
            handle = next_handle;
        }
    }
    subs
}

fn ellipse(c: DVec2, r: DVec2) -> Sub {
    Sub {
        points: (0..64)
            .map(|k| {
                let a = k as f64 / 64.0 * std::f64::consts::TAU;
                c + DVec2::new(r.x * a.cos(), r.y * a.sin())
            })
            .collect(),
        closed: true,
    }
}

/// The outlines of a shape element (none for anything else).
fn outlines(node: roxmltree::Node) -> Vec<Sub> {
    let num = |name: &str| node.attribute(name).and_then(length).unwrap_or(0.0);
    let poly = |closed: bool| {
        let n = numbers(node.attribute("points").unwrap_or(""));
        vec![Sub {
            points: n
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| DVec2::new(p[0], p[1]))
                .collect(),
            closed,
        }]
    };
    match node.tag_name().name() {
        "path" => path_data(node.attribute("d").unwrap_or("")),
        "rect" => {
            let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
            vec![Sub {
                points: vec![
                    DVec2::new(x, y),
                    DVec2::new(x + w, y),
                    DVec2::new(x + w, y + h),
                    DVec2::new(x, y + h),
                ],
                closed: true,
            }]
        }
        "circle" => vec![ellipse(
            DVec2::new(num("cx"), num("cy")),
            DVec2::splat(num("r")),
        )],
        "ellipse" => vec![ellipse(
            DVec2::new(num("cx"), num("cy")),
            DVec2::new(num("rx"), num("ry")),
        )],
        "polygon" => poly(true),
        "polyline" => poly(false),
        "line" => vec![Sub {
            points: vec![
                DVec2::new(num("x1"), num("y1")),
                DVec2::new(num("x2"), num("y2")),
            ],
            closed: false,
        }],
        _ => Vec::new(),
    }
}

/// The triangles that fill `rings`.
/// Triangles filling closed rings (by the even-odd or non-zero rule).
pub fn triangulate(rings: &[Vec<DVec2>], even_odd: bool) -> Vec<[DVec2; 3]> {
    let mut builder = Path::builder();
    for ring in rings.iter().filter(|r| r.len() >= 3) {
        builder.begin(point(ring[0].x as f32, ring[0].y as f32));
        for p in &ring[1..] {
            builder.line_to(point(p.x as f32, p.y as f32));
        }
        builder.end(true);
    }
    let path = builder.build();
    let mut buffers: VertexBuffers<lyon_tessellation::math::Point, u16> = VertexBuffers::new();
    let rule = if even_odd {
        FillRule::EvenOdd
    } else {
        FillRule::NonZero
    };
    let options = FillOptions::default().with_fill_rule(rule);
    let mut tessellator = FillTessellator::new();
    if tessellator
        .tessellate_path(&path, &options, &mut simple_builder(&mut buffers))
        .is_err()
    {
        return Vec::new();
    }
    let at = |i: u16| {
        let v = buffers.vertices[usize::from(i)];
        DVec2::new(f64::from(v.x), f64::from(v.y))
    };
    buffers
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| [at(t[0]), at(t[1]), at(t[2])])
        .collect()
}

fn walk(
    node: roxmltree::Node,
    parent: Affine,
    inherited: Style,
    out: &mut Vec<LogoShape>,
    points: &mut usize,
) {
    if !node.is_element() || node.attribute("display") == Some("none") {
        return;
    }
    let name = node.tag_name().name();
    // Definitions and what this doesn't draw.
    if matches!(
        name,
        "defs"
            | "clipPath"
            | "mask"
            | "symbol"
            | "pattern"
            | "linearGradient"
            | "radialGradient"
            | "style"
            | "metadata"
            | "title"
            | "desc"
            | "text"
            | "image"
            | "filter"
            | "marker"
    ) {
        return;
    }
    let style = Style::of(node, inherited);
    let matrix = match node.attribute("transform") {
        Some(t) => transform(t).then(&parent),
        None => parent,
    };
    let subs = outlines(node);
    if !subs.is_empty() {
        let placed: Vec<Sub> = subs
            .into_iter()
            .map(|s| Sub {
                points: s.points.iter().map(|p| matrix.apply(*p)).collect(),
                closed: s.closed,
            })
            .filter(|s| s.points.iter().all(|p| p.is_finite()))
            .collect();
        *points += placed.iter().map(|s| s.points.len()).sum::<usize>();
        if let Some(color) = style.fill {
            let rings: Vec<Vec<DVec2>> = placed
                .iter()
                .filter(|s| s.points.len() >= 3)
                .map(|s| s.points.clone())
                .collect();
            let triangles = triangulate(&rings, style.even_odd);
            if !triangles.is_empty() {
                out.push(LogoShape::Fill(LogoFill {
                    rings,
                    triangles,
                    color,
                }));
            }
        }
        if let Some(color) = style.stroke
            && style.stroke_width > 0.0
        {
            for s in placed.iter().filter(|s| s.points.len() >= 2) {
                out.push(LogoShape::Stroke(LogoStroke {
                    points: s.points.clone(),
                    closed: s.closed,
                    color,
                    width: style.stroke_width * matrix.scale(),
                }));
            }
        }
    }
    for child in node.children() {
        walk(child, matrix, style, out, points);
    }
}

/// Read a logo from SVG text.
pub fn parse(svg: &str) -> Result<Logo, String> {
    if svg.len() > MAX_BYTES {
        return Err(format!(
            "the SVG is larger than {} KB; simplify it first",
            MAX_BYTES / 1024
        ));
    }
    let doc = roxmltree::Document::parse(svg).map_err(|e| format!("not a readable SVG: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err("not an SVG file".into());
    }
    let style = Style {
        fill: Some(Rgb([0, 0, 0])),
        stroke: None,
        stroke_width: 1.0,
        even_odd: false,
    };
    let mut shapes = Vec::new();
    let mut points = 0;
    walk(root, Affine::IDENTITY, style, &mut shapes, &mut points);
    if points > MAX_POINTS {
        return Err("the SVG is too detailed for a title block logo; simplify it first".into());
    }
    if shapes.is_empty() {
        return Err(
            "nothing to draw in this SVG (text must be converted to outlines, and images and \
             gradients aren't read)"
                .into(),
        );
    }
    // The page it was drawn on: its view box, else its size, else what was drawn.
    let view = root
        .attribute("viewBox")
        .map(numbers)
        .filter(|v| v.len() == 4);
    let (origin, size) = match view {
        Some(v) if v[2] > 0.0 && v[3] > 0.0 => (DVec2::new(v[0], v[1]), DVec2::new(v[2], v[3])),
        _ => {
            let w = root
                .attribute("width")
                .and_then(length)
                .filter(|w| *w > 0.0);
            let h = root
                .attribute("height")
                .and_then(length)
                .filter(|h| *h > 0.0);
            match (w, h) {
                (Some(w), Some(h)) => (DVec2::ZERO, DVec2::new(w, h)),
                _ => {
                    let mut b = Bounds::EMPTY;
                    for shape in &shapes {
                        match shape {
                            LogoShape::Fill(f) => {
                                f.rings.iter().flatten().for_each(|p| b.include(*p));
                            }
                            LogoShape::Stroke(s) => s.points.iter().for_each(|p| b.include(*p)),
                        }
                    }
                    (b.min, b.size())
                }
            }
        }
    };
    if !(size.x > 0.0 && size.y > 0.0) {
        return Err("the SVG has no size".into());
    }
    // From (0, 0).
    for shape in &mut shapes {
        match shape {
            LogoShape::Fill(f) => {
                for p in f.rings.iter_mut().flatten() {
                    *p -= origin;
                }
                for p in f.triangles.iter_mut().flatten() {
                    *p -= origin;
                }
            }
            LogoShape::Stroke(s) => {
                for p in &mut s.points {
                    *p -= origin;
                }
            }
        }
    }
    Ok(Logo { size, shapes })
}

impl Logo {
    /// Draw the logo as large as fits in `area` (paper mm, y up), at its left and middle
    /// height. Returns the width it takes.
    pub fn draw(&self, d: &mut Drawing, layer: &str, area: Bounds) -> f64 {
        let room = area.size();
        if !(room.x > 0.0 && room.y > 0.0 && self.size.x > 0.0 && self.size.y > 0.0) {
            return 0.0;
        }
        let k = (room.x / self.size.x).min(room.y / self.size.y);
        let top = area.center().y + self.size.y * k * 0.5;
        let place = |p: &DVec2| DVec2::new(area.min.x + p.x * k, top - p.y * k);
        for shape in &self.shapes {
            match shape {
                LogoShape::Fill(f) => d.push(
                    layer,
                    Pen::new(0.0).with_color(f.color),
                    Prim::Fill {
                        rings: f
                            .rings
                            .iter()
                            .map(|r| r.iter().map(place).collect())
                            .collect(),
                        triangles: f
                            .triangles
                            .iter()
                            .map(|t| [place(&t[0]), place(&t[1]), place(&t[2])])
                            .collect(),
                        mask: false,
                    },
                ),
                LogoShape::Stroke(s) => d.push(
                    layer,
                    Pen::new((s.width * k).max(0.05)).with_color(s.color),
                    Prim::Polyline {
                        points: s.points.iter().map(place).collect(),
                        closed: s.closed,
                    },
                ),
            }
        }
        self.size.x * k
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fills(logo: &Logo) -> Vec<&LogoFill> {
        logo.shapes
            .iter()
            .filter_map(|s| match s {
                LogoShape::Fill(f) => Some(f),
                LogoShape::Stroke(_) => None,
            })
            .collect()
    }

    fn area(triangles: &[[DVec2; 3]]) -> f64 {
        triangles
            .iter()
            .map(|t| ((t[1] - t[0]).perp_dot(t[2] - t[0]) * 0.5).abs())
            .sum()
    }

    #[test]
    fn reads_shapes_paths_colours_and_transforms() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="10 20 100 50">
  <defs><rect width="999" height="999"/></defs>
  <rect x="10" y="20" width="40" height="50" fill="#f60"/>
  <g transform="translate(60 20) scale(2)" style="fill: rgb(0, 51, 102)">
    <path d="M0 0 h20 v25 h-20 z M5 5 v15 h10 v-15 z" fill-rule="evenodd"/>
  </g>
  <circle cx="90" cy="60" r="5" fill="none" stroke="navy" stroke-width="2"/>
  <text x="0" y="0">Name</text>
</svg>"##;
        let logo = parse(svg).unwrap();
        assert_eq!(logo.size, DVec2::new(100.0, 50.0));
        let f = fills(&logo);
        assert_eq!(f.len(), 2, "the definition and the text are left out");
        // The rectangle, moved to the view box's origin.
        assert_eq!(f[0].color, Rgb([0xff, 0x66, 0x00]));
        assert_eq!(f[0].rings[0][0], DVec2::ZERO);
        assert!((area(&f[0].triangles) - 2000.0).abs() < 1e-6);
        // The frame: twice the size, with its hole (even-odd).
        assert_eq!(f[1].color, Rgb([0, 51, 102]));
        assert_eq!(f[1].rings.len(), 2);
        assert!((area(&f[1].triangles) - (40.0 * 50.0 - 20.0 * 30.0)).abs() < 1e-3);
        assert_eq!(f[1].rings[0][1], DVec2::new(90.0, 0.0));
        // The ring is a stroke, not a fill.
        let strokes: Vec<_> = logo
            .shapes
            .iter()
            .filter_map(|s| match s {
                LogoShape::Stroke(s) => Some(s),
                LogoShape::Fill(_) => None,
            })
            .collect();
        assert_eq!(strokes.len(), 1);
        assert!(strokes[0].closed && strokes[0].width == 2.0);
        assert_eq!(strokes[0].color, Rgb([0, 0, 128]));
    }

    #[test]
    fn curves_and_arcs_are_followed() {
        // A circle of radius 10 as two arcs, and as four cubic quarters: the same area.
        let k = 5.522_847_5;
        let arcs = r#"<svg viewBox="0 0 20 20"><path d="M0 10 A10 10 0 0 1 20 10 A10 10 0 0 1 0 10Z"/></svg>"#;
        let cubics = format!(
            r#"<svg viewBox="0 0 20 20"><path d="M10 0 C{a} 0 20 {b} 20 10 S{a} 20 10 20 S0 {a} 0 10 S{b} 0 10 0z"/></svg>"#,
            a = 10.0 + k,
            b = 10.0 - k
        );
        let disc = std::f64::consts::PI * 100.0;
        for svg in [arcs.to_owned(), cubics] {
            let logo = parse(&svg).unwrap();
            let got = area(&fills(&logo)[0].triangles);
            assert!((got - disc).abs() < disc * 0.01, "{got}");
        }
        // Relative commands and numbers run together, as minifiers write them.
        let logo = parse(r#"<svg viewBox="0 0 4 4"><path d="m0 0l4 0 0 4-4 0zM1 1h2v2h-2z" fill-rule="evenodd"/></svg>"#).unwrap();
        assert!((area(&fills(&logo)[0].triangles) - 12.0).abs() < 1e-6);
        let logo = parse(r#"<svg viewBox="0 0 2 2"><path d="M.5.5 1.5.5 1.5 1.5.5 1.5z"/></svg>"#)
            .unwrap();
        assert!((area(&fills(&logo)[0].triangles) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn what_cannot_be_read_is_explained() {
        assert!(parse("not xml").unwrap_err().contains("readable"));
        assert!(parse("<html/>").unwrap_err().contains("not an SVG"));
        let only_text = r#"<svg viewBox="0 0 10 10"><text>Name</text></svg>"#;
        assert!(parse(only_text).unwrap_err().contains("outlines"));
    }

    #[test]
    fn draws_to_fit_at_the_left() {
        let logo =
            parse(r#"<svg viewBox="0 0 40 20"><rect width="40" height="20"/></svg>"#).unwrap();
        let mut d = Drawing::new(DVec2::new(100.0, 100.0));
        let room = Bounds {
            min: DVec2::new(10.0, 10.0),
            max: DVec2::new(60.0, 20.0),
        };
        // Height decides: 10 mm tall, so 20 mm wide.
        assert_eq!(logo.draw(&mut d, "SHEET", room), 20.0);
        let Prim::Fill { rings, .. } = &d.items[0].prim else {
            panic!("a fill");
        };
        // The SVG's top left is the area's top left (y flipped).
        assert_eq!(rings[0][0], DVec2::new(10.0, 20.0));
        assert_eq!(rings[0][2], DVec2::new(30.0, 10.0));
    }
}
