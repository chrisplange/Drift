//! The Door / window tool and openings in the editor: placing, the swing, typed offset and
//! width, sliding along the wall, flipping, and what happens to openings when their wall
//! moves, splits, explodes or goes (docs/walls-design.md §6, §9.3).

use drift_core::{Hand, OpeningAt, OpeningId, Owner, Part, Station, Symbol, WallFace, WallId};
use drift_geom::SnapOptions;

use super::*;

const TOL: f64 = 5.0;
const T: f64 = 100.0;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn ptr(ed: &Editor, at: DVec2) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    p.snap = ed.index.snap(
        at,
        SnapOptions {
            tolerance: TOL,
            grid: None,
            from: None,
        },
        |_| false,
    );
    p
}

fn click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
    let p = ptr(ed, at);
    tool.handle(Input::Hover(p), ed);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

/// A metric drawing (the generic metric sizes), walls 100 mm thick.
fn editor() -> Editor {
    let mut ed = Editor::default();
    ed.auto_relations = false;
    ed.wall_thickness = T;
    ed
}

fn run(ed: &mut Editor, pts: &[DVec2]) -> WallId {
    let mut tool = ToolKind::Wall.create();
    for at in pts {
        click(ed, tool.as_mut(), *at);
    }
    tool.handle(Input::Finish, ed);
    ed.doc.walls().map(|(id, _)| id).max().expect("a wall")
}

/// A door placed by the tool with the pointer at `at`.
fn place(ed: &mut Editor, at: DVec2) -> Option<OpeningId> {
    let before: Vec<OpeningId> = ed.doc.openings().map(|(id, _)| id).collect();
    let mut tool = ToolKind::Opening.create();
    click(ed, tool.as_mut(), at);
    ed.doc
        .openings()
        .map(|(id, _)| id)
        .find(|id| !before.contains(id))
}

fn poche_area(ed: &mut Editor) -> f64 {
    ed.fill_shapes().values().map(|s| s.region.area).sum()
}

fn symbol(ed: &Editor, o: OpeningId) -> Vec<EntityId> {
    ed.doc.opening(o).unwrap().made.entities().collect()
}

fn jambs(ed: &Editor, w: WallId) -> usize {
    ed.doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .keys()
        .filter(|k| k.part == Part::Jamb)
        .count()
}

/// The span of an opening along its segment.
fn span(ed: &Editor, o: OpeningId) -> (f64, f64) {
    ed.opening_span(o).unwrap()
}

#[test]
fn a_door_cuts_its_wall_and_draws_its_symbol() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let whole = poche_area(&mut ed);
    let o = place(&mut ed, v(2000.0, 10.0)).expect("a door");
    let op = ed.doc.opening(o).unwrap().clone();
    // The generic metric set's usual door: a 900 leaf in a 970 opening, centred on the
    // pointer.
    assert_eq!((op.width, op.leaf), (970.0, 900.0));
    assert!(matches!(&op.symbol, Symbol::Library { family, .. } if family == "door.single"));
    let (u0, u1) = span(&ed, o);
    assert!((u0 - (2000.0 - 485.0)).abs() < 1e-6 && (u1 - (2000.0 + 485.0)).abs() < 1e-6);
    // Two jambs, a leaf and its swing, all the opening's (never solver unknowns).
    assert_eq!(jambs(&ed, w), 2);
    let made = symbol(&ed, o);
    assert!(!made.is_empty());
    let owners = ed.wall_owners();
    for e in &made {
        assert_eq!(owners.get(*e), Some(Owner::Opening(o, w)));
        assert!(owners.generated(*e));
    }
    assert!(
        made.iter()
            .any(|e| matches!(ed.doc.get(*e).unwrap().kind, EntityKind::Arc { .. })),
        "a swing"
    );
    // The poché opens.
    assert!((whole - poche_area(&mut ed) - 970.0 * T).abs() < 1e-6);
    // One undo takes it all back.
    ed.undo();
    assert_eq!(ed.doc.openings().count(), 0);
    assert_eq!(jambs(&ed, w), 0);
    assert!(made.iter().all(|e| !ed.doc.contains(*e)));
    assert!((poche_area(&mut ed) - whole).abs() < 1e-6);
    ed.redo();
    assert_eq!(symbol(&ed, o), made, "redo restores the same ids");
}

