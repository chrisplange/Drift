//! Solves that ease into place (#514): only the drawing moves, picking uses the solved
//! geometry mid-animation, the preference turns it off and is kept with the app's settings,
//! and the canvas is idle again once an animation ends.

use std::time::Duration;

use drift_core::{Constraint, ConstraintId, DVec2, EntityId, FillId};

use super::*;

/// Frames this far apart (seconds): several to an animation.
const STEP: f32 = 0.02;

/// A 40 mm horizontal line fixed at its left end, dimensioned. Returns (right end, the
/// dimension), with frames [`STEP`] apart.
fn dimensioned_line() -> Option<(Harness<'static, DriftApp>, EntityId, ConstraintId)> {
    if !crate::test_gpu::available() {
        return None;
    }
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_step_dt(STEP)
        .with_max_steps(60)
        .wgpu_setup(crate::test_gpu::setup())
        .build_eframe(|cc| DriftApp::new(cc));
    h.run();
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::ZERO);
    let b = ed.doc.add_point(DVec2::new(40.0, 0.0));
    let line = ed.doc.add_line(a, b).unwrap();
    for c in [
        Constraint::Fix {
            point: a,
            at: DVec2::ZERO,
        },
        Constraint::Horizontal { line },
    ] {
        ed.doc.add_constraint(c).unwrap();
    }
    let length = ed
        .doc
        .add_constraint(Constraint::Distance { a, b, value: 40.0 })
        .unwrap();
    assert!(ed.commit("Line"));
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-10.0, -40.0),
        max: DVec2::new(110.0, 40.0),
    });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
    Some((h, b, length))
}

fn now(h: &Harness<'_, DriftApp>) -> f64 {
    h.ctx.input(|i| i.time)
}

/// Whether the last frame asked for another at once.
fn repaints(h: &Harness<'_, DriftApp>) -> bool {
    h.output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|v| v.repaint_delay == Duration::ZERO)
}

#[test]
fn mid_animation_the_solved_position_is_what_picks() {
    let Some((mut h, end, length)) = dimensioned_line() else {
        return;
    };
    assert!(h.state().animate_solves, "on by default");
    h.state_mut()
        .drawing
        .editor
        .set_dimension(length, 70.0)
        .unwrap();
    // The document has it at once.
    assert!((h.state().drawing.editor.doc.point(end).unwrap().x - 70.0).abs() < 1e-6);
    h.step();
    h.step();
    let shown = h
        .state()
        .solve_motion
        .shown_point(end, now(&h))
        .expect("easing in");
    assert!(shown.x > 40.0 && shown.x < 70.0, "part-way: {shown}");
    // The pointer where the end is solved, not where it is drawn: it picks it.
    h.hover_at(screen(&h, DVec2::new(70.0, 0.0)));
    h.step();
    assert!(h.state().solve_motion.animating(), "still easing in");
    assert_eq!(h.state().drawing.editor.hovered, Some(end));
    // Where it is drawn picks nothing.
    let drawn = h.state().solve_motion.shown_point(end, now(&h)).unwrap();
    assert!(drawn.x < 66.0, "{drawn}");
    h.hover_at(screen(&h, DVec2::new(drawn.x - 3.0, 12.0)));
    h.step();
    assert_ne!(h.state().drawing.editor.hovered, Some(end));
    h.run();
    assert!(!h.state().solve_motion.animating());
    assert_eq!(h.state().solve_motion.started, 1);
}

#[test]
fn with_the_preference_off_a_solve_lands_in_one_frame() {
    let Some((mut h, end, length)) = dimensioned_line() else {
        return;
    };
    h.state_mut().animate_solves = false;
    h.run();
    h.state_mut()
        .drawing
        .editor
        .set_dimension(length, 70.0)
        .unwrap();
    for _ in 0..8 {
        h.step();
        assert!(!h.state().solve_motion.animating(), "no frames between");
        assert_eq!(h.state().solve_motion.shown_point(end, now(&h)), None);
    }
    assert_eq!(h.state().solve_motion.started, 0);
    // Undo and redo too.
    h.state_mut().undo();
    h.step();
    assert!(!h.state().solve_motion.animating());
    // On again: the next solve eases in.
    h.state_mut().animate_solves = true;
    h.run();
    h.state_mut().redo();
    h.step();
    assert!(h.state().solve_motion.animating());
    h.run();
}

