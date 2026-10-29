//! Line weights from other files keep their AutoCAD weight through save and open (#255).

use drift_core::{
    Appearance, Block, BlockItem, DVec2, Document, Fill, FillKind, FillStyle, Hatch, Layer,
    LineKind, LineWeight, Path, Shape, Wall,
};
use drift_io::{Metadata, dxf, load, save, svg_import, vector};

/// Two lines drawn on paper at 0.09 mm and 0.40 mm: both on AutoCAD's list, neither ISO.
const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
<line x1="0" y1="0" x2="50" y2="0" stroke="black" stroke-width="0.09"/>
<line x1="0" y1="10" x2="50" y2="10" stroke="black" stroke-width="0.4"/>
</svg>"#;

fn weights(doc: &Document) -> Vec<LineWeight> {
    let mut w: Vec<LineWeight> = doc
        .iter()
        .filter_map(|(_, e)| e.appearance.weight)
        .collect();
    w.sort();
    w.dedup();
    w
}

#[test]
fn imported_weights_survive_save_and_open() {
    let art = svg_import::read(SVG, "Imported").unwrap();
    let import = vector::to_import(&art, &vector::ImportOptions::default());
    let mut doc = Document::new();
    dxf::add_to_document(&mut doc, &import);
    assert_eq!(weights(&doc), [LineWeight(9), LineWeight(40)]);

    let first = save(&doc, &Metadata::default());
    let (loaded, meta) = load(&first).unwrap();
    assert_eq!(weights(&loaded), [LineWeight(9), LineWeight(40)]);
    assert_eq!(save(&loaded, &meta), first, "the second save is the first");
}

/// Every weight AutoCAD offers stays as it is, wherever a drawing keeps one: curves, layers,
/// fills, block items and wall poché.
#[test]
fn every_cad_weight_survives_save_and_open() {
    let mut doc = Document::new();
    let square = [
        DVec2::new(0.0, 0.0),
        DVec2::new(10.0, 0.0),
        DVec2::new(10.0, 10.0),
        DVec2::new(0.0, 10.0),
    ];
    for (i, w) in LineWeight::CAD.into_iter().enumerate() {
        let layer = doc.add_layer(Layer {
            weight: w,
            ..Layer::new(format!("L{i}"))
        });
        let x = 20.0 * i as f64;
        let points: Vec<_> = square
            .iter()
            .map(|p| doc.add_point(*p + DVec2::new(x, 0.0)))
            .collect();
        let curves: Vec<_> = (0..4)
            .map(|k| doc.add_line(points[k], points[(k + 1) % 4]).unwrap())
            .collect();
        for c in &curves {
            doc.set_entity_layer(*c, layer).unwrap();
            doc.set_appearance(
                *c,
                Appearance {
                    weight: Some(w),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let mut fill = Fill::new(
            curves,
            DVec2::new(x + 5.0, 5.0),
            FillKind::Hatch(Hatch::new("ANSI31")),
        );
        fill.weight = Some(w);
        doc.add_fill(fill);
        let a = doc.add_point(DVec2::new(x, 100.0));
        let b = doc.add_point(DVec2::new(x + 10.0, 100.0));
        let centre = doc.add_line(a, b).unwrap();
        doc.set_style(centre, LineKind::Construction);
        let path = doc
            .add_path(Path {
                curves: vec![centre],
                closed: false,
            })
            .unwrap();
        let mut wall = Wall::new(path, 2.0, layer);
        wall.poche = Some(FillStyle {
            weight: Some(w),
            ..FillStyle::solid()
        });
        doc.add_wall(wall).unwrap();
    }
    doc.add_block(Block {
        name: "Weights".into(),
        profile: false,
        fills: Vec::new(),
        items: LineWeight::CAD
            .into_iter()
            .enumerate()
            .map(|(i, w)| BlockItem {
                shape: Shape::Line {
                    a: DVec2::new(0.0, i as f64),
                    b: DVec2::new(10.0, i as f64),
                },
                style: LineKind::Solid,
                appearance: Appearance {
                    weight: Some(w),
                    ..Default::default()
                },
            })
            .collect(),
    });

    let text = save(&doc, &Metadata::default());
    let (loaded, meta) = load(&text).unwrap();
    let again = save(&loaded, &meta);
    // The first lines that changed, to see which weights moved.
    let changed = text.lines().zip(again.lines()).filter(|(a, b)| a != b);
    for (a, b) in changed.take(6) {
        eprintln!("- {a}\n+ {b}");
    }
    assert_eq!(again, text);
}
