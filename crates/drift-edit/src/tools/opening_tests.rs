//! The Opening tool and openings in the editor: placing, the swing, typed offset and
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
    ed.prefs.auto_relations = false;
    ed.prefs.wall_thickness = T;
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
    let start = (ed.prefs.opening.hand, ed.prefs.opening.facing);
    let p = ptr(&ed, v(1000.0, 0.0));
    tool.handle(Input::Hover(p), &mut ed);
    for _ in 0..4 {
        assert!(tool.key(ToolKey::Space, Some(&p), &mut ed));
    }
    assert_eq!(
        (ed.prefs.opening.hand, ed.prefs.opening.facing),
        start,
        "four ways round"
    );
    tool.key(ToolKey::Space, Some(&p), &mut ed);
    assert_ne!((ed.prefs.opening.hand, ed.prefs.opening.facing), start);
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
    assert_eq!(
        (op.hand, op.facing),
        (ed.prefs.opening.hand, ed.prefs.opening.facing)
    );
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
    ed.out.notice = None;
    assert!(place(&mut ed, v(250.0, 3000.0)).is_none());
    assert!(ed.out.notice.as_deref().unwrap_or("").contains("No room"));
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
    ed.out.notice = None;
    ed.out.info = None;
    ed.delete_selection();
    assert_eq!(ed.doc.openings().count(), 0);
    assert!(made.iter().all(|e| !ed.doc.contains(*e)));
    // (#312: it says what was done, not an error.)
    assert!(ed.out.notice.is_none());
    assert!(
        ed.out
            .info
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

/// #338 review: a run cut in two by a deleted middle segment (a trim with nothing across
/// it) takes the openings on its far piece to that piece's wall, where they were.
#[test]
fn a_run_cut_in_two_by_a_delete_takes_its_openings_along() {
    let mut ed = editor();
    let w = run(
        &mut ed,
        &[
            v(0.0, 0.0),
            v(4000.0, 0.0),
            v(4000.0, 3000.0),
            v(0.0, 3000.0),
        ],
    );
    let a = place(&mut ed, v(2000.0, 0.0)).unwrap();
    let b = place(&mut ed, v(2000.0, 3000.0)).unwrap();
    assert_eq!(ed.doc.opening(b).unwrap().seg, 2);
    let (at_a, at_b) = (span(&ed, a), span(&ed, b));
    let middle = ed.wall_curves(w)[1];
    ed.doc.begin();
    ed.doc.delete([middle]);
    ed.commit("Delete");
    assert_eq!(ed.doc.walls().count(), 2);
    let second = ed
        .doc
        .walls()
        .map(|(id, _)| id)
        .find(|id| *id != w)
        .unwrap();
    let oa = ed.doc.opening(a).unwrap();
    assert_eq!((oa.wall, oa.seg), (w, 0));
    let ob = ed.doc.opening(b).unwrap();
    assert_eq!((ob.wall, ob.seg), (second, 0), "re-seated on the new wall");
    assert_eq!(span(&ed, a), at_a);
    assert!((span(&ed, b).0 - at_b.0).abs() < 1e-6);
    assert!((span(&ed, b).1 - at_b.1).abs() < 1e-6);
    assert_eq!(jambs(&ed, w), 2);
    assert_eq!(jambs(&ed, second), 2);
    assert!(!symbol(&ed, b).is_empty());
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
    ed.prefs.wall_arc = true;
    let mut tool = ToolKind::Wall.create();
    click(&mut ed, tool.as_mut(), v(0.0, 0.0));
    click(&mut ed, tool.as_mut(), v(4000.0, 0.0));
    click(&mut ed, tool.as_mut(), v(2000.0, 1000.0));
    tool.handle(Input::Finish, &mut ed);
    ed.prefs.wall_arc = false;
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

#[test]
fn resizing_an_opening_moves_no_other() {
    for auto in [false, true] {
        resizing_moves_no_other(auto);
    }
}

fn resizing_moves_no_other(auto: bool) {
    let mut ed = editor();
    ed.prefs.auto_relations = auto;
    run(
        &mut ed,
        &[
            v(0.0, 0.0),
            v(8000.0, 0.0),
            v(8000.0, 5000.0),
            v(0.0, 5000.0),
        ],
    );
    let a = place(&mut ed, v(2000.0, 0.0)).unwrap();
    let b = place(&mut ed, v(5500.0, 0.0)).unwrap();
    let c = place(&mut ed, v(8000.0, 2500.0)).unwrap();
    let d = place(&mut ed, v(4000.0, 5000.0)).unwrap();
    let where_ = |ed: &Editor, o: OpeningId| {
        let mut pts: Vec<(EntityId, DVec2)> = ed
            .doc
            .opening(o)
            .unwrap()
            .made
            .points
            .values()
            .map(|id| (*id, ed.doc.point(*id).unwrap()))
            .collect();
        pts.sort_by_key(|(id, _)| *id);
        (ed.doc.opening(o).unwrap().at.clone(), span(ed, o), pts)
    };
    for edited in [a, b, c, d] {
        for text in ["1500", "600", "1200"] {
            let others: Vec<_> = [a, b, c, d]
                .into_iter()
                .filter(|o| *o != edited)
                .map(|o| (o, where_(&ed, o)))
                .collect();
            ed.set_opening_width(edited, text).unwrap();
            for (o, before) in others {
                let after = where_(&ed, o);
                assert_eq!(before.0, after.0, "{o} placement after {edited} -> {text}");
                assert!(
                    (before.1.0 - after.1.0).abs() < 1e-6 && (before.1.1 - after.1.1).abs() < 1e-6,
                    "{o} span {:?} -> {:?} after {edited} -> {text}",
                    before.1,
                    after.1
                );
                for ((_, p), (_, q)) in before.2.iter().zip(&after.2) {
                    assert!(
                        p.distance(*q) < 1e-6,
                        "{o} symbol moved after {edited} -> {text}"
                    );
                }
            }
        }
    }
}

/// Every opening but `edited`: where it is set, its span, and its symbol's points; and every
/// wall's centreline points.
fn placed_but(ed: &Editor, edited: OpeningId) -> Vec<String> {
    let mut out = Vec::new();
    for (o, op) in ed.doc.openings() {
        if o == edited {
            continue;
        }
        let (a, b) = span(ed, o);
        out.push(format!(
            "{o} {:?} w{} seg{} {a:.6}..{b:.6}",
            op.at, op.wall.0, op.seg
        ));
        for (k, p) in &op.made.points {
            let q = ed.doc.point(*p).unwrap();
            out.push(format!("{o} pt{k} {:.6},{:.6}", q.x, q.y));
        }
    }
    for (w, wall) in ed.doc.walls() {
        for c in &ed.doc.path(wall.centre).unwrap().curves {
            for p in ed.doc.get(*c).unwrap().kind.points() {
                let q = ed.doc.point(p).unwrap();
                out.push(format!("{w:?} centre {p:?} {:.6},{:.6}", q.x, q.y));
            }
        }
    }
    out
}

fn assert_resizes_move_no_other(ed: &mut Editor, what: &str) {
    let ids: Vec<OpeningId> = ed.doc.openings().map(|(id, _)| id).collect();
    assert!(ids.len() >= 2, "{what}: {} openings", ids.len());
    for &edited in &ids {
        for text in ["1500", "600", "1250", "900"] {
            let before = placed_but(ed, edited);
            let r = ed.set_opening_width(edited, text);
            let after = placed_but(ed, edited);
            let moved: Vec<_> = before.iter().zip(&after).filter(|(a, b)| a != b).collect();
            assert!(
                moved.is_empty() && before.len() == after.len(),
                "{what}: {edited} = {text} ({r:?}) moved others: {moved:#?}"
            );
        }
    }
}

#[test]
fn resizing_an_opening_in_a_closed_room_moves_no_other() {
    for auto in [false, true] {
        let mut ed = editor();
        ed.prefs.auto_relations = auto;
        let mut tool = ToolKind::Wall.create();
        for at in [
            v(0.0, 0.0),
            v(8000.0, 0.0),
            v(8000.0, 5000.0),
            v(0.0, 5000.0),
            v(0.0, 0.0),
        ] {
            click(&mut ed, tool.as_mut(), at);
        }
        // Two close together on the south, one each on the other sides (the west is the
        // closing segment).
        let s1 = place(&mut ed, v(1500.0, 0.0)).unwrap();
        let s2 = place(&mut ed, v(3000.0, 0.0)).unwrap();
        let e = place(&mut ed, v(8000.0, 2500.0)).unwrap();
        let n = place(&mut ed, v(6500.0, 5000.0)).unwrap();
        let w = place(&mut ed, v(0.0, 3500.0)).unwrap();
        // Some measured from their segment's far end, one centred.
        ed.set_opening_offset(n, "400").unwrap();
        ed.set_opening_offset(w, "700").unwrap();
        ed.centre_openings(&[e]);
        let _ = (s1, s2);
        assert_resizes_move_no_other(&mut ed, &format!("closed room, auto {auto}"));
    }
}

fn frame_at(ed: &Editor, o: OpeningId) -> (DVec2, f64) {
    let f = ed.opening_frame(o).unwrap();
    (f.point(DVec2::ZERO), f.width)
}

/// A wall teed into another by where its end is (on the host's face, not its centreline,
/// as a typed length or a paste leaves it): resizing an opening snaps no end, so neither
/// wall's openings move.
#[test]
fn resizing_an_opening_snaps_no_tee_end() {
    let mut ed = editor();
    let _host = run(&mut ed, &[v(0.0, 0.0), v(6000.0, 0.0)]);
    let w2 = run(&mut ed, &[v(3000.0, 4000.0), v(3000.0, 1000.0)]);
    let far = place(&mut ed, v(3000.0, 3300.0)).unwrap();
    let near = place(&mut ed, v(3000.0, 1700.0)).unwrap();
    let h = place(&mut ed, v(1500.0, 0.0)).unwrap();
    ed.centre_openings(&[near]);
    let wall = ed.doc.wall(w2).unwrap().clone();
    let (_, pts, _) = drift_geom::walls::regen::centreline(&ed.doc, &wall).unwrap();
    let end = *pts.last().unwrap();
    ed.doc.begin();
    ed.doc.set_point(end, v(3000.0, T / 2.0)).unwrap();
    assert!(ed.commit("Move end"));
    assert!(
        ed.doc.wall(w2).unwrap().ends[1].tee.is_some(),
        "teed by where it is"
    );
    let (f0, n0) = (frame_at(&ed, far), frame_at(&ed, near));
    ed.set_opening_width(h, "1200").unwrap();
    assert_eq!(
        ed.doc.point(end).unwrap(),
        v(3000.0, T / 2.0),
        "end snapped"
    );
    assert_eq!(frame_at(&ed, near), n0);
    assert_eq!(frame_at(&ed, far), f0);
    // A window on the teed wall itself: its neighbour on that wall stays put.
    ed.set_opening_width(far, "1200").unwrap();
    assert_eq!(
        ed.doc.point(end).unwrap(),
        v(3000.0, T / 2.0),
        "end snapped"
    );
    assert_eq!(frame_at(&ed, near), n0);
}

/// A wall with two doors whose length dimension a join left unmet (it pulled the
/// dimensioned end): the doors, the one by that end first.
fn wall_left_unmet() -> (Editor, OpeningId, OpeningId) {
    let mut ed = editor();
    let w1 = run(&mut ed, &[v(0.0, 0.0), v(4000.0, 0.0)]);
    let near_end = place(&mut ed, v(3200.0, 0.0)).unwrap();
    let edited = place(&mut ed, v(1000.0, 0.0)).unwrap();
    let c = ed.doc.wall(w1).unwrap().centre;
    let line = ed.doc.path(c).unwrap().curves[0];
    let EntityKind::Line { a, b } = ed.doc.get(line).unwrap().kind else {
        panic!()
    };
    ed.add_dimension(
        Constraint::Distance {
            a,
            b,
            value: 4000.0,
        },
        None,
    )
    .unwrap();
    let _w2 = run(&mut ed, &[v(3960.0, 2000.0), v(3960.0, 30.0)]);
    let len = ed.doc.point(a).unwrap().distance(ed.doc.point(b).unwrap());
    assert!(
        (len - 3960.0).abs() < 1e-6,
        "the join left the dimension unmet: {len}"
    );
    (ed, near_end, edited)
}

/// A dimension an earlier edit left unmet is not solved by an edit of openings alone (a
/// width, an offset, a delete): that would move the wall and the openings in it.
#[test]
fn an_opening_edit_solves_nothing_left_unmet() {
    type Edit = fn(&mut Editor, OpeningId);
    let edits: [(&str, Edit); 4] = [
        ("width", |ed, o| ed.set_opening_width(o, "1200").unwrap()),
        ("offset", |ed, o| ed.set_opening_offset(o, "500").unwrap()),
        ("flip", |ed, o| {
            assert!(ed.flip_openings(&[o], crate::openings::Flip::Hand));
        }),
        ("delete", |ed, o| assert!(ed.delete_openings(&[o]))),
    ];
    for (what, edit) in edits {
        let (mut ed, near_end, edited) = wall_left_unmet();
        let before = frame_at(&ed, near_end);
        edit(&mut ed, edited);
        assert_eq!(
            frame_at(&ed, near_end),
            before,
            "{what} moved the other door"
        );
    }
}

/// An opening grown past the stretch it is in is refused, not moved past its neighbour.
#[test]
fn a_widened_opening_never_jumps_past_its_neighbour() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(6000.0, 0.0)]);
    let a = place(&mut ed, v(1200.0, 0.0)).unwrap();
    let b = place(&mut ed, v(2600.0, 0.0)).unwrap();
    let sb = span(&ed, b);
    let r = ed.set_opening_width(a, "2400");
    assert!(r.is_err(), "{:?}", span(&ed, a));
    assert_eq!(span(&ed, b), sb);
}

