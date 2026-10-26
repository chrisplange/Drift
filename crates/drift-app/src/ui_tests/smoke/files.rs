//! Files: save, close and open again; an autosaved session this Drift can't restore kept as
//! a file of its own; a file from a newer Drift opened and saved with its extras kept (#165).

use super::*;
use crate::ui_tests::import::drop_file;

#[test]
fn smoke_files_save_close_and_reopen() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let drawn = positions(&h);
    let fills = h.state().editor.doc.fills().count();
    // Saved to its file (Save As's dialog is the system's: the path it would give).
    let dir = scratch_dir("smoke-files");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Saved.drift");
    h.state_mut().file.path = Some(path.clone());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    assert!(path.exists(), "written");
    assert!(!h.state().is_dirty(), "saved");
    // Closed: File → New, with nothing to ask.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert!(h.state().pending.is_none(), "nothing unsaved to ask about");
    assert!(h.state().editor.doc.is_empty());
    // Open recent → Saved.drift: the drawing as it was.
    open_menu(&mut h, "File");
    h.get_by_label_contains("Open recent").hover();
    h.run();
    h.run();
    h.get_by_label("Saved.drift").click();
    h.run_steps(3);
    let app = h.state();
    assert_eq!(app.file.name, "Saved.drift");
    assert_eq!(app.file.path.as_deref(), Some(path.as_path()));
    assert!(!app.is_dirty());
    assert_eq!(positions(&h), drawn);
    assert_eq!(h.state().editor.doc.fills().count(), fills);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An autosaved session from a newer Drift is kept as a file of its own, said in a window,
/// and never autosaved over.
#[test]
fn smoke_files_recovered_session_is_kept() {
    let Some(mut h) = harness() else { return };
    let (mut storage, text) = newer_session();
    let dir = scratch_dir("smoke-recovered");
    h.state_mut()
        .restore_session_keeping_in(&storage, Some(dir.clone()));
    h.run();
    let kept: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), text);
    assert!(h.state().unrestored.is_some(), "said so");
    assert!(h.state().editor.doc.is_empty(), "a new drawing instead");
    h.get_by_label("OK").click();
    h.run();
    assert!(h.state().unrestored.is_none());
    eframe::App::save(h.state_mut(), &mut storage);
    assert_eq!(
        std::fs::read_to_string(&kept[0]).unwrap(),
        text,
        "untouched"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// #165: a file with a field and a table this Drift doesn't know opens, and saving it after an
/// edit writes them back.
#[test]
fn smoke_files_unknown_fields_are_saved_back() {
    let Some(mut h) = harness() else { return };
    let plain = drift_io::save(&drift_core::demo::mounting_plate(), &Default::default());
    let point = r#"{"type":"point","id":"e1","#;
    assert_eq!(plain.matches(point).count(), 1);
    let text = plain
        .replacen(point, r#"{"type":"point","id":"e1","weld":"fillet","#, 1)
        .replacen('{', r#"{"survey":{"datum":"OD"},"#, 1);
    let dir = scratch_dir("smoke-unknown");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Newer.drift");
    std::fs::write(&path, &text).unwrap();
    // Dropped on the window, it opens as the drawing.
    drop_file(&mut h, &path);
    h.run_steps(2);
    assert_eq!(h.state().file.name, "Newer.drift");
    assert!(!h.state().editor.doc.is_empty());
    // An edit somewhere else, then Ctrl+S: no question, and the extras are still there.
    view(&mut h, DVec2::new(-50.0, -50.0), DVec2::new(250.0, 150.0));
    rectangle(
        &mut h,
        DVec2::new(160.0, 100.0),
        DVec2::new(200.0, 130.0),
        Fill::No,
    );
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    assert!(
        h.state().lossy_save.is_none(),
        "nothing is lost: no question"
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    assert_ne!(saved, text, "the edit is saved");
    assert!(saved.contains(r#""weld":"fillet""#), "the unknown field");
    assert!(saved.contains(r#""datum":"OD""#), "the unknown table");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The record types (group 0) in a DXF file.
fn dxf_kinds(dxf: &str) -> Vec<&str> {
    let lines: Vec<&str> = dxf.lines().map(str::trim).collect();
    lines
        .chunks(2)
        .filter(|p| p.len() == 2 && p[0] == "0")
        .map(|p| p[1])
        .collect()
}

#[test]
fn smoke_files_dxf_export_opens_in_autocad_and_comes_back() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-100.0, -60.0), DVec2::new(120.0, 80.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    // An ellipse, a pen curve and text outside ASCII: AutoCAD discarded the whole R12 file
    // at the first ELLIPSE or SPLINE, and the text came out as question marks.
    let v = DVec2::new;
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let [c, m, n] = [v(-50.0, 0.0), v(-20.0, 10.0), v(-55.0, 15.0)].map(|p| ed.doc.add_point(p));
    ed.doc.add_ellipse(c, m, n).unwrap();
    let p = [
        v(10.0, -40.0),
        v(10.0, -10.0),
        v(60.0, -5.0),
        v(70.0, -40.0),
    ]
    .map(|q| ed.doc.add_point(q));
    ed.doc.add_bezier(p[0], p[1], p[2], p[3]).unwrap();
    ed.doc.add_text(drift_core::SketchText::new(
        "Café ½ – 2 m²",
        v(0.0, 50.0),
        5.0,
    ));
    ed.commit("Shapes");
    h.run();
    let painter = painter(&h);
    let dxf = h.state_mut().dxf_text(&painter);
    assert!(dxf.contains("AC1009"));
    let newer: Vec<&str> = dxf_kinds(&dxf)
        .into_iter()
        .filter(|k| matches!(*k, "ELLIPSE" | "SPLINE" | "LWPOLYLINE" | "MTEXT" | "HATCH"))
        .collect();
    assert!(newer.is_empty(), "entities R12 doesn't have: {newer:?}");
    assert!(dxf.is_ascii() && !dxf.contains('?'));
    // Imported back with File → Import…, the text reads the same and the curves arrive.
    let curves_before = curves(&h, &ids(&h)).len();
    let path = temp_file("dxf-export", "export.dxf", dxf.as_bytes());
    open_menu(&mut h, "File");
    h.get_by_label("Import…").click();
    h.run();
    h.state().files.open_path(&path);
    h.run_steps(3);
    h.get_by_label("Import").click();
    h.run_steps(3);
    let texts: Vec<String> = h
        .state()
        .editor
        .doc
        .texts()
        .map(|(_, t)| t.text.clone())
        .collect();
    assert_eq!(
        texts.iter().filter(|t| *t == "Café ½ – 2 m²").count(),
        2,
        "{texts:?}"
    );
    assert!(
        curves(&h, &ids(&h)).len() > curves_before + 6,
        "the curves came back"
    );
    let _ = std::fs::remove_file(&path);
}