#[test]
fn the_preference_is_kept_with_the_apps_settings() {
    let Some((mut h, _, _)) = dimensioned_line() else {
        return;
    };
    let mut storage = MemoryStorage::default();
    h.state_mut().animate_solves = false;
    eframe::App::save(h.state_mut(), &mut storage);
    let Some(mut fresh) = harness() else { return };
    assert!(fresh.state().animate_solves, "on with nothing saved");
    fresh.state_mut().load_motion_pref(&storage);
    assert!(!fresh.state().animate_solves);
    // And on again, through the same key.
    h.state_mut().animate_solves = true;
    eframe::App::save(h.state_mut(), &mut storage);
    fresh.state_mut().load_motion_pref(&storage);
    assert!(fresh.state().animate_solves);
    // Not in the drawing.
    let saved = drift_io::save(&h.state().drawing.editor.doc, &Default::default());
    assert!(!saved.contains("animate"));
}

#[test]
fn the_canvas_is_idle_once_an_animation_ends() {
    let Some((mut h, _, length)) = dimensioned_line() else {
        return;
    };
    h.step();
    assert!(!repaints(&h), "idle before");
    h.state_mut()
        .drawing
        .editor
        .set_dimension(length, 70.0)
        .unwrap();
    h.step();
    assert!(h.state().solve_motion.animating());
    assert!(repaints(&h), "each frame of it asks for the next");
    let mut frames = 1;
    while h.state().solve_motion.animating() {
        h.step();
        frames += 1;
        assert!(frames < 30, "it ends");
    }
    assert!(frames >= 5, "several frames: {frames}");
    // (egui answers the last frame's request with the one it ended in: after that, nothing
    // more is asked for.)
    h.step();
    assert!(
        !repaints(&h),
        "idle once it ends: {:?}",
        h.ctx.repaint_causes()
    );
    h.step();
    assert!(!repaints(&h));
}

#[test]
fn a_dragged_end_moves_with_the_pointer_without_easing() {
    let Some((mut h, end, length)) = dimensioned_line() else {
        return;
    };
    // Free the end to drag it: the length dimension goes.
    h.state_mut().drawing.editor.remove_constraint(length);
    h.run();
    let from = screen(&h, DVec2::new(40.0, 0.0));
    let to = screen(&h, DVec2::new(60.0, 0.0));
    h.hover_at(from);
    h.step();
    button(&h, from, true);
    h.step();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.step();
        assert!(!h.state().solve_motion.animating(), "not while dragging");
    }
    button(&h, to, false);
    h.step();
    assert!(!h.state().solve_motion.animating(), "nor as it ends");
    assert!(h.state().drawing.editor.doc.point(end).unwrap().x > 50.0);
    assert_eq!(h.state().solve_motion.started, 0);
}

