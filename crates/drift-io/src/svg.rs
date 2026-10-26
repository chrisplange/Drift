//! SVG output of a [`Drawing`], made to open well in Illustrator and Inkscape: true scale
//! (`mm` units), a named group per layer (Inkscape reads it as a layer; Illustrator as a group
//! of that name, which its Release to Layers makes layers of), curves that meet joined into
//! one path (so their corners join), true arcs, even–odd fills, presentation attributes
//! rather than CSS, and text in Helvetica / Arial.
//!
//! Items keep the drawing's order within each layer's group, and the groups come in the
//! order their layers are first drawn: the stacking order on screen, as long as each layer's
//! items are drawn together (a fill's outline on another layer goes in that layer's group).

use std::fmt::Write as _;

use drift_core::DVec2;
use drift_draft::page::{Align, CAP_HEIGHT, Drawing, Item, Pen, Prim, dash_pattern};

fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_owned() }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn stroke(pen: &Pen, lts: f64) -> String {
    let [r, g, b] = pen.color.0;
    let mut s = format!(
        "stroke=\"#{r:02x}{g:02x}{b:02x}\" stroke-width=\"{}\"",
        num(pen.width.max(0.05))
    );
    let dashes = dash_pattern(pen.line_type);
    if !dashes.is_empty() {
        let d: Vec<String> = dashes.iter().map(|v| num(v * lts)).collect();
        let _ = write!(s, " stroke-dasharray=\"{}\"", d.join(" "));
    }
    s
}

