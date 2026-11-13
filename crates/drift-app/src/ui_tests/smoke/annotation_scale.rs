//! Annotations at print size: the View switch (menu, palette, status, saved with the view
//! settings); picking at print size zoomed in and out (greeked); a label dragged at 1:50 kept
//! on paper and undone exactly; the hint when the scale leaves the annotations unreadable.

use drift_core::{AnnotationRef, Constraint, DraftStyle};
use egui_kittest::kittest::NodeT as _;

use super::*;

/// A line `long` mm along y = 0 dimensioned, its label 10 paper mm above it, in a drawing at
/// 1 : `n`; the dimension's id and the line.
fn dimensioned(
    h: &mut Harness<'_, DriftApp>,
    n: f64,
    long: f64,
) -> (drift_core::ConstraintId, EntityId) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    ed.doc.set_draft_style(DraftStyle {
        scale: n,
        ..ed.doc.draft_style()
    });
    let a = ed.doc.add_point(DVec2::new(-long / 2.0, 0.0));
    let b = ed.doc.add_point(DVec2::new(long / 2.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    let id = ed
        .doc
        .add_constraint(Constraint::Distance { a, b, value: long })
        .unwrap();
    ed.doc.set_placement(id, DVec2::new(0.0, 10.0)).unwrap();
    ed.commit("Dimension");
    h.run();
    (id, line)
}

fn picked(h: &Harness<'_, DriftApp>, id: drift_core::ConstraintId) -> bool {
    h.state().annotation_picked(AnnotationRef::Dimension(id))
}

fn clear_picks(h: &mut Harness<'_, DriftApp>) {
    h.key_press(Key::Escape);
    h.run();
    let app = h.state_mut();
    app.drawing.editor.selection.clear();
    app.drawing.editor.annotation_picks.clear();
    app.drawing.selected_constraint = None;
    h.run();
}

#[test]
fn smoke_annotation_scale_view_switch_menu_palette_status_and_prefs() {
    let Some(mut h) = harness() else { return };
    assert!(h.state().print_annotations, "on by default");
    view(&mut h, DVec2::new(-40.0, -30.0), DVec2::new(40.0, 30.0));
    dimensioned(&mut h, 1.0, 40.0);
    // At print size the label grows with the zoom; at a fixed size it doesn't.
    let text_height = |h: &mut Harness<'_, DriftApp>, min: f64| {
        view(h, DVec2::splat(-min), DVec2::splat(min));
        first_dimension(h).text_rect.height()
    };
    let (far, near) = (text_height(&mut h, 60.0), text_height(&mut h, 30.0));
    assert!(
        near > far * 1.8,
        "zooms with the drawing: {far} then {near}"
    );
    // In the View menu, after Dimensions and notes.
    open_menu(&mut h, "View");
    h.get_by_label("Annotations at print size").click();
    h.run();
    assert!(!h.state().print_annotations);
    assert!(
        status(&h).contains("fixed size on screen"),
        "{}",
        status(&h)
    );
    assert!(h.state().view_prefs().contains("print_anno=0"));
    let (far, near) = (text_height(&mut h, 60.0), text_height(&mut h, 30.0));
    assert!(
        (near - far).abs() < 0.5,
        "one size on screen: {far} then {near}"
    );
    // The palette turns it back on, by name or by AutoCAD's.
    palette(&mut h, "ANNOSCALE");
    assert!(h.state().print_annotations);
    assert!(status(&h).contains("print size"), "{}", status(&h));
    // Saved with the view settings, and read back.
    let saved = h.state().view_prefs();
    assert!(saved.contains("print_anno=1"));
    h.state_mut().print_annotations = false;
    h.state_mut().apply_view_prefs(&saved);
    assert!(h.state().print_annotations);
    save_png("drift_smoke_annotation_scale.png", &mut h);
}

