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

/// File → Export → DWG / DXF… offers DWG and DXF for AutoCAD 2018 and DXF R12; the DWG
/// comes back with its fills, hatches, ellipse and a mirrored copy of a block whose hatch is
/// in the block.
#[test]
fn smoke_files_dwg_export_keeps_fills_hatches_and_blocks() {
    use crate::output::CadFormat;
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-80.0, -60.0), DVec2::new(140.0, 80.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let (sides, hatch) = rectangle(
        &mut h,
        DVec2::new(60.0, 0.0),
        DVec2::new(100.0, 30.0),
        Fill::Hatch,
    );
    let v = DVec2::new;
    let ed = &mut h.state_mut().editor;
    ed.selection.extend(sides);
    ed.selected_fills.extend(hatch);
    let copy = ed.make_block("Panel").unwrap();
    let mut mirrored = *ed.doc.instance(copy).unwrap();
    mirrored.mirror = true;
    ed.doc.begin();
    ed.doc.set_instance(copy, mirrored).unwrap();
    let [c, m, n] = [v(-50.0, 0.0), v(-20.0, 10.0), v(-55.0, 15.0)].map(|p| ed.doc.add_point(p));
    ed.doc.add_ellipse(c, m, n).unwrap();
    ed.commit("Shapes");
    h.run();
    let block_fills = |h: &Harness<'_, DriftApp>| -> usize {
        h.state()
            .editor
            .doc
            .blocks()
            .map(|(_, b)| b.fills.len())
            .sum()
    };
    assert_eq!(block_fills(&h), 1, "the hatch went into the block");
    // The dialog offers the three, DWG first; Cancel closes it.
    open_menu(&mut h, "File");
    h.get_by_label_contains("Export").click();
    h.run();
    h.get_by_label("DWG / DXF…").click();
    h.run();
    assert!(h.state().cad_dialog);
    assert_eq!(h.state().cad_format, CadFormat::Dwg);
    h.get_by_label(CadFormat::DxfR12.label()).click();
    h.run();
    assert_eq!(h.state().cad_format, CadFormat::DxfR12);
    h.get_by_label(CadFormat::Dwg.label()).click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert!(!h.state().cad_dialog);
    // The DWG, imported back with File → Import….
    let painter = painter(&h);
    let dwg = h.state_mut().cad_bytes(&painter, CadFormat::Dwg).unwrap();
    assert!(dwg.starts_with(b"AC1032"));
    let doc = &h.state().editor.doc;
    let fills_before = doc.fills().count();
    let copies_before = doc.instances().count();
    let path = temp_file("dwg-export", "export.dwg", &dwg);
    open_menu(&mut h, "File");
    h.get_by_label("Import…").click();
    h.run();
    h.state().files.open_path(&path);
    h.run_steps(3);
    h.get_by_label("Import").click();
    h.run_steps(3);
    let doc = &h.state().editor.doc;
    assert_eq!(doc.fills().count(), fills_before + 1, "the solid fill");
    assert_eq!(doc.instances().count(), copies_before + 1, "the copy");
    assert_eq!(block_fills(&h), 2, "the hatch, in its block");
    let doc = &h.state().editor.doc;
    assert!(
        doc.instances().filter(|(_, c)| c.mirror).count() == 2,
        "still mirrored"
    );
    let ellipses = doc
        .iter()
        .filter(|(id, _)| matches!(doc.shape(*id), Some(drift_core::Shape::Ellipse { .. })))
        .count();
    assert_eq!(ellipses, 2, "a true ellipse");
    let _ = std::fs::remove_file(&path);
}

/// File → Import… of `name` with `contents`, as the system's file dialog would hand it over.
fn import_from_menu(h: &mut Harness<'_, DriftApp>, test: &str, name: &str, contents: &[u8]) {
    let path = temp_file(test, name, contents);
    open_menu(h, "File");
    h.get_by_label("Import…").click();
    h.run();
    h.state().files.open_path(&path);
    h.run_steps(3);
    h.get_by_label("Import").click();
    h.run_steps(3);
    let _ = std::fs::remove_file(&path);
}