/// A 40 × 20 rectangle at (0, -30), its width dimensioned, filled: its corners (from the
/// bottom left, anticlockwise), its width and its fill.
fn filled_rectangle(h: &mut Harness<'_, DriftApp>) -> ([EntityId; 4], ConstraintId, FillId) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let p = [(0.0, -30.0), (40.0, -30.0), (40.0, -10.0), (0.0, -10.0)]
        .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
    let mut sides = Vec::new();
    for i in 0..4 {
        sides.push(ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
    }
    for c in [
        Constraint::Fix {
            point: p[0],
            at: DVec2::new(0.0, -30.0),
        },
        Constraint::Horizontal { line: sides[0] },
        Constraint::Horizontal { line: sides[2] },
        Constraint::Vertical { line: sides[1] },
        Constraint::Vertical { line: sides[3] },
        Constraint::Distance {
            a: p[0],
            b: p[3],
            value: 20.0,
        },
    ] {
        ed.doc.add_constraint(c).unwrap();
    }
    let width = ed
        .doc
        .add_constraint(Constraint::Distance {
            a: p[0],
            b: p[1],
            value: 40.0,
        })
        .unwrap();
    assert!(ed.commit("Rectangle"));
    ed.prefs.fill_new_shapes = true;
    ed.doc.begin();
    let fill = ed.fill_new_shape(&sides).expect("filled");
    assert!(ed.commit("Shape"));
    h.run();
    (p, width, fill)
}

#[test]
fn a_hatched_rectangle_stretches_with_its_dimension() {
    let Some((mut h, _, _)) = dimensioned_line() else {
        return;
    };
    let (p, width, fill) = filled_rectangle(&mut h);
    let ed = &mut h.state_mut().drawing.editor;
    let mut hatch = ed.doc.fill(fill).unwrap().clone();
    hatch.kind = drift_core::FillKind::Hatch(drift_core::Hatch::new("ANSI31"));
    ed.update_fill(fill, hatch);
    h.run();
    h.state_mut()
        .drawing
        .editor
        .set_dimension(width, 80.0)
        .unwrap();
    h.step();
    h.step();
    h.step();
    let t = now(&h);
    let app = h.state();
    let frame = app
        .solve_motion
        .frame(&app.drawing.editor.doc, t)
        .expect("easing in");
    let corner = frame.doc.point(p[1]).unwrap();
    assert!(corner.x > 40.0 && corner.x < 80.0, "{corner}");
    // The fill goes with its outline: its far edge where the outline is shown.
    let curves = h.state_mut().drawing.editor.fill_shapes()[&fill]
        .curves
        .clone();
    let map = frame
        .fill_map(&h.state().drawing.editor.doc, &curves)
        .expect("the fill moves");
    assert!(
        map.apply(DVec2::new(80.0, -20.0))
            .distance(DVec2::new(corner.x, -20.0))
            < 1e-6
    );
    assert!(
        map.apply(DVec2::new(0.0, -20.0))
            .distance(DVec2::new(0.0, -20.0))
            < 1e-6
    );
    save_png("drift_ui_514_mid_solve.png", &mut h);
    h.run();
    save_png("drift_ui_514_solved.png", &mut h);
}

/// A picked fill easing into place is lit where it is shown, not where it lands.
#[test]
fn a_picked_fill_is_lit_where_it_is_shown_while_it_eases() {
    let Some((mut h, _, _)) = dimensioned_line() else {
        return;
    };
    let (_, width, fill) = filled_rectangle(&mut h);
    h.state_mut().drawing.editor.selected_fills = [fill].into();
    h.hover_at(screen(&h, DVec2::new(100.0, 30.0)));
    h.run();
    // Where the rectangle will reach, clear of it and its dimension now.
    let at = screen(&h, DVec2::new(70.0, -15.0));
    let before = h.render().expect("render");
    let bare = before.get_pixel(at.x as u32, at.y as u32).0;
    h.state_mut()
        .drawing
        .editor
        .set_dimension(width, 80.0)
        .unwrap();
    // The first frame shows the rectangle where it was.
    h.step();
    assert!(h.state().solve_motion.animating());
    let shown = h.render().expect("render");
    assert_eq!(
        shown.get_pixel(at.x as u32, at.y as u32).0,
        bare,
        "the fill's highlight waits for the fill"
    );
    h.run();
    let landed = h.render().expect("render");
    assert_ne!(
        landed.get_pixel(at.x as u32, at.y as u32).0,
        bare,
        "the fill, lit, where it lands"
    );
}

/// The world x of the canvas's right edge.
fn canvas_right(h: &Harness<'_, DriftApp>) -> f64 {
    let app = h.state();
    let rect = app.drawing.view.canvas_rect;
    ScreenMap {
        rect,
        camera: app.drawing.view.camera,
    }
    .to_world(rect.right_center())
    .x
}

/// A rectangle with no relations from `x0` to `x1` and y -35 to -15, with a solid fill: its
/// corners.
fn loose_filled_rectangle(h: &mut Harness<'_, DriftApp>, x0: f64, x1: f64) -> [EntityId; 4] {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let p = [(x0, -35.0), (x1, -35.0), (x1, -15.0), (x0, -15.0)]
        .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
    let sides: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    assert!(ed.commit("Rectangle"));
    ed.prefs.fill_new_shapes = true;
    ed.doc.begin();
    let fill = ed.fill_new_shape(&sides).expect("filled");
    assert!(ed.commit("Shape"));
    let mut solid = ed.doc.fill(fill).unwrap().clone();
    solid.kind = drift_core::FillKind::Solid;
    ed.update_fill(fill, solid);
    h.run();
    p
}

/// A picked plain fill eased out past the canvas's edge goes with its outline, lit, and one
/// eased back in shows where its outline is shown: never a bare highlight.
#[test]
fn a_fill_easing_across_the_canvas_edge_goes_with_its_outline() {
    let Some((mut h, _, _)) = dimensioned_line() else {
        return;
    };
    let right = canvas_right(&h);
    let p = loose_filled_rectangle(&mut h, right - 25.0, right - 5.0);
    // Picked: lit, over the fill, wherever it is shown.
    let fill = h.state().drawing.editor.doc.fills().next().unwrap().0;
    h.state_mut().drawing.editor.selected_fills = [fill].into();
    h.hover_at(screen(&h, DVec2::new(20.0, 30.0)));
    h.run();
    let at = screen(&h, DVec2::new(right - 15.0, -25.0));
    let pixel = |h: &mut Harness<'_, DriftApp>| {
        let img = h.render().expect("render");
        img.get_pixel(at.x as u32, at.y as u32).0
    };
    let filled = pixel(&mut h);
    // Moved out past the right edge (its left side 5 beyond it).
    let started = h.state().solve_motion.started;
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection = p.into_iter().collect();
        assert_eq!(ed.nudge(DVec2::new(30.0, 0.0)), Some(true));
        ed.selection.clear();
    }
    h.step();
    assert_eq!(h.state().solve_motion.started, started + 1, "it eases");
    assert_eq!(
        pixel(&mut h),
        filled,
        "the fill goes with its outline as it leaves the canvas"
    );
    h.run();
    assert!(!h.state().solve_motion.animating());
    let bare = pixel(&mut h);
    assert_ne!(bare, filled, "the rectangle is out of view");
    // Undone: it slides back in, the fill where its outline is shown.
    h.state_mut().drawing.editor.undo();
    h.step();
    assert_eq!(h.state().solve_motion.started, started + 2, "it eases back");
    assert_eq!(
        pixel(&mut h),
        bare,
        "no fill where it lands before its outline gets there"
    );
    let mut checked = false;
    while h.state().solve_motion.animating() {
        h.step();
        let t = now(&h);
        let app = h.state();
        let Some(frame) = app.solve_motion.frame(&app.drawing.editor.doc, t) else {
            break;
        };
        let left = frame.doc.point(p[0]).unwrap().x;
        if left < right - 16.0 && left > right - 24.0 {
            assert_eq!(
                pixel(&mut h),
                filled,
                "part-way in, the fill is under its outline"
            );
            checked = true;
        }
    }
    assert!(checked, "a frame part-way in");
}

