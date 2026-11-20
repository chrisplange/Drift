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
    h.get_all_by_label(label)
        .map(|n| n.rect())
        .filter(|r| r.left() >= at.x - 1.0 && r.top() < at.y + 80.0)
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
    assert!(menu_item(&h, "Place again", at).is_some());
    assert!(menu_item(&h, "Delete view", at).is_some());
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
    let item = menu_item(&h, "Delete view", at).unwrap();
    click_at(&mut h, item);
    h.run();
    let view = h.state().drawing.editor.doc.views().next().unwrap().0;
    assert_eq!(h.state().drawing.confirm_delete_view, Some(view));
}

#[test]
fn a_sheets_right_click_menu_exports_or_prints_just_that_sheet() {
    let Some(mut h) = harness() else { return };
    sheet_with_a_view(&mut h);
    h.get_all_by_label("New sheet").next().unwrap().click();
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
    let item = h
        .get_all_by_label_contains("Move to sheet")
        .map(|n| n.rect())
        .find(|r| r.left() >= at.x - 1.0)
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
        .filter(|r| r.width() < 200.0 && r.left() > at.x - 200.0)
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
