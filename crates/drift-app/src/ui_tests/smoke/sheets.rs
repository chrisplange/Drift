//! Sheets: add one, set its title block, give it a logo, see the model in its view, and print
//! and export it.

use super::*;
use crate::ui_tests::import::LOGO_SVG;

/// A rectangle drawn, then a sheet added from the Sheets panel, its settings in the Inspector.
fn a_sheet(h: &mut Harness<'_, DriftApp>) -> drift_core::SheetId {
    view(h, DVec2::new(-10.0, -10.0), DVec2::new(130.0, 90.0));
    rectangle(h, DVec2::ZERO, DVec2::new(120.0, 80.0), Fill::Solid);
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    whole_view(h);
    h.run();
    h.run();
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.state().current_sheet().expect("a sheet")
}

/// What a sheet's page draws outside its frame and title block.
fn drawn(h: &mut Harness<'_, DriftApp>, page: usize) -> usize {
    let painter = painter(h);
    let pages = h.state_mut().pages(&painter);
    pages[page]
        .items
        .iter()
        .filter(|i| i.layer != drift_draft::sheet::FRAME_LAYER)
        .count()
}

#[test]
fn smoke_sheet_with_a_view_title_block_and_export() {
    let Some(mut h) = harness() else { return };
    let id = a_sheet(&mut h);
    // A view of the model on it, which the page draws.
    assert_eq!(placed(&h, id).len(), 1, "a view of the model");
    assert!(drawn(&mut h, 0) > 0, "the rectangle is on the page");
    // The title block, from its menu.
    h.get_by_value("Modern").click();
    h.run();
    h.get_by_label("Border only").click();
    h.run();
    let sheet = |h: &Harness<'_, DriftApp>| h.state().editor.doc.sheet(id).unwrap().clone();
    assert_eq!(sheet(&h).title_block, drift_core::TitleBlock::Border);
    h.state_mut().undo();
    h.run();
    assert_eq!(sheet(&h).title_block, drift_core::TitleBlock::Modern);
    save_png("drift_smoke_sheet.png", &mut h);
    // PDF and SVG of it (what print sends to the viewer is the PDF).
    let painter = painter(&h);
    let pdf = h.state_mut().pdf_bytes(&painter);
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.starts_with("%PDF-1.4"));
    assert!(
        text.contains("/MediaBox [0 0 1190.551 841.89]"),
        "A3 landscape"
    );
    let svg = h.state_mut().svg_text(&painter);
    assert!(svg.contains(r#"width="420mm" height="297mm""#));
    // With two sheets, Print asks which first; Cancel prints nothing.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::P);
    h.run();
    h.run();
    assert!(
        h.state().output_dialog.is_some(),
        "Print asks for the sheets"
    );
    assert!(h.query_by_label("Print…").is_some());
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().output_dialog.is_none());
}

/// #151: an SVG chosen with a sheet's Logo → Choose SVG… is the logo, not a drawing to import.
#[test]
fn smoke_sheet_logo_chosen_as_svg_is_the_logo() {
    let Some(mut h) = harness() else { return };
    a_sheet(&mut h);
    let entities = h.state().editor.doc.iter().count();
    h.get_by_label("Choose SVG…").click();
    h.run();
    // (The file dialog is the system's: hand over the file it would.)
    let path = temp_file("logo", "logo.svg", LOGO_SVG.as_bytes());
    h.state().files.open_path(&path);
    h.run_steps(3);
    let app = h.state();
    assert!(app.pending_import.is_none(), "no import dialog");
    assert!(!app.editor.doc.logo().is_empty(), "it is the logo");
    assert_eq!(app.editor.doc.iter().count(), entities, "nothing drawn");
    assert!(h.query_by_label("Replace…").is_some());
    save_png("drift_smoke_sheet_logo.png", &mut h);
    let _ = std::fs::remove_file(&path);
}

/// #208: right-click on a picked view offers its actions. In the model: Delete view…, which
/// asks first (it is on a sheet) and is one undo step; on the sheet: Remove from sheet.
#[test]
fn smoke_sheet_view_right_click_menu() {
    let Some(mut h) = harness() else { return };
    let sheet = a_sheet(&mut h);
    let (view, _) = h.state().editor.doc.views().next().expect("a view");
    let view_count = |h: &Harness<'_, DriftApp>| h.state().editor.doc.views().count();

    // On the sheet: right-click the view, Remove from sheet; it stays in the model.
    h.state_mut().show_sheet(Some(sheet));
    h.run();
    let centre = h.state().canvas_rect.center();
    right_click(&mut h, centre);
    // (The Inspector has a Remove from sheet button too: the menu's is by the pointer.)
    h.get_all_by_label("Remove from sheet")
        .min_by_key(|n| (n.rect().left_top() - centre).length() as i32)
        .unwrap()
        .click();
    h.run();
    assert!(placed(&h, sheet).is_empty(), "off the sheet");
    assert_eq!(view_count(&h), 1, "still in the model");
    h.state_mut().undo();
    h.run();
    assert_eq!(placed(&h, sheet).len(), 1, "undo puts it back");

    // In the model: pick the view's crop, right-click, Delete view…, and confirm.
    h.state_mut().sheet_preview = false;
    h.state_mut().zoom_to_view(view);
    h.run();
    h.run();
    assert_eq!(h.state().picked_view, Some(view));
    let corner = h.state().canvas_rect.left_top() + egui::vec2(40.0, 40.0);
    right_click(&mut h, corner);
    save_png("drift_smoke_view_menu.png", &mut h);
    h.get_by_label("Delete view…").click();
    h.run();
    assert_eq!(view_count(&h), 1, "on a sheet, it asks first");
    h.get_by_label("Delete it").click();
    h.run();
    assert_eq!(view_count(&h), 0, "the view is gone");
    assert!(placed(&h, sheet).is_empty(), "and off its sheet");
    h.state_mut().undo();
    h.run();
    assert_eq!(view_count(&h), 1, "one undo brings it back");
    assert_eq!(placed(&h, sheet).len(), 1, "on its sheet");
}

