//! Offsets as objects (#247): a circle, an ellipse, a pen curve and a mixed chain each offset
//! with the Offset tool, showing one distance dimension; a value typed into it, a drag of a
//! piece and a change to the source all move every piece; saved and opened it still
//! follows; Detach leaves plain curves. (The rectangle is in `dimensions.rs`.)

use drift_core::{EntityKind, OffsetId, Shape};

use super::*;

/// The one offset in the drawing.
fn the_offset(h: &Harness<'_, DriftApp>) -> OffsetId {
    let mut all = h.state().drawing.editor.doc.offsets();
    let (x, _) = all.next().expect("an offset");
    assert!(all.next().is_none(), "one offset");
    x
}

/// A point mid-way along a piece.
fn middle(shape: &Shape) -> DVec2 {
    match *shape {
        Shape::Line { a, b } => (a + b) * 0.5,
        Shape::Circle { center, radius } => center + DVec2::new(radius, 0.0),
        Shape::Arc { start, sweep, .. } | Shape::Ellipse { start, sweep, .. } => {
            shape.point_at(start + sweep * 0.5)
        }
        Shape::Bezier { .. } => shape.point_at(0.5),
        Shape::Point(p) => p,
    }
}

/// How far each piece of the offset sits from its source, from the piece's middle.
fn gaps(h: &Harness<'_, DriftApp>, x: OffsetId) -> Vec<f64> {
    let doc = &h.state().drawing.editor.doc;
    let o = doc.offset(x).expect("the offset");
    o.made
        .curves
        .values()
        .filter_map(|c| doc.shape(*c))
        .map(|s| {
            let m = middle(&s);
            o.source
                .iter()
                .filter_map(|c| doc.shape(*c))
                .map(|s| drift_geom::distance(&s, m))
                .fold(f64::INFINITY, f64::min)
        })
        .collect()
}

fn all_at(gaps: &[f64], d: f64, tol: f64) -> bool {
    !gaps.is_empty() && gaps.iter().all(|g| (g - d).abs() < tol)
}

/// The dimensions laid out on the canvas.
fn shown(h: &Harness<'_, DriftApp>) -> Vec<crate::annotations::DimLayout> {
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        h.state().drawing.view.canvas_rect,
    );
    h.state().layout_dimensions(&painter, &screen_map(h))
}

/// O, a click on the source at `on`, the pointer at `side`, the distance typed.
fn offset_with_tool(h: &mut Harness<'_, DriftApp>, on: DVec2, side: DVec2, distance: &str) {
    palette(h, "Offset");
    assert_eq!(h.state().tool.kind(), ToolKind::Offset);
    click_world(h, on);
    h.hover_at(screen(h, side));
    h.run();
    type_text(h, distance);
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(
        h.state().drawing.editor.doc.offsets().count(),
        1,
        "the offset placed: {:?} / {}",
        h.state().drawing.editor.out.notice,
        status(h)
    );
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::S);
    h.run();
}

/// Double-click the one dimension shown and type `value` into it.
fn retype_dimension(h: &mut Harness<'_, DriftApp>, value: &str) {
    let dims = shown(h);
    assert_eq!(dims.len(), 1, "one offset dimension on the canvas");
    double_click(h, dims[0].text_rect.center());
    assert!(
        h.state().drawing.property_edit.is_some(),
        "editing the offset's distance"
    );
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    type_text(h, value);
    h.key_press(Key::Enter);
    h.run();
}

/// A drag of a piece from `from` to `to` (world), with Alt when `free`.
fn drag_piece(h: &mut Harness<'_, DriftApp>, from: DVec2, to: DVec2, free: bool) {
    h.state_mut().drawing.editor.selection.clear();
    h.run();
    let (a, b) = (screen(h, from), screen(h, to));
    if free {
        h.event(Event::ModifiersChanged(EguiModifiers::ALT));
    }
    drag_steps(h, a, b, 6, None);
    if free {
        h.event(Event::ModifiersChanged(EguiModifiers::NONE));
    }
    h.run();
}

