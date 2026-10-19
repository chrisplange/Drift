//! Reading SVG artwork (from Illustrator, Inkscape, Affinity, Figma or Drift itself) into
//! [`Artwork`]: paths and the basic shapes with their strokes and fills, text, `<use>`
//! copies, and layers — Illustrator's and Inkscape's, and Drift's own.
//!
//! Sizes come from the root element's `width`, `height` and `viewBox`. Illustrator writes
//! its points as SVG units (72 to the inch, where SVG has 96), so a file it made is read at
//! that size. Styles come from presentation attributes, simple `<style>` rules (by element,
//! class and id) and `style` attributes, inherited through groups. What can't be carried
//! over (images, clipping, gradients, patterns) is counted or noted.

use std::collections::HashMap;

use drift_core::{DVec2, Rgb, TextAlign};
use kurbo::{Affine, BezPath, PathEl, Point};
use roxmltree::{Document, Node, ParsingOptions};

use std::rc::Rc;

use crate::clip::{Clip, clip_paint};
use crate::vector::{Artwork, Painted, Seg, Stroke, Subpath, Text};

/// `<use>` copies and nested documents go at most this deep.
const MAX_DEPTH: usize = 24;

const INKSCAPE: &str = "http://www.inkscape.org/namespaces/inkscape";
const XLINK: &str = "http://www.w3.org/1999/xlink";

/// Read an SVG document. Curves outside any layer go on a layer named `name`.
pub fn read(text: &str, name: &str) -> Result<Artwork, String> {
    let doc = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("not a readable SVG ({e})"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err("not an SVG drawing".into());
    }
    let illustrator =
        text.contains("Adobe Illustrator") || text.contains("ns.adobe.com/AdobeIllustrator");
    let px_mm = 25.4 / if illustrator { 72.0 } else { 96.0 };
    let view = root.attribute("viewBox").and_then(|v| {
        let n: Vec<f64> = numbers(v);
        (n.len() == 4 && n[2] > 0.0 && n[3] > 0.0).then(|| [n[0], n[1], n[2], n[3]])
    });
    let width = root.attribute("width").and_then(|w| paper_mm(w, px_mm));
    let height = root.attribute("height").and_then(|h| paper_mm(h, px_mm));
    let (sx, sy, size) = match (view, width, height) {
        (Some(vb), Some(w), Some(h)) => (w / vb[2], h / vb[3], DVec2::new(w, h)),
        (Some(vb), Some(w), None) => (w / vb[2], w / vb[2], DVec2::new(w, vb[3] * w / vb[2])),
        (Some(vb), None, Some(h)) => (h / vb[3], h / vb[3], DVec2::new(vb[2] * h / vb[3], h)),
        (Some(vb), None, None) => (px_mm, px_mm, DVec2::new(vb[2], vb[3]) * px_mm),
        (None, w, h) => (px_mm, px_mm, DVec2::new(w.unwrap_or(0.0), h.unwrap_or(0.0))),
    };
    let [vx, vy] = view.map_or([0.0, 0.0], |v| [v[0], v[1]]);
    // SVG units to paper millimetres, y up from the bottom of the paper.
    let base = Affine::new([sx, 0.0, 0.0, -sy, -vx * sx, size.y + vy * sy]);

    let mut reader = Reader {
        art: Artwork {
            size,
            ..Default::default()
        },
        rules: Vec::new(),
        ids: HashMap::new(),
        loose: name.to_owned(),
    };
    for node in doc.descendants() {
        if let Some(id) = node.attribute("id") {
            reader.ids.insert(id.to_owned(), node);
        }
        if node.has_tag_name("style") {
            let css: String = node
                .children()
                .filter_map(|c| c.text())
                .collect::<Vec<_>>()
                .join("");
            reader.rules.extend(parse_css(&css));
        }
    }
    if illustrator {
        reader
            .art
            .note("Sized as Illustrator draws: 72 SVG units to the inch (browsers show it at 96)");
    }
    let style = reader.style_of(root, &Style::default());
    // Nothing shows beyond the paper (an artboard, for Illustrator).
    let paper: Vec<Rc<Clip>> = if size.x > 0.0 && size.y > 0.0 {
        vec![Rc::new(Clip::rect(DVec2::ZERO, size))]
    } else {
        Vec::new()
    };
    for child in root.children().filter(Node::is_element) {
        // The root's groups are the layers, as Illustrator writes them.
        let layer = if child.has_tag_name("g") {
            layer_name(child)
        } else {
            None
        };
        reader.node(child, base, &style, layer.as_deref(), &paper, 0);
    }
    reader.art.note_clipping();
    if reader.art.paths.is_empty() && reader.art.texts.is_empty() {
        reader
            .art
            .note("The drawing has nothing Drift can bring in");
    }
    Ok(reader.art)
}

/// A paper length (`210mm`, `8.5in`, `595.3`): millimetres; `None` for a percentage.
fn paper_mm(s: &str, px_mm: f64) -> Option<f64> {
    let (v, unit) = length(s)?;
    let mm = match unit {
        "mm" => v,
        "cm" => v * 10.0,
        "in" => v * 25.4,
        "pt" => v * 25.4 / 72.0,
        "pc" => v * 25.4 / 6.0,
        "" | "px" => v * px_mm,
        _ => return None,
    };
    (mm > 0.0).then_some(mm)
}

/// A number and its unit.
fn length(s: &str) -> Option<(f64, &str)> {
    let s = s.trim();
    let split = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(s.len());
    let v: f64 = s[..split].trim().parse().ok()?;
    v.is_finite().then_some((v, s[split..].trim()))
}

