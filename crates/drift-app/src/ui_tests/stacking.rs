//! Print matches the screen: the canvas and the PDF and SVG exports stack fills, hatches,
//! loose curves and placed pictures in the same order (the drawing's stack).

use std::sync::Arc;

use drift_core::{
    Appearance, Document, EntityId, Fill, FillKind, Hatch, Layer, LayerId, Rgb, Underlay,
    UnderlayImage,
};

use super::*;

const RED: Rgb = Rgb([255, 0, 0]);
const DARK_RED: Rgb = Rgb([128, 0, 0]);
const YELLOW: Rgb = Rgb([255, 255, 0]);
const BLUE: Rgb = Rgb([0, 0, 255]);
const NAVY: Rgb = Rgb([0, 0, 128]);
const MAGENTA: Rgb = Rgb([255, 0, 255]);
const GREEN: Rgb = Rgb([0, 255, 0]);
const DARK_GREEN: Rgb = Rgb([0, 128, 0]);
const CYAN: [u8; 4] = [0, 255, 255, 255];

/// A rectangle from `min` to `max` on `layer`, its outline in `outline`, filled with `fill`.
fn filled(
    doc: &mut Document,
    layer: LayerId,
    min: DVec2,
    max: DVec2,
    outline: Rgb,
    fill: FillKind,
    color: Rgb,
) -> Vec<EntityId> {
    doc.set_current_layer(layer).unwrap();
    let p =
        [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)].map(|q| doc.add_point(q));
    let lines: Vec<EntityId> = (0..4)
        .map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    for l in &lines {
        doc.set_appearance(
            *l,
            Appearance {
                color: Some(outline),
                ..Appearance::default()
            },
        )
        .unwrap();
    }
    let mut f = Fill::new(lines.clone(), (min + max) * 0.5, fill);
    f.color = Some(color);
    f.opacity = 1.0;
    doc.add_fill(f);
    lines
}

/// Two layers. Below: a red square; a hatch with a yellow background over it; a cyan
/// picture placed over the red square (under the hatch); a magenta line across both, loose
/// at the top of the layer. Above: a green square over all of it.
fn drawing() -> Document {
    let mut doc = Document::new();
    let below = doc.current_layer();
    let above = doc.add_layer(Layer::new("Above"));
    doc.begin();
    filled(
        &mut doc,
        below,
        DVec2::new(0.0, 0.0),
        DVec2::new(40.0, 40.0),
        DARK_RED,
        FillKind::Solid,
        RED,
    );
    let mut hatch = Hatch::new("ANSI31");
    hatch.scale = 4.0;
    hatch.model = true;
    hatch.background = Some(YELLOW);
    filled(
        &mut doc,
        below,
        DVec2::new(20.0, 20.0),
        DVec2::new(60.0, 60.0),
        NAVY,
        FillKind::Hatch(hatch),
        BLUE,
    );
    doc.set_current_layer(below).unwrap();
    let a = doc.add_point(DVec2::new(-10.0, 10.0));
    let b = doc.add_point(DVec2::new(80.0, 10.0));
    let line = doc.add_line(a, b).unwrap();
    doc.set_appearance(
        line,
        Appearance {
            color: Some(MAGENTA),
            weight: Some(drift_core::LineWeight(100)),
            ..Appearance::default()
        },
    )
    .unwrap();
    filled(
        &mut doc,
        above,
        DVec2::new(45.0, -5.0),
        DVec2::new(75.0, 25.0),
        DARK_GREEN,
        FillKind::Solid,
        GREEN,
    );
    // The picture: placed on the lower layer, over the red square's fill (its order).
    let mut pictures = Underlay::new("pictures", "test", Vec::new());
    pictures.images = Arc::new(vec![UnderlayImage {
        width: 1,
        height: 1,
        rgba: Arc::new(CYAN.to_vec()),
        at: DVec2::new(2.0, 34.0),
        across: DVec2::new(24.0, 0.0),
        down: DVec2::new(0.0, -10.0),
    }]);
    pictures.opacity = 1.0;
    pictures.layer = Some(below);
    pictures.order = doc.fills().next().map_or(0, |(_, f)| f.order);
    doc.add_underlay(pictures);
    doc.commit("Stacked drawing");
    doc
}

/// Where each colour is first painted in a drawing read back from an export.
fn first_painted(art: &drift_io::vector::Artwork, colours: &[Rgb]) -> Vec<usize> {
    colours
        .iter()
        .map(|c| {
            art.paths
                .iter()
                .position(|p| {
                    p.fill == Some(*c) || p.stroke.as_ref().is_some_and(|s| s.color == *c)
                })
                .unwrap_or_else(|| panic!("{c:?} isn't painted"))
        })
        .collect()
}

