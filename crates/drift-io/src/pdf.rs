//! PDF output of [`Drawing`]s: one true-scale page each (MediaBox in points from paper mm),
//! vector paths with cubic Bézier arcs, even-odd fills, and Helvetica text (WinAnsi). No
//! dependencies; streams are written uncompressed.

use std::fmt::Write as _;

use drift_core::DVec2;
use drift_draft::page::{Align, CAP_HEIGHT, Drawing, Item, Prim, dash_pattern, text_width};

const PT_PER_MM: f64 = 72.0 / 25.4;

fn n(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_owned() }
}

fn pt(p: DVec2) -> String {
    format!("{} {}", n(p.x * PT_PER_MM), n(p.y * PT_PER_MM))
}

/// Text as a PDF string in WinAnsiEncoding (characters it lacks become close equivalents or
/// `?`).
fn pdf_string(s: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in s.chars() {
        let byte = match c {
            '(' | ')' | '\\' => {
                out.push(b'\\');
                c as u8
            }
            ' '..='~' => c as u8,
            '\u{a0}'..='\u{ff}' => c as u32 as u8,
            '−' | '–' => b'-',
            '—' => 0x97,
            '•' => 0x95,
            '…' => 0x85,
            'Σ' => b'S',
            _ => b'?',
        };
        out.push(byte);
    }
    out.push(b')');
    out
}

/// Cubic Bézier segments for an arc (at most 90° each).
fn arc_path(out: &mut String, center: DVec2, r: f64, start: f64, sweep: f64, move_to: bool) {
    let pieces = ((sweep.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize).max(1);
    let step = sweep / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |a: f64| center + DVec2::from_angle(a) * r;
    let tangent = |a: f64| DVec2::from_angle(a).perp() * r;
    if move_to {
        let _ = writeln!(out, "{} m", pt(at(start)));
    }
    for i in 0..pieces {
        let a0 = start + step * i as f64;
        let a1 = a0 + step;
        let c1 = at(a0) + tangent(a0) * k;
        let c2 = at(a1) - tangent(a1) * k;
        let _ = writeln!(out, "{} {} {} c", pt(c1), pt(c2), pt(at(a1)));
    }
}

fn item_ops(out: &mut String, text: &mut Vec<u8>, item: &Item, lts: f64) {
    let pen = &item.pen;
    let [r, g, b] = pen.color.0.map(|v| f64::from(v) / 255.0);
    let stroke_setup = |out: &mut String| {
        let _ = writeln!(out, "{} {} {} RG", n(r), n(g), n(b));
        let _ = writeln!(out, "{} w", n(pen.width.max(0.05) * PT_PER_MM));
        let dashes: Vec<String> = dash_pattern(pen.line_type)
            .iter()
            .map(|v| n(v * lts * PT_PER_MM))
            .collect();
        let _ = writeln!(out, "[{}] 0 d", dashes.join(" "));
    };
    match &item.prim {
        Prim::Line(a, b) => {
            stroke_setup(out);
            let _ = writeln!(out, "{} m {} l S", pt(*a), pt(*b));
        }
        Prim::Polyline { points, closed } => {
            if points.is_empty() {
                return;
            }
            stroke_setup(out);
            let _ = writeln!(out, "{} m", pt(points[0]));
            for p in &points[1..] {
                let _ = writeln!(out, "{} l", pt(*p));
            }
            out.push_str(if *closed { "s\n" } else { "S\n" });
        }
        Prim::Circle { center, radius } => {
            stroke_setup(out);
            arc_path(out, *center, *radius, 0.0, std::f64::consts::TAU, true);
            out.push_str("s\n");
        }
        Prim::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            stroke_setup(out);
            arc_path(out, *center, *radius, *start, *sweep, true);
            out.push_str("S\n");
        }
        Prim::Ellipse { sweep, .. } => {
            stroke_setup(out);
            let pts = drift_draft::page::prim_points(&item.prim);
            for (k, p) in pts.iter().enumerate() {
                let _ = writeln!(out, "{} {}", pt(*p), if k == 0 { "m" } else { "l" });
            }
            out.push_str(if *sweep >= std::f64::consts::TAU - 1e-12 {
                "s\n"
            } else {
                "S\n"
            });
        }
        Prim::Bezier { p } => {
            stroke_setup(out);
            let _ = writeln!(out, "{} m", pt(p[0]));
            let _ = writeln!(out, "{} {} {} c", pt(p[1]), pt(p[2]), pt(p[3]));
            out.push_str("S\n");
        }
        Prim::Fill { rings, mask, .. } => {
            let (r, g, b) = if *mask { (1.0, 1.0, 1.0) } else { (r, g, b) };
            let _ = writeln!(out, "{} {} {} rg", n(r), n(g), n(b));
            for ring in rings {
                for (k, p) in ring.iter().enumerate() {
                    let _ = writeln!(out, "{} {}", pt(*p), if k == 0 { "m" } else { "l" });
                }
                out.push_str("h\n");
            }
            out.push_str("f*\n");
        }
        Prim::Text {
            at,
            height,
            angle,
            align,
            text: s,
        } => {
            let width = text_width(s, *height);
            let shift = match align {
                Align::Left => 0.0,
                Align::Center => width / 2.0,
                Align::Right => width,
            };
            let dir = DVec2::from_angle(*angle);
            let origin = *at - dir * shift;
            let size = height / CAP_HEIGHT * PT_PER_MM;
            let (c, s_) = (angle.cos(), angle.sin());
            // Text goes straight into the byte stream (it may hold non-UTF-8 bytes).
            text.extend_from_slice(out.as_bytes());
            out.clear();
            text.extend_from_slice(
                format!(
                    "BT\n{} {} {} rg\n/F1 {} Tf\n{} {} {} {} {} Tm\n",
                    n(r),
                    n(g),
                    n(b),
                    n(size),
                    n(c),
                    n(s_),
                    n(-s_),
                    n(c),
                    pt(origin)
                )
                .as_bytes(),
            );
            text.extend_from_slice(&pdf_string(s));
            text.extend_from_slice(b" Tj\nET\n");
        }
        Prim::Insert { .. } => {}
    }
}