#[test]
fn smoke_annotation_scale_picking_zoomed_in_and_out() {
    let Some(mut h) = harness() else { return };
    let (id, line) = dimensioned(&mut h, 1.0, 40.0);
    // Zoomed in, the label is big: a click well inside its text, far from where a drag
    // would take it, picks the dimension.
    view(&mut h, DVec2::new(-12.0, 4.0), DVec2::new(12.0, 18.0));
    let dim = first_dimension(&h);
    assert!(dim.text_rect.height() > 60.0, "{:?}", dim.text_rect);
    let inside = dim.text_rect.left_center() + egui::vec2(dim.text_rect.width() * 0.2, 0.0);
    assert!(inside.distance(dim.handle) > 20.0);
    h.hover_at(inside);
    h.run();
    button(&h, inside, true);
    h.run();
    button(&h, inside, false);
    h.run();
    assert!(picked(&h, id), "picked by its big text");
    clear_picks(&mut h);

    // Zoomed far out, its text is greeked, but a click just beside its box still picks it.
    view(&mut h, DVec2::splat(-2000.0), DVec2::splat(2000.0));
    let dim = first_dimension(&h);
    let text_pt = dim.style.text;
    assert!(text_pt < crate::annotations::GREEK_PT, "greeked: {text_pt}");
    let beside =
        dim.text_rect.right_center() + egui::vec2(crate::annotations::TEXT_REACH - 1.0, 0.0);
    h.hover_at(beside);
    h.run();
    button(&h, beside, true);
    h.run();
    button(&h, beside, false);
    h.run();
    assert!(picked(&h, id), "picked within reach of its greeked text");
    clear_picks(&mut h);

    // And a click on the line it measures, beside the label, picks the line.
    view(&mut h, DVec2::new(-30.0, -10.0), DVec2::new(30.0, 25.0));
    click_world(&mut h, DVec2::new(15.0, 0.0));
    assert_eq!(h.state().drawing.editor.selection, [line].into());
    assert!(!picked(&h, id));
}

#[test]
fn smoke_annotation_scale_a_label_dragged_at_1_50_is_kept_on_paper() {
    let Some(mut h) = harness() else { return };
    // A 4 m wall: 80 mm long on paper, its number in the middle.
    let (id, _) = dimensioned(&mut h, 50.0, 4000.0);
    // 10 paper mm: 500 mm in the model at 1:50.
    view(
        &mut h,
        DVec2::new(-2500.0, -600.0),
        DVec2::new(2500.0, 1200.0),
    );
    let before = h.state().drawing.editor.doc.placement(id).unwrap();
    assert_eq!(before, DVec2::new(0.0, 10.0));
    let dim = first_dimension(&h);
    let from = screen_map(&h).to_world(dim.handle);
    // Up by 250 mm in the model: 5 mm more on paper.
    drag(&mut h, from, from + DVec2::new(0.0, 250.0), 6);
    let after = h.state().drawing.editor.doc.placement(id).unwrap();
    assert!(
        (after - DVec2::new(0.0, 15.0)).length() < 0.05,
        "kept on paper: {after}"
    );
    undo(&mut h);
    assert_eq!(h.state().drawing.editor.doc.placement(id), Some(before));
    h.key_press_modifiers(EguiModifiers::COMMAND | EguiModifiers::SHIFT, Key::Z);
    h.run();
    assert_eq!(h.state().drawing.editor.doc.placement(id), Some(after));
}

