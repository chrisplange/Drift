//! Solves that ease into place (#514): only the drawing moves, picking uses the solved
//! geometry mid-animation, the preference turns it off and is kept with the app's settings,
//! and the canvas is idle again once an animation ends.

use std::time::Duration;

use drift_core::{Constraint, ConstraintId, DVec2, EntityId};

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

#[test]
fn a_hatched_rectangle_stretches_with_its_dimension() {
    let Some((mut h, _, _)) = dimensioned_line() else {
        return;
    };
    // A 40 × 20 rectangle at (0, -30), its width dimensioned, hatched.
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
