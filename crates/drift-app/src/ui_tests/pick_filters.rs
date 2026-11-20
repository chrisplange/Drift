//! The Selection filter (drift-releases#9, #480, #496): one control on the select tool's bar
//! naming what it picks, its list of kinds to combine (Alt-click or double-click for one
//! alone, Pick everything), a mark on the Select tool while it is narrowed, the filter applied
//! to clicks, boxes, Select all and the palette, and what a click or box on a kind it leaves
//! out says.

use drift_core::{EntityId, FillId, Rgb};
use egui_kittest::kittest::NodeT as _;

use drift_edit::editor::{PickFilter, PickKind};

use super::*;

/// A filled rectangle (a shape, its fill solid), a loose line and a sketch text, in view.
fn drawing(h: &mut Harness<'_, DriftApp>) -> (Vec<EntityId>, FillId, EntityId) {
    let v = DVec2::new;
    let ed = &mut h.state_mut().drawing.editor;
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
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: v(-10.0, -10.0),
        max: v(110.0, 45.0),
    });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
    (sides, fill, line)
}

fn filter(h: &Harness<'_, DriftApp>) -> PickFilter {
    h.state().drawing.editor.prefs.pick_filter
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
    pick_toggle(&mut h, "Pick Annotations");
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
    pick_toggle(&mut h, "Pick Text");
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
    pick_toggle(&mut h, "Pick everything");
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
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(ed.selection.is_empty(), "{:?}", ed.selection);
    }
    click_world(&mut h, DVec2::new(75.0, 10.0));
    assert!(
        h.state().drawing.editor.selection.is_empty(),
        "the line isn't picked"
    );
    box_all(&mut h);
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selected_fills, [fill].into());
        assert!(ed.selection.is_empty() && ed.selected_texts.is_empty());
    }
    h.state_mut().drawing.editor.selected_fills.clear();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    h.run();
    {
        let ed = &h.state().drawing.editor;
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
        let ed = &h.state().drawing.editor;
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
    let ed = &h.state().drawing.editor;
    assert_eq!(ed.selected_fills, [fill].into());
    assert!(ed.selection.contains(&line));
}

#[test]
fn areas_off_means_a_click_on_blank_paper_never_picks_an_area() {
    let Some(mut h) = harness() else { return };
    let v = DVec2::new;
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.change_rules(|r| r.regions_need_fill = false);
        ed.doc.begin();
        let p =
            [v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)].map(|q| ed.doc.add_point(q));
        for i in 0..4 {
            ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap();
        }
        ed.commit("Rectangle");
    }
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: v(-10.0, -10.0),
        max: v(50.0, 40.0),
    });
    h.run();
    click_world(&mut h, v(20.0, 15.0));
    assert!(h.state().drawing.editor.selected_region.is_some());
    h.key_press(Key::Escape);
    h.run();
    h.state_mut()
        .set_pick_filter(PickFilter::ALL.with(PickKind::Areas, false));
    click_world(&mut h, v(20.0, 15.0));
    assert!(h.state().drawing.editor.selected_region.is_none());
    assert!(h.state().drawing.editor.selection.is_empty());
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
    open(&mut h, "Selection filter: Blocks & groups only");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Blocks));
    open(&mut h, "Selection filter: Areas only");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Areas));
    open(&mut h, "Pick everything");
    assert!(filter(&h).is_all());
}

/// What the bar's Selection filter button says ("All", "Edges", "3 kinds").
fn control_text(h: &Harness<'_, DriftApp>) -> Option<String> {
    h.query_by_label("Selection filter")
        .and_then(|n| n.accesskit_node().value())
}

/// The status message now, if any.
fn status(h: &Harness<'_, DriftApp>) -> Option<String> {
    h.state().status.as_ref().map(|s| s.text.clone())
}

