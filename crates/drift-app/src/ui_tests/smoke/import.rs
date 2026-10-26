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
