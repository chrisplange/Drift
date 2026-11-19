//! The quick-constrain bar (the relations and dimensions that fit what is picked, on a bar
//! at the foot of the canvas), the message that keeps clear of it, and the hint on an empty
//! canvas.

use super::*;
use crate::app::quick_bar;
use drift_core::{Constraint, EntityId};

/// Two slanted lines that aren't parallel, well clear of the foot of the canvas, both picked.
fn two_lines(h: &mut Harness<'_, DriftApp>) -> [EntityId; 2] {
    let v = DVec2::new;
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let ends = [
        (v(-40.0, 10.0), v(0.0, 25.0)),
        (v(-40.0, -10.0), v(10.0, -5.0)),
    ];
    let lines = ends.map(|(a, b)| {
        let (a, b) = (ed.doc.add_point(a), ed.doc.add_point(b));
        ed.doc.add_line(a, b).expect("a line")
    });
    ed.commit("two lines");
    ed.selection = lines.into_iter().collect();
    h.run();
    h.run();
    lines
}

fn constraints(h: &Harness<'_, DriftApp>) -> Vec<Constraint> {
    let doc = &h.state().drawing.editor.doc;
    doc.constraints().map(|(_, c)| *c).collect()
}

/// Where the bar was drawn on the last frame, if it was.
fn bar(h: &Harness<'_, DriftApp>) -> Option<egui::Rect> {
    h.ctx.data(|d| d.get_temp(quick_bar::rect_id()))
}

fn toast(h: &Harness<'_, DriftApp>) -> Option<egui::Rect> {
    h.ctx.data(|d| d.get_temp(crate::overlay::toast_rect_id()))
}

fn shows(h: &Harness<'_, DriftApp>) -> bool {
    h.query_by_label("Constrain: Parallel").is_some()
}

#[test]
fn the_bar_shows_for_two_picked_lines_at_the_foot_of_the_canvas() {
    let Some(mut h) = harness() else { return };
    assert!(!shows(&h) && bar(&h).is_none(), "nothing is picked");
    two_lines(&mut h);
    let canvas = h.state().drawing.view.canvas_rect;
    let at = bar(&h).expect("the bar shows");
    assert!(canvas.contains_rect(at), "{at:?} in {canvas:?}");
    assert_eq!(at.height(), quick_bar::HEIGHT);
    assert!((at.bottom() - (canvas.bottom() - quick_bar::MARGIN)).abs() < 0.5);
    assert!((at.center().x - canvas.center().x).abs() < 1.0, "{at:?}");
    // A button for each relation the Inspector offers, and a chip for each dimension, all
    // inside the bar.
    let offered: Vec<&str> = {
        let app = h.state();
        app.drawing
            .editor
            .relation_options()
            .iter()
            .map(|o| o.label)
            .collect()
    };
    assert!(offered.contains(&"Parallel") && offered.contains(&"Perpendicular"));
    for name in &offered {
        let button = h.get_by_label(&format!("Constrain: {name}")).rect();
        assert!(at.contains_rect(button), "{name}: {button:?} in {at:?}");
    }
    let angle = h.get_by_label("Dimension: Angle").rect();
    assert!(at.contains_rect(angle));
    // The relations come first.
    assert!(h.get_by_label("Constrain: Parallel").rect().right() <= angle.left());
}

#[test]
fn the_bars_parallel_does_what_the_inspectors_chip_does() {
    let Some(mut bar_h) = harness() else { return };
    let Some(mut chip_h) = harness() else { return };
    let lines = two_lines(&mut bar_h);
    two_lines(&mut chip_h);
    assert!(constraints(&bar_h).is_empty());
    bar_h.get_by_label("Constrain: Parallel").click();
    bar_h.run();
    chip_h.get_by_label("Parallel").click();
    chip_h.run();
    let added = constraints(&bar_h);
    assert!(
        matches!(added.as_slice(), [Constraint::Parallel { a, b }]
            if [*a, *b].iter().all(|l| lines.contains(l))),
        "{added:?}"
    );
    assert_eq!(added, constraints(&chip_h), "the same relation either way");
    // One undo step, as from the Inspector.
    bar_h.state_mut().drawing.editor.undo();
    assert!(constraints(&bar_h).is_empty());
    // Once it holds, asking again says so (as the Inspector's chip does).
    bar_h.state_mut().drawing.editor.redo();
    bar_h.run();
    if bar_h.query_by_label("Constrain: Parallel").is_some() {
        bar_h.get_by_label("Constrain: Parallel").click();
        bar_h.run();
        assert_eq!(constraints(&bar_h).len(), 1);
    }
}

