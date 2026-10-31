//! Picking: what the select tool takes under the pointer where things overlap, for hover,
//! click, double-click and drag start alike: text over a line, a block copy with text in it,
//! overlapping fills, a wall's poché and its door, a locked layer, hidden handles, and each
//! pick filter.

use std::collections::BTreeSet;

use drift_core::{InstanceId, LayerId, SketchText, TextId};

use drift_edit::editor::{PickFilter, PickKind};

use super::*;

/// What the pointer lights up: at most one of these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Lit {
    entity: Option<EntityId>,
    midpoint: Option<EntityId>,
    text: Option<TextId>,
    copy: Option<InstanceId>,
    fill: Option<FillId>,
}

/// What is picked.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Picked {
    selection: BTreeSet<EntityId>,
    midpoints: BTreeSet<EntityId>,
    texts: BTreeSet<TextId>,
    copies: BTreeSet<InstanceId>,
    fills: BTreeSet<FillId>,
    region: bool,
}

/// Move the pointer to `p` and say what it lights up.
fn hover(h: &mut Harness<'_, DriftApp>, p: DVec2) -> Lit {
    h.hover_at(screen(h, p));
    h.run();
    let ed = &h.state().drawing.editor;
    Lit {
        entity: ed.hovered,
        midpoint: ed.hovered_midpoint,
        text: ed.hovered_text,
        copy: ed.hovered_instance,
        fill: ed.hovered_fill,
    }
}

fn picked(h: &Harness<'_, DriftApp>) -> Picked {
    let ed = &h.state().drawing.editor;
    Picked {
        selection: ed.selection.clone(),
        midpoints: ed.selected_midpoints.clone(),
        texts: ed.selected_texts.clone(),
        copies: ed.selected_instances.clone(),
        fills: ed.selected_fills.clone(),
        region: ed.selected_region.is_some(),
    }
}

/// Pick nothing.
fn clear(h: &mut Harness<'_, DriftApp>) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.selection.clear();
    ed.selected_midpoints.clear();
    ed.selected_texts.clear();
    ed.selected_instances.clear();
    ed.selected_fills.clear();
    ed.selected_region = None;
    h.run();
}

/// With nothing picked, click at `p` and say what it picked.
fn click(h: &mut Harness<'_, DriftApp>, p: DVec2) -> Picked {
    clear(h);
    click_world(h, p);
    picked(h)
}

/// With nothing picked, double-click at `p`.
fn double_click_at(h: &mut Harness<'_, DriftApp>, p: DVec2) {
    clear(h);
    let pos = screen(h, p);
    double_click(h, pos);
}

/// A line from `a` to `b` on the current layer. Returns it.
fn line(h: &mut Harness<'_, DriftApp>, a: DVec2, b: DVec2) -> EntityId {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let (a, b) = (ed.doc.add_point(a), ed.doc.add_point(b));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    h.run();
    line
}

/// A sketch text, its capitals `height` tall, its baseline starting at `at`.
fn text(h: &mut Harness<'_, DriftApp>, words: &str, at: DVec2, height: f64) -> TextId {
    let id = h
        .state_mut()
        .drawing
        .editor
        .add_text(SketchText::new(words, at, height));
    clear(h);
    id
}

fn text_anchor(h: &Harness<'_, DriftApp>, t: TextId) -> DVec2 {
    h.state().drawing.editor.doc.text(t).unwrap().at
}

fn near(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 0.5
}