#[test]
fn space_steps_the_swing_and_typing_sets_offset_then_width() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let mut tool = ToolKind::Opening.create();
    let start = (ed.opening.hand, ed.opening.facing);
    let p = ptr(&ed, v(1000.0, 0.0));
    tool.handle(Input::Hover(p), &mut ed);
    for _ in 0..4 {
        assert!(tool.key(ToolKey::Space, Some(&p), &mut ed));
    }
    assert_eq!(
        (ed.opening.hand, ed.opening.facing),
        start,
        "four ways round"
    );
    tool.key(ToolKey::Space, Some(&p), &mut ed);
    assert_ne!((ed.opening.hand, ed.opening.facing), start);
    // Offset 300 from the nearer corner (the free end's cap at 0), width 800.
    assert_eq!(tool.fields(), &["Offset", "Width"]);
    tool.set_typed([Some(300.0), Some(800.0)]);
    tool.commit_typed(&p, &mut ed);
    let (o, op) = ed
        .doc
        .openings()
        .next()
        .map(|(id, o)| (id, o.clone()))
        .unwrap();
    assert_eq!(op.width, 800.0);
    assert_eq!(op.leaf, 730.0, "the leaf keeps its frame allowance");
    let (u0, _) = span(&ed, o);
    assert!(
        (u0 - (300.0 + crate::openings::CLEARANCE)).abs() < 1e-6,
        "{u0}"
    );
    assert_eq!((op.hand, op.facing), (ed.opening.hand, ed.opening.facing));
}

#[test]
fn an_opening_keeps_clear_of_corners_and_refuses_no_room() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(3000.0, 0.0), v(3000.0, 2000.0)]);
    // Hovered right at the corner: it slides back clear of the inside corner (at 2950).
    let o = place(&mut ed, v(2990.0, 0.0)).expect("placed");
    let (_, u1) = span(&ed, o);
    assert!(u1 <= 2950.0 - crate::openings::CLEARANCE + 1e-6, "{u1}");
    assert!(ed.doc.opening(o).is_some());
    assert_eq!(jambs(&ed, w), 2);
    // A wall too short for it: refused, with a notice.
    let short = run(&mut ed, &[v(0.0, 3000.0), v(500.0, 3000.0)]);
    ed.notice = None;
    assert!(place(&mut ed, v(250.0, 3000.0)).is_none());
    assert!(ed.notice.as_deref().unwrap_or("").contains("No room"));
    assert_eq!(jambs(&ed, short), 0);
}

#[test]
fn an_opening_follows_its_wall_keeping_its_ids() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0), v(4000.0, 3000.0)]);
    let o = place(&mut ed, v(1500.0, 0.0)).unwrap();
    let made = ed.doc.opening(o).unwrap().made.clone();
    let faces = ed.doc.wall(w).unwrap().made.curves.clone();
    // Move the whole wall: everything rides along, no id changes.
    let curves = ed.wall_curves(w);
    ed.doc.begin();
    assert!(ed.drag(&curves, v(250.0, 400.0), &[]));
    ed.commit("Move");
    assert_eq!(ed.doc.opening(o).unwrap().made, made);
    assert_eq!(ed.doc.wall(w).unwrap().made.curves, faces);
    let jamb = faces
        .iter()
        .find(|(k, _)| k.part == Part::Jamb && k.from == Station::Jamb(o, 0))
        .map(|(_, id)| *id)
        .unwrap();
    let Some(Shape::Line { a, .. }) = ed.doc.shape(jamb) else {
        panic!("a jamb line");
    };
    assert!((a.y - (400.0 + T / 2.0)).abs() < 1e-6 || (a.y - (400.0 - T / 2.0)).abs() < 1e-6);
    // Lengthen the first segment from its start: an opening measured from the start stays
    // put along the wall.
    let (u0, _) = span(&ed, o);
    let start = ed.doc.path(ed.doc.wall(w).unwrap().centre).unwrap().curves[0];
    let EntityKind::Line { a: p0, .. } = ed.doc.get(start).unwrap().kind else {
        panic!("a line");
    };
    ed.doc.begin();
    assert!(ed.drag(&[p0], v(-500.0, 0.0), &[]));
    ed.commit("Drag");
    assert!(
        matches!(ed.doc.opening(o).unwrap().at, OpeningAt::From { .. }),
        "measured from an end"
    );
    let (u0b, _) = span(&ed, o);
    assert!((u0b - u0).abs() < 1e-6, "{u0} -> {u0b}");
    assert_eq!(ed.doc.opening(o).unwrap().made, made);
}

