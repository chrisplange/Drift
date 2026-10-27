//! How layers, blocks, colours, weights and line types go out to CAD and come in from it:
//! - a block placed on two layers is one block whose items are "by block", so each copy
//!   shows its own layer's colour, weight and line type (in DWG, DXF 2018 and R12, and when
//!   the copies are exploded for PDF and SVG);
//! - R12 layer and block names that differ only in case or punctuation stay apart;
//! - the same dashes and the same weight come in alike from DXF and from PDF / SVG.
#![cfg(not(feature = "demo"))]

use drift_core::{DVec2, Document, LineType, LineWeight, Rgb, Shape};
use drift_draft::page::{BlockDef, ByBlock, Drawing, Item, LayerInfo, Pen, Prim};
use drift_io::dxf::{self, DxfImport, DxfUnits, add_to_document};
use drift_io::{pdf_import, svg_import, vector};
use lopdf::{Object, Stream, dictionary};

const RED: Rgb = Rgb([255, 0, 0]);
const BLUE: Rgb = Rgb([0, 0, 255]);

fn layer(name: &str, color: Rgb, width: f64, line_type: LineType) -> LayerInfo {
    LayerInfo {
        name: name.into(),
        color,
        width,
        line_type,
    }
}

// ---------------------------------------------------------------------------------------------
// C1: one block, copies on two layers

/// A square block placed twice: on "Walls" (red, 0.50 mm, continuous) and on "Hidden"
/// (blue, 0.18 mm, dashed). Its sides are all "by block", as Drift exports a block whose
/// curves are in their layer's look; their pens are one copy's (the dashed one).
fn block_on_two_layers() -> Drawing {
    let mut d = Drawing::new(DVec2::splat(200.0));
    d.layer(layer("Walls", RED, 0.5, LineType::Continuous));
    d.layer(layer("Hidden", BLUE, 0.18, LineType::Dashed));
    let by_block = Pen {
        by_block: ByBlock {
            color: true,
            width: true,
            line_type: true,
        },
        ..Pen::new(0.18).with_color(BLUE).with_type(LineType::Dashed)
    };
    let corners = [
        DVec2::ZERO,
        DVec2::new(20.0, 0.0),
        DVec2::new(20.0, 20.0),
        DVec2::new(0.0, 20.0),
    ];
    d.blocks.push(BlockDef {
        name: "Square".into(),
        items: (0..4)
            .map(|k| Item {
                layer: "0".into(),
                pen: by_block,
                prim: Prim::Line(corners[k], corners[(k + 1) % 4]),
                clip: None,
            })
            .collect(),
    });
    for (name, at, pen) in [
        ("Walls", DVec2::ZERO, Pen::new(0.5).with_color(RED)),
        (
            "Hidden",
            DVec2::new(100.0, 0.0),
            Pen::new(0.18).with_color(BLUE).with_type(LineType::Dashed),
        ),
    ] {
        d.push(
            name,
            pen,
            Prim::Insert {
                block: "Square".into(),
                at,
                angle: 0.0,
                scale: 1.0,
                mirror: false,
            },
        );
    }
    d
}

/// The import has one block placed on both layers, its sides with no look of their own;
/// in a document each copy then draws in its own layer's look.
fn check_block_on_two_layers(back: &DxfImport, weights: bool) {
    assert_eq!(back.blocks.len(), 1, "one block, not one per look");
    let block = &back.blocks[0];
    assert_eq!(block.entities.len(), 4);
    for e in &block.entities {
        assert_eq!(e.color, None, "colour by block");
        assert_eq!(e.line_type, None, "line type by block");
        assert_eq!(e.weight, None, "weight by block");
    }
    let mut layers: Vec<&str> = back.inserts.iter().map(|i| i.layer.as_str()).collect();
    layers.sort_unstable();
    assert_eq!(layers.len(), 2);
    assert!(layers[0].eq_ignore_ascii_case("Hidden") && layers[1].eq_ignore_ascii_case("Walls"));
    let look = |name: &str| {
        back.layers
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("layer {name}"))
    };
    assert_eq!(look("Walls").color, Some(RED));
    assert_eq!(look("Hidden").color, Some(BLUE));
    assert_eq!(look("Hidden").line_type, LineType::Dashed);
    if weights {
        assert_eq!(look("Walls").weight, Some(LineWeight(50)));
        assert_eq!(look("Hidden").weight, Some(LineWeight(18)));
    }
    let mut doc = Document::new();
    add_to_document(&mut doc, back);
    let (_, b) = doc.blocks().next().expect("the block");
    assert!(b.items.iter().all(|it| it.appearance.is_by_layer()));
    let copy_layers: Vec<_> = doc
        .instances()
        .map(|(_, c)| doc.layer(c.layer).map(|l| l.color))
        .collect();
    assert_eq!(copy_layers.len(), 2);
    assert!(copy_layers.contains(&Some(Some(RED))) && copy_layers.contains(&Some(Some(BLUE))));
}

