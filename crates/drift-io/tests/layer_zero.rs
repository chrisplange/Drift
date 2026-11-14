//! Layer 0 is a DWG / DXF requirement, not something every Drift drawing has (#423):
//! - a `.drift` file with no layer 0 opens without one, and a file that has one keeps it;
//! - an imported DXF or DWG shows its layer 0;
//! - every DXF and DWG export has a layer 0, empty when the drawing has none.
//!
//! (A new drawing's own layer is tested in drift-core, and the app's exports of one in
//! drift-app.)
#![cfg(not(feature = "demo"))]

use drift_core::{DVec2, Document, LayerId, LineType, Rgb, Units};
use drift_draft::page::{Drawing, LayerInfo, Pen};
use drift_io::dxf::{DxfImport, DxfUnits, add_to_document, read};
use drift_io::{Metadata, load, save};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn units() -> DxfUnits {
    DxfUnits::of(&Units::default())
}

fn layer_names(doc: &Document) -> Vec<String> {
    doc.layers().map(|(_, l)| l.name.clone()).collect()
}

/// A new drawing with a line on its layer.
fn new_drawing_with_a_line() -> Document {
    let mut doc = Document::new();
    let (a, b) = (doc.add_point(v(0.0, 0.0)), doc.add_point(v(100.0, 0.0)));
    doc.add_line(a, b).unwrap();
    doc
}

/// The names of the layers `doc`'s curves are on.
fn curve_layers(doc: &Document) -> Vec<String> {
    let mut names: Vec<String> = doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| doc.layer_of(id).name.clone())
        .collect();
    names.sort();
    names
}

#[test]
#[ignore = "#423: a new drawing still has a layer 0, so its file does too"]
fn a_drift_file_with_no_layer_0_opens_without_one() {
    let text = save(&new_drawing_with_a_line(), &Metadata::default());
    let (doc, _) = load(&text).expect("a file with no layer 0 opens");
    assert_eq!(layer_names(&doc), ["Layer 1"]);
    assert_eq!(curve_layers(&doc), ["Layer 1"]);
    assert_eq!(doc.layer(doc.current_layer()).unwrap().name, "Layer 1");
    // And it is the same file when saved again.
    assert_eq!(save(&doc, &Metadata::default()), text);
}

#[test]
fn a_drift_file_with_a_layer_0_keeps_it() {
    // A file that lists its layers, layer 0 among them (as Drift wrote it at format 44).
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v44.2.drift");
    let listed = std::fs::read_to_string(path).expect("fixture reads");
    let (doc, _) = load(&listed).expect("the fixture opens");
    assert_eq!(layer_names(&doc), ["0", "Walls", "Furniture"]);
    assert_eq!(doc.layer(LayerId::DEFAULT).unwrap().name, "0");
    let (again, _) = load(&save(&doc, &Metadata::default())).unwrap();
    assert_eq!(layer_names(&again), ["0", "Walls", "Furniture"]);

    // A file with no `layers` at all, as every build before #423 wrote a drawing with
    // nothing but layer 0: its curves are on layer 0, and stay there.
    let mut value: serde_json::Value =
        serde_json::from_str(&save(&new_drawing_with_a_line(), &Metadata::default())).unwrap();
    let file = value.as_object_mut().unwrap();
    file.remove("layers");
    file.remove("current_layer");
    let unlisted = serde_json::to_string(&value).unwrap();
    let (doc, _) = load(&unlisted).expect("a file without layers opens");
    assert_eq!(layer_names(&doc), ["0"]);
    assert_eq!(curve_layers(&doc), ["0"]);
    let (again, _) = load(&save(&doc, &Metadata::default())).unwrap();
    assert_eq!(layer_names(&again), ["0"]);
    assert_eq!(curve_layers(&again), ["0"]);
}

/// A CAD drawing with a line on layer 0 and one on a layer of its own.
fn cad_drawing_using_layer_0() -> Drawing {
    let mut d = Drawing::new(v(1000.0, 1000.0));
    d.layer(LayerInfo {
        name: "Walls".into(),
        color: Rgb([200, 30, 40]),
        width: 0.5,
        line_type: LineType::Continuous,
    });
    d.line("0", Pen::new(0.25), v(0.0, 0.0), v(100.0, 0.0));
    d.line("Walls", Pen::new(0.5), v(0.0, 50.0), v(100.0, 50.0));
    d
}

/// What a file's layer 0 and the curves on it come in as, in a new drawing.
fn imported(import: &DxfImport) -> Document {
    assert!(
        import.entities.iter().any(|e| e.layer == "0"),
        "the file has a line on layer 0"
    );
    let mut doc = Document::new();
    add_to_document(&mut doc, import);
    doc
}

#[test]
fn an_imported_dxf_shows_its_layer_0() {
    let text = drift_io::dxf::write(&cad_drawing_using_layer_0(), units());
    let doc = imported(&read(&text, 1.0).unwrap());
    assert!(layer_names(&doc).iter().any(|n| n == "0"));
    assert_eq!(curve_layers(&doc), ["0", "WALLS"]);
}

#[cfg(feature = "dwg")]
#[test]
fn an_imported_dwg_shows_its_layer_0() {
    let bytes = drift_io::cad::write_dwg(&cad_drawing_using_layer_0(), units()).unwrap();
    let doc = imported(&drift_io::dwg_import::read(&bytes, 1.0).unwrap());
    assert!(layer_names(&doc).iter().any(|n| n == "0"));
    assert_eq!(curve_layers(&doc), ["0", "Walls"]);
}

/// A drawing with no layer 0: one line, on "Layer 1".
fn cad_drawing_without_layer_0() -> Drawing {
    let mut d = Drawing::new(v(1000.0, 1000.0));
    d.layer(LayerInfo {
        name: "Layer 1".into(),
        color: Rgb([0, 0, 0]),
        width: 0.25,
        line_type: LineType::Continuous,
    });
    d.line("Layer 1", Pen::new(0.25), v(0.0, 0.0), v(100.0, 0.0));
    d
}

/// The file has a layer 0 with nothing on it, and its line on another layer.
fn has_an_empty_layer_0(back: &DxfImport, format: &str) {
    assert!(
        back.layers.iter().any(|l| l.name == "0"),
        "{format}: no layer 0 among {:?}",
        back.layers.iter().map(|l| &l.name).collect::<Vec<_>>()
    );
    assert_eq!(back.entities.len(), 1, "{format}");
    assert_ne!(back.entities[0].layer, "0", "{format}");
}

#[test]
fn every_dxf_and_dwg_export_contains_a_layer_0_empty_when_the_drawing_has_none() {
    let d = cad_drawing_without_layer_0();
    let r12 = drift_io::dxf::write(&d, units());
    has_an_empty_layer_0(&read(&r12, 1.0).unwrap(), "DXF R12");
    #[cfg(feature = "dwg")]
    {
        let dxf = String::from_utf8(drift_io::cad::write_dxf(&d, units()).unwrap()).unwrap();
        has_an_empty_layer_0(&read(&dxf, 1.0).unwrap(), "DXF R2018");
        let dwg = drift_io::cad::write_dwg(&d, units()).unwrap();
        has_an_empty_layer_0(&drift_io::dwg_import::read(&dwg, 1.0).unwrap(), "DWG");
    }
}
