//! One way in for drawings: File → Import…, the Underlays section's Import…, and files
//! dropped on the window, all through the same import dialog. Only synthetic files.

use super::*;
use crate::app::import_ui::ImportAs;

/// A DXF drawing in millimetres: one line from the origin, `length` long along x.
fn line_dxf(length: f64) -> String {
    format!(
        "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
         0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n{length}\n21\n0\n\
         0\nENDSEC\n0\nEOF\n"
    )
}

/// An SVG page 100 × 100 mm with a 50 mm line across it.
const LINE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm"
 viewBox="0 0 100 100"><path d="M10 50 L60 50" stroke="black" fill="none"/></svg>"#;

/// A one-page PDF (200 × 200 pt) with a 2 × 2 RGB picture over its lower half and a line.
fn pdf_with_picture() -> Vec<u8> {
    let content = b"q 200 0 0 100 0 0 cm /Im Do Q 1 w 10 150 m 190 150 l S";
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
          /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
            content,
            b"\nendstream",
        ]
        .concat(),
        [
            &b"<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
               /BitsPerComponent 8 /Length 12 >>\nstream\n"[..],
            &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
            b"\nendstream",
        ]
        .concat(),
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(body);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    pdf
}

/// Write `contents` to a file of its own for this test, and return its path.
fn temp_file(test: &str, name: &str, contents: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("drift-ui-import-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

/// A file dropped on the window, as the system hands it over.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl eframe::egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Drop a file on the window.
fn drop_file(h: &mut Harness<'_, DriftApp>, path: &std::path::Path) {
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(path.to_owned())));
    h.run();
    h.run();
}