/// Serialize a drawing as an SVG document (y flipped so the page reads upright).
pub fn write(d: &Drawing) -> String {
    let d = &d.exploded();
    let (w, h) = (d.size.x, d.size.y);
    let y = |v: f64| h - v;
    let mut out = String::new();
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="{}mm" height="{}mm" viewBox="0 0 {} {}">"#,
        num(w),
        num(h),
        num(w),
        num(h)
    );
    let _ = writeln!(out, "<title>Drift drawing</title>");
    // One clip path per window the items are seen through (a sheet's views).
    let mut windows: Vec<drift_core::Bounds> = Vec::new();
    for c in d.items.iter().filter_map(|i| i.clip) {
        if !windows.contains(&c) {
            windows.push(c);
        }
    }
    if !windows.is_empty() {
        out.push_str("<defs>");
        for (k, c) in windows.iter().enumerate() {
            let _ = write!(
                out,
                r#"<clipPath id="view{}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath>"#,
                k + 1,
                num(c.min.x),
                num(y(c.max.y)),
                num(c.max.x - c.min.x),
                num(c.max.y - c.min.y)
            );
        }
        out.push_str("</defs>\n");
    }
    let window_of = |item: &drift_draft::page::Item| {
        item.clip
            .and_then(|c| windows.iter().position(|w| *w == c))
            .map_or(0, |k| k + 1)
    };
    // The layers in the order they are first drawn; each is one group.
    let mut layers: Vec<&str> = Vec::new();
    for item in &d.items {
        if !layers.contains(&item.layer.as_str()) {
            layers.push(&item.layer);
        }
    }
    let mut ids: Vec<String> = Vec::new();
    for layer in layers {
        let mut id = layer_id(layer);
        let base = id.clone();
        let mut n = 1;
        while ids.contains(&id) {
            id = format!("{base}_{n}_");
            n += 1;
        }
        ids.push(id.clone());
        let name = escape(layer);
        let _ = writeln!(
            out,
            r#"<g id="{id}" data-name="{name}" data-layer="{name}" inkscape:groupmode="layer" inkscape:label="{name}" fill="none" stroke-linecap="round" stroke-linejoin="round">"#
        );
        // Inside it, the items seen through each window in a group of their own.
        let mut window = 0;
        let mut run: Vec<(DVec2, Vec<Piece>)> = Vec::new();
        let mut run_pen: Option<Pen> = None;
        for item in d.items.iter().filter(|i| i.layer == layer) {
            let w = window_of(item);
            let piece = pieces(&item.prim);
            if run_pen.is_some_and(|p| p != item.pen || w != window || piece.is_none()) {
                write_chains(
                    &mut out,
                    &std::mem::take(&mut run),
                    &run_pen.take().expect("pen"),
                    h,
                    d.line_type_scale,
                );
            }
            if w != window {
                if window > 0 {
                    out.push_str("</g>\n");
                }
                if w > 0 {
                    let _ = writeln!(out, r#"<g clip-path="url(#view{w})">"#);
                }
                window = w;
            }
            match piece {
                Some(p) => {
                    run.push(p);
                    run_pen = Some(item.pen);
                }
                None => item_svg(&mut out, item, h, d.line_type_scale, d.font.as_deref()),
            }
        }
        if let Some(pen) = run_pen {
            write_chains(&mut out, &run, &pen, h, d.line_type_scale);
        }
        if window > 0 {
            out.push_str("</g>\n");
        }
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

/// A layer's name as an XML id, escaped as Illustrator escapes them: spaces as `_`, other
/// characters (and a leading digit) as `_xHH_`.
fn layer_id(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        match c {
            ' ' => out.push('_'),
            c if c.is_ascii_alphabetic() || (i > 0 && (c.is_ascii_digit() || c == '-')) => {
                out.push(c)
            }
            c => {
                let _ = write!(out, "_x{:X}_", u32::from(c));
            }
        }
    }
    if out.is_empty() { "Layer".into() } else { out }
}

/// A piece of a stroked path, from where the one before it ended (page mm, y up).
#[derive(Clone, Copy, Debug)]
enum Piece {
    Line(DVec2),
    /// A circular arc: radius, sweep (anticlockwise positive) and end.
    Arc(f64, f64, DVec2),
    /// An elliptical arc: radii, the major axis's angle, sweep and end.
    Ellipse(f64, f64, f64, f64, DVec2),
    Cubic(DVec2, DVec2, DVec2),
}

impl Piece {
    fn end(&self) -> DVec2 {
        match *self {
            Piece::Line(p) | Piece::Arc(_, _, p) | Piece::Ellipse(_, _, _, _, p) => p,
            Piece::Cubic(_, _, p) => p,
        }
    }
}

/// An open stroke as the pieces of a path: its start, and pieces. `None` for what is drawn
/// on its own (closed shapes, fills, text).
fn pieces(prim: &Prim) -> Option<(DVec2, Vec<Piece>)> {
    Some(match prim {
        Prim::Line(a, b) => (*a, vec![Piece::Line(*b)]),
        Prim::Polyline {
            points,
            closed: false,
        } if points.len() >= 2 => (
            points[0],
            points[1..].iter().map(|p| Piece::Line(*p)).collect(),
        ),
        Prim::Arc {
            center,
            radius,
            start,
            sweep,
        } => (
            *center + DVec2::from_angle(*start) * *radius,
            vec![Piece::Arc(
                *radius,
                *sweep,
                *center + DVec2::from_angle(start + sweep) * *radius,
            )],
        ),
        Prim::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } if *sweep < std::f64::consts::TAU - 1e-12 => {
            let at = |t: f64| drift_core::shape::ellipse_point(*center, *major, *minor, t);
            (
                at(*start),
                vec![Piece::Ellipse(
                    major.length(),
                    *minor,
                    major.y.atan2(major.x),
                    *sweep,
                    at(start + sweep),
                )],
            )
        }
        Prim::Bezier { p } => (p[0], vec![Piece::Cubic(p[1], p[2], p[3])]),
        _ => return None,
    })
}

/// A run of pieces turned round, to go from its end to `start`.
fn reversed(start: DVec2, run: &[Piece]) -> (DVec2, Vec<Piece>) {
    let mut froms = vec![start];
    froms.extend(run.iter().map(Piece::end));
    let end = *froms.last().expect("start");
    let back = run
        .iter()
        .enumerate()
        .rev()
        .map(|(k, piece)| {
            let to = froms[k];
            match *piece {
                Piece::Line(_) => Piece::Line(to),
                Piece::Arc(r, sweep, _) => Piece::Arc(r, -sweep, to),
                Piece::Ellipse(rx, ry, rot, sweep, _) => Piece::Ellipse(rx, ry, rot, -sweep, to),
                Piece::Cubic(c1, c2, _) => Piece::Cubic(c2, c1, to),
            }
        })
        .collect();
    (end, back)
}

