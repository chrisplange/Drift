//! The Sheets tab's layout: views nest under the sheet they are on, a view's right-click menu
//! places, zooms to and deletes it, Add view sits with the Views, there is no Output section
//! (Export and Print are in the File menu), a sheet's right-click menu exports or prints
//! just that sheet, and a view under one sheet moves to another (its menu, or a drag).

use super::*;

fn right_click(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: EguiModifiers::NONE,
        });
        h.run();
    }
}

/// Where the right-click menu's `label` item is: the picked view's buttons have the same
/// names, further down; the menu opens at the pointer, `at`.
fn menu_item(h: &Harness<'_, DriftApp>, label: &str, at: Pos2) -> Option<Pos2> {
    // (Its label starts with `label`: Place names the sheet, Delete may end in "…". A wide
    // menu near the panel's edge opens to the left of the pointer.)
    h.query_all_by(|n| n.label().is_some_and(|l| l.starts_with(label)))
        .map(|n| n.rect())
        .filter(|r| r.top() >= at.y - 4.0 && r.top() < at.y + 300.0)
        .map(|r| r.center())
        .next()
}

fn click_at(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    button(h, at, true);
    h.run();
    button(h, at, false);
    h.run();
    h.run();
}

/// The Sheets tab with one sheet holding one view of the example; the view's title.
fn sheet_with_a_view(h: &mut Harness<'_, DriftApp>) -> String {
    h.get_by_label("File").click();
    h.run();
    h.get_by_label("Open example").click();
    h.run();
    h.state_mut().status = None;
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_all_by_label("New sheet").next().unwrap().click();
    h.run();
    h.state_mut().show_newest_sheet();
    whole_view(h);
    h.state_mut().status = None;
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.run();
    let doc = &h.state().drawing.editor.doc;
    let (_, view) = doc.views().next().expect("a view");
    view.title().to_owned()
}

#[test]
fn views_nest_under_their_sheet_and_the_panel_has_no_output_section() {
    let Some(mut h) = harness() else { return };
    let title = sheet_with_a_view(&mut h);
    save_png("drift_ui_sheets_panel_tree.png", &mut h);

    // The view shows twice: under its sheet, and in the Views list.
    assert_eq!(h.get_all_by_label(&title).count(), 2, "nested and listed");
    // No Output section: Export and Print are in the File menu.
    for gone in [
        "Output",
        "Export",
        "Print…",
        "PDF…",
        "SVG…",
        "DWG / DXF…",
        "Model as SVG…",
        "Model as PDF…",
    ] {
        assert!(
            h.query_by_label(gone).is_none(),
            "{gone} isn't in the panel"
        );
    }
    // Add view sits in the Views section, below the views listed there.
    // (The tool column has an Add view too: this one is in the panel, on the right.)
    let add = h
        .get_all_by_label("Add view")
        .map(|n| n.rect())
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap();
    let listed = h.get_all_by_label(&title).last().unwrap().rect();
    assert!(
        add.top() >= listed.bottom(),
        "Add view is under the Views list"
    );

    // The chevron (at the left of the sheet's row, just above its first view) closes the
    // sheet's views.
    let nested = h.get_all_by_label(&title).next().unwrap().rect();
    let chevron = egui::pos2(
        nested.left() + 9.0,
        nested.top() - crate::theme::LIST_ROW / 2.0,
    );
    h.hover_at(chevron);
    h.run();
    button(&h, chevron, true);
    h.run();
    button(&h, chevron, false);
    h.run();
    h.run();
    assert_eq!(
        h.get_all_by_label(&title).count(),
        1,
        "the sheet's views closed"
    );
    assert!(
        h.state().drawing.view.sheet_preview,
        "the chevron doesn't leave or switch the sheet"
    );
}