#[test]
fn a_dimension_chip_adds_the_dimension_and_its_value_is_typed_straight_away() {
    let Some(mut h) = harness() else { return };
    two_lines(&mut h);
    h.get_by_label("Dimension: Angle").click();
    h.run();
    h.run();
    let angle = |h: &Harness<'_, DriftApp>| {
        constraints(h).iter().find_map(|c| match c {
            Constraint::Angle { degrees, .. } => Some(*degrees),
            _ => None,
        })
    };
    let before = angle(&h).expect("an angle between the lines");
    assert!((before - 30.0).abs() > 1.0, "{before}");
    // The Inspector's field for it has the keyboard: type the value and press Enter.
    assert!(h.state().drawing.focus_dimension.is_none(), "taken up");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(&mut h, "30");
    h.key_press(Key::Enter);
    h.run();
    h.run();
    let after = angle(&h).expect("still there");
    assert!((after - 30.0).abs() < 1e-6, "{after}");
}

#[test]
fn with_the_inspector_out_of_view_a_new_dimension_is_typed_on_the_canvas() {
    let Some(mut h) = harness() else { return };
    two_lines(&mut h);
    h.state_mut().show_panel(Panel::Layers);
    h.run();
    h.run();
    assert!(!h.state().layout.is_shown(Panel::Inspector));
    h.get_by_label("Dimension: Angle").click();
    h.run();
    assert_eq!(constraints(&h).len(), 1);
    assert!(h.state().drawing.focus_dimension.is_none());
    assert!(
        h.state().drawing.dimension_edit.is_some(),
        "its value is typed on the dimension itself"
    );
    // (And the bar is out of the way while it is.)
    h.run();
    assert!(bar(&h).is_none());
}

#[test]
fn the_bar_is_hidden_whenever_it_would_be_in_the_way() {
    let Some(mut h) = harness() else { return };
    let lines = two_lines(&mut h);
    assert!(shows(&h));
    // Nothing picked.
    h.state_mut().drawing.editor.selection.clear();
    h.run();
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "nothing picked");
    h.state_mut().drawing.editor.selection = lines.into_iter().collect();
    h.run();
    h.run();
    assert!(shows(&h));
    // A drawing tool.
    h.key_press(Key::L);
    h.run();
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "the Line tool");
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().set_tool(ToolKind::Select);
    h.state_mut().drawing.editor.selection = lines.into_iter().collect();
    h.run();
    h.run();
    assert!(shows(&h));
    // While the pointer is down on the canvas: a click, a drag, a box.
    let from = screen(&h, DVec2::new(40.0, 40.0));
    h.hover_at(from);
    h.run();
    h.event(Event::ModifiersChanged(EguiModifiers::SHIFT));
    button(&h, from, true);
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "a press on the canvas");
    h.hover_at(from + egui::vec2(30.0, 20.0));
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "a drag on the canvas");
    button(&h, from + egui::vec2(30.0, 20.0), false);
    h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    h.run();
    h.run();
    assert_eq!(h.state().drawing.editor.selection.len(), 2, "still picked");
    assert!(shows(&h), "back once the pointer is up");
    // The command palette.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::K);
    h.run();
    h.run();
    assert!(h.state().palette.is_some());
    assert!(!shows(&h) && bar(&h).is_none(), "the palette is open");
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert!(shows(&h));
    // The relations pie.
    let ctx = h.ctx.clone();
    h.state_mut().open_pie(&ctx);
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "the pie is open");
    h.state_mut().pie = None;
    h.run();
    h.run();
    assert!(shows(&h));
    // A dialog.
    h.state_mut().show_settings = true;
    h.run();
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "Preferences is open");
    h.state_mut().show_settings = false;
    h.run();
    h.run();
    assert!(shows(&h));
    // On a sheet.
    h.state_mut().new_sheet();
    h.run();
    h.run();
    assert!(h.state().drawing.view.sheet_preview);
    assert!(!shows(&h), "on a sheet");
    h.state_mut().show_sheet(None);
    h.run();
    h.run();
    h.state_mut().drawing.editor.selection = lines.into_iter().collect();
    h.run();
    h.run();
    assert!(shows(&h));
    // The preference, which is kept.
    assert!(h.state().view_prefs().ends_with(";quickbar=1"));
    h.state_mut().apply_view_prefs("quickbar=0");
    h.run();
    h.run();
    assert!(!shows(&h) && bar(&h).is_none(), "turned off");
    assert!(h.state().view_prefs().ends_with(";quickbar=0"));
    // (The Inspector's chips are there all along.)
    h.get_by_label("Parallel");
    h.state_mut().apply_view_prefs("quickbar=1");
    h.run();
    h.run();
    assert!(shows(&h));
}