/// The common run for one source: one dimension shown; typed 4, every piece at 4; undo; a
/// free drag (from, to, the distance expected), every piece at it; undo; the source moved by `nudge`
/// (its point `moved`), the offset follows; undo.
fn exercise(
    h: &mut Harness<'_, DriftApp>,
    x: OffsetId,
    distance: f64,
    drag: (DVec2, DVec2, f64),
    moved: drift_core::EntityId,
    nudge: DVec2,
) {
    let (drag_from, drag_to, dragged) = drag;
    let tol = 5e-3;
    assert!(all_at(&gaps(h, x), distance, tol), "{:?}", gaps(h, x));
    retype_dimension(h, "4");
    assert!(all_at(&gaps(h, x), 4.0, tol), "typed: {:?}", gaps(h, x));
    undo(h);
    assert!(
        all_at(&gaps(h, x), distance, tol),
        "undone: {:?}",
        gaps(h, x)
    );
    drag_piece(h, drag_from, drag_to, true);
    assert!(
        all_at(&gaps(h, x), dragged, tol),
        "dragged: {:?}",
        gaps(h, x)
    );
    undo(h);
    assert!(all_at(&gaps(h, x), distance, tol));
    // The source moves: the offset follows.
    let ed = &mut h.state_mut().drawing.editor;
    let was = ed.doc.point(moved).unwrap();
    ed.doc.begin();
    ed.doc.set_point(moved, was + nudge).unwrap();
    ed.commit("Move");
    h.run();
    assert!(
        all_at(&gaps(h, x), distance, tol),
        "followed: {:?}",
        gaps(h, x)
    );
    undo(h);
    assert_eq!(h.state().drawing.editor.doc.point(moved).unwrap(), was);
    assert!(all_at(&gaps(h, x), distance, tol));
}

#[test]
fn smoke_offsets_a_circle_has_one_distance_and_follows() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-30.0, -30.0), DVec2::new(30.0, 30.0));
    let circle = circle(&mut h, DVec2::ZERO, 10.0);
    let EntityKind::Circle { center, .. } = h.state().drawing.editor.doc.get(circle).unwrap().kind
    else {
        unreachable!()
    };
    offset_with_tool(&mut h, DVec2::new(10.0, 0.0), DVec2::new(14.0, 0.0), "2");
    let x = the_offset(&h);
    assert_eq!(
        h.state().drawing.editor.offset_curves(x).len(),
        1,
        "one circle"
    );
    exercise(
        &mut h,
        x,
        2.0,
        (DVec2::new(12.0, 0.0), DVec2::new(14.5, 0.0), 4.5),
        center,
        DVec2::new(3.0, 2.0),
    );
    save_png("drift_smoke_offsets_circle.png", &mut h);
}

#[test]
fn smoke_offsets_an_ellipse_has_one_distance_and_follows() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-40.0, -30.0), DVec2::new(40.0, 30.0));
    let (major, minor) = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let c = ed.doc.add_point(DVec2::ZERO);
        let major = ed.doc.add_point(DVec2::new(20.0, 0.0));
        let minor = ed.doc.add_point(DVec2::new(0.0, 10.0));
        ed.doc.add_ellipse(c, major, minor).unwrap();
        ed.commit("Ellipse");
        (major, minor)
    };
    h.run();
    offset_with_tool(&mut h, DVec2::new(20.0, 0.0), DVec2::new(25.0, 0.0), "3");
    let x = the_offset(&h);
    assert!(
        h.state().drawing.editor.offset_curves(x).len() >= 4,
        "a run of pen curves"
    );
    exercise(
        &mut h,
        x,
        3.0,
        (DVec2::new(0.0, 13.0), DVec2::new(0.0, 14.5), 4.5),
        minor,
        DVec2::new(0.0, 2.0),
    );
    let _ = major;
}