/// The content stream of one page.
fn page_content(d: &Drawing) -> Vec<u8> {
    let d = &d.exploded();
    let mut bytes = Vec::new();
    let mut out = String::from("1 J 1 j\n");
    let mut clipping = false;
    for item in &d.items {
        let clip = item.clipped && d.clip.is_some();
        if clip != clipping {
            if clipping {
                out.push_str("Q\n");
            } else if let Some(c) = d.clip {
                let size = c.max - c.min;
                let _ = writeln!(
                    out,
                    "q {} {} {} re W n",
                    pt(c.min),
                    n(size.x * PT_PER_MM),
                    n(size.y * PT_PER_MM)
                );
            }
            clipping = clip;
        }
        item_ops(&mut out, &mut bytes, item, d.line_type_scale);
    }
    if clipping {
        out.push_str("Q\n");
    }
    bytes.extend_from_slice(out.as_bytes());
    bytes
}

/// Write drawings as a PDF, one page each.
pub fn write(pages: &[Drawing], title: &str) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1: catalog, 2: pages, 3: font, 4: info; then per page: page, content.
    let page_ids: Vec<usize> = (0..pages.len()).map(|i| 5 + 2 * i).collect();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = page_ids.iter().map(|id| format!("{id} 0 R")).collect();
    objects.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .into_bytes(),
    );
    objects.push(
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    );
    let mut info = b"<< /Producer (Drift) /Title ".to_vec();
    info.extend_from_slice(&pdf_string(title));
    info.extend_from_slice(b" >>");
    objects.push(info);
    for (i, d) in pages.iter().enumerate() {
        let content_id = page_ids[i] + 1;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {content_id} 0 R \
                 /Resources << /Font << /F1 3 0 R >> >> >>",
                n(d.size.x * PT_PER_MM),
                n(d.size.y * PT_PER_MM)
            )
            .into_bytes(),
        );
        let content = page_content(d);
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(&content);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }
    let mut out = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, obj) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(obj);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use drift_core::Bounds;
    use drift_draft::page::Pen;

    #[test]
    fn writes_a_valid_true_scale_pdf() {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        d.clip = Some(Bounds {
            min: DVec2::splat(10.0),
            max: DVec2::new(287.0, 200.0),
        });
        d.line("frame", Pen::new(0.7), DVec2::ZERO, DVec2::new(25.4, 0.0));
        d.items.push(Item {
            layer: "geometry".into(),
            pen: Pen::new(0.35),
            prim: Prim::Circle {
                center: DVec2::new(50.0, 50.0),
                radius: 10.0,
            },
            clipped: true,
        });
        d.text(
            "notes",
            Pen::new(0.25),
            DVec2::new(20.0, 20.0),
            3.5,
            Align::Center,
            "Ø6 (THRU) ±0.1 − Σ",
        );
        let pdf = write(&[d.clone(), d], "Plate");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(text.contains("/MediaBox [0 0 841.89 595.276]"));
        assert!(text.contains("/Count 2"));
        // 25.4 mm is exactly 72 pt.
        assert!(text.contains("0 0 m 72 0 l S"));
        assert!(text.contains("re W n"));
        // Escaped parentheses and WinAnsi bytes for Ø (0xD8) and ± (0xB1).
        let needle = b"(\xd86 \\(THRU\\) \xb10.1 - S) Tj";
        assert!(pdf.windows(needle.len()).any(|w| w == needle));
        // The xref offsets point at the objects (byte offsets: the file isn't UTF-8).
        let find = |needle: &[u8]| pdf.windows(needle.len()).rposition(|w| w == needle);
        let tail = find(b"startxref\n").unwrap() + b"startxref\n".len();
        let line = |at: usize| {
            let end = pdf[at..].iter().position(|b| *b == b'\n').unwrap() + at;
            String::from_utf8(pdf[at..end].to_vec()).unwrap()
        };
        let xref_at: usize = line(tail).parse().unwrap();
        assert!(pdf[xref_at..].starts_with(b"xref"));
        for k in 1..=7 {
            // Entry k starts 20 bytes per entry after "xref\n0 N\n" and the free entry.
            let header = line(xref_at + 5).len() + 1;
            let entry = xref_at + 5 + header + 20 * k;
            let offset: usize = line(entry)[..10].parse().unwrap();
            assert!(
                pdf[offset..].starts_with(format!("{k} 0 obj").as_bytes()),
                "object {k}"
            );
        }
    }
}