/// A block with copies on two layers exports as one block whose copies each show their own
/// layer's look (DWG and R12), and R12 keeps "Walls new" and "walls_new" two layers. Each
/// export is imported into a new drawing (into the same one, R12's upper-case WALLS_NEW
/// would join the drawing's "walls_new", as names are matched ignoring case).
#[test]
fn smoke_files_cad_export_block_copies_keep_their_own_layer_look() {
    use crate::output::CadFormat;
    use drift_core::{Layer, LineType, LineWeight, Rgb};
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(140.0, 60.0));
    let (sides, _) = rectangle(&mut h, DVec2::ZERO, DVec2::new(20.0, 20.0), Fill::No);
    let (red, blue) = (Rgb([255, 0, 0]), Rgb([0, 0, 255]));
    let ed = &mut h.state_mut().editor;
    ed.selection.extend(sides);
    let copy = ed.make_block("Square").unwrap();
    ed.doc.begin();
    let walls = ed.doc.add_layer(Layer {
        color: Some(red),
        weight: LineWeight(50),
        ..Layer::new("Walls new")
    });
    let hidden = ed.doc.add_layer(Layer {
        color: Some(blue),
        weight: LineWeight(18),
        line_type: LineType::Dashed,
        ..Layer::new("walls_new")
    });
    let mut first = *ed.doc.instance(copy).unwrap();
    first.layer = walls;
    ed.doc.set_instance(copy, first).unwrap();
    let mut second = first;
    second.at += DVec2::new(100.0, 0.0);
    second.layer = hidden;
    // (A new copy goes on the layer in use; then it is moved.)
    let second_id = ed.doc.add_instance(second).unwrap();
    ed.doc.set_instance(second_id, second).unwrap();
    ed.commit("Copies on two layers");
    h.run();
    let painter = painter(&h);
    let files = [CadFormat::Dwg, CadFormat::DxfR12]
        .map(|f| (f, h.state_mut().cad_bytes(&painter, f).unwrap()));
    for (format, bytes) in files {
        let name = if format == CadFormat::Dwg {
            "copies.dwg"
        } else {
            "copies.dxf"
        };
        let doc = h.state().new_document();
        h.state_mut()
            .replace_document(doc, "new".into(), None, Default::default(), true);
        h.run();
        import_from_menu(&mut h, "cad-copies", name, &bytes);
        let doc = &h.state().editor.doc;
        let copies: Vec<_> = doc.instances().map(|(_, c)| *c).collect();
        assert_eq!(copies.len(), 2, "{name}: both copies");
        assert_eq!(
            copies[0].block, copies[1].block,
            "{name}: one block, not one per look"
        );
        assert_eq!(doc.blocks().count(), 1, "{name}");
        let mut colours: Vec<_> = copies
            .iter()
            .map(|c| doc.layer(c.layer).and_then(|l| l.color))
            .collect();
        colours.sort_unstable_by_key(|c| c.map(|c| c.0));
        assert_eq!(
            colours,
            [Some(blue), Some(red)],
            "{name}: each on its own layer"
        );
        let block = doc.block(copies[0].block).unwrap();
        assert!(
            block.items.iter().all(|it| it.appearance.is_by_layer()),
            "{name}: the block's sides take their copy's look"
        );
    }
}

/// Equal dashes and a 0.30 mm line come in alike from SVG and DXF: dashed, 0.30 mm.
#[test]
fn smoke_files_dashed_line_comes_in_alike_from_svg_and_dxf() {
    use drift_core::{LineType, LineWeight};
    let Some(mut h) = harness() else { return };
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm"
 viewBox="0 0 100 100"><path d="M10 50 L60 50" stroke="black" fill="none"
 stroke-width="0.3" stroke-dasharray="6 2 6 2"/></svg>"#;
    let dxf = [
        "0", "SECTION", "2", "TABLES", "0", "TABLE", "2", "LTYPE", "0", "LTYPE", "2", "EVEN", "70",
        "0", "3", "Even", "72", "65", "73", "4", "40", "16", "49", "6", "49", "-2", "49", "6",
        "49", "-2", "0", "ENDTAB", "0", "ENDSEC", "0", "SECTION", "2", "ENTITIES", "0", "LINE",
        "8", "L", "6", "EVEN", "370", "30", "10", "0", "20", "0", "30", "0", "11", "50", "21", "0",
        "31", "0", "0", "ENDSEC", "0", "EOF",
    ]
    .join("\n");
    for (name, contents) in [
        ("dashed.svg", svg.as_bytes()),
        ("dashed.dxf", dxf.as_bytes()),
    ] {
        let before = ids(&h);
        import_from_menu(&mut h, "dashes", name, contents);
        let doc = &h.state().editor.doc;
        let new: Vec<_> = curves(&h, &ids(&h))
            .into_iter()
            .filter(|id| !before.contains(id))
            .collect();
        assert_eq!(new.len(), 1, "{name}: the line");
        let e = doc.get(new[0]).unwrap();
        let layer = doc.layer(e.layer).unwrap();
        let line_type = e.appearance.line_type.unwrap_or(layer.line_type);
        let weight = e.appearance.weight.unwrap_or(layer.weight);
        assert_eq!(line_type, LineType::Dashed, "{name}");
        assert_eq!(weight, LineWeight(30), "{name}");
    }
}