/// A length in SVG units (CSS pixels), given the font size for `em`.
fn user_length(s: &str, font_size: f64) -> Option<f64> {
    let (v, unit) = length(s)?;
    Some(match unit {
        "" | "px" => v,
        "mm" => v * 96.0 / 25.4,
        "cm" => v * 96.0 / 2.54,
        "in" => v * 96.0,
        "pt" => v * 96.0 / 72.0,
        "pc" => v * 16.0,
        "em" => v * font_size,
        "ex" => v * font_size / 2.0,
        _ => return None,
    })
}

/// The numbers in a list (separated by commas or spaces, or run together as `1-2`).
fn numbers(s: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' {
            let start = i;
            i += 1;
            let mut dot = c == '.';
            while i < b.len() {
                let c = b[i] as char;
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == '.' && !dot {
                    dot = true;
                    i += 1;
                } else if (c == 'e' || c == 'E')
                    && i + 1 < b.len()
                    && (b[i + 1].is_ascii_digit() || b[i + 1] == b'-' || b[i + 1] == b'+')
                {
                    i += 2;
                    dot = true;
                } else {
                    break;
                }
            }
            if let Ok(v) = s[start..i].parse::<f64>() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// A `transform` attribute.
fn transform(s: &str) -> Affine {
    let mut m = Affine::IDENTITY;
    for part in s.split(')') {
        let Some((name, args)) = part.split_once('(') else {
            continue;
        };
        let a = numbers(args);
        let arg = |i: usize| a.get(i).copied().unwrap_or(0.0);
        let t = match name.trim().trim_start_matches(',').trim() {
            "matrix" if a.len() == 6 => Affine::new([a[0], a[1], a[2], a[3], a[4], a[5]]),
            "translate" => Affine::translate((arg(0), arg(1))),
            "scale" => Affine::scale_non_uniform(arg(0), a.get(1).copied().unwrap_or(arg(0))),
            "rotate" => {
                let r = Affine::rotate(arg(0).to_radians());
                if a.len() >= 3 {
                    Affine::translate((arg(1), arg(2))) * r * Affine::translate((-arg(1), -arg(2)))
                } else {
                    r
                }
            }
            "skewX" => Affine::skew(arg(0).to_radians().tan(), 0.0),
            "skewY" => Affine::skew(0.0, arg(0).to_radians().tan()),
            _ => continue,
        };
        m *= t;
    }
    m
}

/// A colour: `#rgb`, `#rrggbb`, `rgb(…)` or a name.
fn color(s: &str) -> Option<Rgb> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        return match hex.len() {
            3 | 4 => Some(Rgb([digit(0)? * 17, digit(1)? * 17, digit(2)? * 17])),
            6 | 8 => Rgb::parse(&format!("#{}", &hex[..6])),
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    if let Some(args) = lower
        .strip_prefix("rgb(")
        .or_else(|| lower.strip_prefix("rgba("))
    {
        let parts: Vec<&str> = args
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|p| !p.trim().is_empty())
            .collect();
        let channel = |p: &str| -> Option<u8> {
            let p = p.trim();
            let v = match p.strip_suffix('%') {
                Some(pc) => pc.parse::<f64>().ok()? * 2.55,
                None => p.parse::<f64>().ok()?,
            };
            Some(v.round().clamp(0.0, 255.0) as u8)
        };
        return Some(Rgb([
            channel(parts.first()?)?,
            channel(parts.get(1)?)?,
            channel(parts.get(2)?)?,
        ]));
    }
    let named = match lower.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "lime" => [0, 255, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "silver" => [192, 192, 192],
        "maroon" => [128, 0, 0],
        "olive" => [128, 128, 0],
        "teal" => [0, 128, 128],
        "navy" => [0, 0, 128],
        "purple" => [128, 0, 128],
        "orange" => [255, 165, 0],
        "brown" => [165, 42, 42],
        "pink" => [255, 192, 203],
        _ => return None,
    };
    Some(Rgb(named))
}

/// How a path is painted.
#[derive(Clone, Debug, PartialEq)]
enum Paint {
    None,
    Color(Rgb),
    Current,
    /// A gradient or pattern, by id.
    Url(String),
}

fn paint(s: &str) -> Option<Paint> {
    let s = s.trim();
    Some(match s {
        "none" | "transparent" => Paint::None,
        "currentColor" | "currentcolor" => Paint::Current,
        _ if s.starts_with("url(") => {
            let id = s
                .trim_start_matches("url(")
                .split(')')
                .next()?
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim_start_matches('#');
            Paint::Url(id.to_owned())
        }
        _ => Paint::Color(color(s)?),
    })
}

/// The inherited style of an element.
#[derive(Clone, Debug)]
struct Style {
    fill: Paint,
    stroke: Paint,
    stroke_width: f64,
    dashes: Vec<f64>,
    color: Rgb,
    fill_opacity: f64,
    stroke_opacity: f64,
    font_size: f64,
    anchor: TextAlign,
    visible: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Paint::Color(Rgb([0, 0, 0])),
            stroke: Paint::None,
            stroke_width: 1.0,
            dashes: Vec::new(),
            color: Rgb([0, 0, 0]),
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
            font_size: 16.0,
            anchor: TextAlign::Left,
            visible: true,
        }
    }
}

/// A `<style>` rule's selector: by element, class or id.
#[derive(Clone, Debug, PartialEq)]
enum Selector {
    Any,
    Tag(String),
    Class(String),
    Id(String),
    TagClass(String, String),
}