#[test]
fn smoke_offsets_a_pen_curve_has_one_distance_and_follows() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(60.0, 40.0));
    let end = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let c1 = ed.doc.add_point(DVec2::new(0.0, 20.0));
        let c2 = ed.doc.add_point(DVec2::new(40.0, 20.0));
        let b = ed.doc.add_point(DVec2::new(40.0, 0.0));
        ed.doc.add_bezier(a, c1, c2, b).unwrap();
        ed.commit("Pen curve");
        b
    };
    h.run();
    // Outside the arch (above its top at y = 15).
    offset_with_tool(&mut h, DVec2::new(20.0, 15.0), DVec2::new(20.0, 20.0), "2");
    let x = the_offset(&h);
    exercise(
        &mut h,
        x,
        2.0,
        (DVec2::new(20.0, 17.0), DVec2::new(20.0, 19.5), 4.5),
        end,
        DVec2::new(5.0, 0.0),
    );
}

#[test]
fn smoke_offsets_a_mixed_chain_follows_saves_and_detaches() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -30.0), DVec2::new(80.0, 40.0));
    // A line, a half-circle arc and a line: a slot's outline, open.
    let (far, sides) = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let p0 = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let p1 = ed.doc.add_point(DVec2::new(40.0, 0.0));
        let c = ed.doc.add_point(DVec2::new(40.0, 10.0));
        let p2 = ed.doc.add_point(DVec2::new(40.0, 20.0));
        let p3 = ed.doc.add_point(DVec2::new(0.0, 20.0));
        let l1 = ed.doc.add_line(p0, p1).unwrap();
        let arc = ed.doc.add_arc(c, p2, p1).unwrap();
        let l2 = ed.doc.add_line(p2, p3).unwrap();
        ed.commit("Slot");
        (p3, [l1, arc, l2])
    };
    h.run();
    offset_with_tool(&mut h, DVec2::new(20.0, 0.0), DVec2::new(20.0, -4.0), "2");
    let x = the_offset(&h);
    assert_eq!(
        h.state().drawing.editor.offset_curves(x).len(),
        3,
        "line, arc, line"
    );
    exercise(
        &mut h,
        x,
        2.0,
        (DVec2::new(8.0, -2.0), DVec2::new(8.0, -4.5), 4.5),
        far,
        DVec2::new(-5.0, 0.0),
    );
    // Saved and opened again: the same offset, still following.
    let before = h.state().drawing.editor.doc.clone();
    let dir = scratch_dir("smoke-offsets");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Offsets.drift");
    h.state_mut().drawing.file.path = Some(path.clone());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::S);
    h.run_steps(3);
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::N);
    h.run_steps(2);
    assert_eq!(h.state().drawing.editor.doc.offsets().count(), 0);
    h.state().files.open_path(&path);
    h.run_steps(3);
    assert!(
        h.state().drawing.editor.out.notice.is_none(),
        "nothing repaired: {:?}",
        h.state().drawing.editor.out.notice
    );
    assert!(
        h.state()
            .drawing
            .editor
            .doc
            .same_sketch(&before.sketch_copy()),
        "the offset as it was"
    );
    let x = the_offset(&h);
    let ed = &mut h.state_mut().drawing.editor;
    let was = ed.doc.point(far).unwrap();
    ed.doc.begin();
    ed.doc.set_point(far, was + DVec2::new(-5.0, 0.0)).unwrap();
    ed.commit("Move");
    h.run_steps(2);
    assert!(
        all_at(&gaps(&h, x), 2.0, 5e-3),
        "still follows: {:?}",
        gaps(&h, x)
    );
    // Detach: plain curves, no offset, the Inspector field gone. (A click on a piece, away
    // from the dimension's label at x = 20.)
    click_world(&mut h, DVec2::new(8.0, -2.0));
    assert_eq!(
        h.state().drawing.editor.selected_offsets().len(),
        1,
        "a piece picks it"
    );
    let pieces = h.state().drawing.editor.offset_curves(x);
    palette(&mut h, "Detach offset");
    h.run_steps(2);
    assert_eq!(h.state().drawing.editor.doc.offsets().count(), 0);
    assert!(
        pieces
            .iter()
            .all(|c| h.state().drawing.editor.doc.contains(*c)),
        "the curves stay"
    );
    assert!(h.state().drawing.editor.doc.generated_entities().is_empty());
    assert_eq!(h.state().drawing.editor.selection.len(), 3, "picked after");
    let _ = sides;
    let _ = std::fs::remove_dir_all(&dir);
}