#[test]
fn a_views_right_click_menu_zooms_places_and_deletes() {
    let Some(mut h) = harness() else { return };
    let title = sheet_with_a_view(&mut h);
    let at = h.get_all_by_label(&title).last().unwrap().rect().center();
    right_click(&mut h, at);
    save_png("drift_ui_sheets_view_menu.png", &mut h);
    // The Views list row: Place names the sheet; nothing that needs a sheet.
    assert!(menu_item(&h, "Place again on ", at).is_some());
    assert!(menu_item(&h, "Delete view\u{2026}", at).is_some());
    assert!(menu_item(&h, "Remove from sheet", at).is_none());
    let item = menu_item(&h, "Zoom to view", at).unwrap();
    click_at(&mut h, item);
    h.run();
    assert!(
        !h.state().drawing.view.sheet_preview,
        "zoomed to it in the model"
    );

    // Delete, from the nested row: a placed view asks first.
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    let at = h.get_all_by_label(&title).next().unwrap().rect().center();
    right_click(&mut h, at);
    save_png("drift_ui_sheets_nested_view_menu.png", &mut h);
    // The one list of a view's actions, in the same order as the Inspector's: Remove sits
    // by Delete.
    for item in [
        "Place again on ",
        "Fit scale to frame",
        "Fill drawing area",
        "Zoom to view",
        "Remove from sheet",
    ] {
        assert!(menu_item(&h, item, at).is_some(), "{item}");
    }
    let item = menu_item(&h, "Delete view\u{2026}", at).unwrap();
    click_at(&mut h, item);
    h.run();
    let view = h.state().drawing.editor.doc.views().next().unwrap().0;
    assert_eq!(h.state().drawing.confirm_delete_view, Some(view));
    // Asked in a dialog, where you are; Esc keeps it.
    save_png("drift_ui_delete_view_dialog.png", &mut h);
    assert!(h.query_by_label("Cancel").is_some());
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().drawing.confirm_delete_view, None);
    assert_eq!(h.state().drawing.editor.doc.views().count(), 1);

    // Remove from sheet, from the nested row: off the sheet, still in the model.
    let at = h.get_all_by_label(&title).next().unwrap().rect().center();
    right_click(&mut h, at);
    let item = menu_item(&h, "Remove from sheet", at).unwrap();
    click_at(&mut h, item);
    h.run();
    assert_eq!(h.state().drawing.editor.doc.views().count(), 1);
    assert!(h.state().drawing.editor.doc.sheets_showing(view).is_empty());
}

#[test]
fn a_sheets_right_click_menu_exports_or_prints_just_that_sheet() {
    let Some(mut h) = harness() else { return };
    sheet_with_a_view(&mut h);
    h.get_all_by_label("New sheet").next().unwrap().click();
    h.run();
    h.state_mut().show_newest_sheet();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    let sheets: Vec<drift_core::SheetId> = h
        .state()
        .drawing
        .editor
        .doc
        .sheets()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(sheets.len(), 2);
    // A choice made in the PDF dialog, which exporting one sheet leaves as it was.
    h.state_mut().drawing.pdf_left_out.insert(sheets[0]);

    // The menu offers both (choosing one opens a real save dialog, so it isn't clicked).
    let label = crate::app::sheet_editor::sheet_label(
        h.state().drawing.editor.doc.sheet(sheets[0]).unwrap(),
    );
    let row = h
        .get_all_by_label(&label)
        .map(|n| n.rect())
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap();
    right_click(&mut h, row.center());
    save_png("drift_ui_sheet_row_menu.png", &mut h);
    assert!(h.query_by_label("Export PDF…").is_some());
    assert!(h.query_by_label("Print…").is_some());
    h.key_press(Key::Escape);
    h.run();

    let painter = h.ctx.layer_painter(egui::LayerId::background());
    let pdf = h.state_mut().sheet_pdf_bytes(&painter, sheets[0]);
    let pages = String::from_utf8_lossy(&pdf)
        .matches("/Type /Page ")
        .count();
    assert_eq!(pages, 1, "just the sheet");
    assert_eq!(
        h.state().drawing.pdf_left_out,
        [sheets[0]].into_iter().collect(),
        "the dialog's choice is kept"
    );
}