/// An opening's offset is from the corner, not a neighbour's jamb: resizing the neighbour
/// changes nothing it shows.
#[test]
fn an_offset_stays_when_a_neighbour_is_resized() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(8000.0, 0.0)]);
    let a = place(&mut ed, v(1500.0, 0.0)).unwrap();
    let b = place(&mut ed, v(3500.0, 0.0)).unwrap();
    let readout = |ed: &Editor| {
        let (lo, hi, start) = ed.opening_offsets(b).unwrap();
        let (u0, u1) = span(ed, b);
        if start { u0 - lo } else { hi - u1 }
    };
    let before = readout(&ed);
    ed.set_opening_width(a, "1500").unwrap();
    assert!(
        (readout(&ed) - before).abs() < 1e-6,
        "{before} -> {}",
        readout(&ed)
    );
}

/// A typed offset that would put an opening onto or past its neighbour is refused; one
/// within its stretch reads back as typed.
#[test]
fn a_typed_offset_never_passes_a_neighbour() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(8000.0, 0.0)]);
    let a = place(&mut ed, v(1500.0, 0.0)).unwrap();
    let b = place(&mut ed, v(3500.0, 0.0)).unwrap();
    let (sa, sb) = (span(&ed, a), span(&ed, b));
    for text in ["600", "1200"] {
        assert!(ed.set_opening_offset(b, text).is_err(), "{text}");
        assert_eq!((span(&ed, a), span(&ed, b)), (sa, sb), "{text}");
    }
    ed.set_opening_offset(b, "2600").unwrap();
    let (lo, _, start) = ed.opening_offsets(b).unwrap();
    assert!(start && (span(&ed, b).0 - lo - 2600.0).abs() < 1e-6);
    assert_eq!(span(&ed, a), sa);
    // Short of a corner it stops at the corner's clearance, as before.
    assert!(ed.set_opening_offset(a, "0").is_ok());
}