#[test]
fn the_bar_needs_a_canvas_wide_enough() {
    if harness().is_none() {
        return;
    }
    let mut h = Harness::builder()
        .with_size([520.0, 800.0])
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    h.run();
    two_lines(&mut h);
    assert!(h.state().drawing.view.canvas_rect.width() < 420.0);
    assert!(!shows(&h) && bar(&h).is_none());
}

#[test]
fn a_message_sits_above_the_bar_and_never_on_it() {
    let Some(mut h) = harness() else { return };
    // With no bar the message is at the foot of the canvas.
    let now = h.ctx.input(|i| i.time);
    h.state_mut()
        .set_status("Nothing to undo".into(), false, now);
    h.run();
    let canvas = h.state().drawing.view.canvas_rect;
    let alone = toast(&h).expect("the message shows");
    assert!((alone.bottom() - (canvas.bottom() - 24.0)).abs() < 0.5);
    assert!((alone.center().x - canvas.center().x).abs() < 1.0);
    // With the bar it moves up, clear of it; a long one that wraps too.
    two_lines(&mut h);
    for text in [
        "Parallel: redundant",
        "A much longer message that says a good deal about what went wrong and what to do \
         about it, so that it wraps on to a second line of the card it is shown on, and more",
    ] {
        let now = h.ctx.input(|i| i.time);
        h.state_mut().set_status(text.into(), true, now);
        h.run();
        let at = bar(&h).expect("the bar shows");
        let message = toast(&h).expect("the message shows");
        assert!(!message.intersects(at), "{message:?} over {at:?}");
        assert!(
            message.bottom() <= at.top() - 4.0,
            "{message:?} above {at:?}"
        );
        assert!(canvas.contains_rect(message));
    }
    save_png("drift_ui_quick_bar_toast.png", &mut h);
    // The message gone, nothing is remembered of it.
    h.state_mut().status = None;
    h.run();
    assert!(toast(&h).is_none());
}

#[test]
fn the_bars_names_are_its_own_beside_the_inspectors_chips() {
    let Some(mut h) = harness() else { return };
    two_lines(&mut h);
    assert!(h.state().layout.is_shown(Panel::Inspector));
    let at = bar(&h).expect("the bar shows");
    let names: Vec<&str> = {
        let ed = &h.state().drawing.editor;
        ed.relation_options()
            .iter()
            .chain(ed.dimension_options().iter())
            .map(|o| o.label)
            .collect()
    };
    assert!(names.len() >= 4, "{names:?}");
    for name in names {
        // Exactly one of each (`get_by_label` panics on two): the Inspector's chip by the
        // bare name, in the dock; the bar's by its own.
        let chip = h.get_by_label(name).rect();
        assert!(!at.intersects(chip), "{name} is the Inspector's");
        let dimension = format!("Dimension: {name}");
        let relation = format!("Constrain: {name}");
        let on_bar = h
            .query_by_label(&relation)
            .or_else(|| h.query_by_label(&dimension))
            .unwrap_or_else(|| panic!("{name} is on the bar"))
            .rect();
        assert!(at.contains_rect(on_bar));
    }
    // The caption is paint, not a thing with a name.
    assert!(h.query_by_label("Constrain").is_none());
}