/// Strokes of one pen as few paths as they join into: each path follows curves that meet
/// end to end, whichever way round they were drawn.
fn write_chains(out: &mut String, run: &[(DVec2, Vec<Piece>)], pen: &Pen, h: f64, lts: f64) {
    let key = |p: DVec2| ((p.x * 1e4).round() as i64, (p.y * 1e4).round() as i64);
    let end_of = |k: usize| run[k].1.last().map_or(run[k].0, Piece::end);
    // Where each stroke starts and ends.
    let mut at: std::collections::HashMap<(i64, i64), Vec<usize>> = Default::default();
    for (k, (start, _)) in run.iter().enumerate() {
        at.entry(key(*start)).or_default().push(k);
        at.entry(key(end_of(k))).or_default().push(k);
    }
    let mut used = vec![false; run.len()];
    let next = |p: DVec2, used: &mut Vec<bool>| -> Option<usize> {
        let k = at.get(&key(p))?.iter().copied().find(|k| !used[*k])?;
        used[k] = true;
        Some(k)
    };
    for first in 0..run.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let (mut start, mut path) = run[first].clone();
        // On from its end…
        loop {
            let end = path.last().map_or(start, Piece::end);
            if key(end) == key(start) && !path.is_empty() {
                break;
            }
            let Some(k) = next(end, &mut used) else {
                break;
            };
            let (s, p) = &run[k];
            if key(*s) == key(end) {
                path.extend(p.iter().copied());
            } else {
                path.extend(reversed(*s, p).1);
            }
        }
        // …and back from its start.
        loop {
            let end = path.last().map_or(start, Piece::end);
            if key(end) == key(start) {
                break;
            }
            let Some(k) = next(start, &mut used) else {
                break;
            };
            let (s, p) = &run[k];
            let (from, mut before) = if key(end_of(k)) == key(start) {
                (*s, p.clone())
            } else {
                reversed(*s, p)
            };
            before.extend(path);
            path = before;
            start = from;
        }
        write_path(out, start, &path, pen, h, lts);
    }
}