#[test]
fn smoke_annotation_scale_hint_when_the_scale_leaves_them_unreadable() {
    let Some(mut h) = harness() else { return };
    // A 20 m plan at 1:1: its 3.5 mm text is far under a point with it all in view.
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(20_000.0, 0.0));
        ed.doc.add_line(a, b).unwrap();
        ed.doc
            .add_constraint(Constraint::Distance {
                a,
                b,
                value: 20_000.0,
            })
            .unwrap();
        ed.commit("Plan");
    }
    view(
        &mut h,
        DVec2::new(-1000.0, -5000.0),
        DVec2::new(21_000.0, 5000.0),
    );
    h.run();
    assert!(h.query_by_label("Set drawing scale…").is_some(), "the hint");
    // Its button opens Drawing settings, and it stays closed for this drawing.
    h.get_by_label("Set drawing scale…").click();
    h.run();
    assert!(h.state().show_drawing_settings);
    h.state_mut().show_drawing_settings = false;
    h.run();
    assert!(h.query_by_label("Set drawing scale…").is_none());
    // At 1:100 it reads (3.5 mm is 350 mm in the model): no hint.
    let app_scale = |h: &mut Harness<'_, DriftApp>, n: f64| {
        let app = h.state_mut();
        app.drawing.scale_hint_closed = false;
        let style = app.drawing.editor.doc.draft_style();
        app.drawing
            .editor
            .doc
            .set_draft_style(DraftStyle { scale: n, ..style });
        h.run();
        h.run();
    };
    app_scale(&mut h, 100.0);
    assert!(h.query_by_label("Set drawing scale…").is_none());
    app_scale(&mut h, 1.0);
    assert!(h.query_by_label("Set drawing scale…").is_some());
}

/// The height of the text made last (model mm).
fn last_text_height(doc: &drift_core::Document) -> f64 {
    doc.texts().last().expect("a text").1.height
}

#[test]
fn smoke_annotation_scale_new_text_is_the_drafting_height_at_the_drawing_scale() {
    let Some(mut h) = harness() else { return };
    let at_1_50 = |doc: &mut drift_core::Document| {
        doc.set_draft_style(DraftStyle {
            scale: 50.0,
            ..doc.draft_style()
        });
    };
    at_1_50(&mut h.state_mut().drawing.editor.doc);
    // 3.5 mm on paper: 175 mm in the model.
    let expected = 3.5 * 50.0;
    view(&mut h, DVec2::splat(-3000.0), DVec2::splat(3000.0));
    // The Text tool.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::T);
    h.run();
    click_world(&mut h, DVec2::new(-1000.0, 0.0));
    type_text(&mut h, "TOOL");
    h.key_press(Key::Enter);
    h.run();
    let doc = &h.state().drawing.editor.doc;
    assert_eq!(doc.texts().count(), 1, "the tool made a text");
    assert!((last_text_height(doc) - expected).abs() < 1e-9);
    // A paste of plain text.
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().paste("PASTED", false, 0.0);
    assert!((last_text_height(&h.state().drawing.editor.doc) - expected).abs() < 1e-9);
    // The AI link, and a script.
    let mut s = crate::agent::Session::new();
    at_1_50(&mut s.editor.doc);
    let out = s.call(
        "add_text",
        &serde_json::json!({ "text": "AI", "at": [0, 0] }),
    );
    assert!(!out.is_error, "{}", out.json);
    assert!((last_text_height(&s.editor.doc) - expected).abs() < 1e-9);
    let out = s.call(
        "run_script",
        &serde_json::json!({ "script": "text \"SCRIPT\" at 0,500" }),
    );
    assert!(!out.is_error, "{}", out.json);
    assert!((last_text_height(&s.editor.doc) - expected).abs() < 1e-9);
    // The tool bar's own paper height (5 mm) for the tool and pastes: 250 mm.
    h.state_mut().drawing.editor.prefs.text_height = 5.0;
    h.state_mut().paste("BIGGER", false, 0.0);
    assert!((last_text_height(&h.state().drawing.editor.doc) - 250.0).abs() < 1e-9);
}

