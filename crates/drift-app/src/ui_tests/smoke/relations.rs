//! Relations between picked elements: the one picked last (the active one) stays, and the
//! others conform to it (#333). The status bar picks the conflicting relations, and the
//! Relations list marks redundant ones (#305).

use drift_core::{Constraint, ConstraintId};

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

/// #305: two points held 30 apart, a third 10 from each. The status bar's red "N
/// conflicting" pill picks the conflicting relations, as the amber overlaps pill picks its
/// curves: the picked relations are the conflicting set, and their geometry is selected.
#[test]
fn smoke_relations_conflicts_pill_picks_the_conflicting_relations() {
    let Some(mut h) = harness() else { return };
    let mut doc = drift_core::Document::new();
    let a = doc.add_point(DVec2::new(0.0, 0.0));
    let f = doc.add_point(DVec2::new(30.0, 0.0));
    let b = doc.add_point(DVec2::new(12.0, 3.0));
    for c in [
        Constraint::Fix {
            point: a,
            at: DVec2::new(0.0, 0.0),
        },
        Constraint::Fix {
            point: f,
            at: DVec2::new(30.0, 0.0),
        },
        Constraint::Distance { a, b, value: 10.0 },
        Constraint::Distance {
            a: b,
            b: f,
            value: 10.0,
        },
    ] {
        doc.add_constraint(c).unwrap();
    }
    h.state_mut().drawing.editor = drift_edit::editor::Editor::new(doc);
    view(&mut h, DVec2::new(-5.0, -10.0), DVec2::new(35.0, 10.0));
    let conflicts: std::collections::BTreeSet<ConstraintId> = h
        .state()
        .drawing
        .editor
        .analysis()
        .conflicts
        .iter()
        .copied()
        .collect();
    assert!(!conflicts.is_empty(), "the sketch conflicts");
    assert!(h.state().picked_relations().is_empty());
    h.get_by_label(&format!("{} conflicting", conflicts.len()))
        .click();
    h.run();
    h.run();
    let picked: std::collections::BTreeSet<ConstraintId> =
        h.state().picked_relations().iter().copied().collect();
    assert_eq!(
        picked, conflicts,
        "the pill picks the conflicting relations"
    );
    // Their geometry is selected, so the Relations list shows their rows.
    let ed = &h.state().drawing.editor;
    let geometry: std::collections::BTreeSet<EntityId> = conflicts
        .iter()
        .filter_map(|id| ed.doc.constraint(*id))
        .flat_map(|c| c.entities())
        .collect();
    assert_eq!(ed.selection, geometry);
    assert!(status(&h).starts_with("Picked"), "{}", status(&h));
    save_png("drift_smoke_relations_conflicts_picked.png", &mut h);
    // Any other pick ends it.
    h.state_mut().drawing.editor.selection.clear();
    h.run();
    assert!(h.state().picked_relations().is_empty());
}

/// #305: two level lines, and the first one's ends held level again, which its Horizontal
/// implies already. The redundant relation's row in the Relations list carries a marker,
/// whose tooltip says so.
#[test]
fn smoke_relations_a_redundant_relation_is_marked_in_its_row() {
    let Some(mut h) = harness() else { return };
    let mut doc = drift_core::Document::new();
    let p = [
        DVec2::new(0.0, 0.0),
        DVec2::new(10.0, 0.0),
        DVec2::new(0.0, 5.0),
        DVec2::new(7.0, 5.0),
    ]
    .map(|at| doc.add_point(at));
    let la = doc.add_line(p[0], p[1]).unwrap();
    let lb = doc.add_line(p[2], p[3]).unwrap();
    for c in [
        Constraint::Horizontal { line: la },
        Constraint::Horizontal { line: lb },
        Constraint::HorizontalPoints { a: p[1], b: p[0] },
    ] {
        doc.add_constraint(c).unwrap();
    }
    h.state_mut().drawing.editor = drift_edit::editor::Editor::new(doc);
    view(&mut h, DVec2::new(-5.0, -5.0), DVec2::new(15.0, 10.0));
    let redundant = h.state().drawing.editor.analysis().redundant.clone();
    assert_eq!(
        redundant.len(),
        1,
        "one relation is redundant: {redundant:?}"
    );
    let ed = &h.state().drawing.editor;
    let name = crate::constraint_label(&ed.doc, ed.doc.constraint(redundant[0]).unwrap());
    // The whole sketch's list, unfolded.
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.get_by_label("Show all").click();
    h.run();
    h.run();
    let markers: Vec<egui::Rect> = h
        .query_all_by_label("Redundant")
        .map(|n| n.rect())
        .collect();
    assert_eq!(markers.len(), 1, "one marker");
    let marker = markers[0];
    // On the redundant relation's row.
    assert!(
        h.query_all_by_label(name)
            .any(|n| (n.rect().center().y - marker.center().y).abs() < 6.0),
        "the marker is on the {name} row"
    );
    assert!(
        h.query_by_label_contains("already implied by the others")
            .is_none()
    );
    h.hover_at(marker.center());
    for _ in 0..30 {
        h.step();
    }
    assert!(
        h.query_by_label_contains("already implied by the others")
            .is_some(),
        "the marker's tooltip"
    );
    save_png("drift_smoke_relations_redundant_marker.png", &mut h);
}
