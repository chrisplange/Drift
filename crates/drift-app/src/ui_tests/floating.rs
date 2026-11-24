//! Floating panels open where they cover nothing (not on each other, not on the picks), go
//! back where they last floated, snap to edges as they are dragged, and the drawing stays
//! where it is on screen when a dock opens, closes or changes width.

use super::*;

/// Every floating window as drawn, with the canvas.
fn windows(h: &Harness<'_, DriftApp>) -> (Vec<egui::Rect>, egui::Rect) {
    let app = h.state();
    let tops: Vec<Panel> = app.layout.floating.iter().map(|s| s[0]).collect();
    let rects = tops
        .iter()
        .map(|p| app.floating_rects(*p).expect("drawn").1)
        .collect();
    (rects, app.drawing.view.canvas_rect)
}

fn assert_apart(rects: &[egui::Rect], canvas: egui::Rect) {
    for (i, a) in rects.iter().enumerate() {
        assert!(
            canvas.expand(0.5).contains_rect(*a),
            "{a:?} outside the canvas {canvas:?}"
        );
        for b in &rects[i + 1..] {
            let both = a.intersect(*b);
            assert!(
                !both.is_positive() || both.area() < 1.0,
                "{a:?} and {b:?} overlap"
            );
        }
    }
}

#[test]
fn floated_panels_open_apart_one_at_a_time_or_all_at_once() {
    for size in [[1280.0, 800.0], [3440.0, 1440.0]] {
        // One after another, as by the menu.
        let Some(mut h) = harness() else { return };
        h.set_size(egui::Vec2::from(size));
        h.run_steps(2);
        for p in [
            Panel::Layers,
            Panel::Blocks,
            Panel::Parameters,
            Panel::Inspector,
        ] {
            h.state_mut().layout.move_to(p, Place::Float, None);
            h.run_steps(3);
        }
        let (rects, canvas) = windows(&h);
        assert_eq!(rects.len(), 4);
        assert_apart(&rects, canvas);
        // All in one frame (a layout put back, a reset).
        let Some(mut h) = harness() else { return };
        h.set_size(egui::Vec2::from(size));
        h.run_steps(2);
        for p in [Panel::Layers, Panel::Blocks, Panel::Parameters] {
            h.state_mut().layout.move_to(p, Place::Float, None);
        }
        h.run_steps(3);
        let (rects, canvas) = windows(&h);
        assert_eq!(rects.len(), 3);
        assert_apart(&rects, canvas);
    }
}

#[test]
fn a_panel_floated_again_goes_back_where_it_was() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.run_steps(3);
    // Moved by its header.
    let header = h.state().header_rect(Panel::Layers).unwrap();
    let from = header.left_center() + egui::vec2(60.0, 0.0);
    drag_steps(&mut h, from, from + egui::vec2(-300.0, 120.0), 10, None);
    let (_, was) = h.state().floating_rects(Panel::Layers).unwrap();
    // Docked, then floated again: where it was.
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Right), None);
    h.run_steps(3);
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.run_steps(3);
    let (_, now) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(
        (now.min - was.min).length() < 1.0,
        "{now:?} not back at {was:?}"
    );
    // And in the next session (no egui memory of the window): only the saved layout knows.
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Right), None);
    h.run_steps(3);
    let prefs = h.state().layout.prefs();
    let pairs: Vec<(&str, &str)> = prefs.iter().filter_map(|s| s.split_once('=')).collect();
    let Some(mut next) = harness() else { return };
    next.state_mut().layout.apply(&pairs);
    next.run_steps(2);
    next.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    next.run_steps(3);
    let (_, then) = next.state().floating_rects(Panel::Layers).unwrap();
    assert!(
        (then.min - was.min).length() < 1.0,
        "{then:?} not back at {was:?} next session"
    );
    // With another window over that place now, it goes somewhere free instead.
    next.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Right), None);
    next.state_mut()
        .layout
        .spots
        .insert(Panel::Blocks, (was.min.x, was.min.y));
    next.state_mut()
        .layout
        .move_to(Panel::Blocks, Place::Float, None);
    next.run_steps(3);
    next.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    next.run_steps(3);
    let (rects, canvas) = windows(&next);
    assert_eq!(rects.len(), 2);
    assert_apart(&rects, canvas);
}