/// Text over a line: on the line, the line; elsewhere in the text's box, the text.
#[test]
fn smoke_picking_text_over_a_line() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(160.0, 110.0));
    let l = line(&mut h, DVec2::new(0.0, 75.0), DVec2::new(100.0, 75.0));
    let t = text(&mut h, "PICK", DVec2::new(10.0, 70.0), 20.0);
    let (on_line, on_text) = (DVec2::new(30.0, 75.0), DVec2::new(20.0, 85.0));
    let on = |id| Lit {
        entity: Some(id),
        ..Lit::default()
    };
    assert_eq!(hover(&mut h, on_line), on(l));
    assert_eq!(
        hover(&mut h, on_text),
        Lit {
            text: Some(t),
            ..Lit::default()
        }
    );
    assert_eq!(
        click(&mut h, on_line),
        Picked {
            selection: [l].into(),
            ..Picked::default()
        }
    );
    assert_eq!(
        click(&mut h, on_text),
        Picked {
            texts: [t].into(),
            ..Picked::default()
        }
    );
    // A drag from the line moves the line and leaves the text; one from the text, the text.
    let (a, _) = ends(&h, l);
    let (a0, t0) = (at(&h, a), text_anchor(&h, t));
    clear(&mut h);
    drag(&mut h, on_line, on_line + DVec2::new(0.0, -10.0), 4);
    assert!(
        near(at(&h, a), a0 + DVec2::new(0.0, -10.0)),
        "the line moved"
    );
    assert_eq!(text_anchor(&h, t), t0, "the text stayed");
    undo(&mut h);
    clear(&mut h);
    drag(&mut h, on_text, on_text + DVec2::new(5.0, 0.0), 4);
    assert!(near(text_anchor(&h, t), t0 + DVec2::new(5.0, 0.0)), "moved");
    assert_eq!(at(&h, a), a0, "the line stayed");
    undo(&mut h);
    // A double-click on the line edits nothing; on the text, types into it.
    double_click_at(&mut h, on_line);
    assert_eq!(h.state().drawing.editor.editing_text, None);
    double_click_at(&mut h, on_text);
    assert_eq!(h.state().drawing.editor.editing_text, Some(t));
}

/// A copy of a block, on its curves or inside it, with a text inside it taking precedence.
#[test]
fn smoke_picking_a_block_copy_and_text_in_it() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(80.0, -10.0), DVec2::new(160.0, 40.0));
    let (sides, _) = rectangle(
        &mut h,
        DVec2::new(100.0, 0.0),
        DVec2::new(140.0, 30.0),
        Fill::No,
    );
    let c = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection = sides.iter().copied().collect();
        ed.make_block("Box").unwrap()
    };
    clear(&mut h);
    let t = text(&mut h, "X", DVec2::new(125.0, 18.0), 5.0);
    let (inside, edge, on_text) = (
        DVec2::new(110.0, 10.0),
        DVec2::new(100.0, 15.0),
        DVec2::new(126.0, 20.0),
    );
    let copy = Lit {
        copy: Some(c),
        ..Lit::default()
    };
    assert_eq!(hover(&mut h, inside), copy);
    assert_eq!(hover(&mut h, edge), copy);
    assert_eq!(
        hover(&mut h, on_text),
        Lit {
            text: Some(t),
            ..Lit::default()
        }
    );
    assert_eq!(
        click(&mut h, inside),
        Picked {
            copies: [c].into(),
            ..Picked::default()
        }
    );
    assert_eq!(
        click(&mut h, edge),
        Picked {
            copies: [c].into(),
            ..Picked::default()
        }
    );
    assert_eq!(
        click(&mut h, on_text),
        Picked {
            texts: [t].into(),
            ..Picked::default()
        }
    );
    // A drag from inside moves the copy.
    let from = h.state().drawing.editor.doc.instance(c).unwrap().at;
    clear(&mut h);
    drag(&mut h, inside, inside + DVec2::new(10.0, 0.0), 4);
    let to = h.state().drawing.editor.doc.instance(c).unwrap().at;
    assert!(near(to, from + DVec2::new(10.0, 0.0)), "{from:?} → {to:?}");
    undo(&mut h);
    // A double-click inside opens it for editing.
    double_click_at(&mut h, inside);
    assert_eq!(
        h.state().drawing.editor.block_edit.map(|e| e.instance),
        Some(c),
        "editing the block"
    );
}

