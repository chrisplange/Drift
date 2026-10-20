//! PDF output of [`Drawing`]s: one true-scale page each (MediaBox in points from paper mm),
//! vector paths with cubic Bézier arcs, even-odd fills, and text.
//!
//! Text is Helvetica (WinAnsi) unless the drawing has its own font. That font is embedded
//! whole (compressed), with its text written by glyph and a map back to Unicode, so it
//! looks right anywhere and can still be selected and searched. A font that can't be
//! embedded (its licence forbids it, or it comes from a font collection) has its text drawn
//! as outlines instead.

use std::fmt::Write as _;

use std::collections::BTreeMap;
use std::rc::Rc;

use drift_core::{DVec2, Shape};
use drift_draft::font::{DrawingFont, Face};
use drift_draft::page::{Align, CAP_HEIGHT, Drawing, ITALIC_SLANT, Item, Prim, dash_pattern};

/// The drawing's font as a PDF uses it, and the glyphs used so far in each weight: glyph id
/// to its advance (ems) and the character it stands for.
struct Fonts {
    font: Rc<DrawingFont>,
    /// Embedded as fonts; otherwise text is drawn as outlines.
    embed: bool,
    used: [BTreeMap<u16, (f64, char)>; 2],
}

impl Fonts {
    fn of(pages: &[Drawing]) -> Option<Fonts> {
        let font = pages.iter().find_map(|d| d.font.clone())?;
        let embed = font.regular.embeddable() && font.bold.as_ref().is_none_or(Face::embeddable);
        Some(Fonts {
            font,
            embed,
            used: Default::default(),
        })
    }
}

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
    let cubics =
        drift_core::shape::arc_cubics(center, r, start, sweep, std::f64::consts::FRAC_PI_2);
    if move_to {
        let _ = writeln!(out, "{} m", pt(cubics[0][0]));
    }
    for [_, c1, c2, end] in cubics {
        let _ = writeln!(out, "{} {} {} c", pt(c1), pt(c2), pt(end));
    }
}