#[test]
fn hiding_and_showing_the_panels_moves_and_folds_nothing() {
    for size in [[700.0, 800.0], [1280.0, 800.0]] {
        let Some(mut h) = harness() else { return };
        h.set_size(egui::Vec2::from(size));
        h.run_steps(2);
        for p in [Panel::Layers, Panel::Blocks] {
            h.state_mut().layout.move_to(p, Place::Float, None);
            h.run_steps(3);
        }
        // (In the narrow window, just floated, they open folded: open them.)
        h.state_mut().layout.folded.clear();
        h.run_steps(3);
        let before = windows(&h).0;
        h.state_mut().dock_view.hide_all = true;
        h.run_steps(3);
        h.state_mut().dock_view.hide_all = false;
        h.run_steps(3);
        assert!(h.state().layout.folded.is_empty(), "{size:?}: folded");
        assert_eq!(windows(&h).0, before, "{size:?}: moved");
    }
}

#[test]
fn a_panel_just_floated_in_a_narrow_window_opens_folded() {
    let Some(mut h) = harness() else { return };
    h.set_size(egui::vec2(700.0, 800.0));
    h.run_steps(3);
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.run_steps(3);
    assert!(h.state().layout.folded.contains(&Panel::Layers));
}

#[test]
fn the_rest_of_a_stack_stays_put_when_one_leaves_it() {
    for (leaving, how) in [
        (Panel::Layers, "top docked"),
        (Panel::Blocks, "lower docked"),
        (Panel::Blocks, "lower closed"),
    ] {
        let Some(mut h) = harness() else { return };
        h.state_mut()
            .layout
            .move_to(Panel::Layers, Place::Float, None);
        h.run_steps(3);
        // Moved off its first place, so a window egui made afresh would show it.
        let header = h.state().header_rect(Panel::Layers).unwrap();
        let from = header.left_center() + egui::vec2(60.0, 0.0);
        drag_steps(&mut h, from, from + egui::vec2(-250.0, 90.0), 8, None);
        h.state_mut()
            .layout
            .stack(Panel::Blocks, Panel::Layers, true);
        h.run_steps(3);
        let (_, was) = h.state().floating_rects(Panel::Layers).unwrap();
        // From inside the window, as its header button and menu do.
        h.state_mut()
            .leave_stack_mid_frame(leaving, how.ends_with("closed"));
        h.run_steps(3);
        let stays = if leaving == Panel::Layers {
            Panel::Blocks
        } else {
            Panel::Layers
        };
        let (_, now) = h.state().floating_rects(stays).unwrap();
        assert!(
            (now.min - was.min).length() < 1.0,
            "{how}: {now:?} not where the stack was, {was:?}"
        );
        assert!(!h.state().layout.folded.contains(&stays), "{how}: folded");
    }
}

#[test]
fn a_dragged_floating_panel_snaps_to_the_canvas_edge_and_to_another() {
    let Some(mut h) = harness() else { return };
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.run_steps(3);
    let canvas = h.state().drawing.view.canvas_rect;
    let (_, w) = h.state().floating_rects(Panel::Layers).unwrap();
    // Dragged to 5 points short of the canvas's right edge: flush with it.
    let header = h.state().header_rect(Panel::Layers).unwrap();
    let from = header.left_center() + egui::vec2(60.0, 0.0);
    let gap = canvas.right() - w.right();
    drag_steps(&mut h, from, from + egui::vec2(gap - 5.0, 40.0), 8, None);
    let (_, w) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(
        (w.right() - canvas.right()).abs() < 0.5,
        "{w:?} not flush with {canvas:?}"
    );
    // A second window dragged to 6 points short of the first's left side: abuts it.
    h.state_mut()
        .layout
        .move_to(Panel::Blocks, Place::Float, None);
    h.run_steps(3);
    let (_, b) = h.state().floating_rects(Panel::Blocks).unwrap();
    let header = h.state().header_rect(Panel::Blocks).unwrap();
    let from = header.left_center() + egui::vec2(60.0, 0.0);
    let to = from + egui::vec2(w.left() - 6.0 - b.right(), w.top() + 3.0 - b.top());
    drag_steps(&mut h, from, to, 10, None);
    let (_, b) = h.state().floating_rects(Panel::Blocks).unwrap();
    assert!(
        (b.right() - w.left()).abs() < 0.5,
        "{b:?} doesn't abut {w:?}"
    );
    assert!(
        (b.top() - w.top()).abs() < 0.5,
        "{b:?} not lined up with {w:?}"
    );
}