/// Two filled rectangles, the second over the first: where they overlap, the top one.
#[test]
fn smoke_picking_overlapping_fills() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 60.0));
    let (a_sides, fa) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let (b_sides, fb) = rectangle(
        &mut h,
        DVec2::new(20.0, 10.0),
        DVec2::new(60.0, 40.0),
        Fill::Solid,
    );
    let (fa, fb) = (fa.unwrap(), fb.unwrap());
    let (both, a_only, edge) = (
        DVec2::new(30.0, 20.0),
        DVec2::new(10.0, 5.0),
        // (Off the side's middle.)
        DVec2::new(0.0, 8.0),
    );
    let fill = |f| Lit {
        fill: Some(f),
        ..Lit::default()
    };
    assert_eq!(hover(&mut h, both), fill(fb), "the top one");
    assert_eq!(hover(&mut h, a_only), fill(fa));
    let lit = hover(&mut h, edge);
    assert!(lit.entity.is_some_and(|e| a_sides.contains(&e)), "{lit:?}");
    assert_eq!(
        lit,
        Lit {
            entity: lit.entity,
            ..Lit::default()
        }
    );
    let shape = |sides: &[EntityId], f| Picked {
        selection: sides.iter().copied().collect(),
        fills: [f].into(),
        ..Picked::default()
    };
    assert_eq!(click(&mut h, both), shape(&b_sides, fb));
    assert_eq!(click(&mut h, a_only), shape(&a_sides, fa));
    assert_eq!(
        click(&mut h, edge),
        Picked {
            selection: [lit.entity.unwrap()].into(),
            ..Picked::default()
        },
        "a side alone"
    );
    // A double-click in a shape of its own: its fill alone.
    double_click_at(&mut h, a_only);
    assert_eq!(
        picked(&h),
        Picked {
            fills: [fa].into(),
            ..Picked::default()
        }
    );
    // A drag from a fill that isn't picked box-selects; from a picked one, moves its shape.
    let before = positions(&h);
    clear(&mut h);
    drag(&mut h, a_only, a_only + DVec2::new(5.0, 3.0), 4);
    assert_eq!(positions(&h), before, "nothing moved");
    click(&mut h, both);
    let corner = ends(&h, b_sides[0]).0;
    let c0 = at(&h, corner);
    drag(&mut h, both, both + DVec2::new(10.0, 0.0), 4);
    let c1 = at(&h, corner);
    // (Its base point, the centroid, snaps on the way: only that it moved counts here.)
    assert!(c1.distance(c0) > 1.0, "B moved: {c0:?} → {c1:?}");
    let a_corner = ends(&h, a_sides[0]).0;
    assert_eq!(
        at(&h, a_corner),
        before.iter().find(|(id, _)| *id == a_corner).unwrap().1
    );
    undo(&mut h);
}

/// A ring of walls round a 100 × 60 room, 10 thick, with the Wall tool; back to Select.
fn room(h: &mut Harness<'_, DriftApp>) {
    h.state_mut().drawing.editor.prefs.wall_thickness = 10.0;
    h.state_mut().drawing.editor.prefs.wall_justify = drift_core::Justify::Centre;
    h.state_mut().drawing.editor.prefs.wall_arc = false;
    h.key_press(Key::W);
    h.run();
    for p in [
        DVec2::new(0.0, 0.0),
        DVec2::new(100.0, 0.0),
        DVec2::new(100.0, 60.0),
        DVec2::new(0.0, 60.0),
        DVec2::new(0.0, 0.0),
    ] {
        click_world(h, p);
    }
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
}