/// The lengths of the drawing's lines.
fn lines(app: &DriftApp) -> Vec<f64> {
    let doc = &app.editor.doc;
    doc.iter()
        .filter_map(|(id, _)| match doc.shape(id) {
            Some(drift_core::Shape::Line { a, b }) => Some((b - a).length()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_dropped_drawing_asks_how_it_comes_in() {
    let Some(mut h) = harness() else { return };
    let path = temp_file("ask", "wall.dxf", line_dxf(100.0).as_bytes());
    drop_file(&mut h, &path);
    assert!(h.state().dropped.is_some(), "the drop dialog is up");
    save_png("drift_ui_drop_dialog.png", &mut h);
    // Cancel leaves everything as it was.
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().dropped.is_none());
    assert!(h.state().pending_import.is_none());
    assert!(lines(h.state()).is_empty());

    // Import as geometry: the import dialog follows, set to geometry.
    drop_file(&mut h, &path);
    h.get_by_label("Import as geometry").click();
    h.run_steps(3);
    let pending = h
        .state()
        .pending_import
        .as_ref()
        .expect("the import dialog");
    assert!(!pending.underlay);
    // DWG and DXF get the same scale controls as artwork.
    h.get_by_label("Drawn at");
    h.get_by_label("Then calibrate: pick two points on it and type their real distance");
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert!(h.state().pending_import.is_none());
    let got = lines(h.state());
    assert_eq!(got.len(), 1);
    assert!((got[0] - 100.0).abs() < 1e-6, "{got:?}");

    // Underlay to trace: the import dialog follows, set to an underlay.
    drop_file(&mut h, &path);
    h.get_by_label("Underlay to trace").click();
    h.run_steps(3);
    assert!(h.state().pending_import.as_ref().expect("dialog").underlay);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(h.state().editor.doc.underlays().count(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn opening_a_dropped_drawing_as_new_guards_unsaved_changes() {
    let Some(mut h) = harness() else { return };
    // Something unsaved in the drawing open now.
    h.state_mut()
        .import_now("old.dxf", line_dxf(30.0).into_bytes(), 0.0);
    h.run();
    assert!(h.state().is_dirty());
    let path = temp_file("guard", "plan.dxf", line_dxf(100.0).as_bytes());

    // Open as new drawing asks first; Cancel keeps the drawing.
    drop_file(&mut h, &path);
    h.get_by_label("Open as new drawing").click();
    h.run_steps(2);
    assert!(h.state().pending.is_some(), "asks to save first");
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().pending.is_none());
    let got = lines(h.state());
    assert!(got.len() == 1 && (got[0] - 30.0).abs() < 1e-6, "{got:?}");

    // Don't save: the dropped drawing opens as a new drawing of its own, unsaved.
    drop_file(&mut h, &path);
    h.get_by_label("Open as new drawing").click();
    h.run_steps(2);
    h.get_by_label("Don't save").click();
    h.run_steps(4);
    let app = h.state();
    assert!(
        app.pending_import.is_none(),
        "no import dialog for a new drawing"
    );
    let got = lines(app);
    assert!(got.len() == 1 && (got[0] - 100.0).abs() < 1e-6, "{got:?}");
    assert_eq!(app.file.name, "plan.drift");
    assert!(app.file.path.is_none(), "saving asks where");
    assert!(app.is_dirty());

    // With nothing unsaved it opens straight away.
    let path2 = temp_file("guard", "second.dxf", line_dxf(70.0).as_bytes());
    h.state_mut().file.clean_revision = Some(h.state().editor.doc.revision());
    drop_file(&mut h, &path2);
    h.get_by_label("Open as new drawing").click();
    h.run_steps(4);
    assert!(h.state().pending.is_none());
    let got = lines(h.state());
    assert!(got.len() == 1 && (got[0] - 70.0).abs() < 1e-6, "{got:?}");
    assert_eq!(h.state().file.name, "second.drift");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&path2);
}

#[test]
fn a_dropped_drift_file_still_opens() {
    let Some(mut h) = harness() else { return };
    let mut doc = drift_core::Document::new();
    doc.begin();
    let a = doc.add_point(DVec2::ZERO);
    let b = doc.add_point(DVec2::new(40.0, 0.0));
    doc.add_line(a, b).unwrap();
    doc.commit("Line");
    let text = drift_io::save(&doc, &drift_io::Metadata::default());
    let path = temp_file("drift", "kept.drift", text.as_bytes());
    drop_file(&mut h, &path);
    assert!(h.state().dropped.is_none(), "no question for a sketch");
    assert_eq!(h.state().file.name, "kept.drift");
    assert_eq!(lines(h.state()).len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn import_from_the_underlays_section_starts_on_underlay() {
    let Some(mut h) = harness() else { return };
    h.state_mut().show_panel(Panel::Layers);
    h.run_steps(2);
    // There is no separate "Import underlay…" any more.
    assert!(h.query_by_label("Import underlay…").is_none());
    h.get_by_label("Import…").click();
    h.run();
    assert_eq!(h.state().import_as, Some(ImportAs::Underlay));
    // The file picked comes into the dialog set to Underlay.
    let path = temp_file("preset", "plan.svg", LINE_SVG.as_bytes());
    h.state().files.open_path(&path);
    h.run_steps(3);
    assert!(h.state().pending_import.as_ref().expect("dialog").underlay);
    save_png("drift_ui_import_underlay_preset.png", &mut h);
    h.get_by_label("Cancel").click();
    h.run();

    // File → Import… (here from the palette, by what it's for) starts on geometry.
    for words in ["trace", "underlay"] {
        h.state_mut().import_as = None;
        h.state_mut().open_palette();
        h.run();
        h.event(Event::Text(words.into()));
        h.run();
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(
            h.state().import_as,
            Some(ImportAs::Geometry),
            "\"{words}\" finds Import…"
        );
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn an_svg_underlay_has_the_scale_controls() {
    let Some(mut h) = harness() else { return };
    h.state_mut().begin_import(
        "plan.svg",
        LINE_SVG.as_bytes().to_vec(),
        Some(ImportAs::Underlay),
        0.0,
    );
    h.run_steps(3);
    // Drawn at 1 : 50 from its menu, then calibrate.
    h.get_by_value("1 : 1").click();
    h.run();
    h.get_by_label("1 : 50").click();
    h.run();
    h.get_by_label("Then calibrate: pick two points on it and type their real distance")
        .click();
    h.run();
    save_png("drift_ui_import_svg_underlay.png", &mut h);
    h.get_by_label("Import").click();
    h.run_steps(3);
    let app = h.state();
    let (_, u) = app.editor.doc.underlays().next().expect("an underlay");
    assert!((u.scale - 50.0).abs() < 1e-9, "{}", u.scale);
    // The 50 mm line on paper is 2.5 m at 1:50.
    let b = u.bounds();
    assert!((b.size().x - 2500.0).abs() < 1.0, "{b:?}");
    assert_eq!(app.tool.kind(), ToolKind::Calibrate);
}

#[test]
fn a_dxf_underlay_is_drawn_at_its_scale() {
    let Some(mut h) = harness() else { return };
    h.state_mut().begin_import(
        "plan.dxf",
        line_dxf(100.0).into_bytes(),
        Some(ImportAs::Underlay),
        0.0,
    );
    h.run_steps(3);
    h.state_mut().pending_import.as_mut().unwrap().scale = 20.0;
    h.run();
    h.get_by_label("Import").click();
    h.run_steps(3);
    let (_, u) = h
        .state()
        .editor
        .doc
        .underlays()
        .next()
        .expect("an underlay");
    assert!((u.scale - 20.0).abs() < 1e-9);
    assert!((u.bounds().size().x - 2000.0).abs() < 1e-6);
    // As geometry, drawn at 1 : 20 too.
    h.state_mut()
        .begin_import("plan.dxf", line_dxf(100.0).into_bytes(), None, 0.0);
    h.run_steps(3);
    h.state_mut().pending_import.as_mut().unwrap().scale = 20.0;
    h.run();
    h.get_by_label("Import").click();
    h.run_steps(3);
    let got = lines(h.state());
    assert!(got.len() == 1 && (got[0] - 2000.0).abs() < 1e-6, "{got:?}");
}

#[test]
fn pdf_pictures_come_in_only_when_asked() {
    let Some(mut h) = harness() else { return };
    let pictures = |h: &Harness<'_, DriftApp>| {
        h.state()
            .editor
            .doc
            .underlays()
            .map(|(_, u)| u.images.len())
            .sum::<usize>()
    };
    // On by default, as before.
    h.state_mut()
        .begin_import("photo.pdf", pdf_with_picture(), None, 0.0);
    h.run_steps(3);
    assert!(h.state().pending_import.as_ref().unwrap().images);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(pictures(&h), 1);
    // Off: the line comes in, the picture doesn't.
    h.state_mut().editor.undo();
    h.state_mut()
        .begin_import("photo.pdf", pdf_with_picture(), None, 0.0);
    h.run_steps(3);
    h.get_by_label("Bring in images as pictures").click();
    h.run();
    assert!(!h.state().pending_import.as_ref().unwrap().images);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(pictures(&h), 0);
    assert_eq!(lines(h.state()).len(), 1);
    // As an underlay too.
    h.state_mut().begin_import(
        "photo.pdf",
        pdf_with_picture(),
        Some(ImportAs::Underlay),
        0.0,
    );
    h.run_steps(3);
    h.get_by_label("Bring in images as pictures").click();
    h.run();
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(pictures(&h), 0);
    assert_eq!(h.state().editor.doc.underlays().count(), 1);
}