/// #228: views on a sheet picked together (Shift-click, a box, Select all) move together,
/// keeping their places relative to each other, in one undo step; arrows nudge them all.
#[test]
fn smoke_sheet_several_views_move_together() {
    let Some(mut h) = harness() else { return };
    let id = a_sheet(&mut h);
    // Two small views of a corner of the rectangle, side by side.
    for (name, at) in [
        ("A", DVec2::new(90.0, 200.0)),
        ("B", DVec2::new(170.0, 200.0)),
    ] {
        let view = {
            let ed = &mut h.state_mut().editor;
            ed.doc.begin();
            let crop = drift_core::Bounds {
                min: DVec2::ZERO,
                max: DVec2::new(40.0, 30.0),
            };
            let v = ed.doc.add_view(drift_core::View::new(name, crop, 1.0));
            ed.commit("View");
            v
        };
        h.state_mut().place_view(id, view, Some(at));
    }
    h.state_mut().show_sheet(Some(id));
    h.run();
    h.run();
    let first = placed(&h, id);
    assert_eq!(first.len(), 3);
    let on_screen = |h: &Harness<'_, DriftApp>, i: usize| {
        let app = h.state();
        let at = placed(h, id)[i].frame.at;
        app.paper_to_preview(app.canvas_rect, at)
    };
    // A click on A, Shift-click on B: both picked.
    let a = on_screen(&h, 1);
    click_with(&mut h, a, EguiModifiers::NONE);
    let b = on_screen(&h, 2);
    click_with(&mut h, b, EguiModifiers::SHIFT);
    let picked = |h: &Harness<'_, DriftApp>| {
        let app = h.state();
        app.picked_views(app.editor.doc.sheet(id).unwrap())
    };
    assert_eq!(picked(&h), [1, 2].into());
    assert!(
        h.query_by_label("2 views").is_some(),
        "the Inspector says so"
    );
    // A drag of A takes B along (Ctrl: no lining up, so the offset is the pointer's).
    let to = a + egui::vec2(-40.0, 30.0);
    h.event(Event::ModifiersChanged(EguiModifiers::COMMAND));
    drag_steps(&mut h, a, to, 6, None);
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    let now = placed(&h, id);
    let moved = now[1].frame.at - first[1].frame.at;
    assert!(moved.length() > 5.0, "A moved: {moved:?}");
    assert!(
        (now[2].frame.at - first[2].frame.at - moved).length() < 1e-9,
        "B moved as far"
    );
    assert_eq!(now[0].frame, first[0].frame, "the other view stays");
    assert_eq!(picked(&h), [1, 2].into(), "still both picked");
    // The arrow keys nudge both.
    h.key_press(Key::ArrowRight);
    h.run();
    let nudged = placed(&h, id);
    for i in [1, 2] {
        assert_eq!(nudged[i].frame.at, now[i].frame.at + DVec2::X);
    }
    // One undo for the nudge, one for the drag.
    undo(&mut h);
    undo(&mut h);
    assert_eq!(placed(&h, id), first);
    // A box from the paper's corner round both picks them; Select all picks every view.
    h.key_press(Key::Escape);
    h.run();
    assert!(picked(&h).is_empty());
    let (a, b) = (on_screen(&h, 1), on_screen(&h, 2));
    let rect = h.state().canvas_rect;
    let from = egui::pos2(a.x - 60.0, a.y - 60.0).max(rect.min + egui::vec2(4.0, 4.0));
    drag_steps(&mut h, from, b + egui::vec2(60.0, 60.0), 4, None);
    assert_eq!(picked(&h), [1, 2].into(), "the box");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    h.run();
    assert_eq!(picked(&h), [0, 1, 2].into(), "Select all");
}

/// A new sheet starts empty: no view is made or placed. The Inspector offers one for the whole
/// drawing, and one undo takes it off again.
#[test]
fn smoke_sheet_a_new_sheet_is_empty() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(130.0, 90.0));
    rectangle(&mut h, DVec2::ZERO, DVec2::new(120.0, 80.0), Fill::Solid);
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    let id = h.state().current_sheet().expect("a sheet");
    assert!(placed(&h, id).is_empty(), "nothing on it");
    assert_eq!(h.state().editor.doc.views().count(), 0, "no view made");
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.get_by_label("Add a view of the whole drawing").click();
    h.run();
    h.run();
    assert_eq!(placed(&h, id).len(), 1);
    undo(&mut h);
    assert!(placed(&h, id).is_empty());
}