/// A wall's poché (its hidden centreline under the pointer), and a door in it.
#[test]
fn smoke_picking_wall_poche_and_its_door() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-20.0, -20.0), DVec2::new(160.0, 80.0));
    room(&mut h);
    // A door 30 wide in the bottom wall, from x 35 to 65.
    h.key_press_modifiers(EguiModifiers::SHIFT, Key::O);
    h.run();
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.set_opening_family("door.single");
        ed.prefs.opening.width = 30.0;
        ed.prefs.opening.leaf = 28.0;
    }
    click_world(&mut h, DVec2::new(50.0, 1.0));
    h.key_press(Key::Escape);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    let door = h.state().drawing.editor.doc.openings().next().unwrap().0;
    let poche = h.state().drawing.editor.doc.fills().next().unwrap().0;
    let (centre, in_poche, jamb) = (
        DVec2::new(20.0, 0.0),
        DVec2::new(20.0, 3.0),
        DVec2::new(35.0, 3.0),
    );
    let fill = Lit {
        fill: Some(poche),
        ..Lit::default()
    };
    assert_eq!(hover(&mut h, centre), fill, "not the hidden centreline");
    assert_eq!(hover(&mut h, in_poche), fill);
    // A click in the poché picks the wall; on the jamb, the door.
    click(&mut h, in_poche);
    {
        let ed = &h.state().drawing.editor;
        assert_eq!(ed.selected_walls().len(), 1, "the wall");
        assert!(ed.selected_openings().is_empty() && ed.selected_fills.is_empty());
    }
    click(&mut h, centre);
    assert_eq!(
        h.state().drawing.editor.selected_walls().len(),
        1,
        "the wall"
    );
    click(&mut h, jamb);
    assert_eq!(h.state().drawing.editor.selected_openings(), [door].into());
    // Dragged by its jamb, the door slides along the wall.
    let (u0, _) = h.state().drawing.editor.opening_span(door).unwrap();
    drag(&mut h, jamb, jamb + DVec2::new(10.0, 0.0), 4);
    let (u1, _) = h.state().drawing.editor.opening_span(door).unwrap();
    assert!((u1 - u0 - 10.0).abs() < 0.5, "{u0} → {u1}");
    undo(&mut h);
    // A drag in the poché with nothing picked box-selects: the wall stays.
    let before = positions(&h);
    clear(&mut h);
    drag(&mut h, in_poche, in_poche + DVec2::new(10.0, -15.0), 4);
    assert_eq!(positions(&h), before, "nothing moved");
    // A double-click in the poché picks the wall and opens nothing.
    double_click_at(&mut h, in_poche);
    let ed = &h.state().drawing.editor;
    assert_eq!(ed.selected_walls().len(), 1);
    assert_eq!(ed.open_group, None);
    assert_eq!(ed.editing_text, None);
    assert!(ed.block_edit.is_none());
}

/// A filled rectangle and, over it, a line on a locked layer: the line is never picked, and
/// the pointer goes through it to the fill. (C4: hover used to light the locked line, and a
/// click on its middle picked its midpoint.)
#[test]
fn smoke_picking_through_a_locked_layer() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(-10.0, -10.0), DVec2::new(110.0, 60.0));
    let (sides, f) = rectangle(&mut h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    let f = f.unwrap();
    let locked = {
        let ed = &mut h.state_mut().drawing.editor;
        let layer = ed.add_layer("Locked");
        ed.set_current_layer(layer);
        layer
    };
    let l = line(&mut h, DVec2::new(10.0, 15.0), DVec2::new(30.0, 15.0));
    {
        let ed = &mut h.state_mut().drawing.editor;
        let mut layer = ed.doc.layer(locked).unwrap().clone();
        layer.locked = true;
        ed.update_layer(locked, layer);
        ed.set_current_layer(LayerId::DEFAULT);
    }
    assert!(h.state().drawing.editor.is_locked(l));
    let (on_line, middle, end) = (
        DVec2::new(14.0, 15.0),
        DVec2::new(20.0, 15.0),
        DVec2::new(10.0, 15.0),
    );
    let fill = Lit {
        fill: Some(f),
        ..Lit::default()
    };
    for p in [on_line, middle, end] {
        assert_eq!(hover(&mut h, p), fill, "through the locked line at {p:?}");
    }
    let shape = Picked {
        selection: sides.iter().copied().collect(),
        fills: [f].into(),
        ..Picked::default()
    };
    for p in [on_line, middle, end] {
        assert_eq!(click(&mut h, p), shape, "through the locked line at {p:?}");
    }
    // A double-click: the fill alone.
    double_click_at(&mut h, on_line);
    assert_eq!(
        picked(&h),
        Picked {
            fills: [f].into(),
            ..Picked::default()
        }
    );
    // A drag from the locked line box-selects: nothing moves.
    let before = positions(&h);
    clear(&mut h);
    drag(&mut h, on_line, on_line + DVec2::new(5.0, 3.0), 4);
    assert_eq!(positions(&h), before, "nothing moved");
}

