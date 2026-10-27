//! Import: File → Import… for an SVG, a PDF and a DXF (synthetic files), the choices a dropped
//! drawing offers, and a dropped file that can't be read leaving the drawing as it was.

use super::*;
use crate::app::import_ui::ImportAs;
use crate::ui_tests::import::{LINE_SVG, drop_file, line_dxf, pdf_with_picture};

#[test]
fn smoke_import_svg_pdf_and_dxf_from_the_file_menu() {
    let Some(mut h) = harness() else { return };
    let files = [
        ("plan.svg", LINE_SVG.as_bytes().to_vec()),
        ("plan.pdf", pdf_with_picture()),
        ("plan.dxf", line_dxf(100.0).into_bytes()),
    ];
    for (i, (name, contents)) in files.into_iter().enumerate() {
        open_menu(&mut h, "File");
        h.get_by_label("Import…").click();
        h.run();
        assert_eq!(h.state().import_as, Some(ImportAs::Geometry), "{name}");
        // (The file dialog is the system's: hand over the file it would.)
        let path = temp_file("import", name, &contents);
        h.state().files.open_path(&path);
        h.run_steps(3);
        let pending = h
            .state()
            .pending_import
            .as_ref()
            .expect("the import dialog");
        assert_eq!(pending.mode, ImportAs::Geometry, "{name}");
        h.get_by_label("Import").click();
        h.run_steps(3);
        assert!(h.state().pending_import.is_none(), "{name}");
        assert_eq!(lines(&h), i + 1, "{name}'s line came in");
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn smoke_import_drop_dialog_choices_and_an_unreadable_drop() {
    let Some(mut h) = harness() else { return };
    let dxf = temp_file("drop", "wall.dxf", line_dxf(100.0).as_bytes());
    drop_file(&mut h, &dxf);
    // A dropped drawing asks how it comes in.
    for choice in ["Open as new drawing", "Import into this drawing…", "Cancel"] {
        assert!(h.query_by_label(choice).is_some(), "{choice}");
    }
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().dropped.is_none() && h.state().pending_import.is_none());
    assert_eq!(lines(&h), 0, "Cancel brings nothing in");
    drop_file(&mut h, &dxf);
    h.get_by_label("Import into this drawing…").click();
    h.run_steps(3);
    let pending = h
        .state()
        .pending_import
        .as_ref()
        .expect("the import dialog");
    assert_eq!(pending.mode, ImportAs::Geometry);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(lines(&h), 1);
    // A dropped sketch that isn't one, and a PDF that isn't one: the drawing stays.
    h.state_mut().file.clean_revision = Some(h.state().editor.doc.revision());
    let name = h.state().file.name.clone();
    let recent = h.state().recent.clone();
    let doc = h.state().editor.doc.clone();
    let bad = temp_file("drop", "broken.drift", b"not a drawing");
    drop_file(&mut h, &bad);
    let pdf = temp_file("drop", "broken.pdf", b"%PDF-1.4 nothing here");
    drop_file(&mut h, &pdf);
    if h.query_by_label("Open as new drawing").is_some() {
        h.get_by_label("Open as new drawing").click();
        h.run_steps(3);
    }
    let app = h.state();
    assert!(app.pending_import.is_none());
    assert_eq!(app.editor.doc, doc, "the drawing is untouched");
    assert_eq!(app.file.name, name);
    assert_eq!(app.recent, recent, "not a recent file");
    for p in [dxf, bad, pdf] {
        let _ = std::fs::remove_file(p);
    }
}

/// A door block placed twice: as drawn, and mirrored the way AutoCAD writes it (41 = −1 and
/// no 42, which came in turned 180°).
fn doors_dxf() -> String {
    [
        "0",
        "SECTION",
        "2",
        "HEADER",
        "9",
        "$INSUNITS",
        "70",
        "4",
        "0",
        "ENDSEC", //
        "0",
        "SECTION",
        "2",
        "BLOCKS",
        "0",
        "BLOCK",
        "2",
        "DOOR",
        "10",
        "0",
        "20",
        "0", //
        "0",
        "LINE",
        "8",
        "0",
        "10",
        "0",
        "20",
        "0",
        "11",
        "0",
        "21",
        "900", //
        "0",
        "ARC",
        "8",
        "0",
        "10",
        "0",
        "20",
        "0",
        "40",
        "900",
        "50",
        "0",
        "51",
        "90", //
        "0",
        "ENDBLK",
        "0",
        "ENDSEC", //
        "0",
        "SECTION",
        "2",
        "ENTITIES", //
        "0",
        "INSERT",
        "8",
        "DOORS",
        "2",
        "DOOR",
        "10",
        "0",
        "20",
        "0", //
        "0",
        "INSERT",
        "8",
        "DOORS",
        "2",
        "DOOR",
        "10",
        "3000",
        "20",
        "0",
        "41",
        "-1", //
        "0",
        "ENDSEC",
        "0",
        "EOF",
    ]
    .join("\n")
        + "\n"
}

/// File → Import… of `path` into the drawing, through the dialog.
fn import_through_the_dialog(h: &mut Harness<'_, DriftApp>, path: &std::path::PathBuf) {
    open_menu(h, "File");
    h.get_by_label("Import…").click();
    h.run();
    h.state().files.open_path(path);
    h.run_steps(3);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert!(h.state().pending_import.is_none());
}

#[test]
fn smoke_import_dxf_mirrored_door_stays_mirrored_and_blocks_stay_their_own() {
    let Some(mut h) = harness() else { return };
    let path = temp_file("doors", "doors.dxf", doors_dxf().as_bytes());
    import_through_the_dialog(&mut h, &path);
    let doc = &h.state().editor.doc;
    let copies: Vec<drift_core::Instance> = doc.instances().map(|(_, i)| *i).collect();
    assert_eq!(copies.len(), 2, "both doors are copies of the block");
    let mirrored = copies
        .iter()
        .find(|i| i.at == DVec2::new(3000.0, 0.0))
        .expect("the second door");
    assert!(mirrored.mirror, "mirrored");
    assert!(
        mirrored.angle.abs() < 1e-9,
        "not turned: {}",
        mirrored.angle
    );
    // The same file again: the same DOOR block is used, not a second one.
    import_through_the_dialog(&mut h, &path);
    let doc = &h.state().editor.doc;
    assert_eq!(doc.blocks().count(), 1);
    assert_eq!(doc.instances().count(), 4);
    let _ = std::fs::remove_file(&path);
}

/// An inch drawing with ANSI31 at 16 as AutoCAD writes it (lines 2" apart), a SOLID, and a
/// block whose square is hatched solid, placed twice.
fn hatches_dxf() -> String {
    let square = |x: &str, y: &str, w: &str| -> Vec<String> {
        let (x0, y0, d): (f64, f64, f64) =
            (x.parse().unwrap(), y.parse().unwrap(), w.parse().unwrap());
        [(x0, y0), (x0 + d, y0), (x0 + d, y0 + d), (x0, y0 + d)]
            .iter()
            .flat_map(|(a, b)| {
                [
                    "10".to_owned(),
                    a.to_string(),
                    "20".to_owned(),
                    b.to_string(),
                ]
            })
            .collect()
    };
    let mut lines: Vec<String> = [
        "0",
        "SECTION",
        "2",
        "HEADER",
        "9",
        "$INSUNITS",
        "70",
        "1",
        "0",
        "ENDSEC", //
        "0",
        "SECTION",
        "2",
        "BLOCKS",
        "0",
        "BLOCK",
        "2",
        "TILE",
        "10",
        "0",
        "20",
        "0", //
        "0",
        "LWPOLYLINE",
        "8",
        "0",
        "90",
        "4",
        "70",
        "1",
    ]
    .map(String::from)
    .to_vec();
    lines.extend(square("0", "0", "10"));
    lines.extend(
        [
            "0", "HATCH", "8", "0", "2", "SOLID", "70", "1", "91", "1", "92", "3", "72", "0", "73",
            "1", "93", "4",
        ]
        .map(String::from),
    );
    lines.extend(square("0", "0", "10"));
    lines.extend(
        [
            "97", "0", "75", "0", "76", "1", "98", "0", "0", "ENDBLK", "0", "ENDSEC", //
            "0", "SECTION", "2", "ENTITIES", //
            "0", "HATCH", "8", "HATCH", "2", "ANSI31", "70", "0", "91", "1", "92", "3", "72", "0",
            "73", "1", "93", "4",
        ]
        .map(String::from),
    );
    lines.extend(square("0", "0", "100"));
    lines.extend(
        [
            "97",
            "0",
            "75",
            "0",
            "76",
            "1",
            "52",
            "0",
            "41",
            "16",
            "77",
            "0",
            "78",
            "1",
            "53",
            "45",
            "43",
            "0",
            "44",
            "0",
            "45",
            "-1.414213562373095",
            "46",
            "1.414213562373095",
            "79",
            "0",
            "98",
            "0", //
            "0",
            "SOLID",
            "8",
            "POCHE",
            "10",
            "200",
            "20",
            "0",
            "11",
            "250",
            "21",
            "0",
            "12",
            "200",
            "22",
            "50",
            "13",
            "250",
            "23",
            "50", //
            "0",
            "INSERT",
            "2",
            "TILE",
            "10",
            "400",
            "20",
            "0", //
            "0",
            "INSERT",
            "2",
            "TILE",
            "10",
            "450",
            "20",
            "0",
            "50",
            "45", //
            "0",
            "ENDSEC",
            "0",
            "EOF",
        ]
        .map(String::from),
    );
    lines.join("\n") + "\n"
}

#[test]
fn smoke_import_dxf_hatches_at_real_size_solids_and_hatched_blocks() {
    let Some(mut h) = harness() else { return };
    let path = temp_file("hatches", "hatches.dxf", hatches_dxf().as_bytes());
    import_through_the_dialog(&mut h, &path);
    let doc = &h.state().editor.doc;
    let fills: Vec<drift_core::Fill> = doc.fills().map(|(_, f)| f.clone()).collect();
    assert_eq!(fills.len(), 2, "the hatch and the SOLID");
    let hatch = fills
        .iter()
        .find_map(|f| match &f.kind {
            FillKind::Hatch(h) => Some(h.clone()),
            FillKind::Solid => None,
        })
        .expect("the hatch");
    // ANSI31 at real size: 2" apart, whatever the drawing's scale.
    assert_eq!(hatch.pattern, "ANSI31");
    assert!(hatch.model);
    let spacing = drift_draft::find("ANSI31").unwrap().min_spacing() * hatch.scale;
    assert!((spacing - 50.8).abs() < 1e-6, "{spacing}");
    // The SOLID is filled, opaque.
    let solid = fills.iter().find(|f| f.kind == FillKind::Solid).unwrap();
    assert!((solid.opacity - 1.0).abs() < 1e-9);
    // The block's hatch is its own fill, and each copy shows it.
    let (_, block) = doc.blocks().next().expect("the block");
    assert_eq!(block.fills.len(), 1);
    assert_eq!(h.state_mut().editor.copy_fills().len(), 2);
    // The drawing shows it all.
    view(
        &mut h,
        DVec2::new(-1000.0, -1000.0),
        DVec2::new(13000.0, 3000.0),
    );
    h.run();
    save_png("drift_smoke_import_hatches.png", &mut h);
    let _ = std::fs::remove_file(&path);
}

/// A block placed on a green layer whose square is on a grey layer of its own, and whose
/// cross is on layer 0.
fn layered_block_dxf() -> String {
    let line = |layer: &str, a: (i32, i32), b: (i32, i32)| {
        format!(
            "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n11\n{}\n21\n{}\n",
            a.0, a.1, b.0, b.1
        )
    };
    let layer = |name: &str, color: i32| {
        format!("0\nLAYER\n2\n{name}\n70\n0\n62\n{color}\n6\nCONTINUOUS\n")
    };
    let mut s = String::from("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n");
    s += "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n";
    s += &(layer("0", 7) + &layer("Green", 3) + &layer("Grey", 8));
    s += "0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nBOX\n10\n0\n20\n0\n";
    for (a, b) in [
        ((0, 0), (100, 0)),
        ((100, 0), (100, 100)),
        ((100, 100), (0, 100)),
        ((0, 100), (0, 0)),
    ] {
        s += &line("Grey", a, b);
    }
    s += &line("0", (0, 0), (100, 100));
    s += "0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n";
    s += "0\nINSERT\n8\nGreen\n2\nBOX\n10\n0\n20\n0\n0\nENDSEC\n0\nEOF\n";
    s
}

/// A block's curves on a layer of their own keep that layer's colour, whatever layer the
/// copy is on (grey lines in a copy on a green layer came in green); on layer 0 they take
/// the copy's.
#[test]
fn smoke_import_dxf_block_curves_keep_their_own_layers_look() {
    let Some(mut h) = harness() else { return };
    let path = temp_file("layered-block", "box.dxf", layered_block_dxf().as_bytes());
    import_through_the_dialog(&mut h, &path);
    let doc = &h.state().editor.doc;
    let (_, copy) = doc.instances().next().expect("the copy");
    assert_eq!(doc.layer(copy.layer).unwrap().name, "Green");
    let block = doc.block(copy.block).unwrap();
    let grey = Some(drift_core::Rgb([128, 128, 128]));
    let colours: Vec<_> = block.items.iter().map(|it| it.appearance.color).collect();
    assert_eq!(
        colours.iter().filter(|c| **c == grey).count(),
        4,
        "{colours:?}"
    );
    assert_eq!(
        colours.iter().filter(|c| c.is_none()).count(),
        1,
        "{colours:?}"
    );
    let _ = std::fs::remove_file(&path);
}
