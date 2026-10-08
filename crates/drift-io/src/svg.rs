//! SVG output of a [`Drawing`]: true-scale (`mm` units), one group per layer run, true arcs,
//! even-odd fills, and text in Helvetica / Arial.

use std::fmt::Write as _;

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
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}mm" height="{}mm" viewBox="0 0 {} {}">"#,
        num(w),
        num(h),
        num(w),
        num(h)
    );
    let _ = writeln!(out, "<title>Drift drawing</title>");
    if let Some(c) = d.clip {
        let _ = writeln!(
            out,
            r#"<defs><clipPath id="view"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath></defs>"#,
            num(c.min.x),
            num(y(c.max.y)),
            num(c.max.x - c.min.x),
            num(c.max.y - c.min.y)
        );
    }
    let _ = writeln!(
        out,
        r#"<rect width="{}" height="{}" fill="white"/>"#,
        num(w),
        num(h)
    );
    // Consecutive items on the same layer (and clip) share a group.
    let mut open: Option<(&str, bool)> = None;
    for item in &d.items {
        let key = (item.layer.as_str(), item.clipped && d.clip.is_some());
        if open != Some(key) {
            if open.is_some() {
                out.push_str("</g>\n");
            }
            let clip = if key.1 {
                r#" clip-path="url(#view)""#
            } else {
                ""
            };
            let _ = writeln!(
                out,
                r#"<g data-layer="{}" fill="none" stroke-linecap="round" stroke-linejoin="round"{clip}>"#,
                escape(key.0)
            );
            open = Some(key);
        }
        item_svg(&mut out, item, h, d.line_type_scale);
    }
    if open.is_some() {
        out.push_str("</g>\n");
    }
    out.push_str("</svg>\n");
    out
}

fn item_svg(out: &mut String, item: &Item, h: f64, lts: f64) {
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
        } => {
            let [r, g, b] = pen.color.0;
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
                r##"<text x="{}" y="{}" font-family="Helvetica, Arial, sans-serif" font-size="{}" fill="#{r:02x}{g:02x}{b:02x}" stroke="none" text-anchor="{anchor}"{rotate}>{}</text>"##,
                num(at.x),
                num(y(at.y)),
                num(height / CAP_HEIGHT),
                escape(text)
            );
        }
        Prim::Insert { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::{Bounds, DVec2, LineType};
    use drift_draft::page::Pen;

    #[test]
    fn writes_true_scale_svg() {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        d.clip = Some(Bounds {
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
            clipped: true,
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
        assert!(svg.contains(r#"<line x1="0" y1="210" x2="10" y2="210""#));
        // Arc from (60, 50) to (50, 40) the long way round, y flipped.
        assert!(svg.contains(r#"M 60 160 A 10 10 0 1 0 50 170"#), "{svg}");
        assert!(svg.contains("stroke-dasharray=\"12 1.5 1.5 1.5\""));
        assert!(svg.contains(r#"clip-path="url(#view)""#));
        assert!(svg.contains("data-layer=\"Walls &amp; doors\""));
        assert!(svg.contains("Ø6 &lt;THRU&gt;"));
        assert!(svg.contains(r#"text-anchor="middle""#));
    }
}