/// A Bézier's handles are picked only while they show (its curve picked or under the
/// pointer).
#[test]
fn smoke_picking_hidden_handles() {
    let Some(mut h) = harness() else { return };
    view(&mut h, DVec2::new(130.0, 40.0), DVec2::new(210.0, 100.0));
    let (curve, c1) = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let [a, c1, c2, b] = [(150.0, 60.0), (150.0, 90.0), (190.0, 90.0), (190.0, 60.0)]
            .map(|(x, y)| ed.doc.add_point(DVec2::new(x, y)));
        let curve = ed.doc.add_bezier(a, c1, c2, b).unwrap();
        ed.commit("Curve");
        (curve, c1)
    };
    h.run();
    let handle = DVec2::new(150.0, 90.0);
    assert_eq!(hover(&mut h, handle), Lit::default(), "hidden");
    assert_eq!(click(&mut h, handle), Picked::default());
    // With the curve picked, its handles show and pick.
    let on_curve = DVec2::new(170.0, 82.5);
    assert!(click(&mut h, on_curve).selection.contains(&curve));
    assert_eq!(
        hover(&mut h, handle),
        Lit {
            entity: Some(c1),
            ..Lit::default()
        }
    );
    click_world(&mut h, handle);
    assert_eq!(h.state().drawing.editor.selection, [c1].into());
}

/// What a hover lit, in a word.
fn lit_kind(h: &Harness<'_, DriftApp>, lit: Lit) -> &'static str {
    let doc = &h.state().drawing.editor.doc;
    match lit {
        Lit {
            entity: Some(id), ..
        } if doc.get(id).is_some_and(|e| e.kind.is_point()) => "point",
        Lit {
            entity: Some(_), ..
        } => "edge",
        Lit {
            midpoint: Some(_), ..
        } => "midpoint",
        Lit { text: Some(_), .. } => "text",
        Lit { copy: Some(_), .. } => "copy",
        Lit { fill: Some(_), .. } => "fill",
        _ => "none",
    }
}

/// What a click picked, in a word.
fn picked_kind(h: &Harness<'_, DriftApp>, p: &Picked) -> &'static str {
    let doc = &h.state().drawing.editor.doc;
    if !p.fills.is_empty() {
        "fill"
    } else if !p.copies.is_empty() {
        "copy"
    } else if !p.texts.is_empty() {
        "text"
    } else if !p.midpoints.is_empty() {
        "midpoint"
    } else if p.region {
        "area"
    } else if p
        .selection
        .iter()
        .all(|id| doc.get(*id).is_some_and(|e| e.kind.is_point()))
        && !p.selection.is_empty()
    {
        "point"
    } else if !p.selection.is_empty() {
        "edge"
    } else {
        "none"
    }
}

/// One of everything: a filled rectangle, an empty one, a block copy, a line and a text,
/// with empty areas pickable. Returns where each is, by name.
fn everything(h: &mut Harness<'_, DriftApp>) -> [(&'static str, DVec2); 7] {
    h.state_mut()
        .drawing
        .editor
        .change_rules(|r| r.regions_need_fill = false);
    view(h, DVec2::new(-10.0, -10.0), DVec2::new(160.0, 90.0));
    rectangle(h, DVec2::ZERO, DVec2::new(40.0, 30.0), Fill::Solid);
    rectangle(h, DVec2::new(0.0, 50.0), DVec2::new(40.0, 80.0), Fill::No);
    let (sides, _) = rectangle(h, DVec2::new(120.0, 0.0), DVec2::new(150.0, 30.0), Fill::No);
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.selection = sides.iter().copied().collect();
        ed.make_block("Box").unwrap();
    }
    clear(h);
    line(h, DVec2::new(60.0, 0.0), DVec2::new(100.0, 20.0));
    text(h, "Txt", DVec2::new(60.0, 50.0), 10.0);
    [
        ("end", DVec2::new(100.0, 20.0)),
        ("line", DVec2::new(70.0, 5.0)),
        ("middle", DVec2::new(80.0, 10.0)),
        ("text", DVec2::new(64.0, 55.0)),
        ("fill", DVec2::new(20.0, 15.0)),
        ("area", DVec2::new(20.0, 65.0)),
        ("copy", DVec2::new(135.0, 15.0)),
    ]
}