impl Selector {
    fn specificity(&self) -> u8 {
        match self {
            Selector::Any => 0,
            Selector::Tag(_) => 1,
            Selector::Class(_) => 2,
            Selector::TagClass(..) => 3,
            Selector::Id(_) => 4,
        }
    }

    fn matches(&self, node: Node) -> bool {
        let has_class = |c: &str| {
            node.attribute("class")
                .is_some_and(|cs| cs.split_whitespace().any(|x| x == c))
        };
        match self {
            Selector::Any => true,
            Selector::Tag(t) => node.tag_name().name() == t,
            Selector::Class(c) => has_class(c),
            Selector::Id(i) => node.attribute("id") == Some(i.as_str()),
            Selector::TagClass(t, c) => node.tag_name().name() == t && has_class(c),
        }
    }
}

type Declarations = Vec<(String, String)>;

fn declarations(s: &str) -> Declarations {
    s.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let v = v.trim().trim_end_matches("!important").trim();
            Some((k.trim().to_ascii_lowercase(), v.to_owned()))
        })
        .collect()
}

/// The simple rules of a style sheet; at-rules and combinators are passed over.
fn parse_css(css: &str) -> Vec<(Selector, Declarations)> {
    // Without comments.
    let mut text = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        text.push_str(&rest[..i]);
        rest = rest[i + 2..].split_once("*/").map_or("", |(_, r)| r);
    }
    text.push_str(rest);
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut head = String::new();
    let mut body = String::new();
    let mut skipping = false;
    for c in text.chars() {
        match c {
            '{' => {
                depth += 1;
                if depth == 1 {
                    skipping = head.trim_start().starts_with('@');
                    body.clear();
                } else {
                    body.push(c);
                }
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    if !skipping {
                        let decls = declarations(&body);
                        for sel in head.split(',') {
                            let sel = sel.trim();
                            if sel.contains([' ', '>', '+', '~', ':', '[']) {
                                continue;
                            }
                            let selector = if sel == "*" {
                                Selector::Any
                            } else if let Some(c) = sel.strip_prefix('.') {
                                Selector::Class(c.to_owned())
                            } else if let Some(i) = sel.strip_prefix('#') {
                                Selector::Id(i.to_owned())
                            } else if let Some((t, c)) = sel.split_once('.') {
                                Selector::TagClass(t.to_owned(), c.to_owned())
                            } else if !sel.is_empty() {
                                Selector::Tag(sel.to_owned())
                            } else {
                                continue;
                            };
                            out.push((selector, decls.clone()));
                        }
                    }
                    head.clear();
                } else {
                    body.push(c);
                }
            }
            _ if depth == 0 => head.push(c),
            _ => body.push(c),
        }
    }
    // Lower specificity first, so more specific rules win when applied in order.
    out.sort_by_key(|(s, _)| s.specificity());
    out
}

/// A group's layer name: Inkscape's label, Illustrator's `data-name` or id, or Drift's.
fn layer_name(node: Node) -> Option<String> {
    if let Some(label) = node.attribute((INKSCAPE, "label")) {
        return Some(label.to_owned());
    }
    if let Some(name) = node
        .attribute("data-name")
        .or_else(|| node.attribute("data-layer"))
    {
        return Some(name.to_owned());
    }
    node.attribute("id").map(illustrator_id)
}

