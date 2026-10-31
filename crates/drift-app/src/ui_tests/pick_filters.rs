//! Selection filters you can combine (drift-releases#9): a toggle per kind in the select
//! tool's bar, Alt-click or double-click for one kind alone, All, a badge on the Select tool
//! while a filter is on, and the filter applied to clicks, boxes, Select all and the palette.

use drift_core::{EntityId, FillId, Rgb};

use drift_edit::editor::{PickFilter, PickKind};

use super::*;

/// A filled rectangle (a shape, its fill solid), a loose line and a sketch text, in view.
fn drawing(h: &mut Harness<'_, DriftApp>) -> (Vec<EntityId>, FillId, EntityId) {
    let v = DVec2::new;
    let ed = &mut h.state_mut().editor;
    ed.prefs.fill_new_shapes = true;
    ed.prefs.fill_style.kind = drift_core::FillKind::Solid;
    ed.prefs.fill_style.color = Some(Rgb([0, 0, 255]));
    ed.doc.begin();
    let p = [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)].map(|q| ed.doc.add_point(q));
    let sides: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    ed.add_shape(&sides).expect("a shape");
    ed.prefs.fill_new_shapes = false;
    let (a, b) = (
        ed.doc.add_point(v(60.0, 0.0)),
        ed.doc.add_point(v(90.0, 20.0)),
    );
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Drawing");
    ed.add_text(drift_core::SketchText::new("ROOM", v(60.0, 30.0), 4.0));
    let fill = ed.doc.fills().next().map(|(id, _)| id).unwrap();
    ed.selection.clear();
    ed.selected_fills.clear();
    ed.selected_texts.clear();
    h.state_mut().fit_target = Some(drift_core::Bounds {
        min: v(-10.0, -10.0),
        max: v(110.0, 45.0),
    });
    h.run();
    h.state_mut().fit_target = None;
    h.run();
    (sides, fill, line)
}

fn filter(h: &Harness<'_, DriftApp>) -> PickFilter {
    h.state().editor.prefs.pick_filter
}

/// A window round the whole drawing.
fn box_all(h: &mut Harness<'_, DriftApp>) {
    let (from, to) = (
        screen(h, DVec2::new(-8.0, -8.0)),
        screen(h, DVec2::new(105.0, 42.0)),
    );
    drag_steps(h, from, to, 4, None);
}

fn badge(h: &Harness<'_, DriftApp>) -> bool {
    h.query_by_label("Selection filter on").is_some()
}

#[test]
fn filter_toggles_combine_and_alt_or_double_click_picks_one_kind() {
    let Some(mut h) = harness() else { return };
    h.key_press(Key::S);
    h.run();
    assert!(filter(&h).is_all());
    assert!(!badge(&h), "no badge without a filter");
    // A click turns one kind off; the Select tool shows a badge.
    h.get_by_label("Pick Annotations").click();
    h.run();
    assert_eq!(
        filter(&h),
        PickFilter::ALL.with(PickKind::Annotations, false)
    );
    assert!(badge(&h));
    save_png("drift_ui_pick_filters.png", &mut h);
    // Alt-click: that kind alone.
    pick_only(&mut h, "Fills");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Fills));
    // And another turned on with it.
    h.get_by_label("Pick Text").click();
    h.run();
    assert_eq!(
        filter(&h),
        PickFilter::only(PickKind::Fills).with(PickKind::Text, true)
    );
    // A double-click: that kind alone too.
    let at = h.get_by_label("Pick Points").rect().center();
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        button(&h, at, true);
        h.step();
        button(&h, at, false);
        h.step();
    }
    h.run();
    assert_eq!(filter(&h), PickFilter::only(PickKind::Points));
    // All: everything again, and the badge goes.
    h.get_by_label("All").click();
    h.run();
    assert!(filter(&h).is_all());
    assert!(!badge(&h));
}

#[test]
fn the_filter_applies_to_clicks_boxes_and_select_all() {
    let Some(mut h) = harness() else { return };
    let (sides, fill, line) = drawing(&mut h);
    // Fills only: a click inside the shape picks its fill without its outline; a box and
    // Select all take only the fill.
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Fills));
    click_world(&mut h, DVec2::new(20.0, 15.0));
    {
        let ed = &h.state().editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(ed.selection.is_empty(), "{:?}", ed.selection);
    }
    click_world(&mut h, DVec2::new(75.0, 10.0));
    assert!(
        h.state().editor.selection.is_empty(),
        "the line isn't picked"
    );
    box_all(&mut h);
    {
        let ed = &h.state().editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(ed.selection.is_empty() && ed.selected_texts.is_empty());
    }
    h.state_mut().editor.selected_fills.clear();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    h.run();
    {
        let ed = &h.state().editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(ed.selection.is_empty() && ed.selected_texts.is_empty());
    }
    // Edges and text: the box takes the lines and the text, not the fill.
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Edges).with(PickKind::Text, true));
    h.key_press(Key::Escape);
    h.run();
    box_all(&mut h);
    {
        let ed = &h.state().editor;
        assert!(ed.selected_fills.is_empty());
        assert!(sides.iter().all(|s| ed.selection.contains(s)));
        assert!(ed.selection.contains(&line));
        assert_eq!(ed.selected_texts.len(), 1);
    }
    // No filter: the box takes the fill as well (the shape whole).
    h.state_mut().set_pick_filter(PickFilter::ALL);
    h.key_press(Key::Escape);
    h.run();
    box_all(&mut h);
    let ed = &h.state().editor;
    assert_eq!(ed.selected_fills, [fill].into());
    assert!(ed.selection.contains(&line));
}

#[test]
fn areas_off_means_a_click_on_blank_paper_never_picks_an_area() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    {
        let ed = &mut h.state_mut().editor;
        ed.change_rules(|r| r.regions_need_fill = false);
        ed.doc.begin();
        let p =
            [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)].map(|q| ed.doc.add_point(q));
        for i in 0..4 {
            ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
        }
        ed.commit("Rectangle");
    }
    h.state_mut().fit_target = Some(drift_core::Bounds {
        min: v(-10.0, -10.0),
        max: v(50.0, 40.0),
    });
    h.run();
    click_world(&mut h, v(20.0, 15.0));
    assert!(h.state().editor.selected_region.is_some());
    h.key_press(Key::Escape);
    h.run();
    h.state_mut()
        .set_pick_filter(PickFilter::ALL.with(PickKind::Areas, false));
    click_world(&mut h, v(20.0, 15.0));
    assert!(h.state().editor.selected_region.is_none());
    assert!(h.state().editor.selection.is_empty());
}

#[test]
fn the_palette_offers_one_kind_only_and_everything() {
    let Some(mut h) = harness() else { return };
    let open = |h: &mut Harness<'_, DriftApp>, text: &str| {
        h.event(Event::Key {
            key: Key::K,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: EguiModifiers::COMMAND,
        });
        h.run();
        h.run();
        type_text(h, text);
        h.key_press(Key::Enter);
        h.run();
    };
    open(&mut h, "Select blocks & groups only");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Blocks));
    open(&mut h, "Select areas only");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Areas));
    open(&mut h, "Select everything");
    assert!(filter(&h).is_all());
}