#[test]
fn dragging_a_picked_opening_slides_it_along_its_wall() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let o = place(&mut ed, v(1500.0, 0.0)).unwrap();
    let made = ed.doc.opening(o).unwrap().made.clone();
    let face_ids: Vec<EntityId> = ed
        .doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .values()
        .copied()
        .collect();
    // Pick it by its leaf, then drag it 1 m along (and a little off: it stays on the wall).
    let leaf = made.curves.values().copied().next().unwrap();
    ed.selection = ed.pick_set(leaf);
    assert_eq!(
        ed.selected_openings().into_iter().collect::<Vec<_>>(),
        vec![o]
    );
    assert!(ed.transform_box().is_none(), "no scale box on an opening");
    let at = ed.doc.shape(leaf).unwrap().bounds().center();
    let mut tool = ToolKind::Select.create();
    let press = Pointer::new(at, TOL);
    tool.handle(Input::DragStart(press, Modifiers::default()), &mut ed);
    let (u0, _) = span(&ed, o);
    for k in 1..=4 {
        let p = Pointer::new(at + v(250.0 * f64::from(k), 30.0), TOL);
        tool.handle(Input::Drag(p), &mut ed);
    }
    tool.handle(
        Input::DragEnd(
            Pointer::new(at + v(1000.0, 30.0), TOL),
            Modifiers::default(),
        ),
        &mut ed,
    );
    let (u0b, _) = span(&ed, o);
    assert!((u0b - u0 - 1000.0).abs() < 1e-6, "{u0} -> {u0b}");
    // Its symbol and the wall's pieces kept their ids; one undo step.
    assert_eq!(ed.doc.opening(o).unwrap().made, made);
    let now: Vec<EntityId> = ed
        .doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .values()
        .copied()
        .collect();
    assert_eq!(now, face_ids);
    ed.undo();
    assert!((span(&ed, o).0 - u0).abs() < 1e-6);
}

