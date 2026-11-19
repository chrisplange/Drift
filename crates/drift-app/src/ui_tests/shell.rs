//! The frame: one top bar (menus, undo and redo, the sheet tabs, search, the panels' switch),
//! the options bar under it, and the status bar.

use super::*;
use crate::theme::bar;

fn harness_at(w: f32, h: f32) -> Option<Harness<'static, DriftApp>> {
    harness()?;
    let mut harness = Harness::builder()
        .with_size([w, h])
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    harness.run();
    Some(harness)
}

fn rect(h: &Harness<'_, DriftApp>, label: &str) -> egui::Rect {
    h.get_by_label(label).rect()
}

/// The tab of a sheet (or the model's) in the top bar, if it has one. (The Sheets panel and
/// the "More sheets" menu name sheets too, lower down.)
fn tab(h: &Harness<'_, DriftApp>, label: &str) -> Option<egui::Rect> {
    h.query_all_by_label(label)
        .map(|n| n.rect())
        .find(|r| r.bottom() <= bar::TOP)
}

fn lines(h: &Harness<'_, DriftApp>) -> usize {
    h.state()
        .drawing
        .editor
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .count()
}

/// One row holds the menus, undo and redo, the tabs, the search field and the panels'
/// switch, in that order; the tabs sit in the middle of the window; the canvas starts right
/// under the options bar.
#[test]
fn the_top_bar_is_one_row_with_the_tabs_in_the_middle() {
    let Some(mut h) = harness() else { return };
    let row = [
        "File",
        "Help",
        "Undo last change",
        "Redo last change",
        "Model",
        "Add a sheet",
        "Search commands",
        "Show or hide panels",
    ]
    .map(|name| (name, rect(&h, name)));
    for (name, r) in row {
        assert!(
            r.top() >= 0.0 && r.bottom() <= bar::TOP,
            "{name} {r:?} is in the top bar"
        );
    }
    for pair in row.windows(2) {
        assert!(
            pair[0].1.right() <= pair[1].1.left() + 0.5,
            "{} {:?} is left of {} {:?}",
            pair[0].0,
            pair[0].1,
            pair[1].0,
            pair[1].1
        );
    }
    let (model, plus) = (rect(&h, "Model"), rect(&h, "Add a sheet"));
    let middle = (model.left() + plus.right()) / 2.0;
    assert!((middle - 640.0).abs() < 1.5, "the tabs' middle: {middle}");
    let canvas = h.state().drawing.view.canvas_rect;
    assert!(
        (canvas.top() - (bar::TOP + bar::OPTIONS)).abs() < 0.5,
        "the canvas starts under the two bars: {canvas:?}"
    );
    // A sheet's tab joins the model's, and the tab shown is the selected one.
    h.get_by_label("Add a sheet").click();
    h.run();
    h.run();
    let sheet = tab(&h, "A-101  Sheet 1").expect("the new sheet's tab");
    assert!(rect(&h, "Model").right() <= sheet.left() + 0.5);
    assert!(sheet.right() <= rect(&h, "Add a sheet").left() + 0.5);
    assert!(h.state().drawing.view.sheet_preview);
    h.get_by_label("Model").click();
    h.run();
    assert!(!h.state().drawing.view.sheet_preview);
}