#[test]
fn smoke_annotation_scale_custom_scale_field_keeps_the_menu_open_to_type() {
    let Some(mut h) = harness() else { return };
    // At 1:25, labelled the same in metric and imperial drawings.
    {
        let doc = &mut h.state_mut().drawing.editor.doc;
        let style = doc.draft_style();
        doc.set_draft_style(DraftStyle {
            scale: 25.0,
            ..style
        });
    }
    h.state_mut().show_drawing_settings = true;
    h.run();
    h.run();
    // The drawing scale menu, scrolled into view.
    h.get_by_value("1:25").scroll_to_me();
    h.run();
    h.get_by_value("1:25").click();
    h.run();
    let placeholder = "1:37 or 5\" = 1'-0\"";
    let field = |h: &Harness<'_, DriftApp>| {
        h.query_all_by_role(egui::accesskit::Role::TextInput)
            .find(|n| n.accesskit_node().placeholder() == Some(placeholder))
            .map(|n| n.rect().center())
    };
    // The Custom field is at the foot of the menu's list, below its fold.
    h.query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some(placeholder))
        .expect("the Custom field in the open menu")
        .scroll_to_me();
    h.run();
    h.run();
    let at = field(&h).expect("the menu is still open");
    // A click into the field keeps the menu open and the field focused.
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    button(&h, at, false);
    h.run();
    h.run();
    assert!(
        field(&h).is_some(),
        "the menu closed on a click into its field"
    );
    type_text(&mut h, "1:37");
    h.key_press(Key::Enter);
    h.run();
    h.run();
    assert_eq!(h.state().drawing.editor.doc.draft_style().scale, 37.0);
    assert!(
        field(&h).is_none(),
        "Enter applies the scale and closes the menu"
    );
}

/// Opens the drawing-settings scale menu, types `text` in its Custom field (Enter too when
/// `enter`), (Enter too when `enter`).
fn type_in_custom_field(h: &mut Harness<'_, DriftApp>, text: &str, enter: bool) {
    let placeholder = "1:37 or 5\" = 1'-0\"";
    h.get_by_value("1:25").scroll_to_me();
    h.run();
    h.get_by_value("1:25").click();
    h.run();
    h.query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some(placeholder))
        .expect("the Custom field in the open menu")
        .scroll_to_me();
    h.run();
    h.run();
    let at = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| n.accesskit_node().placeholder() == Some(placeholder))
        .map(|n| n.rect().center())
        .expect("the menu is open");
    h.hover_at(at);
    h.run();
    button(h, at, true);
    h.run();
    button(h, at, false);
    h.run();
    type_text(h, text);
    if enter {
        h.key_press(Key::Enter);
    }
    h.run();
    h.run();
}

/// The Custom field's text, and whether an error line shows, if the menu is open.
fn custom_field_state(h: &Harness<'_, DriftApp>) -> Option<(String, bool)> {
    let placeholder = "1:37 or 5\" = 1'-0\"";
    let field = h
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .find(|n| {
            // An empty field shows its placeholder; a typed one its text.
            n.accesskit_node().placeholder() == Some(placeholder)
                || n.value().as_deref() == Some("nonsense")
        })?;
    let text = field.value().unwrap_or_default().to_string();
    let has_error = h.query_by_label_contains("Not a scale").is_some();
    Some((text, has_error))
}

