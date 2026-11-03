//! A sheet's views cropped to their windows on export (#258): what is outside a view isn't in
//! the PDF or SVG at all, so releasing the clip shows nothing more.

use drift_core::{
    Bounds, DVec2, Document, EntityId, Fill, FillKind, Hatch, Layer, LayerId, LineKind, Sheet,
    SketchText, View, Viewport,
};
use drift_draft::page::{Drawing, Prim};
use drift_edit::editor::Editor;
use drift_edit::fills::PatternLibrary;

use crate::output::{sheet_drawing, uncropped_sheet};

const PT_PER_MM: f64 = 72.0 / 25.4;
/// How far past a window a written coordinate may lie: the crop's own edge, and rounding.
const SLACK: f64 = drift_draft::crop::EDGE + 1e-3;

fn painter() -> eframe::egui::Painter {
    eframe::egui::Painter::new(
        eframe::egui::Context::default(),
        eframe::egui::LayerId::background(),
        eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 800.0),
        ),
    )
}

fn line(doc: &mut Document, a: [f64; 2], b: [f64; 2]) -> EntityId {
    let a = doc.add_point(DVec2::from(a));
    let b = doc.add_point(DVec2::from(b));
    doc.add_line(a, b).unwrap()
}

/// A square of construction lines (they don't print) from `min`, `size` across, filled.
fn filled_square(doc: &mut Document, min: [f64; 2], size: f64, kind: FillKind) {
    let c = [
        min,
        [min[0] + size, min[1]],
        [min[0] + size, min[1] + size],
        [min[0], min[1] + size],
    ];
    let mut edges = Vec::new();
    for i in 0..4 {
        let edge = line(doc, c[i], c[(i + 1) % 4]);
        doc.set_style(edge, LineKind::Construction);
        edges.push(edge);
    }
    let seed = DVec2::new(min[0] + size / 2.0, min[1] + size / 2.0);
    doc.add_fill(Fill::new(edges, seed, kind));
}

/// The sheet of one view of `doc`'s world 0..100 × 0..50 at 1 : 1: the drawing (cropped, or
/// as before #258) and the view's window on the page.
fn export(doc: Document, cropped: bool) -> (Drawing, Bounds) {
    let mut doc = doc;
    let view = View::new(
        "Plan",
        Bounds {
            min: DVec2::ZERO,
            max: DVec2::new(100.0, 50.0),
        },
        1.0,
    );
    let id = doc.add_view(view);
    let mut sheet = Sheet::new("A-101");
    sheet
        .viewports
        .push(Viewport::new(id, DVec2::new(200.0, 150.0)));
    doc.add_sheet(sheet.clone());
    let views = doc.placed_views(&sheet);
    let window = views[0].frame.bounds();
    let mut ed = Editor::new(doc);
    let make = if cropped {
        sheet_drawing
    } else {
        uncropped_sheet
    };
    let d = make(
        &mut ed,
        &PatternLibrary::default(),
        &painter(),
        &sheet,
        &views,
        (1, 1),
    );
    (d, window)
}

/// On `Curves`: a line inside the view, one across its right edge, one outside it, a circle
/// across its right edge and a centre line across its left one, none meeting another. On
/// `Fills`: a solid fill across the bottom edge and a hatch across the top one.
fn crossing() -> Document {
    let mut doc = Document::new();
    let curves = doc.add_layer(Layer::new("Curves"));
    let fills = doc.add_layer(Layer::new("Fills"));
    doc.set_current_layer(curves).unwrap();
    line(&mut doc, [10.0, 10.0], [40.0, 10.0]);
    line(&mut doc, [50.0, 20.0], [150.0, 20.0]);
    line(&mut doc, [200.0, 0.0], [250.0, 30.0]);
    let centre = doc.add_point(DVec2::new(100.0, 40.0));
    doc.add_circle(centre, 5.0).unwrap();
    let dashed = line(&mut doc, [-30.0, 30.0], [30.0, 30.0]);
    doc.set_style(dashed, LineKind::Center);
    doc.set_current_layer(fills).unwrap();
    filled_square(&mut doc, [60.0, -10.0], 20.0, FillKind::Solid);
    filled_square(
        &mut doc,
        [20.0, 40.0],
        20.0,
        FillKind::Hatch(Hatch::new("ANSI31")),
    );
    doc.set_current_layer(LayerId::DEFAULT).unwrap();
    doc
}

fn within(p: DVec2, w: Bounds) -> bool {
    p.x >= w.min.x - SLACK
        && p.x <= w.max.x + SLACK
        && p.y >= w.min.y - SLACK
        && p.y <= w.max.y + SLACK
}