/// A hatch eased in from past the canvas's edge reaches the edge, not cut where the view
/// ended as it lands.
#[test]
fn a_hatch_easing_past_the_canvas_edge_reaches_the_edge() {
    let Some((mut h, _, _)) = dimensioned_line() else {
        return;
    };
    let right = canvas_right(&h);
    h.hover_at(screen(&h, DVec2::new(20.0, 30.0)));
    h.run();
    // Near the canvas's right edge, inside the rectangle wherever it is shown.
    let (a, b) = (
        screen(&h, DVec2::new(right - 8.0, -22.0)),
        screen(&h, DVec2::new(right - 3.0, -28.0)),
    );
    let bare = h.render().expect("render");
    let ink = |h: &mut Harness<'_, DriftApp>| {
        let img = h.render().expect("render");
        let mut n = 0;
        for y in a.y as u32..b.y as u32 {
            for x in a.x as u32..b.x as u32 {
                n += usize::from(img.get_pixel(x, y) != bare.get_pixel(x, y));
            }
        }
        n
    };
    let p = loose_filled_rectangle(&mut h, right - 30.0, right + 10.0);
    {
        let ed = &mut h.state_mut().drawing.editor;
        let fill = ed.doc.fills().next().unwrap().0;
        let mut hatch = ed.doc.fill(fill).unwrap().clone();
        hatch.kind = drift_core::FillKind::Hatch(drift_core::Hatch::new("ANSI31"));
        ed.update_fill(fill, hatch);
    }
    h.run();
    assert!(ink(&mut h) > 0, "hatched to the edge");
    // Moved right: where it lands, the view ends 20 short of the rectangle's right side.
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection = p.into_iter().collect();
        assert_eq!(ed.nudge(DVec2::new(20.0, 0.0)), Some(true));
        ed.selection.clear();
    }
    h.step();
    assert!(h.state().solve_motion.animating());
    assert!(
        ink(&mut h) > 0,
        "the hatch reaches the canvas's edge as it eases"
    );
    h.run();
    assert!(ink(&mut h) > 0);
}