/// Two sheets, the first holding the example's view, shown in the Sheets tab; the sheets and
/// the view.
fn two_sheets(h: &mut Harness<'_, DriftApp>) -> (drift_core::SheetId, drift_core::SheetId, String) {
    let title = sheet_with_a_view(h);
    h.get_all_by_label("New sheet").next().unwrap().click();
    h.run();
    h.state_mut().show_newest_sheet();
    h.run();
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.run();
    let sheets: Vec<drift_core::SheetId> = h
        .state()
        .drawing
        .editor
        .doc
        .sheets()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(sheets.len(), 2);
    assert_eq!(
        h.state()
            .drawing
            .editor
            .doc
            .sheet(sheets[1])
            .unwrap()
            .viewports
            .len(),
        0
    );
    (sheets[0], sheets[1], title)
}

fn views_on(h: &Harness<'_, DriftApp>, sheet: drift_core::SheetId) -> usize {
    h.state()
        .drawing
        .editor
        .doc
        .sheet(sheet)
        .unwrap()
        .viewports
        .len()
}

/// Where a sheet's row is in the Sheets list (the tab bar has one of the same name, further
/// left).
fn sheet_row_at(h: &Harness<'_, DriftApp>, sheet: drift_core::SheetId) -> Pos2 {
    let label =
        crate::app::sheet_editor::sheet_label(h.state().drawing.editor.doc.sheet(sheet).unwrap());
    h.get_all_by_label(&label)
        .map(|n| n.rect())
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap()
        .center()
}

#[test]
fn a_view_moves_to_another_sheet_from_its_right_click_menu() {
    let Some(mut h) = harness() else { return };
    let (one, two, title) = two_sheets(&mut h);
    let frame = h.state().drawing.editor.doc.sheet(one).unwrap().viewports[0].at;
    let at = h.get_all_by_label(&title).next().unwrap().rect().center();
    right_click(&mut h, at);
    // (A submenu's button carries its arrow in its label.)
    // (A wide menu near the panel's edge opens to the left of the pointer: find it by height.)
    let item = h
        .get_all_by_label_contains("Move to sheet")
        .map(|n| n.rect())
        .find(|r| r.top() >= at.y - 4.0 && r.top() < at.y + 300.0)
        .expect("Move to sheet on a placed view")
        .center();
    h.hover_at(item);
    h.run();
    h.run();
    save_png("drift_ui_sheets_move_to_sheet.png", &mut h);
    // (The submenu, beside the menu (on the left, at the panel's edge): the sheet's row, the
    // panel's width, and its tab in the top bar have the same name.)
    let label =
        crate::app::sheet_editor::sheet_label(h.state().drawing.editor.doc.sheet(two).unwrap());
    let to = h
        .get_all_by_label(&label)
        .map(|n| n.rect())
        .filter(|r| r.width() < 200.0 && r.top() >= at.y - 4.0)
        .map(|r| r.center())
        .next()
        .expect("the other sheet in the submenu");
    click_at(&mut h, to);
    assert_eq!(views_on(&h, one), 0, "off the first sheet");
    assert_eq!(views_on(&h, two), 1, "on the second");
    let moved = h.state().drawing.editor.doc.sheet(two).unwrap().viewports[0].at;
    assert!(
        moved.distance(frame) < 1e-9,
        "same spot on the same paper size"
    );
    assert_eq!(
        h.state().current_sheet(),
        Some(two),
        "the sheet it went to shows"
    );
    // One undo step.
    h.state_mut().undo();
    assert_eq!((views_on(&h, one), views_on(&h, two)), (1, 0));
}