/// The points an SVG path's data passes through (ends and control points; arcs by their
/// ends), in its own coordinates.
fn path_points(d: &str) -> Vec<DVec2> {
    let mut spaced = String::new();
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            spaced.push(' ');
            spaced.push(c);
            spaced.push(' ');
        } else {
            spaced.push(c);
        }
    }
    let mut out = Vec::new();
    let mut tokens = spaced.split_whitespace().peekable();
    let mut command = 'M';
    let mut nums = Vec::new();
    loop {
        let token = tokens.next();
        let number = token.and_then(|t| t.parse::<f64>().ok());
        if let Some(v) = number {
            nums.push(v);
        }
        let need = match command {
            'A' => 7,
            'C' => 6,
            'Z' => 0,
            _ => 2,
        };
        if need > 0 && nums.len() == need {
            match command {
                'A' => out.push(DVec2::new(nums[5], nums[6])),
                _ => out.extend(nums.chunks(2).map(|c| DVec2::new(c[0], c[1]))),
            }
            nums.clear();
        }
        match token {
            None => break,
            Some(t) if number.is_none() => {
                command = t.chars().next().unwrap();
                nums.clear();
            }
            _ => {}
        }
    }
    out
}

/// Each stroke or fill drawn on `layer` in the SVG (a layer's group), with the points it
/// passes through, in page mm with y up.
fn svg_shapes(svg: &str, layer: &str, height: f64) -> Vec<Vec<DVec2>> {
    let doc = roxmltree::Document::parse(svg).expect("SVG parses");
    let group = doc
        .descendants()
        .find(|n| n.has_tag_name("g") && n.attribute("id") == Some(layer))
        .unwrap_or_else(|| panic!("a group for {layer}"));
    let num = |n: &roxmltree::Node, a: &str| n.attribute(a).unwrap().parse::<f64>().unwrap();
    let flip = |p: DVec2| DVec2::new(p.x, height - p.y);
    group
        .descendants()
        .filter_map(|n| match n.tag_name().name() {
            "path" => Some(path_points(n.attribute("d").unwrap())),
            "line" => Some(vec![
                DVec2::new(num(&n, "x1"), num(&n, "y1")),
                DVec2::new(num(&n, "x2"), num(&n, "y2")),
            ]),
            "circle" | "ellipse" => {
                let c = DVec2::new(num(&n, "cx"), num(&n, "cy"));
                let r = n
                    .attribute("r")
                    .or(n.attribute("rx"))
                    .unwrap()
                    .parse::<f64>()
                    .unwrap();
                Some(vec![c - DVec2::splat(r), c + DVec2::splat(r)])
            }
            _ => None,
        })
        .map(|pts| pts.into_iter().map(flip).collect())
        .collect()
}

/// The page's content stream, inflated.
fn pdf_content(pdf: &[u8]) -> String {
    let mut at = 0;
    while let Some(start) = find(&pdf[at..], b"stream\n").map(|i| at + i + 7) {
        let end = start + find(&pdf[start..], b"\nendstream").expect("stream ends");
        // (The page's content is written as it is; font files and maps are compressed.)
        let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&pdf[start..end])
            .unwrap_or_else(|_| pdf[start..end].to_vec());
        let text = String::from_utf8_lossy(&raw).into_owned();
        if text.contains(" re W n") {
            return text;
        }
        at = end + b"\nendstream".len();
    }
    panic!("no page content with a view's clip");
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The points each path drawn through a view's clip (`q … re W n` to `Q`) passes through,
/// in page mm, a path per painting operator.
fn pdf_clipped_paths(content: &str) -> Vec<Vec<DVec2>> {
    let mut out = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    let (mut clipped, mut text) = (false, false);
    let mut path: Vec<DVec2> = Vec::new();
    for token in content.split_whitespace() {
        if let Ok(v) = token.parse::<f64>() {
            nums.push(v);
            continue;
        }
        let pt = |k: usize| DVec2::new(nums[k], nums[k + 1]) / PT_PER_MM;
        match token {
            "W" => clipped = true,
            "Q" => clipped = false,
            "BT" => text = true,
            "ET" => text = false,
            "m" | "l" if clipped && !text => path.push(pt(nums.len() - 2)),
            "c" if clipped && !text => {
                let k = nums.len() - 6;
                path.extend([pt(k), pt(k + 2), pt(k + 4)]);
            }
            "S" | "s" | "f" | "f*" | "B" | "b" if !path.is_empty() => {
                out.push(std::mem::take(&mut path))
            }
            _ => {}
        }
        nums.clear();
    }
    out
}