fn write_path(out: &mut String, start: DVec2, path: &[Piece], pen: &Pen, h: f64, lts: f64) {
    let y = |v: f64| h - v;
    let mut d = format!("M {} {}", num(start.x), num(y(start.y)));
    for piece in path {
        // Anticlockwise with y up is clockwise once flipped: sweep flag 0.
        let flags = |sweep: f64| {
            (
                u8::from(sweep.abs() > std::f64::consts::PI),
                u8::from(sweep < 0.0),
            )
        };
        match *piece {
            Piece::Line(p) => {
                let _ = write!(d, " L {} {}", num(p.x), num(y(p.y)));
            }
            Piece::Arc(r, sweep, p) => {
                let (large, flag) = flags(sweep);
                let _ = write!(
                    d,
                    " A {} {} 0 {large} {flag} {} {}",
                    num(r),
                    num(r),
                    num(p.x),
                    num(y(p.y))
                );
            }
            Piece::Ellipse(rx, ry, rot, sweep, p) => {
                let (large, flag) = flags(sweep);
                let _ = write!(
                    d,
                    " A {} {} {} {large} {flag} {} {}",
                    num(rx),
                    num(ry),
                    num(-rot.to_degrees()),
                    num(p.x),
                    num(y(p.y))
                );
            }
            Piece::Cubic(c1, c2, p) => {
                let _ = write!(
                    d,
                    " C {} {} {} {} {} {}",
                    num(c1.x),
                    num(y(c1.y)),
                    num(c2.x),
                    num(y(c2.y)),
                    num(p.x),
                    num(y(p.y))
                );
            }
        }
    }
    let end = path.last().map_or(start, Piece::end);
    if path.len() > 1 && end.distance(start) < 1e-4 {
        d.push_str(" Z");
    }
    let _ = writeln!(out, r#"<path d="{d}" {}/>"#, stroke(pen, lts));
}

fn item_svg(
    out: &mut String,
    item: &Item,
    h: f64,
    lts: f64,
    font: Option<&drift_draft::font::DrawingFont>,
) {
    let y = |v: f64| h - v;
    let pen = &item.pen;
    match &item.prim {
        Prim::Line(a, b) => {
            let _ = writeln!(
                out,
                r#"<line x1="{}" y1="{}" x2="{}" y2="{}" {}/>"#,
                num(a.x),
                num(y(a.y)),
                num(b.x),
                num(y(b.y)),
                stroke(pen, lts)
            );
        }
        Prim::Polyline { points, closed } => {
            let pts: Vec<String> = points
                .iter()
                .map(|p| format!("{},{}", num(p.x), num(y(p.y))))
                .collect();
            let tag = if *closed { "polygon" } else { "polyline" };
            let _ = writeln!(
                out,
                r#"<{tag} points="{}" {}/>"#,
                pts.join(" "),
                stroke(pen, lts)
            );
        }
        Prim::Circle { center, radius } => {
            let _ = writeln!(
                out,
                r#"<circle cx="{}" cy="{}" r="{}" {}/>"#,
                num(center.x),
                num(y(center.y)),
                num(*radius),
                stroke(pen, lts)
            );
        }
        Prim::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let p = *center + drift_core::DVec2::from_angle(*start) * *radius;
            let q = *center + drift_core::DVec2::from_angle(start + sweep) * *radius;
            // Counter-clockwise in y-up is clockwise once flipped: sweep flag 0.
            let large = u8::from(*sweep > std::f64::consts::PI);
            let _ = writeln!(
                out,
                r#"<path d="M {} {} A {} {} 0 {large} 0 {} {}" {}/>"#,
                num(p.x),
                num(y(p.y)),
                num(*radius),
                num(*radius),
                num(q.x),
                num(y(q.y)),
                stroke(pen, lts)
            );
        }
        Prim::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => {
            // Exact elliptical arcs; a whole ellipse as two halves.
            let at = |t: f64| drift_core::shape::ellipse_point(*center, *major, *minor, t);
            let rx = major.length();
            // The page is flipped in y, so angles turn the other way.
            let rot = -major.y.atan2(major.x).to_degrees();
            let whole = *sweep >= std::f64::consts::TAU - 1e-12;
            let steps: Vec<f64> = if whole {
                vec![
                    *start + std::f64::consts::PI,
                    *start + std::f64::consts::TAU,
                ]
            } else {
                vec![start + sweep]
            };
            let p = at(*start);
            let mut d = format!("M {} {}", num(p.x), num(y(p.y)));
            let mut from = *start;
            for to in steps {
                let q = at(to);
                let large = u8::from(to - from > std::f64::consts::PI);
                let _ = write!(
                    d,
                    " A {} {} {} {large} 0 {} {}",
                    num(rx),
                    num(*minor),
                    num(rot),
                    num(q.x),
                    num(y(q.y))
                );
                from = to;
            }
            if whole {
                d.push_str(" Z");
            }
            let _ = writeln!(out, r#"<path d="{d}" {}/>"#, stroke(pen, lts));
        }
        Prim::Bezier { p } => {
            let _ = writeln!(
                out,
                r#"<path d="M {} {} C {} {} {} {} {} {}" {}/>"#,
                num(p[0].x),
                num(y(p[0].y)),
                num(p[1].x),
                num(y(p[1].y)),
                num(p[2].x),
                num(y(p[2].y)),
                num(p[3].x),
                num(y(p[3].y)),
                stroke(pen, lts)
            );
        }
        Prim::Fill { rings, mask, .. } => {
            let mut d = String::new();
            for ring in rings {
                for (k, p) in ring.iter().enumerate() {
                    let _ = write!(
                        d,
                        "{}{} {} ",
                        if k == 0 { "M" } else { "L" },
                        num(p.x),
                        num(y(p.y))
                    );
                }
                d.push_str("Z ");
            }
            let [r, g, b] = if *mask { [255, 255, 255] } else { pen.color.0 };
            let _ = writeln!(
                out,
                r##"<path d="{}" fill="#{r:02x}{g:02x}{b:02x}" fill-rule="evenodd" stroke="none"/>"##,
                d.trim_end()
            );
        }
        Prim::Text {
            at,
            height,
            angle,
            align,
            text,
            style,
            font: sketch,
        } => {
            let [r, g, b] = pen.color.0;
            // Sketch text names its own font; other text, the drawing's (SVG can't carry
            // fonts); then the standard ones.
            let family = match sketch {
                Some(drift_core::TextFont::Sans) => "'Ubuntu', ".to_owned(),
                Some(drift_core::TextFont::Mono) => "'Hack', monospace, ".to_owned(),
                Some(drift_core::TextFont::System(f)) => format!("'{}', ", escape(f)),
                None => font.map_or(String::new(), |f| {
                    format!("'{}', ", escape(&f.regular.family))
                }),
            };
            let cap = match sketch {
                Some(f) => height / drift_draft::outline::em_size(f, *height),
                None => font.map_or(CAP_HEIGHT, |f| f.face(style.bold).cap_ratio()),
            };
            let mut look = String::new();
            if style.bold {
                look.push_str(r#" font-weight="700""#);
            }
            if style.italic {
                look.push_str(r#" font-style="italic""#);
            }
            if style.tracking != 0.0 {
                look.push_str(&format!(
                    r#" letter-spacing="{}""#,
                    num(style.tracking * height / cap)
                ));
            }
            let anchor = match align {
                Align::Left => "start",
                Align::Center => "middle",
                Align::Right => "end",
            };
            let rotate = if angle.abs() > 1e-9 {
                format!(
                    r#" transform="rotate({} {} {})""#,
                    num(-angle.to_degrees()),
                    num(at.x),
                    num(y(at.y))
                )
            } else {
                String::new()
            };
            let _ = writeln!(
                out,
                r##"<text x="{}" y="{}" font-family="{family}Helvetica, Arial, sans-serif" font-size="{}"{look} fill="#{r:02x}{g:02x}{b:02x}" stroke="none" text-anchor="{anchor}"{rotate}>{}</text>"##,
                num(at.x),
                num(y(at.y)),
                num(height / cap),
                escape(text)
            );
        }
        Prim::Insert { .. } => {}
        Prim::Hatch(hatch) => {
            for prim in hatch.prims(drift_draft::page::HATCH_LIMIT) {
                let part = Item {
                    prim,
                    ..item.clone()
                };
                item_svg(out, &part, h, lts, font);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use drift_core::{Bounds, DVec2, LineType};
    use drift_draft::page::Pen;

    /// A red line, a blue fill, green text and a mask, in a sheet's colours.
    fn coloured(colours: drift_core::SheetColours) -> Drawing {
        use drift_core::Rgb;
        let mut d = Drawing::new(DVec2::new(100.0, 100.0));
        d.layer(drift_draft::page::LayerInfo {
            name: "Walls".into(),
            color: Rgb([255, 0, 0]),
            width: 0.35,
            line_type: drift_core::LineType::Continuous,
        });
        let pen = |c| Pen::new(0.35).with_color(Rgb(c));
        d.line("Walls", pen([255, 0, 0]), DVec2::ZERO, DVec2::splat(50.0));
        d.filled_rect("Walls", pen([0, 0, 255]), DVec2::ZERO, DVec2::splat(20.0));
        d.text(
            "Walls",
            pen([0, 200, 0]),
            DVec2::splat(30.0),
            3.5,
            Align::Left,
            "Ø6",
        );
        d.push(
            "Walls",
            pen([0, 0, 255]),
            Prim::Fill {
                rings: vec![vec![DVec2::ZERO, DVec2::X, DVec2::ONE]],
                triangles: vec![[DVec2::ZERO, DVec2::X, DVec2::ONE]],
                mask: true,
            },
        );
        d.set_colours(colours);
        d
    }

    #[test]
    fn writes_a_sheets_colours() {
        let colour = write(&coloured(drift_core::SheetColours::Colour));
        assert!(colour.contains(r##"stroke="#ff0000""##));
        assert!(colour.contains(r##"fill="#0000ff""##) && colour.contains(r##"fill="#00c800""##));
        let grey = write(&coloured(drift_core::SheetColours::Greyscale));
        // Red's grey for the line, blue's for the fill, green's for the text.
        assert!(grey.contains(r##"stroke="#4c4c4c""##), "{grey}");
        assert!(grey.contains(r##"fill="#1d1d1d""##), "{grey}");
        assert!(grey.contains(r##"fill="#757575""##), "{grey}");
        let mono = write(&coloured(drift_core::SheetColours::BlackWhite));
        assert!(mono.contains(r##"stroke="#000000""##) && !mono.contains("#4c4c4c"));
        // Text black, the fill its grey, the mask white.
        assert!(mono.contains(r##"fill="#000000""##) && mono.contains(r##"fill="#1d1d1d""##));
        assert!(mono.contains(r##"fill="#ffffff""##), "{mono}");
        for svg in [&grey, &mono] {
            assert!(!svg.contains("#ff0000") && !svg.contains("#0000ff"));
        }
    }

    #[test]
    fn writes_true_scale_svg() {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        let window = Some(Bounds {
            min: DVec2::splat(10.0),
            max: DVec2::new(287.0, 200.0),
        });
        d.line("frame", Pen::new(0.7), DVec2::ZERO, DVec2::new(10.0, 0.0));
        d.items.push(drift_draft::page::Item {
            layer: "Walls & doors".into(),
            pen: Pen::new(0.35).with_type(LineType::Center),
            prim: Prim::Arc {
                center: DVec2::new(50.0, 50.0),
                radius: 10.0,
                start: 0.0,
                sweep: std::f64::consts::PI * 1.5,
            },
            clip: window,
        });
        d.text(
            "notes",
            Pen::new(0.25),
            DVec2::new(20.0, 20.0),
            3.5,
            Align::Center,
            "Ø6 <THRU>",
        );
        let svg = write(&d);
        assert!(svg.contains(r#"width="297mm" height="210mm" viewBox="0 0 297 210""#));
        assert!(svg.contains(r#"<path d="M 0 210 L 10 210""#), "{svg}");
        // Arc from (60, 50) to (50, 40) the long way round, y flipped.
        assert!(svg.contains(r#"M 60 160 A 10 10 0 1 0 50 170"#), "{svg}");
        assert!(svg.contains("stroke-dasharray=\"12 1.5 1.5 1.5\""));
        assert!(svg.contains(r#"clip-path="url(#view1)""#));
        assert!(svg.contains("data-layer=\"Walls &amp; doors\""));
        assert!(svg.contains("Ø6 &lt;THRU&gt;"));
        assert!(svg.contains(r#"text-anchor="middle""#));
    }

    /// Model space in world mm: a closed outline on one layer, a half circle on another.
    pub(crate) fn model() -> Drawing {
        let mut d = Drawing::new(DVec2::ZERO);
        let pen = Pen::new(0.35);
        let v = DVec2::new;
        let corners = [
            v(-50.0, -20.0),
            v(50.0, -20.0),
            v(50.0, 20.0),
            v(-50.0, 20.0),
        ];
        for k in 0..4 {
            d.line("Profile", pen, corners[k], corners[(k + 1) % 4]);
        }
        // Its top reaches 30 (the ends are at ±10, 20): the bounds follow the arc.
        d.push(
            "Holes",
            pen,
            Prim::Arc {
                center: v(0.0, 20.0),
                radius: 10.0,
                start: 0.0,
                sweep: std::f64::consts::PI,
            },
        );
        d
    }

    #[test]
    fn the_model_on_a_page_fitted_to_it() {
        let page = model().fitted(5.0);
        // 100 × 50 of geometry and 5 mm all round.
        assert_eq!(page.size, DVec2::new(110.0, 60.0));
        let svg = write(&page);
        assert!(
            svg.contains(r#"width="110mm" height="60mm" viewBox="0 0 110 60""#),
            "{svg}"
        );
        // The outline is one closed path from (5, 5) on the page, y flipped.
        assert!(
            svg.contains(r#"d="M 5 55 L 105 55 L 105 15 L 5 15 L 5 55 Z""#),
            "{svg}"
        );
        // A group per layer, in the order drawn.
        let profile = svg.find(r#"inkscape:label="Profile""#).expect("Profile");
        let holes = svg.find(r#"inkscape:label="Holes""#).expect("Holes");
        assert!(profile < holes);
        assert_eq!(svg.matches("<g ").count(), 2, "{svg}");
        // Nothing drawn: an empty page.
        assert_eq!(Drawing::new(DVec2::ZERO).fitted(5.0).size, DVec2::ZERO);
    }

    #[test]
    fn ellipses_are_exact_arcs() {
        use drift_core::DVec2;
        let mut d = Drawing::new(DVec2::new(120.0, 80.0));
        let pen = drift_draft::page::Pen::new(0.35);
        d.push(
            "0",
            pen,
            Prim::Ellipse {
                center: DVec2::new(40.0, 40.0),
                major: DVec2::new(25.0, 10.0),
                minor: 8.0,
                start: 0.0,
                sweep: std::f64::consts::TAU,
            },
        );
        d.push(
            "0",
            pen,
            Prim::Ellipse {
                center: DVec2::new(90.0, 40.0),
                major: DVec2::new(0.0, 20.0),
                minor: 10.0,
                start: 0.5,
                sweep: 2.5,
            },
        );
        let svg = write(&d);
        assert_eq!(svg.matches(" A ").count(), 3, "{svg}");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("drift_ellipses.svg"), &svg).unwrap();
        std::fs::write(
            dir.join("drift_ellipses.pdf"),
            crate::pdf::write(&[d], "Ellipses"),
        )
        .unwrap();
    }

    #[test]
    fn strokes_that_meet_are_one_path_in_a_group_per_layer() {
        let mut d = Drawing::new(DVec2::new(100.0, 100.0));
        let pen = Pen::new(0.5);
        let v = DVec2::new;
        // A square drawn as four lines, one of them backwards, and a rounded end.
        d.line("Walls 1", pen, v(0.0, 0.0), v(10.0, 0.0));
        d.line("Walls 1", pen, v(10.0, 10.0), v(10.0, 0.0));
        d.line("Walls 1", pen, v(10.0, 10.0), v(0.0, 10.0));
        d.line("Walls 1", pen, v(0.0, 10.0), v(0.0, 0.0));
        d.push(
            "2nd floor",
            pen,
            Prim::Arc {
                center: v(50.0, 50.0),
                radius: 5.0,
                start: 0.0,
                sweep: std::f64::consts::PI,
            },
        );
        d.line("2nd floor", pen, v(45.0, 50.0), v(45.0, 30.0));
        // Another pen isn't joined to them.
        d.line("2nd floor", Pen::new(0.25), v(45.0, 30.0), v(55.0, 30.0));
        let svg = write(&d);
        assert_eq!(svg.matches("<path").count(), 3, "{svg}");
        assert!(
            svg.contains(r#"d="M 0 100 L 10 100 L 10 90 L 0 90 L 0 100 Z""#),
            "{svg}"
        );
        assert!(
            svg.contains(r#"d="M 55 50 A 5 5 0 0 0 45 50 L 45 70""#),
            "{svg}"
        );
        // A named group per layer, as Illustrator and Inkscape read them.
        assert!(
            svg.contains(r#"<g id="Walls_1" data-name="Walls 1""#),
            "{svg}"
        );
        assert!(svg.contains(r#"id="_x32_nd_floor""#), "{svg}");
        assert!(svg.contains(r#"inkscape:groupmode="layer" inkscape:label="2nd floor""#));
        assert!(
            !svg.contains("fill=\"white\""),
            "no background to get in the way"
        );

        // Read back: its layers, and the arc still an arc.
        let art = crate::svg_import::read(&svg, "drawing").unwrap();
        assert_eq!(art.layers, ["Walls 1", "2nd floor"]);
        let shapes = crate::vector::shapes_of(&art.paths[1].subpaths[0]);
        assert!(
            matches!(shapes[0], drift_core::Shape::Arc { radius, sweep, .. }
                if (radius - 5.0).abs() < 1e-6 && (sweep - std::f64::consts::PI).abs() < 1e-6),
            "{shapes:?}"
        );
        assert!((art.paths[0].stroke.as_ref().unwrap().width - 0.5).abs() < 1e-9);
    }
}