#[test]
fn a_view_dragged_onto_another_sheet_moves_there() {
    let Some(mut h) = harness() else { return };
    let (one, two, title) = two_sheets(&mut h);
    let from = h.get_all_by_label(&title).next().unwrap().rect().center();
    let to = sheet_row_at(&h, two);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=8 {
        h.hover_at(from + (to - from) * (k as f32 / 8.0));
        h.run();
    }
    // (Its name follows the pointer, on top: see the render.)
    assert!(egui::DragAndDrop::has_payload_of_type::<
        crate::app::sheets_ui::PlacedDrag,
    >(&h.ctx));
    save_png("drift_ui_sheets_view_drag.png", &mut h);
    button(&h, to, false);
    h.run();
    h.run();
    assert_eq!((views_on(&h, one), views_on(&h, two)), (0, 1));
    // Dragged back onto the sheet it is on: nothing changes.
    let from = h.get_all_by_label(&title).next().unwrap().rect().center();
    let to = sheet_row_at(&h, two);
    h.hover_at(from);
    h.run();
    button(&h, from, true);
    h.run();
    for k in 1..=8 {
        h.hover_at(from + (to - from) * (k as f32 / 8.0));
        h.run();
    }
    button(&h, to, false);
    h.run();
    assert_eq!((views_on(&h, one), views_on(&h, two)), (0, 1));
}

/// The Inspector of the view on sheet `one`, picked.
fn view_inspector(h: &mut Harness<'_, DriftApp>, one: drift_core::SheetId) {
    let app = h.state_mut();
    app.show_sheet(Some(one));
    app.drawing.view.sheet_view = 0;
    app.drawing.picked_view = Some(app.drawing.editor.doc.sheet(one).unwrap().viewports[0].view);
    app.drawing.view.sheet_pick = crate::app::sheet_editor::SheetPick::View;
    app.show_panel(Panel::Inspector);
    h.run();
    h.run();
}

#[test]
fn a_views_inspector_keeps_its_actions_in_the_title_menu() {
    let Some(mut h) = harness() else { return };
    let (one, two, _) = two_sheets(&mut h);
    // On a second sheet too: the title says so.
    let view = h.state().drawing.editor.doc.views().next().unwrap().0;
    h.state_mut().place_view(two, view, None);
    view_inspector(&mut h, one);
    save_png("drift_ui_view_inspector.png", &mut h);
    assert!(h.query_by_label_contains("also on A-102").is_some());

    h.get_by_label("View actions").click();
    h.run();
    save_png("drift_ui_view_inspector_actions.png", &mut h);
    for item in [
        "Fit scale to frame",
        "Fill drawing area",
        "Remove from sheet",
    ] {
        assert!(h.query_by_label(item).is_some(), "{item} in the menu");
    }
    assert!(h.query_by_label_contains("Move to sheet").is_some());
    // Delete asks first, in the one dialog.
    h.get_by_label("Delete view\u{2026}").click();
    h.run();
    assert!(h.query_by_label("Cancel").is_some());
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.query_by_label("Cancel").is_none());

    // Remove from sheet: off this sheet, still in the model.
    h.get_by_label("View actions").click();
    h.run();
    h.get_by_label("Remove from sheet").click();
    h.run();
    assert_eq!(views_on(&h, one), 0);
    assert_eq!(h.state().drawing.editor.doc.views().count(), 1);
}

#[test]
fn deleting_a_sheet_with_views_asks_first_and_an_empty_one_goes_at_once() {
    let Some(mut h) = harness() else { return };
    sheet_with_a_view(&mut h);
    let sheet = h.state().current_sheet().unwrap();
    // An empty copy of it, for later.
    h.state_mut().duplicate_sheet(sheet, false);
    h.run();
    let empty = h.state().current_sheet().unwrap();
    assert_ne!(empty, sheet);
    assert!(
        h.state()
            .drawing
            .editor
            .doc
            .sheet(empty)
            .unwrap()
            .viewports
            .is_empty()
    );
    // The same dialog as for a view: it says the views stay in the model.
    h.state_mut().request_delete_sheet(sheet);
    h.run();
    save_png("drift_ui_delete_sheet_dialog.png", &mut h);
    assert!(
        h.state().drawing.editor.doc.sheet(sheet).is_some(),
        "not yet"
    );
    assert!(h.query_by_label_contains("stays in the model").is_some());
    h.get_by_label("Delete sheet").click();
    h.run();
    assert!(h.state().drawing.editor.doc.sheet(sheet).is_none());
    assert_eq!(h.state().drawing.editor.doc.views().count(), 1);
    assert_eq!(h.state().drawing.confirm_delete_sheet, None);

    // Nothing on it: nothing to lose, so no question (undo still brings it back).
    h.state_mut().request_delete_sheet(empty);
    h.run();
    assert!(h.state().drawing.editor.doc.sheet(empty).is_none());
    assert!(h.query_by_label("Cancel").is_none());
}