/// #480: one control names the filter; its list sets it; picking follows it; with something
/// picked, only the compact button shows, and only while the filter is narrowed.
#[test]
fn one_control_names_the_filter_and_picking_follows_it() {
    let Some(mut h) = harness() else { return };
    let (sides, fill, line) = drawing(&mut h);
    // Everything: one control saying All, the kinds out of sight until it is opened.
    assert_eq!(control_text(&h).as_deref(), Some("All"));
    assert!(
        h.query_by_label("Pick Points").is_none(),
        "no chips on the bar"
    );
    let width = h.get_by_label("Selection filter").rect().width();
    assert!(width < 80.0, "a small control: {width}");
    open_pick_filter(&mut h);
    for kind in PickKind::ALL {
        assert!(
            h.query_by_label(&format!("Pick {}", kind.label()))
                .is_some(),
            "{kind:?} in the list"
        );
    }
    // Edges alone, through the list: the control names it.
    pick_only(&mut h, "Edges");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Edges));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(control_text(&h).as_deref(), Some("Edges"));
    // A click inside the shape picks nothing (its fill is filtered out); on the line, the
    // line.
    click_world(&mut h, DVec2::new(20.0, 15.0));
    assert!(h.state().drawing.editor.selected_fills.is_empty());
    assert!(h.state().drawing.editor.selection.is_empty());
    click_world(&mut h, DVec2::new(75.0, 10.0));
    assert_eq!(h.state().drawing.editor.selection, [line].into());
    // Something picked: still named, as the button alone, no wider than before.
    assert_eq!(control_text(&h).as_deref(), Some("Edges"));
    assert!(h.get_by_label("Selection filter").rect().width() < 90.0);
    // A kind added: "Edges & Fills"; a third: "3 kinds".
    pick_toggle(&mut h, "Pick Fills");
    pick_toggle(&mut h, "Pick Text");
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(control_text(&h).as_deref(), Some("3 kinds"));
    // Everything again: the control stays, saying All, with something picked too.
    pick_toggle(&mut h, "Pick everything");
    assert!(filter(&h).is_all());
    click_world(&mut h, DVec2::new(20.0, 15.0));
    assert_eq!(h.state().drawing.editor.selected_fills, [fill].into());
    assert!(
        sides
            .iter()
            .all(|s| h.state().drawing.editor.selection.contains(s))
    );
    assert_eq!(control_text(&h).as_deref(), Some("All"));
    // With any tool, not only Select.
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::L);
    h.run();
    assert_eq!(
        control_text(&h).as_deref(),
        Some("All"),
        "with the Line tool"
    );
    pick_only(&mut h, "Fills");
    assert_eq!(filter(&h), PickFilter::only(PickKind::Fills));
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(control_text(&h).as_deref(), Some("Fills"));
}

/// #496: a click or box on kinds the filter leaves out says which, and the filter; a click on
/// the message picks everything again. The Select hint and tooltip name the filter, and its
/// mark on the rail has a 16-point target.
#[test]
fn a_click_on_a_filtered_out_kind_says_so() {
    let Some(mut h) = harness() else { return };
    drawing(&mut h);
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Edges));
    h.run();
    // The status bar's hint names the filter.
    assert!(
        h.query_by_label_contains("Selection filter: Edges only · Click to select")
            .is_some(),
        "the Select hint names the filter"
    );
    // A click inside the filled shape: the fill is filtered out, and the message says so.
    click_world(&mut h, DVec2::new(20.0, 15.0));
    h.run();
    assert_eq!(
        status(&h).as_deref(),
        Some(
            "Fills are filtered out · Selection filter: Edges only · Click here to pick everything"
        )
    );
    // On the text: singular.
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Fills));
    click_world(&mut h, DVec2::new(64.0, 32.0));
    h.run();
    assert_eq!(
        status(&h).as_deref(),
        Some("Text is filtered out · Selection filter: Fills only · Click here to pick everything")
    );
    // A box round the loose line, with Text only: its edge is filtered out.
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Text));
    let (from, to) = (
        screen(&h, DVec2::new(57.0, -4.0)),
        screen(&h, DVec2::new(93.0, 24.0)),
    );
    drag_steps(&mut h, from, to, 4, None);
    h.run();
    assert_eq!(
        status(&h).as_deref(),
        Some(
            "Edges are filtered out · Selection filter: Text only · Click here to pick everything"
        )
    );
    // A click on blank paper says nothing.
    h.state_mut().status = None;
    click_world(&mut h, DVec2::new(100.0, 40.0));
    h.run();
    assert!(status(&h).is_none(), "{:?}", status(&h));
    // The rail's mark: a 16-point target, its tooltip naming the filter; the Select tool's
    // tooltip names it too.
    let mark = h.get_by_label("Selection filter on").rect();
    assert!(mark.width() >= 16.0 && mark.height() >= 16.0, "{mark:?}");
    h.hover_at(mark.center());
    h.run_steps(30);
    assert!(
        h.query_by_label("Selection filter: Text only").is_some(),
        "the mark's tooltip"
    );
    let tile = h.get_all_by_label("Select").next().unwrap().rect();
    h.hover_at(egui::pos2(tile.left() + 6.0, tile.bottom() - 6.0));
    h.run_steps(30);
    assert!(
        h.query_by_label("Selection filter: Text only").is_some(),
        "the Select tool's tooltip"
    );
    // The message, clicked: everything again.
    h.state_mut()
        .set_pick_filter(PickFilter::only(PickKind::Edges));
    click_world(&mut h, DVec2::new(20.0, 15.0));
    h.run();
    let offer = h
        .get_by_label("Pick everything (Selection filter message)")
        .rect();
    h.hover_at(offer.center());
    h.run();
    button(&h, offer.center(), true);
    h.run();
    button(&h, offer.center(), false);
    h.run();
    assert!(filter(&h).is_all());
    assert!(
        h.state().drawing.editor.selected_fills.is_empty(),
        "the click on the message picked nothing"
    );
}

