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