/// How many rows labelled `label` read as selected (egui gives a selectable row's state to
/// AccessKit as toggled).
fn selected_rows(h: &Harness<'_, DriftApp>, label: &str) -> usize {
    h.query_all_by(|n| {
        n.label().as_deref() == Some(label) && format!("{:?}", n.toggled()) == "Some(True)"
    })
    .count()
}

#[test]
fn one_row_reads_as_picked_when_a_view_under_the_sheet_is() {
    let Some(mut h) = harness() else { return };
    let title = sheet_with_a_view(&mut h);
    let sheet = h.state().current_sheet().unwrap();
    // Pick the view under its sheet: its row is the selection. The sheet stays shown (bold,
    // still read as the open one) without its fill, and the Views list doesn't fill it again.
    let at = h.get_all_by_label(&title).next().unwrap().rect().center();
    click_at(&mut h, at);
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    save_png("drift_ui_sheets_one_selection.png", &mut h);
    assert_eq!(
        h.state().drawing.view.sheet_pick,
        crate::app::sheet_editor::SheetPick::View
    );
    assert_eq!(
        selected_rows(&h, &title),
        1,
        "the nested row is picked, not the Views list's row too"
    );
    let label =
        crate::app::sheet_editor::sheet_label(h.state().drawing.editor.doc.sheet(sheet).unwrap());
    assert_eq!(
        selected_rows(&h, &label),
        2,
        "the sheet still reads as the open one (its row, and its tab)"
    );
}

#[test]
fn a_layer_hidden_in_the_model_isnt_counted_as_left_out_of_the_view() {
    let Some(mut h) = harness() else { return };
    let (one, ..) = two_sheets(&mut h);
    let id = h.state_mut().drawing.editor.add_layer("Hidden");
    let mut layer = h.state().drawing.editor.doc.layer(id).cloned().unwrap();
    layer.visible = false;
    h.state_mut().drawing.editor.update_layer(id, layer);
    view_inspector(&mut h, one);
    // The view leaves nothing out: every layer that can show, shows.
    assert!(h.query_by_value("All visible layers").is_some());
}

/// A new sheet, from the panel or the top bar's +, is added without leaving the canvas: on
/// the model it stays on the model, and on a sheet it stays on that sheet.
#[test]
fn a_new_sheet_leaves_the_canvas_where_it_is() {
    let Some(mut h) = harness() else { return };
    h.state_mut().show_panel(Panel::Sheets);
    h.run();
    h.get_by_label("New sheet").click();
    h.run();
    h.run();
    assert_eq!(h.state().drawing.editor.doc.sheets().count(), 1);
    assert!(!h.state().drawing.view.sheet_preview, "still on the model");
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    assert_eq!(h.state().drawing.editor.doc.sheets().count(), 2);
    assert!(!h.state().drawing.view.sheet_preview, "still on the model");
    // On the first sheet, a new one leaves it showing.
    let first = h
        .state()
        .drawing
        .editor
        .doc
        .sheets()
        .next()
        .map(|(id, _)| id);
    h.state_mut().show_sheet(first);
    h.run();
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    assert_eq!(h.state().drawing.editor.doc.sheets().count(), 3);
    assert!(h.state().drawing.view.sheet_preview);
    assert_eq!(h.state().current_sheet(), first, "still on the first sheet");
}