#[test]
fn svg_and_pdf_hold_only_what_each_view_shows() {
    let (d, window) = export(crossing(), true);
    let h = d.size.y;
    let svg = drift_io::svg::write(&d);
    // The curves: the line inside, the parts of the line and the circle across the edge,
    // and the centre line's part; the line outside is gone.
    let curves = svg_shapes(&svg, "Curves", h);
    assert_eq!(curves.len(), 4, "{svg}");
    for shape in svg_shapes(&svg, "Curves", h)
        .into_iter()
        .chain(svg_shapes(&svg, "Fills", h))
    {
        for p in shape {
            assert!(within(p, window), "{p} is outside {window:?}");
        }
    }
    let pdf = drift_io::pdf::write(std::slice::from_ref(&d), "Plan");
    let paths = pdf_clipped_paths(&pdf_content(&pdf));
    assert!(!paths.is_empty());
    for p in paths.iter().flatten() {
        assert!(within(*p, window), "{p} is outside {window:?}");
    }
    // Four curves, a solid fill and the hatch's lines.
    let hatch_lines = d
        .items
        .iter()
        .filter(|i| i.layer == "Fills" && matches!(i.prim, Prim::Line(..)))
        .count();
    assert!(hatch_lines > 0);
    assert_eq!(paths.len(), 4 + 1 + hatch_lines);
}

#[test]
fn fills_and_hatches_across_the_edge_are_cut_to_it() {
    let (d, window) = export(crossing(), true);
    let (whole, _) = export(crossing(), false);
    let fills: Vec<_> = d.items.iter().filter(|i| i.layer == "Fills").collect();
    let solid: Vec<_> = fills
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Fill {
                rings, triangles, ..
            } => Some((rings, triangles)),
            _ => None,
        })
        .collect();
    assert_eq!(solid.len(), 1);
    let (rings, triangles) = solid[0];
    assert!(rings.iter().flatten().all(|p| within(*p, window)));
    assert!(triangles.iter().flatten().all(|p| within(*p, window)));
    // Half the 20 mm square shows: 20 × 10 on paper at 1 : 1.
    let area: f64 = rings
        .iter()
        .map(|r| drift_core::polygon::area(r).abs())
        .sum();
    assert!((area - 200.0).abs() < 0.2, "{area}");
    // The hatch's lines: each where it was, cut at the edge, fewer than in the whole hatch.
    let cut: Vec<_> = fills
        .iter()
        .filter_map(|i| match i.prim {
            Prim::Line(a, b) => Some((a, b)),
            _ => None,
        })
        .collect();
    let all: Vec<_> = whole
        .items
        .iter()
        .filter(|i| i.layer == "Fills")
        .filter_map(|i| match i.prim {
            Prim::Line(a, b) => Some((a, b)),
            _ => None,
        })
        .collect();
    assert!(!cut.is_empty() && cut.len() < all.len());
    for (a, b) in cut {
        assert!(within(a, window) && within(b, window));
        // On a whole hatch line, in the same direction: the pattern stays put.
        assert!(
            all.iter().any(|(c, e)| {
                let d = (*e - *c).normalize();
                (a - *c).perp_dot(d).abs() < 1e-6 && (b - *c).perp_dot(d).abs() < 1e-6
            }),
            "{a} {b} isn't on a hatch line"
        );
    }
}

#[test]
fn text_across_the_edge_is_kept_whole_and_text_outside_left_out() {
    let mut doc = Document::new();
    let notes = doc.add_layer(Layer::new("Notes"));
    doc.set_current_layer(notes).unwrap();
    doc.add_text(SketchText::new("Across", DVec2::new(95.0, 25.0), 3.0));
    doc.add_text(SketchText::new("Beyond", DVec2::new(130.0, 25.0), 3.0));
    let (d, _) = export(doc, true);
    let texts: Vec<_> = d
        .items
        .iter()
        .filter_map(|i| match &i.prim {
            Prim::Text { text, .. } if i.layer == "Notes" => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["Across"]);
}

/// A sheet whose views hold all of their drawings comes out as it did before the crop.
#[test]
fn views_holding_their_whole_drawing_export_as_before() {
    let mut doc = Document::new();
    line(&mut doc, [10.0, 10.0], [90.0, 40.0]);
    let centre = doc.add_point(DVec2::new(50.0, 25.0));
    doc.add_circle(centre, 10.0).unwrap();
    let dashed = line(&mut doc, [5.0, 5.0], [95.0, 5.0]);
    doc.set_style(dashed, LineKind::Center);
    filled_square(&mut doc, [20.0, 20.0], 10.0, FillKind::Solid);
    filled_square(
        &mut doc,
        [60.0, 20.0],
        10.0,
        FillKind::Hatch(Hatch::new("ANSI31")),
    );
    doc.add_text(SketchText::new("Inside", DVec2::new(30.0, 40.0), 3.0));
    let (cropped, _) = export(doc.clone(), true);
    let (whole, _) = export(doc, false);
    assert_eq!(cropped, whole);
    assert_eq!(drift_io::svg::write(&cropped), drift_io::svg::write(&whole));
    assert_eq!(
        drift_io::pdf::write(std::slice::from_ref(&cropped), "Plan"),
        drift_io::pdf::write(std::slice::from_ref(&whole), "Plan")
    );
}