/// An Illustrator id as the name it stands for: `_x5F_` style escapes decoded, other
/// underscores spaces, and the `_1_` it adds to tell copies apart dropped.
fn illustrator_id(id: &str) -> String {
    let mut out = String::new();
    let mut rest = id;
    while let Some(i) = rest.find("_x") {
        out.push_str(&rest[..i].replace('_', " "));
        let after = &rest[i + 2..];
        match after
            .find('_')
            .filter(|n| (2..=4).contains(n))
            .and_then(|n| Some((u32::from_str_radix(&after[..n], 16).ok()?, n)))
            .and_then(|(code, n)| Some((char::from_u32(code)?, n)))
        {
            Some((c, n)) => {
                out.push(c);
                rest = &after[n + 1..];
            }
            None => {
                out.push(' ');
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(&rest.replace('_', " "));
    let trimmed = out.trim();
    // "Walls 1 " from "Walls_1_": a copy's number.
    let name = if id.ends_with('_') {
        trimmed
            .rsplit_once(' ')
            .filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit()))
            .map_or(trimmed, |(n, _)| n.trim())
    } else {
        trimmed
    };
    if name.is_empty() {
        id.to_owned()
    } else {
        name.to_owned()
    }
}

struct Reader<'a, 'input> {
    art: Artwork,
    rules: Vec<(Selector, Declarations)>,
    ids: HashMap<String, Node<'a, 'input>>,
    /// The layer for what isn't in one.
    loose: String,
}

impl<'a, 'input> Reader<'a, 'input> {
    /// An element's style, from its parent's: its attributes, then matching rules, then its
    /// `style` attribute.
    fn style_of(&mut self, node: Node, parent: &Style) -> Style {
        let mut s = parent.clone();
        let mut decls: Declarations = Vec::new();
        for name in [
            "fill",
            "stroke",
            "stroke-width",
            "stroke-dasharray",
            "color",
            "fill-opacity",
            "stroke-opacity",
            "opacity",
            "font-size",
            "text-anchor",
            "visibility",
            "display",
        ] {
            if let Some(v) = node.attribute(name) {
                decls.push((name.to_owned(), v.to_owned()));
            }
        }
        for (selector, rule) in &self.rules {
            if selector.matches(node) {
                decls.extend(rule.iter().cloned());
            }
        }
        if let Some(style) = node.attribute("style") {
            decls.extend(declarations(style));
        }
        // Opacity isn't inherited, but what it fades is the element and all inside it.
        let mut opacity = 1.0;
        for (k, v) in &decls {
            match k.as_str() {
                "fill" => {
                    if let Some(p) = paint(v) {
                        s.fill = p;
                    }
                }
                "stroke" => {
                    if let Some(p) = paint(v) {
                        s.stroke = p;
                    }
                }
                "stroke-width" => {
                    if let Some(w) = user_length(v, s.font_size) {
                        s.stroke_width = w.max(0.0);
                    }
                }
                "stroke-dasharray" => {
                    s.dashes = if v.trim() == "none" {
                        Vec::new()
                    } else {
                        v.split([',', ' '])
                            .filter_map(|d| user_length(d, s.font_size))
                            .collect()
                    };
                }
                "color" => {
                    if let Some(c) = color(v) {
                        s.color = c;
                    }
                }
                "fill-opacity" => s.fill_opacity = v.trim().parse().unwrap_or(1.0),
                "stroke-opacity" => s.stroke_opacity = v.trim().parse().unwrap_or(1.0),
                "opacity" => opacity = v.trim().parse().unwrap_or(1.0),
                "font-size" => {
                    if let Some(size) = user_length(v, parent.font_size) {
                        s.font_size = size;
                    }
                }
                "text-anchor" => {
                    s.anchor = match v.trim() {
                        "middle" => TextAlign::Center,
                        "end" => TextAlign::Right,
                        _ => TextAlign::Left,
                    }
                }
                "visibility" => s.visible = !matches!(v.trim(), "hidden" | "collapse"),
                "display" if v.trim() == "none" => s.visible = false,
                _ => {}
            }
        }
        if opacity < 1.0 {
            s.fill_opacity *= opacity;
            s.stroke_opacity *= opacity;
        }
        s
    }

    /// The region a `<clipPath>` or `<mask>` makes in the user space `ctm` maps from: its
    /// shapes (or the shapes of what it uses). `None` for one sized to the clipped element
    /// (`objectBoundingBox` units), or with nothing in it.
    fn clip_of(&mut self, id: &str, ctm: Affine) -> Option<Clip> {
        let node = *self.ids.get(id)?;
        let units = node
            .attribute("clipPathUnits")
            .or_else(|| node.attribute("maskContentUnits"));
        if units == Some("objectBoundingBox") {
            return None;
        }
        let ctm = ctm
            * node
                .attribute("transform")
                .map_or(Affine::IDENTITY, transform);
        let mut subpaths_in: Vec<Subpath> = Vec::new();
        let mut stack: Vec<(Node, Affine, usize)> = node
            .children()
            .filter(Node::is_element)
            .map(|c| (c, ctm, 0))
            .collect();
        while let Some((n, m, depth)) = stack.pop() {
            if depth > MAX_DEPTH {
                continue;
            }
            let m = m * n.attribute("transform").map_or(Affine::IDENTITY, transform);
            match n.tag_name().name() {
                "g" => stack.extend(
                    n.children()
                        .filter(Node::is_element)
                        .map(|c| (c, m, depth + 1)),
                ),
                "use" => {
                    let href = n
                        .attribute((XLINK, "href"))
                        .or_else(|| n.attribute("href"))
                        .map(|h| h.trim_start_matches('#').to_owned());
                    if let Some(target) = href.and_then(|h| self.ids.get(&h).copied()) {
                        let at = |k: &str| {
                            n.attribute(k)
                                .and_then(|v| user_length(v, 16.0))
                                .unwrap_or(0.0)
                        };
                        stack.push((target, m * Affine::translate((at("x"), at("y"))), depth + 1));
                    }
                }
                _ => {
                    if let Some(path) = shape_path(n, 16.0).and_then(|d| BezPath::from_svg(&d).ok())
                    {
                        let mut parts = subpaths(&(m * path));
                        // A clip shape is closed, however it's drawn.
                        for p in &mut parts {
                            p.closed = true;
                        }
                        subpaths_in.extend(parts);
                    }
                }
            }
        }
        Clip::from_subpaths(&subpaths_in)
    }

    /// The colour a paint comes to: a gradient's first stop, with a note.
    fn color_of(&mut self, paint: &Paint, style: &Style, opacity: f64) -> Option<Rgb> {
        if opacity <= 0.0 {
            return None;
        }
        if opacity < 1.0 {
            self.art
                .note("Transparency of lines and text is left out: they come in solid");
        }
        match paint {
            Paint::None => None,
            Paint::Color(c) => Some(*c),
            Paint::Current => Some(style.color),
            Paint::Url(id) => {
                let stop = self.ids.get(id).and_then(|g| {
                    g.descendants()
                        .find(|n| n.has_tag_name("stop"))
                        .and_then(|stop| {
                            let from_style = stop.attribute("style").and_then(|st| {
                                declarations(st)
                                    .into_iter()
                                    .find(|(k, _)| k == "stop-color")
                                    .map(|(_, v)| v)
                            });
                            from_style
                                .as_deref()
                                .or_else(|| stop.attribute("stop-color"))
                                .and_then(color)
                        })
                });
                match stop {
                    Some(c) => {
                        self.art
                            .note("Gradients come in as a solid fill of their first colour: presentation artwork will look \
                             different");
                        Some(c)
                    }
                    None => {
                        self.art.skip("pattern fill");
                        None
                    }
                }
            }
        }
    }

    fn node(
        &mut self,
        node: Node<'a, 'input>,
        ctm: Affine,
        parent: &Style,
        layer: Option<&str>,
        clips: &[Rc<Clip>],
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        let tag = node.tag_name().name();
        if matches!(
            tag,
            "defs"
                | "clipPath"
                | "mask"
                | "marker"
                | "pattern"
                | "linearGradient"
                | "radialGradient"
                | "symbol"
                | "style"
                | "script"
                | "metadata"
                | "title"
                | "desc"
                | "filter"
                | "foreignObject"
        ) {
            return;
        }
        let style = self.style_of(node, parent);
        if node.attribute("display") == Some("none")
            || node
                .attribute("style")
                .is_some_and(|s| s.replace(' ', "").contains("display:none"))
        {
            return;
        }
        let ctm = ctm
            * node
                .attribute("transform")
                .map_or(Affine::IDENTITY, transform);
        // Its clipping path or mask (a mask's shapes, as a clip), on top of those around it.
        let mut clips = clips.to_vec();
        for (attr, mask) in [("clip-path", false), ("mask", true)] {
            let Some(id) = node.attribute(attr).and_then(|v| match paint(v) {
                Some(Paint::Url(id)) => Some(id),
                _ => None,
            }) else {
                continue;
            };
            match self.clip_of(&id, ctm) {
                Some(clip) => {
                    clips.push(Rc::new(clip));
                    if mask {
                        self.art.note(
                            "Masks are used as clips by their shapes: their fades are left out",
                        );
                    }
                }
                None => self.art.note(
                    "A clipping path or mask Drift can't read is left out: what it hides \
                     comes in too",
                ),
            }
        }
        // Inkscape layers can be anywhere; Drift's own groups name theirs.
        let own = if tag == "g"
            && (node.attribute((INKSCAPE, "groupmode")) == Some("layer")
                || node.attribute("data-layer").is_some())
        {
            layer_name(node)
        } else {
            None
        };
        let layer = own.as_deref().or(layer);
        match tag {
            "g" | "a" | "switch" | "svg" => {
                let ctm = if tag == "svg" && depth > 0 {
                    let x = node
                        .attribute("x")
                        .and_then(|v| user_length(v, style.font_size));
                    let y = node
                        .attribute("y")
                        .and_then(|v| user_length(v, style.font_size));
                    ctm * Affine::translate((x.unwrap_or(0.0), y.unwrap_or(0.0)))
                } else {
                    ctm
                };
                for child in node.children().filter(Node::is_element) {
                    self.node(child, ctm, &style, layer, &clips, depth + 1);
                }
            }
            "use" => {
                let href = node
                    .attribute((XLINK, "href"))
                    .or_else(|| node.attribute("href"))
                    .map(|h| h.trim_start_matches('#').to_owned());
                let Some(target) = href.and_then(|h| self.ids.get(&h).copied()) else {
                    return;
                };
                let at = |k: &str| {
                    node.attribute(k)
                        .and_then(|v| user_length(v, style.font_size))
                        .unwrap_or(0.0)
                };
                let ctm = ctm * Affine::translate((at("x"), at("y")));
                if target.has_tag_name("symbol") {
                    for child in target.children().filter(Node::is_element) {
                        self.node(child, ctm, &style, layer, &clips, depth + 1);
                    }
                } else {
                    self.node(target, ctm, &style, layer, &clips, depth + 1);
                }
            }
            "image" => self.art.skip("image"),
            "text" => {
                if style.visible {
                    self.text(node, ctm, &style, layer, &clips);
                }
            }
            _ => {
                let Some(d) = shape_path(node, style.font_size) else {
                    return;
                };
                if !style.visible {
                    return;
                }
                if node.attribute("marker-start").is_some()
                    || node.attribute("marker-end").is_some()
                    || node.attribute("marker-mid").is_some()
                {
                    self.art.skip("arrowhead (marker)");
                }
                let Ok(path) = BezPath::from_svg(&d) else {
                    self.art.skip("unreadable path");
                    return;
                };
                let subpaths = subpaths(&(ctm * path));
                if subpaths.is_empty() {
                    return;
                }
                let (stroke_parts, fill_parts, clipped) = clip_paint(&subpaths, &clips);
                self.art.clipping.add(clipped);
                let scale = ctm.determinant().abs().sqrt();
                let stroke = self
                    .color_of(&style.stroke.clone(), &style, style.stroke_opacity)
                    .map(|color| Stroke {
                        color,
                        width: style.stroke_width * scale,
                        dashes: style.dashes.iter().map(|d| d * scale).collect(),
                    });
                // A fill keeps its opacity.
                let fill = if tag == "line" || style.fill_opacity <= 0.0 {
                    None
                } else {
                    self.color_of(&style.fill.clone(), &style, 1.0)
                };
                let fill_opacity = style.fill_opacity.min(1.0);
                if stroke.is_none() && fill.is_none() {
                    return;
                }
                let layer = self.art.layer(layer.unwrap_or(&self.loose.clone()));
                if stroke_parts == fill_parts {
                    if !stroke_parts.is_empty() {
                        self.art.paths.push(Painted {
                            layer,
                            subpaths: stroke_parts,
                            stroke,
                            fill,
                            fill_opacity,
                        });
                    }
                    return;
                }
                // Clipped differently: the outline and the fill apart.
                if stroke.is_some() && !stroke_parts.is_empty() {
                    self.art.paths.push(Painted {
                        layer: layer.clone(),
                        subpaths: stroke_parts,
                        stroke,
                        fill: None,
                        fill_opacity: 1.0,
                    });
                }
                if fill.is_some() && !fill_parts.is_empty() {
                    self.art.paths.push(Painted {
                        layer,
                        subpaths: fill_parts,
                        stroke: None,
                        fill,
                        fill_opacity,
                    });
                }
            }
        }
    }

    /// A `<text>` element: a line per run that sets its own position.
    fn text(
        &mut self,
        node: Node<'a, 'input>,
        ctm: Affine,
        style: &Style,
        layer: Option<&str>,
        clips: &[Rc<Clip>],
    ) {
        let first = |v: Option<&str>| v.and_then(|v| numbers(v).first().copied());
        let mut pos = (
            first(node.attribute("x")).unwrap_or(0.0),
            first(node.attribute("y")).unwrap_or(0.0),
        );
        // (position, text, style) for each run.
        let mut runs: Vec<((f64, f64), String, Style)> = vec![(pos, String::new(), style.clone())];
        for child in node.children() {
            if let Some(t) = child.text().filter(|_| child.is_text()) {
                if let Some(run) = runs.last_mut() {
                    run.1.push_str(t);
                }
            } else if child.has_tag_name("tspan") {
                let own = self.style_of(child, style);
                let x = first(child.attribute("x"));
                let y = first(child.attribute("y"));
                let dy = first(child.attribute("dy")).unwrap_or(0.0);
                let text: String = child
                    .descendants()
                    .filter(Node::is_text)
                    .filter_map(|n| n.text())
                    .collect();
                if x.is_some() || y.is_some() || dy != 0.0 {
                    pos = (x.unwrap_or(pos.0), y.unwrap_or(pos.1) + dy);
                    runs.push((pos, text, own));
                } else if let Some(run) = runs.last_mut() {
                    run.1.push_str(&text);
                }
            }
        }
        let layer = self.art.layer(layer.unwrap_or(&self.loose.clone()));
        let scale = ctm.determinant().abs().sqrt();
        let along = ctm * Point::new(1.0, 0.0) - ctm * Point::new(0.0, 0.0);
        let angle = along.y.atan2(along.x).to_degrees();
        for (p, text, s) in runs {
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                continue;
            }
            let at = ctm * Point::new(p.0, p.1);
            if !clips.iter().all(|c| c.contains(DVec2::new(at.x, at.y))) {
                continue;
            }
            let color = self.color_of(&s.fill.clone(), &s, s.fill_opacity);
            self.art.texts.push(Text {
                layer: layer.clone(),
                text,
                at: DVec2::new(at.x, at.y),
                size: s.font_size * scale,
                angle,
                color,
                align: s.anchor,
            });
        }
    }
}