#[test]
fn the_bar_moves_to_the_top_when_the_picks_lie_under_it() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    let map = screen_map(&h);
    // Two lines across the foot of the canvas, where the bar would be.
    let y = canvas.bottom() - quick_bar::MARGIN - quick_bar::HEIGHT / 2.0;
    let ends = [
        (
            egui::pos2(canvas.center().x - 120.0, y - 6.0),
            egui::pos2(canvas.center().x + 40.0, y + 2.0),
        ),
        (
            egui::pos2(canvas.center().x - 60.0, y + 8.0),
            egui::pos2(canvas.center().x + 120.0, y - 4.0),
        ),
    ];
    let lines = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let lines = ends.map(|(a, b)| {
            let (a, b) = (
                ed.doc.add_point(map.to_world(a)),
                ed.doc.add_point(map.to_world(b)),
            );
            ed.doc.add_line(a, b).expect("a line")
        });
        ed.commit("two lines");
        ed.selection = lines.into_iter().collect();
        lines
    };
    h.run();
    h.run();
    let at = bar(&h).expect("the bar shows");
    assert!(
        (at.top() - (canvas.top() + quick_bar::MARGIN)).abs() < 0.5,
        "{at:?}"
    );
    assert!(h.get_by_label("Constrain: Parallel").rect().bottom() < canvas.center().y);
    // So what is picked can still be clicked: a click on one of the lines picks it alone.
    let on_line = ends[0].0 + (ends[0].1 - ends[0].0) * 0.25;
    h.hover_at(on_line);
    h.run();
    button(&h, on_line, true);
    h.run();
    button(&h, on_line, false);
    h.run();
    h.run();
    let picked: Vec<EntityId> = h.state().drawing.editor.selection.iter().copied().collect();
    assert_eq!(picked, vec![lines[0]]);
}

#[test]
fn an_empty_canvas_says_how_to_start_without_adding_anything_to_click() {
    let Some(mut h) = harness() else { return };
    let canvas = h.state().drawing.view.canvas_rect;
    assert!(h.state().empty_hint_wanted(canvas));
    // Painted only: nothing is named after it, and the tools it mentions are still the
    // rail's alone.
    assert!(
        h.query_all_by_label_contains("Search every tool")
            .next()
            .is_none()
    );
    h.get_by_label("Line");
    h.get_by_label("Rectangle");
    // It is there to see: the canvas under the middle isn't blank.
    let image = h.render().expect("render");
    let background = *image.get_pixel(canvas.right() as u32 - 40, canvas.top() as u32 + 40);
    let marked = (canvas.center().y as u32..canvas.bottom() as u32 - 80)
        .flat_map(|y| {
            (canvas.center().x as u32 - 150..canvas.center().x as u32 + 150).map(move |x| (x, y))
        })
        .filter(|(x, y)| {
            let p = image.get_pixel(*x, *y);
            (0..3).any(|c| p.0[c].abs_diff(background.0[c]) > 60)
        })
        .count();
    assert!(marked > 200, "{marked} pixels of the hint");
    save_png("drift_ui_empty_canvas_hint.png", &mut h);
    // It takes no clicks: the canvas under it has the pointer, and a box can be dragged
    // from the middle of it.
    let on_hint = canvas.center() + egui::vec2(0.0, canvas.height() * 0.16);
    h.hover_at(on_hint);
    h.run();
    assert!(h.state().drawing.view.pointer.is_some());
    button(&h, on_hint, true);
    h.run();
    h.hover_at(on_hint + egui::vec2(40.0, 30.0));
    h.run();
    assert!(
        h.state().tool.is_busy(),
        "a selection box, from on the hint"
    );
    assert!(
        !h.state().empty_hint_wanted(canvas),
        "out of the way meanwhile"
    );
    button(&h, on_hint + egui::vec2(40.0, 30.0), false);
    h.run();
    assert!(h.state().empty_hint_wanted(canvas));
    // Not with the start panel, nor with a drawing tool.
    h.state_mut().show_start = true;
    h.run();
    assert!(!h.state().empty_hint_wanted(canvas));
    h.get_by_label("New sketch").click();
    h.run();
    h.run();
    assert!(h.state().empty_hint_wanted(canvas));
    h.key_press(Key::L);
    h.run();
    assert!(!h.state().empty_hint_wanted(canvas));
    // Gone for good once anything exists.
    click_world(&mut h, DVec2::new(-20.0, 10.0));
    click_world(&mut h, DVec2::new(20.0, 20.0));
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().set_tool(ToolKind::Select);
    h.run();
    assert_eq!(lines(&h), 1);
    assert!(!h.state().empty_hint_wanted(canvas));
}
