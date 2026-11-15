//! Which relations show their symbols (#337): View → Relations is Off, Picked only or All,
//! Alt+R steps through them, and the choice is saved.

use drift_core::{Constraint, ConstraintId, EntityId};

use super::*;

/// Two parallel lines and, below them, a third that is only horizontal; in view.
fn drawing(h: &mut Harness<'_, DriftApp>) -> ([EntityId; 3], ConstraintId, ConstraintId) {
    let v = DVec2::new;
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let mut line = |a: DVec2, b: DVec2| {
        let (a, b) = (ed.doc.add_point(a), ed.doc.add_point(b));
        ed.doc.add_line(a, b).unwrap()
    };
    let first = line(v(0.0, 40.0), v(60.0, 50.0));
    let second = line(v(0.0, 20.0), v(60.0, 30.0));
    let third = line(v(0.0, 0.0), v(60.0, 0.0));
    let parallel = ed
        .doc
        .add_constraint(Constraint::Parallel {
            a: first,
            b: second,
        })
        .unwrap();
    let horizontal = ed
        .doc
        .add_constraint(Constraint::Horizontal { line: third })
        .unwrap();
    ed.commit("Drawing");
    ed.selection.clear();
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: v(-10.0, -10.0),
        max: v(70.0, 60.0),
    });
    h.run();
    h.run();
    ([first, second, third], parallel, horizontal)
}

/// The relations whose symbols the canvas lays out, one entry per symbol.
fn shown(h: &Harness<'_, DriftApp>) -> Vec<ConstraintId> {
    h.state()
        .relation_glyphs(&screen_map(h))
        .iter()
        .map(|g| g.id)
        .collect()
}

fn status(h: &Harness<'_, DriftApp>) -> String {
    h.state()
        .status
        .as_ref()
        .map(|s| s.text.clone())
        .unwrap_or_default()
}

/// Press and release on the canvas at a screen position.
fn click_at(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    button(h, at, true);
    h.run();
    button(h, at, false);
    h.run();
}

#[test]
fn picked_only_shows_the_relations_of_what_is_picked_on_both_of_a_pair() {
    let Some(mut h) = harness() else { return };
    let ([first, second, third], parallel, horizontal) = drawing(&mut h);
    // All, as a new start is: a symbol on each line of the pair, and the third line's.
    assert_eq!(h.state().show_relations, ShowRelations::All);
    assert_eq!(shown(&h), [parallel, parallel, horizontal]);

    // View → Relations → Picked only.
    // (The first "View" is the menu; the toolbar has a View tool.)
    h.get_all_by_label("View").next().unwrap().click();
    h.run();
    h.get_by_label("Relations ⏵").click();
    h.run();
    for mode in ShowRelations::ALL {
        // ("All" is also one of the select tool's filters.)
        assert!(
            h.query_all_by_label(mode.label()).next().is_some(),
            "{mode:?}"
        );
    }
    h.get_by_label("Picked only").click();
    h.run();
    h.run();
    assert_eq!(h.state().show_relations, ShowRelations::Picked);
    assert_eq!(status(&h), "Relations shown for what is picked only");
    assert!(h.query_by_label("Relations off · Show").is_none());

    // Nothing picked: no symbols.
    assert!(shown(&h).is_empty());

    // One line of the pair picked: the pair's relation on both lines, where each is with
    // every relation shown, and not the third line's.
    let all = crate::annotations::layout_glyphs(&h.state().drawing.editor.doc, &screen_map(&h));
    let of = |id: ConstraintId| -> Vec<egui::Rect> {
        all.iter().filter(|g| g.id == id).map(|g| g.rect).collect()
    };
    let (pair, lone) = (of(parallel), of(horizontal)[0]);
    assert_eq!(pair.len(), 2);
    assert!(
        (pair[0].center().y - pair[1].center().y).abs() > 40.0,
        "one on each line: {pair:?}"
    );
    for picked in [first, second] {
        h.state_mut().drawing.editor.selection = [picked].into_iter().collect();
        h.run();
        let glyphs = h.state().relation_glyphs(&screen_map(&h));
        assert_eq!(
            glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
            [parallel, parallel]
        );
        assert_eq!(glyphs.iter().map(|g| g.rect).collect::<Vec<_>>(), pair);
    }
    save_png("drift_ui_relations_picked_only.png", &mut h);
    // The other line of the pair's symbol is on the canvas: a click picks the relation,
    // which then shows whatever else is picked.
    click_at(&mut h, pair[0].center());
    assert_eq!(h.state().drawing.selected_constraint, Some(parallel));
    assert!(shown(&h).contains(&parallel));

    // The third line picked: its relation alone.
    h.state_mut().drawing.selected_constraint = None;
    h.state_mut().drawing.editor.selection = [third].into_iter().collect();
    h.run();
    assert_eq!(shown(&h), [horizontal]);
    // With the pair picked instead, the third line's symbol isn't on the canvas: a click
    // where it would be picks no relation.
    h.state_mut().drawing.editor.selection = [first].into_iter().collect();
    h.run();
    click_at(&mut h, lone.center());
    assert_eq!(h.state().drawing.selected_constraint, None);

    // Alt+R steps on: all, off, picked only.
    for (mode, says) in [
        (ShowRelations::All, "Relations shown"),
        (ShowRelations::Off, "Relations hidden (Alt+R shows them)"),
        (
            ShowRelations::Picked,
            "Relations shown for what is picked only",
        ),
    ] {
        h.key_press_modifiers(EguiModifiers::ALT, Key::R);
        h.run();
        h.run();
        assert_eq!(h.state().show_relations, mode);
        assert_eq!(status(&h), says);
    }
    h.state_mut().show_relations = ShowRelations::Off;
    h.run();
    assert!(shown(&h).is_empty());
    assert!(h.query_by_label("Relations off · Show").is_some());
}

#[test]
fn which_relations_show_is_saved_and_an_older_on_off_setting_still_reads() {
    let Some(mut h) = harness() else { return };
    h.state_mut().show_relations = ShowRelations::Picked;
    h.run();
    let prefs = h.state().view_prefs();
    assert!(prefs.split(';').any(|p| p == "rels=picked"), "{prefs}");
    let mut storage = MemoryStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let Some(mut fresh) = harness() else { return };
    assert_eq!(fresh.state().show_relations, ShowRelations::All);
    fresh.state_mut().restore_session(&storage);
    fresh.run();
    assert_eq!(fresh.state().show_relations, ShowRelations::Picked);
    // The on/off switch an older build saved: on is All, off is Off.
    for (saved, mode) in [
        ("rels=0", ShowRelations::Off),
        ("rels=1", ShowRelations::All),
        ("rels=off", ShowRelations::Off),
        ("rels=picked", ShowRelations::Picked),
        ("rels=all", ShowRelations::All),
    ] {
        fresh.state_mut().apply_view_prefs(saved);
        assert_eq!(fresh.state().show_relations, mode, "{saved}");
    }
    // Something else leaves it as it is.
    fresh.state_mut().apply_view_prefs("rels=some");
    assert_eq!(fresh.state().show_relations, ShowRelations::All);
}