#[test]
fn smoke_annotation_scale_custom_field_is_empty_after_esc_or_click_outside() {
    for close_by_esc in [true, false] {
        let Some(mut h) = harness() else { return };
        {
            let doc = &mut h.state_mut().drawing.editor.doc;
            let style = doc.draft_style();
            doc.set_draft_style(DraftStyle {
                scale: 25.0,
                ..style
            });
        }
        h.state_mut().show_drawing_settings = true;
        h.run();
        h.run();
        // A bad scale typed and entered: the field keeps it and shows an error.
        type_in_custom_field(&mut h, "nonsense", true);
        let (text, has_error) = custom_field_state(&h).expect("a bad scale keeps the menu open");
        assert_eq!(text, "nonsense");
        assert!(has_error, "the bad scale shows its error");
        // Close it without applying.
        if close_by_esc {
            h.key_press(Key::Escape);
        } else {
            let out = egui::pos2(2.0, 2.0);
            h.hover_at(out);
            h.run();
            button(&h, out, true);
            h.run();
            button(&h, out, false);
        }
        h.run();
        h.run();
        assert!(custom_field_state(&h).is_none(), "the menu closed");
        assert_eq!(h.state().drawing.editor.doc.draft_style().scale, 25.0);
        // Reopened (Esc may have closed the settings window too): empty field, no error.
        h.state_mut().show_drawing_settings = true;
        h.run();
        h.run();
        h.get_by_value("1:25").scroll_to_me();
        h.run();
        h.get_by_value("1:25").click();
        h.run();
        h.run();
        let placeholder = "1:37 or 5\" = 1'-0\"";
        h.query_all_by_role(egui::accesskit::Role::TextInput)
            .find(|n| n.accesskit_node().placeholder() == Some(placeholder))
            .expect("the Custom field")
            .scroll_to_me();
        h.run();
        h.run();
        let (text, has_error) = custom_field_state(&h).expect("the menu reopened");
        assert_eq!(text, "", "the field starts empty (esc: {close_by_esc})");
        assert!(!has_error, "no old error shows (esc: {close_by_esc})");
    }
}

/// A 20 m dimensioned plan at 1:1 in view, its sheets showing views at `scales` (one sheet
/// per scale; none for an empty list).
fn plan_with_sheet_views(h: &mut Harness<'_, DriftApp>, scales: &[f64]) {
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(20_000.0, 0.0));
        ed.doc.add_line(a, b).unwrap();
        ed.doc
            .add_constraint(Constraint::Distance {
                a,
                b,
                value: 20_000.0,
            })
            .unwrap();
        for (k, n) in scales.iter().enumerate() {
            let crop = drift_core::Bounds {
                min: DVec2::new(0.0, -1000.0),
                max: DVec2::new(20_000.0, 1000.0),
            };
            let view = ed
                .doc
                .add_view(drift_core::View::new(format!("V{k}"), crop, *n));
            let mut sheet = drift_core::Sheet::new(format!("S{k}"));
            sheet
                .viewports
                .push(drift_core::Viewport::new(view, DVec2::new(200.0, 150.0)));
            ed.doc.add_sheet(sheet);
        }
        ed.commit("Plan");
    }
    view(
        h,
        DVec2::new(-1000.0, -5000.0),
        DVec2::new(21_000.0, 5000.0),
    );
    h.run();
    h.run();
    assert!(h.query_by_label("Set drawing scale…").is_some(), "the hint");
}

#[test]
fn smoke_annotation_scale_hint_offers_the_scale_the_sheets_share() {
    const OFFER: &str = "Set to 1:50 (the scale its sheets use)";
    // Every sheet view at 1:50: one click sets it, as one undo step, and the hint goes.
    {
        let Some(mut h) = harness() else { return };
        plan_with_sheet_views(&mut h, &[50.0, 50.0]);
        h.get_by_label(OFFER).click();
        h.run();
        h.run();
        assert_eq!(h.state().drawing.editor.doc.draft_style().scale, 50.0);
        assert!(h.query_by_label(OFFER).is_none());
        assert!(h.query_by_label("Set drawing scale…").is_none());
        undo(&mut h);
        assert_eq!(h.state().drawing.editor.doc.draft_style().scale, 1.0);
    }
    // Views that disagree: the hint as before.
    {
        let Some(mut h) = harness() else { return };
        plan_with_sheet_views(&mut h, &[50.0, 100.0]);
        assert!(
            h.query_by_label_contains("the scale its sheets use")
                .is_none()
        );
    }
    // No views: the hint as before.
    {
        let Some(mut h) = harness() else { return };
        plan_with_sheet_views(&mut h, &[]);
        assert!(
            h.query_by_label_contains("the scale its sheets use")
                .is_none()
        );
    }
}