/// For each filter in `table`, what a hover lights and a click picks at each of
/// [`everything`]'s spots.
fn check_filters(table: &[(PickFilter, [&str; 7], [&str; 7])]) {
    let Some(mut h) = harness() else { return };
    let spots = everything(&mut h);
    let mut wrong = Vec::new();
    for (filter, hovers, clicks) in table {
        h.state_mut().drawing.editor.prefs.pick_filter = *filter;
        for (i, (name, p)) in spots.iter().enumerate() {
            let lit = hover(&mut h, *p);
            let got = lit_kind(&h, lit);
            if got != hovers[i] {
                wrong.push(format!("{filter:?} hover {name}: {got}, not {}", hovers[i]));
            }
            let picked = click(&mut h, *p);
            let got = picked_kind(&h, &picked);
            if got != clicks[i] {
                wrong.push(format!("{filter:?} click {name}: {got}, not {}", clicks[i]));
            }
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// No filter, points alone and edges alone, over one of everything.
#[test]
fn smoke_picking_filters_for_geometry() {
    let only = PickFilter::only;
    check_filters(&[
        (
            PickFilter::ALL,
            ["point", "edge", "midpoint", "text", "fill", "none", "copy"],
            ["point", "edge", "midpoint", "text", "fill", "area", "copy"],
        ),
        (
            only(PickKind::Points),
            ["point", "none", "none", "none", "none", "none", "none"],
            ["point", "none", "none", "none", "none", "none", "none"],
        ),
        (
            only(PickKind::Edges),
            ["edge", "edge", "edge", "none", "none", "none", "none"],
            ["edge", "edge", "edge", "none", "none", "none", "none"],
        ),
    ]);
}

/// Fills, areas, blocks and text, each alone, over one of everything.
#[test]
fn smoke_picking_filters_for_the_rest() {
    let only = PickFilter::only;
    check_filters(&[
        (
            only(PickKind::Fills),
            ["none", "none", "none", "none", "fill", "none", "none"],
            ["none", "none", "none", "none", "fill", "none", "none"],
        ),
        (
            only(PickKind::Areas),
            ["none"; 7],
            // (A filled area too, with fills off.)
            ["none", "none", "none", "none", "area", "area", "none"],
        ),
        (
            only(PickKind::Blocks),
            ["none", "none", "none", "none", "none", "none", "copy"],
            ["none", "none", "none", "none", "none", "none", "copy"],
        ),
        (
            only(PickKind::Text),
            ["none", "none", "none", "text", "none", "none", "none"],
            ["none", "none", "none", "text", "none", "none", "none"],
        ),
    ]);
}