#[test]
fn a_block_on_two_layers_takes_each_copys_look_in_r12() {
    let text = dxf::write(&block_on_two_layers(), DxfUnits::MM);
    let back = dxf::read(&text, 1.0).unwrap();
    // R12 has no line weights.
    check_block_on_two_layers(&back, false);
}

#[cfg(feature = "dwg")]
#[test]
fn a_block_on_two_layers_takes_each_copys_look_in_dwg_and_dxf_2018() {
    use drift_io::cad::{write_dwg, write_dxf};
    let d = block_on_two_layers();
    let text = String::from_utf8(write_dxf(&d, DxfUnits::MM).unwrap()).unwrap();
    let back = dxf::read(&text, 1.0).unwrap();
    check_block_on_two_layers(&back, true);
    let bytes = write_dwg(&d, DxfUnits::MM).unwrap();
    let back = drift_io::dwg_import::read(&bytes, 1.0).unwrap();
    check_block_on_two_layers(&back, true);
    if let Ok(dir) = std::env::var("DRIFT_CAD_OUT") {
        let dir = std::path::Path::new(&dir);
        let _ = std::fs::write(dir.join("block-two-layers.dwg"), &bytes);
        let _ = std::fs::write(dir.join("block-two-layers-2018.dxf"), text);
        let r12 = dxf::write(&d, DxfUnits::MM);
        let _ = std::fs::write(dir.join("block-two-layers-r12.dxf"), r12);
    }
}

#[test]
fn exploded_copies_take_their_own_look() {
    let d = block_on_two_layers().exploded().into_owned();
    let pens: Vec<Pen> = d.items.iter().map(|i| i.pen).collect();
    assert_eq!(pens.len(), 8);
    assert!(pens[..4].iter().all(|p| p.color == RED
        && (p.width - 0.5).abs() < 1e-9
        && p.line_type == LineType::Continuous));
    assert!(pens[4..].iter().all(|p| p.color == BLUE
        && (p.width - 0.18).abs() < 1e-9
        && p.line_type == LineType::Dashed));
    assert!(d.items[..4].iter().all(|i| i.layer == "Walls"));
}

// ---------------------------------------------------------------------------------------------
// C2: R12 names

#[test]
fn r12_names_that_differ_in_case_or_punctuation_stay_apart() {
    let mut d = Drawing::new(DVec2::splat(100.0));
    d.layer(layer("Walls new", RED, 0.25, LineType::Continuous));
    d.layer(layer("walls_new", BLUE, 0.25, LineType::Continuous));
    d.line(
        "Walls new",
        Pen::new(0.25).with_color(RED),
        DVec2::ZERO,
        DVec2::X * 10.0,
    );
    d.line(
        "walls_new",
        Pen::new(0.25).with_color(BLUE),
        DVec2::Y * 5.0,
        DVec2::new(10.0, 5.0),
    );
    for (name, y) in [("Door", 20.0), ("door", 40.0)] {
        d.blocks.push(BlockDef {
            name: name.into(),
            items: vec![Item {
                layer: "0".into(),
                pen: Pen::new(0.25),
                prim: Prim::Line(DVec2::ZERO, DVec2::new(0.0, y)),
                clip: None,
            }],
        });
        d.push(
            "Walls new",
            Pen::new(0.25).with_color(RED),
            Prim::Insert {
                block: name.into(),
                at: DVec2::ZERO,
                angle: 0.0,
                scale: 1.0,
                mirror: false,
            },
        );
    }
    let text = dxf::write(&d, DxfUnits::MM);
    let back = dxf::read(&text, 1.0).unwrap();
    let colour = |name: &str| back.layers.iter().find(|l| l.name == name).map(|l| l.color);
    assert_eq!(
        colour("WALLS_NEW"),
        Some(Some(RED)),
        "the first keeps the name"
    );
    assert_eq!(
        colour("WALLS_NEW_2"),
        Some(Some(BLUE)),
        "the second has its own"
    );
    let on = |name: &str| back.entities.iter().filter(|e| e.layer == name).count();
    assert_eq!((on("WALLS_NEW"), on("WALLS_NEW_2")), (1, 1));
    // Two blocks, each placed by its own copy, each its own length.
    assert_eq!(back.blocks.len(), 2);
    let mut lengths: Vec<f64> = back
        .inserts
        .iter()
        .map(|i| {
            let b = back.blocks.iter().find(|b| b.name == i.block).unwrap();
            match b.entities[0].shape {
                Shape::Line { a, b } => a.distance(b),
                _ => 0.0,
            }
        })
        .collect();
    lengths.sort_by(f64::total_cmp);
    assert!((lengths[0] - 20.0).abs() < 1e-9 && (lengths[1] - 40.0).abs() < 1e-9);
}