/// A shape element's outline as path data.
fn shape_path(node: Node, font_size: f64) -> Option<String> {
    let len = |k: &str| {
        node.attribute(k)
            .and_then(|v| user_length(v, font_size))
            .unwrap_or(0.0)
    };
    Some(match node.tag_name().name() {
        "path" => node.attribute("d")?.to_owned(),
        "line" => format!(
            "M {} {} L {} {}",
            len("x1"),
            len("y1"),
            len("x2"),
            len("y2")
        ),
        "polyline" | "polygon" => {
            let n = numbers(node.attribute("points")?);
            if n.len() < 4 {
                return None;
            }
            let mut d = format!("M {} {}", n[0], n[1]);
            for p in n[2..].as_chunks::<2>().0 {
                d.push_str(&format!(" L {} {}", p[0], p[1]));
            }
            if node.has_tag_name("polygon") {
                d.push_str(" Z");
            }
            d
        }
        "rect" => {
            let (x, y, w, h) = (len("x"), len("y"), len("width"), len("height"));
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            let rx = node.attribute("rx").and_then(|v| user_length(v, font_size));
            let ry = node.attribute("ry").and_then(|v| user_length(v, font_size));
            let (rx, ry) = match (rx, ry) {
                (Some(rx), Some(ry)) => (rx, ry),
                (Some(r), None) | (None, Some(r)) => (r, r),
                (None, None) => (0.0, 0.0),
            };
            let (rx, ry) = (rx.clamp(0.0, w / 2.0), ry.clamp(0.0, h / 2.0));
            if rx <= 0.0 || ry <= 0.0 {
                format!("M {x} {y} H {} V {} H {x} Z", x + w, y + h)
            } else {
                format!(
                    "M {} {y} H {} A {rx} {ry} 0 0 1 {} {} V {} A {rx} {ry} 0 0 1 {} {} H {} \
                     A {rx} {ry} 0 0 1 {x} {} V {} A {rx} {ry} 0 0 1 {} {y} Z",
                    x + rx,
                    x + w - rx,
                    x + w,
                    y + ry,
                    y + h - ry,
                    x + w - rx,
                    y + h,
                    x + rx,
                    y + h - ry,
                    y + ry,
                    x + rx,
                )
            }
        }
        "circle" | "ellipse" => {
            let (cx, cy) = (len("cx"), len("cy"));
            let (rx, ry) = if node.has_tag_name("circle") {
                (len("r"), len("r"))
            } else {
                (len("rx"), len("ry"))
            };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            format!(
                "M {} {cy} A {rx} {ry} 0 0 1 {cx} {} A {rx} {ry} 0 0 1 {} {cy} \
                 A {rx} {ry} 0 0 1 {cx} {} A {rx} {ry} 0 0 1 {} {cy} Z",
                cx + rx,
                cy + ry,
                cx - rx,
                cy - ry,
                cx + rx,
            )
        }
        _ => return None,
    })
}

