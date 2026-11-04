//! Relations between picked elements: the one picked last (the active one) stays, and the
//! others conform to it (#333).

use super::*;

/// Two free slanted lines apart: (line, line).
fn two_lines(h: &mut Harness<'_, DriftApp>) -> (EntityId, EntityId) {
    view(h, DVec2::new(-5.0, -5.0), DVec2::new(30.0, 15.0));
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let p = [
        DVec2::new(0.0, 0.0),
        DVec2::new(10.0, 1.0),
        DVec2::new(20.0, 0.0),
        DVec2::new(23.0, 10.0),
    ]
    .map(|at| ed.doc.add_point(at));
    let a = ed.doc.add_line(p[0], p[1]).unwrap();
    let b = ed.doc.add_line(p[2], p[3]).unwrap();
    ed.commit("Lines");
    h.run();
    (a, b)
}

/// A third of the way along `line` (its middle would pick its midpoint).
fn on(h: &Harness<'_, DriftApp>, line: EntityId) -> Pos2 {
    let (a, b) = ends(h, line);
    screen(h, at(h, a).lerp(at(h, b), 1.0 / 3.0))
}

/// Click one line, Shift+click the other, then Q and P (the pie's Perpendicular key): the
/// line picked last doesn't move, the other turns to it.
#[test]
fn smoke_relations_keep_the_one_picked_last() {
    let Some(mut h) = harness() else { return };
    let (a, b) = two_lines(&mut h);
    let pa = on(&h, a);
    let pb = on(&h, b);
    click_with(&mut h, pa, EguiModifiers::NONE);
    click_with(&mut h, pb, EguiModifiers::SHIFT);
    assert_eq!(h.state().drawing.editor.active(), Some(b));
    let (b0, b1) = ends(&h, b);
    let kept = (at(&h, b0), at(&h, b1));
    let (a0, a1) = ends(&h, a);
    let turned = (at(&h, a0), at(&h, a1));
    h.key_press(Key::Q);
    h.run();
    h.key_press(Key::P);
    h.run();
    assert_eq!((at(&h, b0), at(&h, b1)), kept, "the active line stays");
    assert_ne!((at(&h, a0), at(&h, a1)), turned, "the other turns");
    let (da, db) = (at(&h, a1) - at(&h, a0), at(&h, b1) - at(&h, b0));
    assert!(da.normalize().dot(db.normalize()).abs() < 1e-9);
    // One undo step puts it back.
    undo(&mut h);
    assert_eq!((at(&h, a0), at(&h, a1)), turned);
}

/// The Inspector says which of what is picked stays: the one picked last, or none after a
/// box pick; and the select tool's hint says Shift picks the active one.
#[test]
fn smoke_relations_say_which_one_stays() {
    let Some(mut h) = harness() else { return };
    let (a, b) = two_lines(&mut h);
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    assert!(
        h.state()
            .tool
            .hint()
            .contains("Shift adds (the last picked is active)")
    );
    let pa = on(&h, a);
    let pb = on(&h, b);
    click_with(&mut h, pa, EguiModifiers::NONE);
    click_with(&mut h, pb, EguiModifiers::SHIFT);
    h.run();
    let kept = "The one picked last (drawn heavier) stays; the others conform to it";
    assert!(
        h.query_by_label(kept).is_some(),
        "the hint names the active one"
    );
    // None active (as after a box pick): the hint says they all move.
    h.state_mut().drawing.editor.set_active(None);
    h.run();
    let all = "No active element: they all move. Shift-click one to keep it";
    assert!(
        h.query_by_label(all).is_some(),
        "the hint says they all move"
    );
    assert!(h.query_by_label(kept).is_none());
}