fn item_ops(
    out: &mut String,
    text: &mut Vec<u8>,
    item: &Item,
    lts: f64,
    fonts: Option<&mut Fonts>,
) {
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
            style,
            font: sketch,
        } => {
            let width = drift_draft::page::line_width_in(s, *height, *style, sketch.as_ref());
            let shift = match align {
                Align::Left => 0.0,
                Align::Center => width / 2.0,
                Align::Right => width,
            };
            let dir = DVec2::from_angle(*angle);
            let origin = *at - dir * shift;
            // Sketch text is written as text in the drawing's font when that is its font and
            // the PDF carries it; in any other font, its glyphs are filled (a PDF can only
            // write text in a font it carries).
            if let Some(f) = sketch {
                let carried = match (f, fonts.as_deref()) {
                    (drift_core::TextFont::System(family), Some(fonts)) => {
                        fonts.embed && fonts.font.regular.family == *family
                    }
                    _ => false,
                };
                if !carried {
                    let contours = drift_draft::outline::text_contours(
                        s,
                        f,
                        *height,
                        origin,
                        angle.to_degrees(),
                    );
                    let _ = writeln!(out, "{} {} {} rg", n(r), n(g), n(b));
                    for ring in drift_draft::outline::contour_rings(&contours) {
                        for (k, p) in ring.iter().enumerate() {
                            let _ = writeln!(out, "{} {}", pt(*p), if k == 0 { "m" } else { "l" });
                        }
                        out.push_str("h\n");
                    }
                    out.push_str("f\n");
                    return;
                }
            }
            if let Some(fonts) = fonts {
                let font = fonts.font.clone();
                let face = font.face(style.bold);
                let size = height / face.cap_ratio() * PT_PER_MM;
                if !fonts.embed {
                    // Its outlines, filled.
                    let contours = drift_draft::outline::contours_in(
                        &face.data,
                        face.index,
                        s,
                        *height,
                        origin,
                        angle.to_degrees(),
                        style.tracking,
                    );
                    let _ = writeln!(out, "{} {} {} rg", n(r), n(g), n(b));
                    for contour in &contours {
                        let mut first = true;
                        for piece in contour {
                            match *piece {
                                Shape::Line { a, b } => {
                                    if first {
                                        let _ = writeln!(out, "{} m", pt(a));
                                    }
                                    let _ = writeln!(out, "{} l", pt(b));
                                }
                                Shape::Bezier { p } => {
                                    if first {
                                        let _ = writeln!(out, "{} m", pt(p[0]));
                                    }
                                    let _ =
                                        writeln!(out, "{} {} {} c", pt(p[1]), pt(p[2]), pt(p[3]));
                                }
                                _ => {}
                            }
                            first = false;
                        }
                        out.push_str("h\n");
                    }
                    out.push_str("f\n");
                    return;
                }
                // By glyph, in the embedded font.
                let slot = usize::from(style.bold && font.bold.is_some());
                let glyphs = face.glyphs(s);
                let mut hex = String::with_capacity(glyphs.len() * 4 + 2);
                hex.push('<');
                for ((gid, advance), ch) in glyphs.iter().zip(s.chars()) {
                    let _ = write!(hex, "{gid:04X}");
                    fonts.used[slot].entry(*gid).or_insert((*advance, ch));
                }
                hex.push('>');
                let (c, s_) = (angle.cos(), angle.sin());
                let lean = if style.italic { ITALIC_SLANT } else { 0.0 };
                let _ = writeln!(
                    out,
                    "BT\n{} {} {} rg\n/F{} {} Tf\n{} Tc\n{} {} {} {} {} Tm",
                    n(r),
                    n(g),
                    n(b),
                    slot + 1,
                    n(size),
                    n(style.tracking * size),
                    n(c),
                    n(s_),
                    n(-s_ + lean * c),
                    n(c + lean * s_),
                    pt(origin)
                );
                let _ = writeln!(out, "{hex} Tj");
                out.push_str("ET\n");
                return;
            }
            let size = height / CAP_HEIGHT * PT_PER_MM;
            let (c, s_) = (angle.cos(), angle.sin());
            let lean = if style.italic { ITALIC_SLANT } else { 0.0 };
            // Text goes straight into the byte stream (it may hold non-UTF-8 bytes).
            text.extend_from_slice(out.as_bytes());
            out.clear();
            text.extend_from_slice(
                format!(
                    "BT\n{} {} {} rg\n/{} {} Tf\n{} Tc\n{} {} {} {} {} Tm\n",
                    n(r),
                    n(g),
                    n(b),
                    if style.bold { "F2" } else { "F1" },
                    n(size),
                    n(style.tracking * size),
                    n(c),
                    n(s_),
                    n(-s_ + lean * c),
                    n(c + lean * s_),
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
fn page_content(d: &Drawing, mut fonts: Option<&mut Fonts>) -> Vec<u8> {
    let d = &d.exploded();
    let mut bytes = Vec::new();
    let mut out = String::from("1 J 1 j\n");
    // Runs of items seen through the same window share a clip.
    let mut clipping: Option<drift_core::Bounds> = None;
    for item in &d.items {
        if item.clip != clipping {
            if clipping.is_some() {
                out.push_str("Q\n");
            }
            if let Some(c) = item.clip {
                let size = c.max - c.min;
                let _ = writeln!(
                    out,
                    "q {} {} {} re W n",
                    pt(c.min),
                    n(size.x * PT_PER_MM),
                    n(size.y * PT_PER_MM)
                );
            }
            clipping = item.clip;
        }
        item_ops(
            &mut out,
            &mut bytes,
            item,
            d.line_type_scale,
            fonts.as_deref_mut(),
        );
    }
    if clipping.is_some() {
        out.push_str("Q\n");
    }
    bytes.extend_from_slice(out.as_bytes());
    bytes
}

/// A stream object, compressed.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let packed = miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
    let mut out = format!(
        "<< {dict} /Filter /FlateDecode /Length {} >>\nstream\n",
        packed.len()
    )
    .into_bytes();
    out.extend_from_slice(&packed);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A name PDF accepts for a font: its family without spaces or odd characters.
fn font_name(face: &Face, bold: bool) -> String {
    let base: String = face
        .family
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    let base = if base.is_empty() {
        "Font".to_owned()
    } else {
        base
    };
    if bold { format!("{base}-Bold") } else { base }
}

/// The font dictionary a page's text names (`/F1`, `/F2`): a composite font whose codes are
/// glyph ids, pointing at the objects [`font_objects`] writes from `first` on.
fn type0(face: &Face, bold: bool, first: usize) -> Vec<u8> {
    format!(
        "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H \
         /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>",
        font_name(face, bold),
        first,
        first + 3
    )
    .into_bytes()
}

/// The four objects behind an embedded face, numbered from `first`: the font by glyph id
/// with the widths of the glyphs used, its descriptor, the font file, and the map from
/// glyph back to character.
fn font_objects(
    face: &Face,
    bold: bool,
    first: usize,
    used: &BTreeMap<u16, (f64, char)>,
) -> Vec<Vec<u8>> {
    let name = font_name(face, bold);
    let widths: String = used
        .iter()
        .map(|(gid, (advance, _))| format!("{gid} [{}]", n(advance * 1000.0)))
        .collect::<Vec<_>>()
        .join(" ");
    let (kind, map) = if face.cff {
        ("CIDFontType0", "")
    } else {
        ("CIDFontType2", " /CIDToGIDMap /Identity")
    };
    let descendant = format!(
        "<< /Type /Font /Subtype /{kind} /BaseFont /{name} \
         /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
         /FontDescriptor {} 0 R /DW 500 /W [{widths}]{map} >>",
        first + 1
    );
    let (ascent, descent) = face.extent();
    let file_key = if face.cff { "FontFile3" } else { "FontFile2" };
    let descriptor = format!(
        "<< /Type /FontDescriptor /FontName /{name} /Flags 4 \
         /FontBBox [-1000 {} 2000 {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} \
         /StemV {} /{file_key} {} 0 R >>",
        n(-descent * 1000.0 - 200.0),
        n(ascent * 1000.0 + 200.0),
        n(ascent * 1000.0),
        n(-descent * 1000.0),
        n(face.cap_ratio() * 1000.0),
        if bold { 140 } else { 80 },
        first + 2
    );
    let file = if face.cff {
        stream("/Subtype /OpenType", &face.data)
    } else {
        stream(&format!("/Length1 {}", face.data.len()), &face.data)
    };
    // Glyph ids back to text, so the PDF can be searched and copied from.
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let pairs: Vec<(u16, char)> = used.iter().map(|(gid, (_, ch))| (*gid, *ch)).collect();
    for block in pairs.chunks(100) {
        let _ = writeln!(cmap, "{} beginbfchar", block.len());
        for (gid, ch) in block {
            let mut units = [0u16; 2];
            let code: String = ch
                .encode_utf16(&mut units)
                .iter()
                .map(|u| format!("{u:04X}"))
                .collect();
            let _ = writeln!(cmap, "<{gid:04X}> <{code}>");
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    vec![
        descendant.into_bytes(),
        descriptor.into_bytes(),
        file,
        stream("", cmap.as_bytes()),
    ]
}

/// Write drawings as a PDF, one page each.
pub fn write(pages: &[Drawing], title: &str) -> Vec<u8> {
    let mut fonts = Fonts::of(pages);
    let embedded = fonts.as_ref().filter(|f| f.embed).map(|f| f.font.clone());
    // The objects behind the embedded faces come after the pages.
    let after_pages = 6 + 2 * pages.len();
    let mut objects: Vec<Vec<u8>> = Vec::new();
    // 1: catalog, 2: pages, 3: font, 4: info, 5: bold font; then per page: page, content.
    let page_ids: Vec<usize> = (0..pages.len()).map(|i| 6 + 2 * i).collect();
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
    objects.push(match &embedded {
        Some(font) => type0(&font.regular, false, after_pages),
        None => {
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec()
        }
    });
    let mut info = b"<< /Producer (Drift) /Title ".to_vec();
    info.extend_from_slice(&pdf_string(title));
    info.extend_from_slice(b" >>");
    objects.push(info);
    objects.push(match &embedded {
        // Without a bold face, bold text is set in the regular face.
        Some(font) => match &font.bold {
            Some(bold) => type0(bold, true, after_pages + 4),
            None => type0(&font.regular, false, after_pages),
        },
        None => b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_vec(),
    });
    for (i, d) in pages.iter().enumerate() {
        let content_id = page_ids[i] + 1;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {content_id} 0 R \
                 /Resources << /Font << /F1 3 0 R /F2 5 0 R >> >> >>",
                n(d.size.x * PT_PER_MM),
                n(d.size.y * PT_PER_MM)
            )
            .into_bytes(),
        );
        let content = page_content(d, fonts.as_mut());
        let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        stream.extend_from_slice(&content);
        stream.extend_from_slice(b"\nendstream");
        objects.push(stream);
    }
    if let (Some(font), Some(fonts)) = (&embedded, &fonts) {
        objects.extend(font_objects(
            &font.regular,
            false,
            after_pages,
            &fonts.used[0],
        ));
        if let Some(bold) = &font.bold {
            objects.extend(font_objects(bold, true, after_pages + 4, &fonts.used[1]));
        }
    }
    // OpenType font files in a PDF need version 1.6.
    let mut out = if embedded.is_some() {
        b"%PDF-1.6\n%\xe2\xe3\xcf\xd3\n".to_vec()
    } else {
        b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec()
    };
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
        let content = |colours| {
            let pdf = write(&[coloured(colours)], "Colours");
            let parsed = lopdf::Document::load_mem(&pdf).expect("a valid PDF");
            let page = *parsed.get_pages().values().next().expect("a page");
            String::from_utf8_lossy(&parsed.get_page_content(page)).into_owned()
        };
        let colour = content(drift_core::SheetColours::Colour);
        assert!(
            colour.contains("1 0 0 RG") && colour.contains("0 0 1 rg"),
            "{colour}"
        );
        // Greyscale: red's grey (76) for the line, blue's (29) for the fill.
        let grey = content(drift_core::SheetColours::Greyscale);
        let g = |v: u8| n(f64::from(v) / 255.0);
        assert!(grey.contains(&format!("{0} {0} {0} RG", g(76))), "{grey}");
        assert!(grey.contains(&format!("{0} {0} {0} rg", g(29))), "{grey}");
        assert!(!grey.contains("1 0 0 RG") && !grey.contains("0 0 1 rg"));
        // Black & white: black lines and text, the fill grey, the mask white.
        let mono = content(drift_core::SheetColours::BlackWhite);
        assert!(mono.contains("0 0 0 RG") && !mono.contains(&format!("{0} {0} {0} RG", g(76))));
        assert!(mono.contains(&format!("{0} {0} {0} rg", g(29))), "{mono}");
        assert!(
            mono.contains("0 0 0 rg") && mono.contains("1 1 1 rg"),
            "{mono}"
        );
    }

    #[test]
    fn the_model_on_a_page_fitted_to_it() {
        let page = crate::svg::tests::model().fitted(5.0);
        let pdf = write(&[page], "Model");
        let parsed = lopdf::Document::load_mem(&pdf).expect("a valid PDF");
        assert_eq!(parsed.get_pages().len(), 1);
        // 110 × 60 mm in points.
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox [0 0 311.811 170.079]"), "{text}");
        let page = *parsed.get_pages().values().next().expect("a page");
        let content = String::from_utf8_lossy(&parsed.get_page_content(page)).into_owned();
        // The outline's first corner at (5, 5) mm.
        assert!(content.contains("14.173 14.173 m"), "{content}");
    }

    #[test]
    fn writes_a_valid_true_scale_pdf() {
        let mut d = Drawing::new(DVec2::new(297.0, 210.0));
        let window = Some(Bounds {
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
            clip: window,
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

#[cfg(test)]
mod font_tests {
    use super::*;
    use drift_draft::page::{Pen, TextStyle};

    fn page(font: Option<Rc<DrawingFont>>) -> Drawing {
        // Measured in the same font it is written in, as the app does.
        drift_draft::font::set_drawing_font(font.clone());
        let mut d = Drawing::new(DVec2::new(210.0, 148.0));
        d.font = font;
        d.styled_text(
            "notes",
            Pen::new(0.25),
            DVec2::new(20.0, 100.0),
            5.0,
            Align::Left,
            TextStyle::REGULAR,
            "Mounting plate Ø12",
        );
        d.styled_text(
            "notes",
            Pen::new(0.25),
            DVec2::new(190.0, 80.0),
            3.0,
            Align::Right,
            TextStyle::BOLD.tracked(0.1),
            "SHEET 01",
        );
        drift_draft::font::set_drawing_font(None);
        d
    }

    fn ubuntu() -> Face {
        Face::new(drift_draft::outline::builtin_font(false), 0).expect("the built-in sans")
    }

    #[test]
    fn the_drawing_font_is_embedded_and_its_text_can_be_read_back() {
        let font = Rc::new(DrawingFont {
            regular: ubuntu(),
            bold: None,
        });
        let pdf = write(&[page(Some(font.clone()))], "Fonts");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.6"));
        assert!(text.contains("/Subtype /Type0") && text.contains("/CIDFontType2"));
        assert!(text.contains("/BaseFont /Ubuntu") && text.contains("/FontFile2"));
        assert!(!text.contains("/Helvetica"), "no stand-in font");
        // Bold without a bold face: the regular face, as it is (no stroke).
        assert!(!text.contains("2 Tr") && !text.contains(" RG "));
        // The font file is in there whole, compressed: it comes back byte for byte.
        let parsed = lopdf::Document::load_mem(&pdf).expect("a PDF another reader accepts");
        let file = parsed
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .find(|s| s.dict.has(b"Length1"))
            .expect("the font file");
        let unpacked = file.decompressed_content().expect("inflates");
        assert_eq!(unpacked.len(), font.regular.data.len());
        assert_eq!(unpacked, **font.regular.data);
        // Selecting or searching the text gives the words back, Ø included.
        let words = parsed.extract_text(&[1]).expect("text extraction");
        assert!(words.contains("Mounting plate Ø12"), "{words:?}");
        assert!(words.contains("SHEET 01"), "{words:?}");
        // Only the glyphs used have widths listed.
        let used: std::collections::BTreeSet<char> = "Mounting plate Ø12SHEET 01".chars().collect();
        let map = parsed
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter_map(|s| s.decompressed_content().ok())
            .map(|c| String::from_utf8_lossy(&c).into_owned())
            .find(|c| c.contains("beginbfchar"))
            .expect("the glyph to character map");
        assert!(
            map.contains(&format!("{} beginbfchar", used.len())),
            "{map}"
        );
    }

    #[test]
    fn a_font_that_may_not_be_embedded_is_drawn_as_outlines() {
        let mut face = ubuntu();
        face.restricted = true;
        let font = Rc::new(DrawingFont {
            regular: face,
            bold: None,
        });
        let pdf = write(&[page(Some(font))], "Fonts");
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-1.4"));
        assert!(!text.contains("/FontFile2") && !text.contains("Tj"));
        // Curves and closed paths instead, filled (bold too: never stroked).
        assert!(
            text.matches(" c\n").count() > 50 && text.contains("h\nf\n") && !text.contains(" B\n")
        );
        assert!(lopdf::Document::load_mem(&pdf).is_ok());
        // And with no drawing font at all, nothing has changed: Helvetica by name.
        let plain = write(&[page(None)], "Fonts");
        let text = String::from_utf8_lossy(&plain);
        assert!(text.starts_with("%PDF-1.4") && text.contains("/BaseFont /Helvetica"));
        assert!(text.contains("(Mounting plate \u{fffd}12) Tj") || text.contains("12) Tj"));
    }
}