/// A centred opening widened past what its stretch leaves either side of the middle goes as
/// near the middle as its stretch lets it, never onto its neighbour.
#[test]
fn a_widened_centred_opening_stays_off_its_neighbour() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(8000.0, 0.0)]);
    let a = place(&mut ed, v(4000.0, 0.0)).unwrap();
    let b = place(&mut ed, v(5600.0, 0.0)).unwrap();
    ed.centre_openings(&[a]);
    let sb = span(&ed, b);
    ed.set_opening_width(a, "3000").unwrap();
    let (a0, a1) = span(&ed, a);
    assert!(a1 <= sb.0, "{:?} onto {sb:?}", (a0, a1));
    assert!((a1 - a0 - 3000.0).abs() < 1e-6);
    assert_eq!(span(&ed, b), sb);
    // Narrowed again so it fits about the middle: centred once more when typed so.
    ed.set_opening_width(a, "900").unwrap();
    assert_eq!(span(&ed, b), sb);
}

/// Placing or deleting an opening snaps no tee end left off its host's centreline either, so
/// no other opening moves (#553).
#[test]
fn placing_or_deleting_an_opening_snaps_no_tee_end() {
    for what in ["delete", "place"] {
        let mut ed = editor();
        let _host = run(&mut ed, &[v(0.0, 0.0), v(6000.0, 0.0)]);
        let w2 = run(&mut ed, &[v(3000.0, 4000.0), v(3000.0, 1000.0)]);
        let far = place(&mut ed, v(3000.0, 3300.0)).unwrap();
        let near = place(&mut ed, v(3000.0, 1700.0)).unwrap();
        let h = place(&mut ed, v(1500.0, 0.0)).unwrap();
        ed.centre_openings(&[near]);
        let wall = ed.doc.wall(w2).unwrap().clone();
        let (_, pts, _) = drift_geom::walls::regen::centreline(&ed.doc, &wall).unwrap();
        let end = *pts.last().unwrap();
        ed.doc.begin();
        ed.doc.set_point(end, v(3000.0, T / 2.0)).unwrap();
        assert!(ed.commit("Move end"));
        assert!(ed.doc.wall(w2).unwrap().ends[1].tee.is_some());
        let (f0, n0) = (frame_at(&ed, far), frame_at(&ed, near));
        match what {
            "place" => {
                place(&mut ed, v(4800.0, 0.0)).unwrap();
            }
            _ => assert!(ed.delete_openings(&[h])),
        }
        assert_eq!(
            ed.doc.point(end).unwrap(),
            v(3000.0, T / 2.0),
            "{what}: end snapped"
        );
        assert_eq!(frame_at(&ed, near), n0, "{what}: near moved");
        assert_eq!(frame_at(&ed, far), f0, "{what}: far moved");
    }
}