/// An edit from outside the canvas (the AI link, say), which a file picker doesn't stop.
fn edit_from_link(h: &mut Harness<'_, DriftApp>) {
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    ed.doc.add_point(DVec2::new(5.0, 5.0));
    ed.commit("Point");
}

/// Save As: the picker holds the canvas like a modal dialog, and an edit made while it is
/// open isn't in the file, so it stays unsaved (#218).
#[test]
fn smoke_files_edits_during_save_as_stay_unsaved() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    // Save As: the drawing as it is now goes to the picker (a stand-in here).
    let picker = h.state().files.picker();
    h.state_mut().save_as();
    h.run();
    assert!(h.query_by_label("Waiting for the file dialog").is_some());
    // The canvas waits: a click on a side picks nothing.
    click_world(&mut h, DVec2::new(20.0, 0.0));
    assert!(
        h.state().editor.selection.is_empty(),
        "the canvas is blocked"
    );
    edit_from_link(&mut h);
    // The picker answers: written as it was when Save As was chosen.
    let path = scratch_dir("smoke-save-as").join("Saved.drift");
    h.state().files.deliver(drift_platform::FileEvent::Saved {
        name: "Saved.drift".into(),
        path: Some(path.clone()),
    });
    drop(picker);
    h.run_steps(2);
    let app = h.state();
    assert_eq!(app.file.path.as_deref(), Some(path.as_path()));
    assert!(app.is_dirty(), "the point added meanwhile isn't saved");
    assert!(h.query_by_label("Waiting for the file dialog").is_none());
}

/// Open: an edit made while the picker is open isn't dropped without asking (#218).
#[test]
fn smoke_files_edits_during_open_are_asked_about() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(60.0, 40.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::No);
    let saved = h.state().editor.doc.revision();
    h.state_mut().file.clean_revision = Some(saved);
    // File → Open with nothing unsaved: straight to the picker (a stand-in here).
    let picker = h.state().files.picker();
    h.state_mut().request(crate::Pending::Open);
    h.run();
    assert!(h.state().pending.is_none());
    edit_from_link(&mut h);
    let other = h.state().new_document();
    let text = drift_io::save(&other, &drift_io::Metadata::default());
    h.state().files.deliver(drift_platform::FileEvent::Opened {
        name: "Other.drift".into(),
        path: None,
        contents: text.into_bytes(),
    });
    drop(picker);
    h.run_steps(2);
    // Asked first; the drawing is still there.
    assert_eq!(h.state().pending, Some(crate::Pending::Opened));
    assert_ne!(h.state().file.name, "Other.drift");
    assert!(!h.state().editor.doc.is_empty());
    h.get_by_label("Don't save").click();
    h.run_steps(2);
    let app = h.state();
    assert_eq!(app.file.name, "Other.drift");
    assert!(app.editor.doc.is_empty());
    assert!(!app.is_dirty());
}

/// A drawing holding a value that isn't a number isn't saved: the file would never open
/// again. The status line names the point (#215).
#[test]
fn smoke_files_non_finite_is_not_saved() {
    let Some(mut h) = harness() else { return };
    let dir = scratch_dir("smoke-non-finite");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Broken.drift");
    std::fs::write(&path, b"the good version").unwrap();
    h.state_mut().file.path = Some(path.clone());
    let ed = &mut h.state_mut().editor;
    ed.doc.begin();
    let bad = ed.doc.add_point(DVec2::new(f64::NAN, 0.0));
    ed.commit("Point");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    assert_eq!(std::fs::read(&path).unwrap(), b"the good version");
    let status = h.state().status.as_ref().map(|s| s.text.clone());
    assert!(
        status
            .as_deref()
            .is_some_and(|s| s.starts_with("Not saved") && s.contains(&bad.to_string())),
        "{status:?}"
    );
    assert!(h.state().is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}