/// Undo and redo in the top bar do what the Edit menu's do, and the panels' switch hides
/// every panel and brings them back.
#[test]
fn the_top_bar_buttons_undo_redo_and_switch_the_panels() {
    let Some(mut h) = harness() else { return };
    assert!(!h.state().drawing.editor.history.can_undo());
    h.key_press(Key::L);
    h.run();
    click_world(&mut h, DVec2::new(-20.0, 0.0));
    click_world(&mut h, DVec2::new(20.0, 10.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let drawn = lines(&h);
    assert!(drawn >= 1, "a line was drawn");
    h.get_by_label("Undo last change").click();
    h.run();
    assert!(lines(&h) < drawn, "the button undid it");
    assert!(h.state().drawing.editor.history.can_redo());
    h.get_by_label("Redo last change").click();
    h.run();
    assert_eq!(lines(&h), drawn, "and redid it");

    assert!(h.query_by_label("Inspector").is_some());
    h.get_by_label("Show or hide panels").click();
    h.run();
    h.run();
    assert!(h.state().dock_view.hide_all);
    assert!(h.query_by_label("Inspector").is_none());
    h.get_by_label("Show or hide panels").click();
    h.run();
    h.run();
    assert!(!h.state().dock_view.hide_all);
    assert!(h.query_by_label("Inspector").is_some());
}

/// With more sheets than the bar has room for, the tabs stay between the bar's two ends: the
/// sheet shown keeps its tab, and the rest are one click away under "More sheets".
#[test]
fn sheets_without_room_for_a_tab_go_under_more_sheets() {
    let Some(mut h) = harness() else { return };
    assert!(h.query_by_label("More sheets").is_none());
    for _ in 0..9 {
        h.state_mut().new_sheet();
    }
    h.run();
    h.run();
    let more = rect(&h, "More sheets");
    let (from, to) = (
        rect(&h, "Redo last change").right(),
        rect(&h, "Search commands").left(),
    );
    let label = |k: usize| format!("A-10{k}  Sheet {k}");
    let tabs: Vec<(usize, egui::Rect)> = (1..=9)
        .filter_map(|k| tab(&h, &label(k)).map(|r| (k, r)))
        .collect();
    assert!(tabs.len() < 9, "not every sheet has room: {tabs:?}");
    assert!(
        tabs.iter().any(|(k, _)| *k == 9),
        "the sheet shown keeps its tab: {tabs:?}"
    );
    let model = tab(&h, "Model").expect("the model's tab");
    for r in tabs
        .iter()
        .map(|(_, r)| *r)
        .chain([model, more, rect(&h, "Add a sheet")])
    {
        assert!(
            r.left() >= from && r.right() <= to,
            "{r:?} is between the bar's ends ({from}..{to})"
        );
    }
    // A sheet without a tab, from the menu: it is shown, and has its tab now.
    let hidden = (1..=9)
        .find(|k| tabs.iter().all(|(t, _)| t != k))
        .expect("a sheet without a tab");
    h.get_by_label("More sheets").click();
    h.run();
    h.get_by_label(&label(hidden)).click();
    h.run();
    h.run();
    let shown = h.state().current_sheet().expect("a sheet is shown");
    assert_eq!(
        crate::app::sheet_editor::sheet_label(h.state().drawing.editor.doc.sheet(shown).unwrap()),
        label(hidden)
    );
    assert!(tab(&h, &label(hidden)).is_some());
}

/// In a narrow window the bar is still one row: every menu, the model's tab and the +, and
/// the search as a button of the same name; nothing runs off the window.
#[test]
fn a_narrow_window_keeps_the_top_bar_in_one_row() {
    for width in [900.0, 720.0] {
        let Some(h) = harness_at(width, 700.0) else {
            return;
        };
        let search = rect(&h, "Search commands");
        assert!(search.width() < 40.0, "{width}: search is a button");
        let row = [
            "File", "Edit", "View", "Draw", "Build", "Modify", "Annotate", "Help",
        ]
        .map(|name| {
            h.get_all_by_label(name)
                .next()
                .expect("a menu of that name")
                .rect()
        })
        .into_iter()
        .chain([
            tab(&h, "Model").expect("the model's tab"),
            rect(&h, "Add a sheet"),
            search,
            rect(&h, "Show or hide panels"),
        ])
        .collect::<Vec<_>>();
        for pair in row.windows(2) {
            assert!(
                pair[0].right() <= pair[1].left() + 0.5,
                "{width}: {:?} is left of {:?}",
                pair[0],
                pair[1]
            );
        }
        for r in &row {
            assert!(
                r.left() >= 0.0 && r.right() <= width && r.bottom() <= bar::TOP,
                "{width}: {r:?} is in the bar"
            );
        }
    }
}

/// The status bar, left to right: the hint, the snaps, what the drawing shows, the units'
/// menu, and zoom out / in round the percentage; the zoom buttons zoom the canvas (the sheet,
/// on a sheet).
#[test]
fn the_status_bar_reads_hint_snaps_shows_and_zoom() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    let hint = rect(&h, &h.state().tool.hint());
    assert!(hint.left() < 20.0, "the hint starts the bar: {hint:?}");
    let order = [
        "Snap to grid",
        "Perpendicular & tangent",
        "Angle snap",
        "Auto relations",
        "Grid",
        "Origin axes",
        "End points",
        "Thin lines",
        "Status colors",
        "Zoom out",
        "Zoom in",
    ]
    .map(|name| (name, rect(&h, name)));
    assert!(hint.right() <= order[0].1.left());
    for (name, r) in order {
        assert!(
            r.top() > canvas.bottom() && r.bottom() <= 800.0,
            "{name} {r:?} is in the status bar"
        );
    }
    for pair in order.windows(2) {
        assert!(
            pair[0].1.right() <= pair[1].1.left() + 0.5,
            "{} is left of {}",
            pair[0].0,
            pair[1].0
        );
    }
    assert!(
        h.query_by_label_contains("Pointer").is_some(),
        "the pointer's position is read out"
    );

    let scale = h.state().drawing.view.camera.scale;
    h.get_by_label("Zoom in").click();
    h.run();
    let zoomed = h.state().drawing.view.camera.scale;
    assert!((zoomed / scale - 1.25).abs() < 1e-9, "{scale} to {zoomed}");
    h.get_by_label("Zoom out").click();
    h.run();
    assert!((h.state().drawing.view.camera.scale / scale - 1.0).abs() < 1e-9);

    // On a sheet the same buttons zoom the sheet, and leave the model's view alone.
    h.state_mut().new_sheet();
    h.run();
    h.run();
    let paper = h.state().drawing.view.preview.zoom;
    h.get_by_label("Zoom in").click();
    h.run();
    let closer = h.state().drawing.view.preview.zoom;
    assert!((closer / paper - 1.25).abs() < 1e-4, "{paper} to {closer}");
    assert!((h.state().drawing.view.camera.scale / scale - 1.0).abs() < 1e-9);
}