#[test]
fn flipping_and_resizing_an_opening() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let o = place(&mut ed, v(2000.0, 0.0)).unwrap();
    let before = ed.doc.opening(o).unwrap().clone();
    // The flip arrows of a picked opening.
    ed.selection = ed.opening_pick_set(o);
    let arrows = ed.opening_flip_arrows(o);
    assert_eq!(arrows.len(), 2);
    let (at, _, flip) = arrows[0];
    assert_eq!(ed.opening_arrow_at(at, TOL), Some((o, flip)));
    let mut tool = ToolKind::Select.create();
    tool.handle(
        Input::Click(Pointer::new(at, TOL), Modifiers::default()),
        &mut ed,
    );
    let flipped = ed.doc.opening(o).unwrap().clone();
    assert_eq!(flipped.facing, before.facing.flipped());
    assert_eq!(flipped.made.curves.len(), before.made.curves.len());
    assert!(ed.flip_openings(&[o], crate::openings::Flip::Hand));
    assert_eq!(ed.doc.opening(o).unwrap().hand, before.hand.flipped());
    // Typed into: the width grows about its middle (the leaf with it), the offset is from
    // the nearer corner.
    let mid = |ed: &Editor| {
        let (a, b) = span(ed, o);
        (a + b) / 2.0
    };
    let m = mid(&ed);
    ed.set_opening_width(o, "1200").unwrap();
    let op = ed.doc.opening(o).unwrap();
    assert_eq!((op.width, op.leaf), (1200.0, 1130.0));
    assert!((mid(&ed) - m).abs() < 1e-6);
    ed.set_opening_offset(o, "250").unwrap();
    let (u0, _) = span(&ed, o);
    assert!((u0 - 250.0).abs() < 1e-6, "{u0}");
    // An expression: kept and re-evaluated.
    let var = ed
        .add_variable("dw", "1000", drift_core::Kind::Length)
        .unwrap();
    ed.set_opening_width(o, "dw").unwrap();
    assert_eq!(ed.doc.opening(o).unwrap().width, 1000.0);
    assert!(ed.set_variable(var, "1100").is_ok());
    assert_eq!(ed.doc.opening(o).unwrap().width, 1100.0);
}

#[test]
fn deleting_a_picked_opening_closes_the_wall() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let whole = poche_area(&mut ed);
    let o = place(&mut ed, v(2000.0, 0.0)).unwrap();
    let made = symbol(&ed, o);
    ed.selection = ed.opening_pick_set(o);
    ed.delete_selection();
    assert!(ed.doc.opening(o).is_none());
    assert!(ed.doc.wall(w).is_some(), "the wall stays");
    assert_eq!(jambs(&ed, w), 0);
    assert!(made.iter().all(|e| !ed.doc.contains(*e)));
    assert!((poche_area(&mut ed) - whole).abs() < 1e-6);
}

#[test]
fn an_opening_goes_with_its_wall_and_stays_when_exploded() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let o = place(&mut ed, v(2000.0, 0.0)).unwrap();
    let made = symbol(&ed, o);
    // Deleting the wall (by its centreline) deletes the opening, and says so.
    ed.selection = ed.wall_curves(w).into_iter().collect();
    ed.notice = None;
    ed.delete_selection();
    assert_eq!(ed.doc.openings().count(), 0);
    assert!(made.iter().all(|e| !ed.doc.contains(*e)));
    assert!(
        ed.notice
            .as_deref()
            .unwrap_or("")
            .contains("went with its wall")
    );
    ed.undo();
    assert_eq!(symbol(&ed, o), made);
    // Exploding it leaves the symbol as plain lines and arcs.
    assert!(ed.explode_walls(&[w]));
    assert_eq!(ed.doc.openings().count(), 0);
    assert!(made.iter().all(|e| ed.doc.contains(*e)));
    let owners = ed.wall_owners();
    assert!(made.iter().all(|e| owners.get(*e).is_none()));
}

#[test]
fn splitting_a_wall_takes_its_openings_along() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0), v(4000.0, 3000.0)]);
    let a = place(&mut ed, v(2000.0, 0.0)).unwrap();
    let b = place(&mut ed, v(4000.0, 1500.0)).unwrap();
    assert_eq!(ed.doc.opening(b).unwrap().seg, 1);
    let at_b = span(&ed, b);
    assert!(ed.split_wall(w, 0));
    let second = ed
        .doc
        .walls()
        .map(|(id, _)| id)
        .find(|id| *id != w)
        .unwrap();
    assert_eq!(ed.doc.opening(a).unwrap().wall, w);
    let ob = ed.doc.opening(b).unwrap();
    assert_eq!((ob.wall, ob.seg), (second, 0), "re-seated on the new wall");
    assert!((span(&ed, b).0 - at_b.0).abs() < 1e-6);
    assert_eq!(jambs(&ed, w), 2);
    assert_eq!(jambs(&ed, second), 2);
}

