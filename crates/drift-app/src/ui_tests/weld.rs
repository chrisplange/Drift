//! Weld (1) and Weld at centre (Shift+1) on picked points.

use drift_core::{DVec2, EntityId};

use super::*;

/// Two lines apart, picked at their facing ends: (10,0) and (12,2), the latter active.
fn two_ends(h: &mut Harness<'_, DriftApp>) -> [EntityId; 2] {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
    let b = ed.doc.add_point(DVec2::new(10.0, 0.0));
    let c = ed.doc.add_point(DVec2::new(12.0, 2.0));
    let d = ed.doc.add_point(DVec2::new(20.0, 10.0));
    ed.doc.add_line(a, b).unwrap();
    ed.doc.add_line(c, d).unwrap();
    assert!(ed.commit("Lines"));
    ed.selection = [b, c].into();
    ed.set_active(Some(c));
    [b, c]
}

fn points(h: &Harness<'_, DriftApp>) -> usize {
    let doc = &h.state().drawing.editor.doc;
    doc.iter().filter(|(_, e)| e.kind.is_point()).count()
}

#[test]
fn one_welds_the_picked_points_to_the_active_one() {
    let Some(mut h) = harness() else { return };
    let [_, c] = two_ends(&mut h);
    h.key_press(Key::Num1);
    h.run();
    assert_eq!(points(&h), 3);
    let at = h.state().drawing.editor.doc.point(c).unwrap();
    assert_eq!(at, DVec2::new(12.0, 2.0));
}

#[test]
fn shift_one_welds_at_the_centre_though_it_types_an_exclamation_mark() {
    let Some(mut h) = harness() else { return };
    let [_, c] = two_ends(&mut h);
    // What a US keyboard sends for Shift+1: the "!" key, at the 1 key's place.
    h.event_modifiers(
        Event::Key {
            key: Key::Exclamationmark,
            physical_key: Some(Key::Num1),
            pressed: true,
            repeat: false,
            modifiers: EguiModifiers::SHIFT,
        },
        EguiModifiers::SHIFT,
    );
    h.run();
    assert_eq!(points(&h), 3);
    let at = h.state().drawing.editor.doc.point(c).unwrap();
    assert!(at.distance(DVec2::new(11.0, 1.0)) < 1e-9);
}

#[test]
fn typing_a_one_into_a_tools_field_does_not_weld() {
    let Some(mut h) = harness() else { return };
    two_ends(&mut h);
    // Fillet takes a typed radius while it waits for corners.
    h.key_press(Key::F);
    h.run();
    assert!(!h.state().tool.fields().is_empty());
    h.event(Event::Text("1".into()));
    h.key_press(Key::Num1);
    h.run();
    assert_eq!(points(&h), 4, "nothing welded");
}

/// #428: 1 on points on different layers holds them together without merging, and the
/// status bar says so.
#[test]
fn one_across_layers_holds_the_points_and_says_so() {
    let Some(mut h) = harness() else { return };
    let [b, c] = two_ends(&mut h);
    let ed = &mut h.state_mut().drawing.editor;
    let other = ed.add_layer("Other");
    let second = ed.doc.users_of(c).next().unwrap();
    ed.selection = [second].into();
    ed.move_selection_to_layer(other);
    ed.selection = [b, c].into();
    ed.set_active(Some(c));
    h.key_press(Key::Num1);
    h.run();
    assert_eq!(points(&h), 4, "nothing merged");
    let doc = &h.state().drawing.editor.doc;
    assert!(doc.point(b).unwrap().distance(DVec2::new(12.0, 2.0)) < 1e-9);
    let status = h.state().status.as_ref().map(|s| s.text.clone());
    assert_eq!(
        status.as_deref(),
        Some("On different layers: held together, not merged")
    );
}