#[test]
fn print_pdf_and_svg_stack_as_the_canvas_does() {
    let Some(mut h) = harness() else { return };
    h.state_mut().editor = crate::editor::Editor::new(drawing());
    assert!(h.state().editor.doc.rules().stacked_fills);
    // Curves in their own colours, not by how far they are defined.
    h.state_mut().status_colors = false;
    let bounds = drift_core::Bounds {
        min: DVec2::new(-12.0, -8.0),
        max: DVec2::new(82.0, 62.0),
    };
    h.state_mut().fit_target = Some(bounds);
    h.run();
    h.state_mut().fit_target = None;
    h.run();

    // On screen: what is over what.
    save_png("drift_stacked_screen.png", &mut h);
    let image = h.render().expect("render");
    let ppp = h.ctx.pixels_per_point();
    let at = |w: (f64, f64)| {
        let p = screen(&h, DVec2::new(w.0, w.1));
        let px = image.get_pixel((p.x * ppp) as u32, (p.y * ppp) as u32).0;
        [px[0], px[1], px[2]]
    };
    let near = |px: [u8; 3], c: [u8; 3]| px.iter().zip(c).all(|(a, b)| a.abs_diff(b) < 40);
    assert!(
        near(at((5.0, 4.0)), RED.0),
        "the red square: {:?}",
        at((5.0, 4.0))
    );
    assert!(
        near(at((7.0, 29.0)), [0, 255, 255]),
        "the picture over it: {:?}",
        at((7.0, 29.0))
    );
    assert!(
        !near(at((24.0, 29.0)), [0, 255, 255]),
        "the hatch over the picture"
    );
    assert!(
        !near(at((30.0, 30.0)), RED.0),
        "the hatch over the red square"
    );
    assert!(
        near(at((5.0, 10.0)), MAGENTA.0),
        "the loose line over the square"
    );
    assert!(
        near(at((50.0, 10.0)), GREEN.0),
        "the upper layer over the line"
    );
    assert!(
        near(at((50.0, 22.0)), GREEN.0),
        "the upper layer over the hatch"
    );

    // On paper: the same order, in PDF and SVG alike.
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 1000.0)),
    );
    let app = h.state_mut();
    let page =
        crate::output::model_page(&mut app.editor, &app.hatch_patterns, &painter, None, false);
    let svg = drift_io::svg::write(&page);
    let pdf = drift_io::pdf::write(std::slice::from_ref(&page), "Stacked");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp");
    std::fs::create_dir_all(&dir).expect("create target/tmp");
    std::fs::write(dir.join("drift_stacked.svg"), &svg).expect("write svg");
    std::fs::write(dir.join("drift_stacked.pdf"), &pdf).expect("write pdf");
    let order = [
        RED, DARK_RED, YELLOW, BLUE, NAVY, MAGENTA, GREEN, DARK_GREEN,
    ];
    for (format, art) in [
        (
            "SVG",
            drift_io::svg_import::read(&svg, "stacked").expect("reads"),
        ),
        (
            "PDF",
            drift_io::pdf_import::read_artwork(&pdf, 1, "stacked").expect("reads"),
        ),
    ] {
        let first = first_painted(&art, &order);
        assert!(
            first.is_sorted(),
            "{format}: each fill under its outline, the hatch over the red square, the loose \
             line over both, the upper layer over all: {first:?}"
        );
    }
    // The SVG keeps a named group per layer.
    assert!(svg.contains(r#"inkscape:label="Above""#), "{svg}");

    // Without stacking, every fill prints under every curve (as the canvas draws it then).
    let app = h.state_mut();
    let mut rules = app.editor.doc.rules();
    rules.stacked_fills = false;
    app.editor.doc.begin();
    app.editor.doc.set_rules(rules);
    app.editor.commit("Rules");
    let page =
        crate::output::model_page(&mut app.editor, &app.hatch_patterns, &painter, None, false);
    // (The PDF: an SVG groups each layer's items together.)
    let pdf = drift_io::pdf::write(std::slice::from_ref(&page), "Flat");
    let art = drift_io::pdf_import::read_artwork(&pdf, 1, "flat").expect("reads");
    let first = first_painted(&art, &[RED, YELLOW, GREEN, DARK_RED, MAGENTA]);
    assert!(first.is_sorted(), "fills, then curves: {first:?}");
}