// ---------------------------------------------------------------------------------------------
// C3: dashes and weights, from DXF and from PDF / SVG

/// A DXF with one line in a line type of the given pattern (mm; gaps negative) and weight.
fn dxf_line(pattern: &[f64], weight: i32) -> DxfImport {
    let mut s = String::new();
    let mut pair = |code: i32, v: &str| s.push_str(&format!("{code}\n{v}\n"));
    for (c, v) in [(0, "SECTION"), (2, "TABLES"), (0, "TABLE"), (2, "LTYPE")] {
        pair(c, v);
    }
    pair(0, "LTYPE");
    pair(2, "MINE");
    pair(70, "0");
    pair(3, "Mine");
    pair(72, "65");
    pair(73, &pattern.len().to_string());
    pair(
        40,
        &pattern.iter().map(|d| d.abs()).sum::<f64>().to_string(),
    );
    for d in pattern {
        pair(49, &d.to_string());
    }
    for (c, v) in [
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "ENTITIES"),
    ] {
        pair(c, v);
    }
    for (c, v) in [
        (0, "LINE"),
        (8, "L"),
        (6, "MINE"),
        (370, &weight.to_string()),
        (10, "0"),
        (20, "0"),
        (30, "0"),
        (11, "100"),
        (21, "0"),
        (31, "0"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ] {
        pair(c, v);
    }
    dxf::read(&s, 1.0).unwrap()
}

/// An SVG (in mm) with one line of the given dash array and width.
fn svg_line(dashes: &[f64], width: f64) -> DxfImport {
    let dashes: Vec<String> = dashes.iter().map(f64::to_string).collect();
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="200mm" height="100mm" viewBox="0 0 200 100">
<line x1="0" y1="50" x2="100" y2="50" stroke="black" stroke-width="{width}" stroke-dasharray="{}"/>
</svg>"#,
        dashes.join(" ")
    );
    let art = svg_import::read(&svg, "line").unwrap();
    vector::to_import(&art, &vector::ImportOptions::default())
}

/// A PDF page with one line of the given dash array and width (both in mm).
fn pdf_line(dashes: &[f64], width: f64) -> DxfImport {
    let pt = 72.0 / 25.4;
    let dashes: Vec<String> = dashes.iter().map(|d| format!("{:.5}", d * pt)).collect();
    let content = format!(
        "{:.5} w [{}] 0 d 10 400 m 300 400 l S",
        width * pt,
        dashes.join(" ")
    );
    let mut doc = lopdf::Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        "Contents" => content_id,
        "Resources" => dictionary! {},
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let art = pdf_import::read_artwork(&bytes, 1, "line").unwrap();
    vector::to_import(&art, &vector::ImportOptions::default())
}

/// The one line's type and weight (the DXF's type is its line type's, by name).
fn look_of(import: &DxfImport) -> (LineType, Option<LineWeight>) {
    assert_eq!(import.entities.len(), 1, "one line");
    let e = &import.entities[0];
    (e.line_type.unwrap_or(LineType::Continuous), e.weight)
}

#[test]
fn the_same_dashes_and_weight_come_in_alike_from_dxf_pdf_and_svg() {
    // (marks and gaps, in mm; what a reader expects)
    let cases: [(&[f64], LineType); 4] = [
        (&[6.0, 2.0, 6.0, 2.0], LineType::Dashed),
        (&[6.0, 2.0], LineType::Dashed),
        (&[2.0, 1.0], LineType::Hidden),
        (&[12.0, 3.0, 2.0, 3.0], LineType::Center),
    ];
    for (pattern, expected) in cases {
        let dxf_pattern: Vec<f64> = pattern
            .iter()
            .enumerate()
            .map(|(k, d)| if k % 2 == 1 { -d } else { *d })
            .collect();
        let from_dxf = look_of(&dxf_line(&dxf_pattern, 30));
        let from_svg = look_of(&svg_line(pattern, 0.3));
        let from_pdf = look_of(&pdf_line(pattern, 0.3));
        assert_eq!(from_dxf.0, expected, "DXF {pattern:?}");
        assert_eq!(from_svg.0, expected, "SVG {pattern:?}");
        assert_eq!(from_pdf.0, expected, "PDF {pattern:?}");
        for (from, w) in [
            ("DXF", from_dxf.1),
            ("SVG", from_svg.1),
            ("PDF", from_pdf.1),
        ] {
            assert_eq!(w, Some(LineWeight(30)), "{from}: 0.30 mm stays 0.30 mm");
        }
    }
}