/// Renders for review (#480, #496): the Selection filter at 3440 × 1440 and 740 points wide, in
/// Midnight, Light and Obsidian, into `target/tmp/pick-filter/`. Ignored: it sets the look,
/// which every test shares.
#[test]
#[ignore]
fn pick_filter_review_renders() {
    use crate::theme::{DarkLook, Look};
    if !crate::test_gpu::available() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/pick-filter");
    std::fs::create_dir_all(&dir).unwrap();
    for (look, look_name) in [
        (Look::Midnight, "midnight"),
        (Look::Light, "light"),
        (Look::Obsidian, "obsidian"),
    ] {
        for (size, size_name) in [([3440.0, 1440.0], "3440x1440"), ([740.0, 800.0], "740w")] {
            let mut h = Harness::builder()
                .with_size(size)
                .wgpu_setup(crate::test_gpu::setup())
                .build_eframe(|cc| DriftApp::new(cc));
            crate::theme::set_dark_look(if look == Look::Obsidian {
                DarkLook::Obsidian
            } else {
                DarkLook::Midnight
            });
            crate::theme::set_look(&h.ctx, look);
            h.run();
            drawing(&mut h);
            let shot = |h: &mut Harness<'_, DriftApp>, scene: &str| {
                h.run();
                let image = h.render().expect("render");
                image
                    .save(dir.join(format!("{scene}_{size_name}_{look_name}.png")))
                    .unwrap();
            };
            // a: everything, nothing picked.
            shot(&mut h, "a_all");
            // b: the list open, everything on.
            open_pick_filter(&mut h);
            shot(&mut h, "b_list_all");
            // c: Edges alone, the list open.
            pick_only(&mut h, "Edges");
            shot(&mut h, "c_list_edges");
            h.key_press(Key::Escape);
            h.run();
            // d: Edges, closed: the control names it; the rail's mark.
            shot(&mut h, "d_edges");
            // e: a click on the fill: the message, and the hint.
            click_world(&mut h, DVec2::new(20.0, 15.0));
            shot(&mut h, "e_filtered_click");
            // f: the Select tool's tooltip with the filter line.
            let tile = h.get_all_by_label("Select").next().unwrap().rect();
            h.hover_at(egui::pos2(tile.left() + 6.0, tile.bottom() - 6.0));
            h.run_steps(30);
            shot(&mut h, "f_select_tip");
            // g: the mark's own tooltip.
            let mark = h.get_by_label("Selection filter on").rect();
            h.hover_at(mark.center());
            h.run_steps(30);
            shot(&mut h, "g_mark_tip");
            // h: something picked, narrowed: only the button; i: picked, everything.
            h.state_mut()
                .set_pick_filter(PickFilter::only(PickKind::Edges).with(PickKind::Fills, true));
            click_world(&mut h, DVec2::new(20.0, 15.0));
            h.hover_at(screen(&h, DVec2::new(100.0, 40.0)));
            shot(&mut h, "h_picked_narrowed");
            h.state_mut().set_pick_filter(PickFilter::ALL);
            click_world(&mut h, DVec2::new(20.0, 15.0));
            h.hover_at(screen(&h, DVec2::new(100.0, 40.0)));
            shot(&mut h, "i_picked_all");
            // j: another tool: the Selection filter is still there.
            h.key_press(Key::Escape);
            h.run();
            h.key_press(Key::L);
            shot(&mut h, "j_line_tool");
        }
    }
    crate::theme::set_dark_look(DarkLook::Obsidian);
}