#[test]
fn copying_a_wall_copies_its_openings() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    place(&mut ed, v(2000.0, 0.0)).unwrap();
    ed.selection = ed.wall_curves(w).into_iter().collect();
    assert!(ed.duplicate_selection() > 0);
    assert_eq!(ed.doc.walls().count(), 2);
    assert_eq!(ed.doc.openings().count(), 2);
    let copy = ed
        .doc
        .walls()
        .map(|(id, _)| id)
        .find(|id| *id != w)
        .unwrap();
    assert_eq!(jambs(&ed, copy), 2);
    let o2 = ed.doc.openings().find(|(_, o)| o.wall == copy).unwrap().0;
    assert!(!symbol(&ed, o2).is_empty());
}

#[test]
fn a_window_in_an_arc_wall_and_a_size_set_by_units() {
    let mut ed = editor();
    ed.wall_arc = true;
    let mut tool = ToolKind::Wall.create();
    click(&mut ed, tool.as_mut(), v(0.0, 0.0));
    click(&mut ed, tool.as_mut(), v(4000.0, 0.0));
    click(&mut ed, tool.as_mut(), v(2000.0, 1000.0));
    tool.handle(Input::Finish, &mut ed);
    ed.wall_arc = false;
    let w = ed.doc.walls().next().unwrap().0;
    ed.set_opening_family("window.casement");
    assert_eq!(ed.size_set().id, "metric");
    let o = place(&mut ed, v(2000.0, 1000.0)).expect("a window");
    assert_eq!(jambs(&ed, w), 2);
    let op = ed.doc.opening(o).unwrap();
    assert!(matches!(&op.symbol, Symbol::Library { family, .. } if family == "window.casement"));
    // An imperial drawing gets US sizes.
    let mut us = editor();
    us.doc.set_units(drift_core::Units::architectural());
    assert_eq!(us.size_set().id, "us");
    let row = us.opening_type_now();
    assert_eq!(row.name, "3′0″ × 6′8″");
    assert_eq!(row.opening.0, 38.0 * 25.4);
}

#[test]
fn a_door_near_a_tee_and_one_through_it_is_not_cut() {
    let mut ed = editor();
    let host = run(&mut ed, &[v(0.0, 0.0), v(6000.0, 0.0)]);
    run(&mut ed, &[v(3000.0, 40.0), v(3000.0, 2000.0)]);
    // Beside the T: clamped clear of its corner.
    let o = place(&mut ed, v(2800.0, 0.0)).unwrap();
    let (_, u1) = span(&ed, o);
    assert!(
        u1 <= 3000.0 - T / 2.0 - crate::openings::CLEARANCE + 1e-6,
        "{u1}"
    );
    assert_eq!(jambs(&ed, host), 2);
    // Opposite hand and face, by the record: still cut.
    assert!(ed.edit_openings(&[o], "Swing", |x| {
        x.hand = Hand::Right;
        x.facing = WallFace::Left;
    }));
    assert_eq!(jambs(&ed, host), 2);
}

#[test]
fn a_drag_that_changes_a_join_keeps_the_openings_where_they_are() {
    let mut ed = editor();
    let w = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    run(&mut ed, &[v(5000.0, -1500.0), v(5000.0, 1500.0)]);
    let o = place(&mut ed, v(1500.0, 0.0)).unwrap();
    let at = span(&ed, o);
    // Its free end dragged into the other wall: a T now (every wall is made again).
    let line = ed.wall_curves(w)[0];
    let EntityKind::Line { b: end, .. } = ed.doc.get(line).unwrap().kind else {
        panic!("a line");
    };
    ed.doc.begin();
    assert!(ed.drag(&[end], v(1000.0, 0.0), &[]));
    ed.commit("Drag");
    assert!(ed.doc.wall(w).unwrap().ends[1].tee.is_some(), "a T");
    let op = ed.doc.opening(o).expect("the door stays");
    assert_eq!((op.wall, op.seg), (w, 0));
    assert!((span(&ed, o).0 - at.0).abs() < 1e-6);
    assert_eq!(jambs(&ed, w), 2);
}