/// A path's subpaths, with quadratic curves made cubic.
fn subpaths(path: &BezPath) -> Vec<Subpath> {
    let v = |p: Point| DVec2::new(p.x, p.y);
    let mut out: Vec<Subpath> = Vec::new();
    let mut current: Option<Subpath> = None;
    let mut at = DVec2::ZERO;
    let finish = |current: &mut Option<Subpath>, out: &mut Vec<Subpath>| {
        if let Some(s) = current.take()
            && !s.segs.is_empty()
        {
            out.push(s);
        }
    };
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                finish(&mut current, &mut out);
                at = v(p);
                current = Some(Subpath {
                    start: at,
                    segs: Vec::new(),
                    closed: false,
                });
            }
            PathEl::LineTo(p) => {
                let s = current.get_or_insert_with(|| Subpath {
                    start: at,
                    segs: Vec::new(),
                    closed: false,
                });
                at = v(p);
                s.segs.push(Seg::Line(at));
            }
            PathEl::QuadTo(q, p) => {
                let s = current.get_or_insert_with(|| Subpath {
                    start: at,
                    segs: Vec::new(),
                    closed: false,
                });
                let (q, p) = (v(q), v(p));
                s.segs.push(Seg::Cubic(
                    at + (q - at) * (2.0 / 3.0),
                    p + (q - p) * (2.0 / 3.0),
                    p,
                ));
                at = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let s = current.get_or_insert_with(|| Subpath {
                    start: at,
                    segs: Vec::new(),
                    closed: false,
                });
                at = v(p);
                s.segs.push(Seg::Cubic(v(c1), v(c2), at));
            }
            PathEl::ClosePath => {
                if let Some(mut s) = current.take() {
                    s.closed = true;
                    at = s.start;
                    if !s.segs.is_empty() {
                        out.push(s);
                    }
                }
            }
        }
    }
    finish(&mut current, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::{ImportOptions, shapes_of, to_import};
    use drift_core::Shape;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    #[test]
    fn an_illustrator_file_comes_in_with_its_layers_styles_and_size() {
        // As Illustrator writes it: points as units, layers as the root's groups, styles as
        // classes.
        let svg = r##"<?xml version="1.0" encoding="utf-8"?>
<!-- Generator: Adobe Illustrator 28.0.0, SVG Export Plug-In . SVG Version: 6.00 Build 0)  -->
<svg version="1.1" id="Layer_1" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" x="0px" y="0px"
	 viewBox="0 0 720 360" style="enable-background:new 0 0 720 360;" xml:space="preserve">
<style type="text/css">
	.st0{fill:none;stroke:#FF0000;stroke-width:2;stroke-miterlimit:10;}
	.st1{fill:#0000FF;}
	.st2{font-family:'Helvetica';font-size:24px;}
</style>
<g id="Walls_x5F_new">
	<rect x="72" y="72" class="st0" width="144" height="72"/>
	<circle class="st0" cx="360" cy="180" r="36"/>
</g>
<g id="Furniture_1_">
	<path class="st1" d="M432,72h72v72h-72V72z M450,90v36h36V90H450z"/>
	<text transform="matrix(1 0 0 1 72 300)" class="st2">Kitchen</text>
</g>
</svg>"##;
        let art = read(svg, "plan").unwrap();
        // 720 points: 10 inches.
        assert!((art.size.x - 254.0).abs() < 1e-9, "{:?}", art.size);
        assert_eq!(art.layers, ["Walls_new", "Furniture"]);
        assert_eq!(art.paths.len(), 3);
        let rect = &art.paths[0];
        assert_eq!(rect.layer, "Walls_new");
        assert_eq!(rect.fill, None);
        let stroke = rect.stroke.as_ref().unwrap();
        assert_eq!(stroke.color, Rgb([255, 0, 0]));
        assert!((stroke.width - 2.0 * 25.4 / 72.0).abs() < 1e-9);
        // The rectangle: an inch in from the left, and from the top (y is up in Drift).
        let shapes = shapes_of(&rect.subpaths[0]);
        assert_eq!(shapes.len(), 4);
        let Shape::Line { a, .. } = shapes[0] else {
            panic!()
        };
        assert!(a.distance(v(25.4, 127.0 - 25.4)) < 1e-9, "{a}");
        // The circle is a circle.
        let shapes = shapes_of(&art.paths[1].subpaths[0]);
        assert!(
            matches!(shapes[..], [Shape::Circle { radius, .. }] if (radius - 12.7).abs() < 1e-6),
            "{shapes:?}"
        );
        // A filled square with a square hole.
        let furniture = &art.paths[2];
        assert_eq!(furniture.fill, Some(Rgb([0, 0, 255])));
        assert_eq!(furniture.subpaths.len(), 2);
        // The text: 24 pt, cap height of it, on its baseline.
        let t = &art.texts[0];
        assert_eq!(t.text, "Kitchen");
        assert_eq!(t.layer, "Furniture");
        assert!((t.size - 24.0 * 25.4 / 72.0).abs() < 1e-9);
        assert!(t.at.distance(v(25.4, 127.0 - 300.0 * 25.4 / 72.0)) < 1e-9);
        // Into a sketch at 1:50: one hatch, holed.
        let import = to_import(
            &art,
            &ImportOptions {
                scale: 50.0,
                construction_edges: true,
            },
        );
        assert_eq!(import.hatches.len(), 1);
        assert_eq!(import.hatches[0].loops.len(), 2);
        assert_eq!(import.texts.len(), 1);
    }

    #[test]
    fn inkscape_layers_units_transforms_and_uses() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape"
     xmlns:xlink="http://www.w3.org/1999/xlink" width="200mm" height="100mm" viewBox="0 0 200 100">
  <defs><path id="tick" d="M 0 0 L 10 0" stroke="black" stroke-width="0.5"/></defs>
  <g inkscape:groupmode="layer" inkscape:label="Grid lines" transform="translate(10 20)">
    <line x1="0" y1="0" x2="50" y2="0" style="stroke: rgb(0, 128, 0); stroke-width: 0.25; stroke-dasharray: 3 1"/>
    <use xlink:href="#tick" x="5" y="5"/>
  </g>
  <g style="display:none"><line x1="0" y1="0" x2="5" y2="5" stroke="black"/></g>
  <image href="photo.png" width="10" height="10"/>
  <path d="M 100 50 Q 120 10 140 50" fill="none" stroke="#123"/>
</svg>"##;
        let art = read(svg, "sketch").unwrap();
        assert_eq!(art.size, v(200.0, 100.0));
        assert_eq!(art.layers, ["Grid lines", "sketch"]);
        // The line, moved by its layer and flipped: 20 down from the top.
        let line = &art.paths[0];
        let Shape::Line { a, b } = shapes_of(&line.subpaths[0])[0] else {
            panic!()
        };
        assert!(a.distance(v(10.0, 80.0)) < 1e-9 && b.distance(v(60.0, 80.0)) < 1e-9);
        let stroke = line.stroke.as_ref().unwrap();
        assert_eq!(stroke.color, Rgb([0, 128, 0]));
        assert_eq!(stroke.dashes, [3.0, 1.0]);
        // The copy, at its place, on the layer it's used in.
        let tick = &art.paths[1];
        assert_eq!(tick.layer, "Grid lines");
        let Shape::Line { a, .. } = shapes_of(&tick.subpaths[0])[0] else {
            panic!()
        };
        assert!(a.distance(v(15.0, 75.0)) < 1e-9, "{a}");
        // The hidden group isn't there; the image is counted; the curve is one Bézier.
        assert_eq!(art.paths.len(), 3);
        assert_eq!(art.skipped.get("image"), Some(&1));
        assert!(matches!(
            shapes_of(&art.paths[2].subpaths[0])[..],
            [Shape::Bezier { .. }]
        ));
        assert_eq!(
            art.paths[2].stroke.as_ref().unwrap().color,
            Rgb([17, 34, 51])
        );
    }

    #[test]
    fn illustrator_ids_read_as_names() {
        assert_eq!(illustrator_id("Layer_1"), "Layer 1");
        assert_eq!(illustrator_id("Walls_x5F_new"), "Walls_new");
        assert_eq!(illustrator_id("Furniture_1_"), "Furniture");
        assert_eq!(illustrator_id("A-101_x2C__Plan"), "A-101, Plan");
    }

    #[test]
    fn clipping_paths_and_the_artboard_clip() {
        // A group clipped to a 20 × 20 square, a line across it, and art off the artboard.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
  <defs><clipPath id="crop"><rect x="10" y="10" width="20" height="20"/></clipPath></defs>
  <g clip-path="url(#crop)">
    <line x1="0" y1="20" x2="50" y2="20" stroke="black"/>
    <rect x="40" y="40" width="5" height="5" fill="red"/>
    <rect x="0" y="0" width="100" height="100" fill="blue"/>
  </g>
  <line x1="90" y1="50" x2="150" y2="50" stroke="black"/>
  <line x1="200" y1="50" x2="250" y2="50" stroke="black"/>
</svg>"##;
        let art = read(svg, "plan").unwrap();
        // The line through the clip: just the part inside, 10 to 30 (y flipped: 80).
        let crossing = shapes_of(&art.paths[0].subpaths[0]);
        let Shape::Line { a, b } = crossing[0] else {
            panic!()
        };
        assert!(
            a.distance(v(10.0, 80.0)) < 1e-9 && b.distance(v(30.0, 80.0)) < 1e-9,
            "{a} {b}"
        );
        // The red square is hidden by the clip; the blue background is cut to the clip.
        assert!(art.paths.iter().all(|p| p.fill != Some(Rgb([255, 0, 0]))));
        let blue = art
            .paths
            .iter()
            .find(|p| p.fill == Some(Rgb([0, 0, 255])))
            .expect("the background, cut to the clip");
        let points: Vec<DVec2> = std::iter::once(blue.subpaths[0].start)
            .chain(blue.subpaths[0].segs.iter().map(Seg::end))
            .collect();
        assert!(
            points
                .iter()
                .all(|p| (10.0 - 1e-9..=30.0 + 1e-9).contains(&p.x))
        );
        // The line off the artboard's edge is trimmed to it; the one beyond it is left out.
        let trimmed = shapes_of(&art.paths[2].subpaths[0]);
        let Shape::Line { b, .. } = trimmed[0] else {
            panic!()
        };
        assert!((b.x - 100.0).abs() < 1e-9, "{b}");
        assert_eq!(art.paths.len(), 3);
        assert_eq!(art.clipping.hidden, 2);
        assert!(art.notes.iter().any(|n| n.contains("Clipping is applied")));
    }
}