#[test]
fn a_floated_panel_keeps_off_the_picks() {
    let Some(mut h) = harness() else { return };
    // A rectangle at the canvas's top right, where a floating panel would first go, picked.
    let map = screen_map(&h);
    let canvas = h.state().drawing.view.canvas_rect;
    let a = map.to_world(canvas.right_top() + egui::vec2(-280.0, 40.0));
    let b = map.to_world(canvas.right_top() + egui::vec2(-60.0, 200.0));
    let lines = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let p = [a, DVec2::new(b.x, a.y), b, DVec2::new(a.x, b.y)].map(|at| ed.doc.add_point(at));
        let lines: Vec<_> = (0..4)
            .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
            .collect();
        ed.commit("Rectangle");
        lines
    };
    h.state_mut().drawing.editor.selection = lines.into_iter().collect();
    h.run_steps(2);
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Float, None);
    h.run_steps(3);
    let map = screen_map(&h);
    let picks = egui::Rect::from_two_pos(map.to_screen(a), map.to_screen(b));
    let (_, w) = h.state().floating_rects(Panel::Layers).unwrap();
    assert!(!w.intersects(picks), "{w:?} on the picks {picks:?}");
}

#[test]
fn the_drawing_holds_still_when_a_dock_opens_closes_or_folds() {
    let Some(mut h) = harness() else { return };
    h.run_steps(2);
    let spot = h.state().drawing.view.camera.center + DVec2::new(10.0, 5.0);
    let at = screen_map(&h).to_screen(spot);
    let still = |h: &Harness<'_, DriftApp>, what: &str| {
        let now = screen_map(h).to_screen(spot);
        assert!(
            (now - at).length() < 0.5,
            "{what}: moved from {at:?} to {now:?}"
        );
    };
    // A dock opens on the left.
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Left), None);
    h.run_steps(3);
    assert!(h.state().drawing.view.canvas_rect.left() > 100.0);
    still(&h, "left dock opened");
    // The right one folds to its rail, then everything hides and comes back.
    h.state_mut().layout.collapsed[1] = true;
    h.run_steps(3);
    still(&h, "right dock folded");
    h.state_mut().dock_view.hide_all = true;
    h.run_steps(3);
    still(&h, "panels hidden");
    h.state_mut().dock_view.hide_all = false;
    h.state_mut().layout.collapsed[1] = false;
    h.run_steps(3);
    still(&h, "panels back");
}

#[test]
fn a_fit_asked_for_as_a_dock_changes_is_still_made() {
    let Some(mut h) = harness() else { return };
    h.run_steps(2);
    let bounds = drift_core::Bounds {
        min: DVec2::new(500.0, 500.0),
        max: DVec2::new(700.0, 600.0),
    };
    // (As an underlay brought in opens its panel and zooms to it.)
    h.state_mut().drawing.view.fit_target = Some(bounds);
    h.state_mut()
        .layout
        .move_to(Panel::Layers, Place::Dock(Side::Left), None);
    h.run_steps(3);
    let camera = h.state().drawing.view.camera;
    assert!(
        (camera.center - bounds.center()).length() < 1e-6,
        "{camera:?} not fitted to {bounds:?}"
    );
}

#[test]
fn a_fit_already_shown_is_made_again_when_the_window_resizes() {
    let Some(mut h) = harness() else { return };
    h.run_steps(2);
    let bounds = drift_core::Bounds {
        min: DVec2::new(500.0, 500.0),
        max: DVec2::new(700.0, 600.0),
    };
    h.state_mut().drawing.view.fit_target = Some(bounds);
    h.run_steps(3);
    let small = h.state().drawing.view.camera.scale;
    // (A window maximised as it opens.)
    h.set_size(egui::vec2(2560.0, 1400.0));
    h.run_steps(3);
    let camera = h.state().drawing.view.camera;
    assert!(
        (camera.center - bounds.center()).length() < 1e-6,
        "{camera:?}"
    );
    assert!(
        camera.scale > small * 1.5,
        "{} not bigger than {small}",
        camera.scale
    );
}
