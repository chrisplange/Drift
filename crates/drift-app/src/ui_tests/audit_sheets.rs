//! A walk through the sheets and views workflow, step by step, saved to
//! `target/tmp/audit-sheets/` for a usability review. Ignored in normal runs:
//!
//! ```text
//! cargo test -p drift-app audit_sheets -- --ignored
//! ```

use super::*;

fn shot(name: &str, h: &mut Harness<'_, DriftApp>) {
    h.run();
    h.run();
    let image = h.render().expect("render");
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/audit-sheets");
    std::fs::create_dir_all(&dir).expect("create target/tmp/audit-sheets");
    image
        .save(dir.join(format!("{name}.png")))
        .expect("save png");
}

fn open_example(h: &mut Harness<'_, DriftApp>) {
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().status = None;
}

/// Where a paper point is on screen in the sheet editor.
fn on_paper(h: &Harness<'_, DriftApp>, p: DVec2) -> Pos2 {
    let app = h.state();
    let rect = app.canvas_rect;
    let d = (p - app.preview.center) * f64::from(app.preview.zoom);
    rect.center() + egui::vec2(d.x as f32, -d.y as f32)
}

fn drag(h: &mut Harness<'_, DriftApp>, from: Pos2, to: Pos2, mid_shot: Option<&str>) {
    h.hover_at(from);
    h.run();
    button(h, from, true);
    h.run();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run();
    }
    if let Some(name) = mid_shot {
        shot(name, h);
    }
    button(h, to, false);
    h.run();
}

fn click(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    button(h, at, true);
    h.run();
    button(h, at, false);
    h.run();
}

#[test]
#[ignore = "writes the sheets workflow review's screenshots; run with --ignored"]
fn audit_sheets() {
    let Some(mut h) = harness() else { return };
    h.ctx.set_theme(egui::Theme::Light);
    open_example(&mut h);

    // 1. Where sheets start: the panel, before any sheet.
    h.state_mut().show_panel(Panel::Sheets);
    shot("01-panel-empty", &mut h);

    // 2. New sheet.
    h.get_all_by_label("New sheet").next().unwrap().click();
    whole_view(&mut h);
    h.run();
    h.state_mut().status = None;
    shot("02-new-sheet", &mut h);

    // 3. The Inspector while on the sheet.
    h.state_mut().show_panel(Panel::Inspector);
    shot("03-inspector-on-sheet", &mut h);

    // 4. A drawing tool picked while on the sheet (from a menu or the palette).
    h.state_mut().set_tool(ToolKind::Line);
    h.run();
    shot("04-tool-on-sheet", &mut h);
    eprintln!(
        "after picking Line on the sheet: sheet_preview = {}, tool = {:?}",
        h.state().sheet_preview,
        h.state().tool.kind()
    );
    h.key_press(Key::Escape);
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    h.state_mut().show_sheet(Some(id));
    h.run();

    // 5. Dragging on the only view (it fills the sheet): what does a drag do?
    let before = placed(&h, id)[0].center;
    let mid = on_paper(&h, DVec2::new(150.0, 160.0));
    drag(&mut h, mid, mid + egui::vec2(80.0, 0.0), None);
    let after = placed(&h, id)[0].center;
    eprintln!(
        "dragging the sheet-filling view moved the model by {:?}",
        after - before
    );
    shot("05-dragged-fill-view", &mut h);

    // 6. Add view: the region, dragged out in the model.
    h.get_all_by_label("Add view").next().unwrap().click();
    h.run();
    h.run();
    let (a, b) = (
        screen(&h, DVec2::new(55.0, 25.0)),
        screen(&h, DVec2::new(100.0, 58.0)),
    );
    drag(&mut h, a, b, Some("06a-view-region"));
    h.run();
    shot("06-added-view", &mut h);

    // 7. The new view, picked, dragged far left: it stays on the paper.
    let sheet = h.state().editor.doc.sheet(id).unwrap().clone();
    let frame = placed(&h, id)[1].frame;
    let from = on_paper(&h, frame.at);
    drag(
        &mut h,
        from,
        from + egui::vec2(-600.0, 40.0),
        Some("07-dragging-view"),
    );
    shot("08-moved-view", &mut h);

    // 8. Cropping it from a corner handle.
    let frame = placed(&h, id)[1].frame;
    let corner = on_paper(&h, frame.at + frame.size * 0.5);
    drag(&mut h, corner, corner + egui::vec2(-30.0, 20.0), None);
    shot("09-cropped-view", &mut h);

    // 9. The picked view's Inspector, and the sheet's (a click on bare paper).
    shot("10-view-inspector", &mut h);
    let area = drift_draft::sheet::drawing_area(&sheet);
    let bare = on_paper(&h, area.max - DVec2::splat(6.0));
    click(&mut h, bare);
    shot("10-paper-inspector", &mut h);
    if let Some(node) = h.query_by_label("Add revision") {
        node.scroll_to_me();
        h.run();
        shot("10-paper-inspector-lower", &mut h);
    }

    // 10. A second sheet: the tabs, and its number.
    h.get_by_label("Add a sheet").click();
    whole_view(&mut h);
    h.run();
    h.state_mut().status = None;
    shot("11-second-sheet", &mut h);
    let s2 = h.state().current_sheet().unwrap();
    let f = &h.state().editor.doc.sheet(s2).unwrap().fields;
    eprintln!("second sheet fields: {f:?}");

    // 11. The Sheets panel with two sheets.
    h.state_mut().show_panel(Panel::Sheets);
    shot("12-panel-two-sheets", &mut h);

    // 12. A section mark in the model.
    h.get_by_label("Model").click();
    h.run();
    h.get_by_label("Section mark").click();
    h.run();
    click_world(&mut h, DVec2::new(0.0, 40.0));
    click_world(&mut h, DVec2::new(120.0, 40.0));
    h.get_by_label("Select").click();
    h.run();
    h.state_mut().status = None;
    shot("13-section-mark-model", &mut h);
}
