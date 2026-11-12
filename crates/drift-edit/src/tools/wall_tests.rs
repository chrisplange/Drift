//! The Wall tool: drawing runs, joining walls (L, T, against), closing rings, arcs and
//! bends, the keys, and undo per segment (docs/walls-design.md §9.3).

use drift_core::{EntityKind, FillKind, Justify, LineKind, Owner, WallId};
use drift_geom::SnapOptions;

use super::*;

const TOL: f64 = 0.5;

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

fn editor() -> Editor {
    let mut ed = Editor::default();
    ed.prefs.auto_relations = false;
    ed.prefs.wall_thickness = 10.0;
    ed.show_centrelines = true;
    ed
}

/// Draw a run through `pts` and finish it.
fn run(ed: &mut Editor, pts: &[DVec2]) {
    let mut tool = ToolKind::Wall.create();
    for at in pts {
        click(ed, tool.as_mut(), *at);
    }
    tool.handle(Input::Finish, ed);
}

fn poche_area(ed: &mut Editor) -> f64 {
    let shapes = ed.fill_shapes();
    shapes.values().map(|s| s.region.area).sum()
}

fn faces(ed: &Editor) -> usize {
    ed.doc
        .iter()
        .filter(|(id, e)| {
            !e.kind.is_point() && matches!(ed.wall_owners().get(*id), Some(Owner::Wall(_)))
        })
        .count()
}

#[test]
fn an_l_wall_is_one_poche_and_undoes_per_segment() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 50.0)]);
    assert_eq!(ed.doc.walls().count(), 1);
    // Two faces of two segments, and a cap at each end: one cluster, one loop.
    assert_eq!(faces(&ed), 6);
    assert_eq!(ed.doc.fills().count(), 1);
    assert!((poche_area(&mut ed) - 150.0 * 10.0).abs() < 1e-6);
    // The centreline is construction, and nothing went through the solver.
    let w = ed.doc.walls().next().unwrap().1.clone();
    for c in &ed.doc.path(w.centre).unwrap().curves {
        assert_eq!(ed.doc.get(*c).unwrap().style, LineKind::Construction);
    }
    assert_eq!(ed.doc.constraint_count(), 0);
    // One undo per segment.
    ed.undo();
    assert_eq!(faces(&ed), 4);
    assert!((poche_area(&mut ed) - 1000.0).abs() < 1e-6);
    ed.undo();
    assert_eq!(ed.doc.walls().count(), 0);
    assert_eq!(ed.doc.len(), 0);
}

#[test]
fn back_on_the_start_closes_a_ring() {
    let mut ed = editor();
    let mut tool = ToolKind::Wall.create();
    for at in [
        v(0.0, 0.0),
        v(100.0, 0.0),
        v(100.0, 60.0),
        v(0.0, 60.0),
        v(0.0, 0.0),
    ] {
        click(&mut ed, tool.as_mut(), at);
    }
    assert!(!tool.is_busy(), "closing finishes the run");
    let w = ed.doc.walls().next().unwrap().1.clone();
    assert!(ed.doc.path(w.centre).unwrap().closed);
    assert_eq!(faces(&ed), 8, "no caps");
    assert!((poche_area(&mut ed) - (110.0 * 70.0 - 90.0 * 50.0)).abs() < 1e-6);
}

#[test]
fn a_wall_started_on_another_makes_a_t_keeping_the_hosts_ids() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(200.0, 0.0)]);
    let host = ed.doc.walls().next().unwrap().0;
    let before = ed.doc.wall(host).unwrap().made.clone();
    // From the host's upper face, up.
    run(&mut ed, &[v(100.0, 5.0), v(100.0, 80.0)]);
    assert_eq!(ed.doc.walls().count(), 2);
    let branch = ed
        .doc
        .walls()
        .map(|(id, _)| id)
        .find(|w| *w != host)
        .unwrap();
    assert_eq!(ed.doc.wall(branch).unwrap().ends[0].tee, Some((host, 0)));
    let now = &ed.doc.wall(host).unwrap().made;
    for (k, id) in &before.curves {
        assert_eq!(now.curves.get(k), Some(id), "the host kept {k:?}");
    }
    assert_eq!(ed.doc.fills().count(), 1, "one poché for the T");
    assert!((poche_area(&mut ed) - (2000.0 + 75.0 * 10.0)).abs() < 1e-6);
    // Moving the host drags the branch's end along.
    let host_curve = ed.wall_curves(host)[0];
    ed.doc.begin();
    ed.doc.translate([host_curve], v(0.0, -20.0));
    ed.commit("Move");
    assert!((poche_area(&mut ed) - (2000.0 + 95.0 * 10.0)).abs() < 1e-6);
}

#[test]
fn a_wall_ending_on_anothers_end_shares_its_point() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0)]);
    run(&mut ed, &[v(100.0, 0.0), v(100.0, 100.0)]);
    assert_eq!(ed.doc.walls().count(), 2);
    assert_eq!(ed.doc.fills().count(), 1, "an L of two walls is one poché");
    assert!((poche_area(&mut ed) - (2000.0)).abs() < 1e-6);
}

#[test]
fn a_wall_ending_on_a_line_stops_against_it() {
    let mut ed = editor();
    ed.doc.begin();
    let a = ed.doc.add_point(v(-100.0, 50.0));
    let b = ed.doc.add_point(v(100.0, 50.0));
    let grid = ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    run(&mut ed, &[v(0.0, 0.0), v(0.0, 50.0)]);
    let w = ed.doc.walls().next().unwrap().1.clone();
    assert_eq!(w.ends[1].against, Some(grid));
    assert!((poche_area(&mut ed) - 500.0).abs() < 1e-6);
}

#[test]
fn arcs_and_bends() {
    // W again: an arc through a third point.
    let mut ed = editor();
    ed.prefs.wall_arc = true;
    let mut tool = ToolKind::Wall.create();
    for at in [v(50.0, 0.0), v(-50.0, 0.0), v(0.0, 50.0)] {
        click(&mut ed, tool.as_mut(), at);
    }
    tool.handle(Input::Finish, &mut ed);
    let half_ring = std::f64::consts::PI * (55.0f64.powi(2) - 45.0f64.powi(2)) / 2.0;
    assert!((poche_area(&mut ed) - half_ring).abs() < 1e-6);
    // A click-drag bends the next segment: arriving at (100, 0) going down.
    let mut ed = editor();
    let mut tool = ToolKind::Wall.create();
    click(&mut ed, tool.as_mut(), v(0.0, 0.0));
    let p = ptr(&ed, v(100.0, 0.0));
    tool.handle(Input::DragStart(p, Modifiers::default()), &mut ed);
    tool.handle(Input::Drag(ptr(&ed, v(100.0, -30.0))), &mut ed);
    tool.handle(
        Input::DragEnd(ptr(&ed, v(100.0, -30.0)), Modifiers::default()),
        &mut ed,
    );
    tool.handle(Input::Finish, &mut ed);
    let arcs = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Arc { .. }))
        .count();
    assert_eq!(arcs, 3, "the centreline and both faces are arcs");
}

#[test]
fn keys_flip_switch_and_take_back() {
    let mut ed = editor();
    let mut tool = ToolKind::Wall.create();
    click(&mut ed, tool.as_mut(), v(0.0, 0.0));
    click(&mut ed, tool.as_mut(), v(100.0, 0.0));
    click(&mut ed, tool.as_mut(), v(100.0, 50.0));
    let w = ed.doc.walls().next().unwrap().0;
    // Space flips the run (in its last undo step).
    ed.prefs.wall_justify = Justify::Left;
    tool.key(ToolKey::Space, None, &mut ed);
    assert_eq!(ed.doc.wall(w).unwrap().justify, Justify::Right);
    // W again: arcs; L: straight again.
    tool.key(ToolKey::Again, None, &mut ed);
    assert!(ed.prefs.wall_arc);
    tool.key(ToolKey::LineNext, None, &mut ed);
    assert!(!ed.prefs.wall_arc);
    // Backspace: the last segment goes.
    assert!(tool.key(ToolKey::Backspace, None, &mut ed));
    assert_eq!(ed.wall_curves(w).len(), 1);
    assert!(tool.is_busy());
    click(&mut ed, tool.as_mut(), v(100.0, -50.0));
    assert_eq!(ed.wall_curves(w).len(), 2);
    tool.handle(Input::Finish, &mut ed);
}

#[test]
fn picking_a_face_or_the_poche_picks_the_wall() {
    let mut ed = editor();
    ed.show_centrelines = false;
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 50.0)]);
    let w = ed.doc.walls().next().unwrap().0;
    let curves: std::collections::BTreeSet<EntityId> = ed.wall_curves(w).into_iter().collect();
    let mut select = SelectTool::default();
    // On the outer face.
    click(&mut ed, &mut select, v(50.0, -5.0));
    assert_eq!(ed.selection, curves);
    assert_eq!(ed.selected_walls().into_iter().collect::<Vec<_>>(), vec![w]);
    ed.selection.clear();
    // Inside the poché.
    click(&mut ed, &mut select, v(50.0, 1.0));
    assert_eq!(ed.selection, curves);
    // On a face, just inside the poché: the face and the poché pick the same wall, so a
    // click again there picks it again, and says nothing (PAPERCUTS 2026-11-11).
    ed.out.info = None;
    for _ in 0..2 {
        click(&mut ed, &mut select, v(50.0, -4.8));
        assert_eq!(ed.selection, curves);
        assert_eq!(ed.out.info.take(), None);
    }
    // Trim on a face is refused.
    let face = ed
        .doc
        .wall(w)
        .unwrap()
        .made
        .curves
        .values()
        .next()
        .copied()
        .unwrap();
    assert!(!ed.guard_owned(&[face], crate::walls::Op::Cut));
    assert!(
        ed.out
            .notice
            .as_deref()
            .unwrap_or("")
            .contains("follow the wall")
    );
}

#[test]
fn explode_leaves_plain_geometry() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 50.0)]);
    let w = ed.doc.walls().next().unwrap().0;
    assert!(ed.explode_walls(&[w]));
    assert_eq!(ed.doc.walls().count(), 0);
    let lines = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .count();
    assert_eq!(lines, 6, "faces and caps, no centreline");
    assert_eq!(
        ed.doc.fills().count(),
        1,
        "the poché stays, as a shape's fill"
    );
    assert!(matches!(
        ed.doc.fills().next().unwrap().1.kind,
        FillKind::Solid
    ));
}

#[test]
fn walls_from_curves_and_thickness() {
    let mut ed = editor();
    ed.doc.begin();
    let p: Vec<EntityId> = [v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0)]
        .iter()
        .map(|q| ed.doc.add_point(*q))
        .collect();
    let l1 = ed.doc.add_line(p[0], p[1]).unwrap();
    let l2 = ed.doc.add_line(p[1], p[2]).unwrap();
    ed.commit("Lines");
    let made = ed.walls_from_curves(&[l1, l2]).unwrap();
    assert_eq!(made.len(), 1);
    assert!((poche_area(&mut ed) - 2000.0).abs() < 1e-6);
    ed.set_wall_thickness(&made, "20").unwrap();
    assert!((poche_area(&mut ed) - 4000.0).abs() < 1e-6);
    // A thickness from a variable follows it.
    ed.add_variable("t", "30", drift_core::Kind::Length)
        .unwrap();
    ed.set_wall_thickness(&made, "t").unwrap();
    assert!((poche_area(&mut ed) - 6000.0).abs() < 1e-6);
    let t = ed.doc.variable_named("t").unwrap();
    ed.set_variable(t, "5").unwrap();
    assert!((poche_area(&mut ed) - 1000.0).abs() < 1e-6);
    // Not again: they are walls now.
    assert!(ed.walls_from_curves(&[l1]).is_err());
}

#[test]
fn deleting_a_wall_takes_what_it_made() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 50.0)]);
    let w = ed.doc.walls().next().unwrap().0;
    ed.selection = ed.wall_curves(w).into_iter().collect();
    ed.delete_selection();
    assert_eq!(ed.doc.walls().count(), 0);
    assert_eq!(ed.doc.len(), 0);
    assert_eq!(ed.doc.fills().count(), 0);
    ed.undo();
    assert_eq!(ed.doc.walls().count(), 1);
    assert_eq!(faces(&ed), 6);
}

/// Outlines and holes of the walls as they are now.
fn loops(ed: &Editor) -> (usize, usize) {
    let (inputs, _) = drift_geom::walls::regen::wall_inputs(&ed.doc);
    let g = drift_geom::walls::generate(&inputs, drift_geom::walls::regen::snap(&ed.doc));
    let outers = g.loops.iter().filter(|l| l.key.hole == 0).count();
    (outers, g.loops.len() - outers)
}

#[test]
fn a_click_in_the_starts_corner_closes_the_room() {
    let mut ed = editor();
    let mut tool = ToolKind::Wall.create();
    // The last click lands inside the first wall, a little along from its start.
    for at in [
        v(0.0, 0.0),
        v(100.0, 0.0),
        v(100.0, -60.0),
        v(8.0, -60.0),
        v(8.0, 1.0),
    ] {
        click(&mut ed, tool.as_mut(), at);
    }
    assert!(!tool.is_busy(), "joining finishes the run");
    assert_eq!(ed.doc.walls().count(), 1);
    assert_eq!(loops(&ed), (1, 1), "a ring: no stub, no notch");
    // The end went on the first wall's centreline where it was clicked: the wall stays square.
    let w = ed.doc.walls().next().unwrap().1.clone();
    let end = *ed
        .doc
        .defining_points(ed.doc.path(w.centre).unwrap().curves.clone())
        .iter()
        .find(|p| ed.doc.point(**p).unwrap().distance(v(8.0, 0.0)) < 1e-9)
        .expect("an end at (8, 0)");
    assert!(ed.doc.contains(end));
    // A ring round 92 × 60 (the first wall's stub before x = 8 taken back): outside
    // 102 × 70 less inside 82 × 50.
    assert!((poche_area(&mut ed) - (102.0 * 70.0 - 82.0 * 50.0)).abs() < 1e-6);
}

#[test]
fn a_click_next_to_the_start_outside_the_wall_closes_too() {
    let mut ed = editor();
    run(
        &mut ed,
        &[
            v(0.0, 0.0),
            v(100.0, 0.0),
            v(100.0, 60.0),
            v(0.0, 60.0),
            v(-3.0, -2.0),
        ],
    );
    assert_eq!(loops(&ed), (1, 1));
}

#[test]
fn a_run_can_tee_into_itself() {
    let mut ed = editor();
    run(
        &mut ed,
        &[
            v(0.0, 0.0),
            v(0.0, 100.0),
            v(50.0, 100.0),
            v(50.0, 50.0),
            v(1.0, 50.0),
        ],
    );
    assert_eq!(ed.doc.walls().count(), 1);
    assert_eq!(loops(&ed), (1, 1), "a P: one outline round a hole");
}

#[test]
fn join_walls_makes_an_l_or_a_t() {
    let mut ed = editor();
    // Two walls whose ends stop short of each other.
    run(&mut ed, &[v(0.0, 0.0), v(80.0, 0.0)]);
    run(&mut ed, &[v(100.0, 20.0), v(100.0, 100.0)]);
    assert_eq!(loops(&ed), (2, 0));
    let walls: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
    assert!(ed.join_walls(&walls));
    assert_eq!(loops(&ed), (1, 0), "an L");
    assert!((poche_area(&mut ed) - (100.0 + 100.0) * 10.0).abs() < 1e-6);
    // They share the corner now: moving it moves both.
    assert_eq!(ed.doc.walls().count(), 2);
    assert!(ed.history.undo(&mut ed.doc));
    ed.sync();
    assert_eq!(loops(&ed), (2, 0), "one undo step");

    // A wall pointing at another's middle runs on to it.
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(200.0, 0.0)]);
    run(&mut ed, &[v(100.0, 100.0), v(100.0, 30.0)]);
    let walls: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
    assert!(ed.join_walls(&walls));
    assert_eq!(loops(&ed), (1, 0), "a T");
    let branch = ed.doc.wall(walls[1]).unwrap();
    assert_eq!(branch.ends[1].tee, Some((walls[0], 0)));
    assert!((poche_area(&mut ed) - (2000.0 + 95.0 * 10.0)).abs() < 1e-6);
}

#[test]
fn unjoin_caps_the_ends_and_join_puts_them_back() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0)]);
    run(&mut ed, &[v(100.0, 0.0), v(100.0, 100.0)]);
    assert_eq!(loops(&ed), (1, 0));
    let walls: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
    assert!(ed.unjoin_walls(&walls));
    assert_eq!(loops(&ed), (2, 0), "capped, overlapping");
    assert!(!ed.unjoin_walls(&walls), "nothing joins now");
    assert!(ed.join_walls(&walls));
    assert_eq!(loops(&ed), (1, 0));
}

#[test]
fn dragging_an_end_into_a_wall_joins_it_as_it_goes() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(200.0, 0.0)]);
    run(&mut ed, &[v(100.0, 100.0), v(100.0, 40.0)]);
    assert_eq!(loops(&ed), (2, 0));
    let branch = ed.doc.walls().map(|(id, _)| id).nth(1).unwrap();
    let w = ed.doc.wall(branch).unwrap().clone();
    let end = *ed
        .doc
        .defining_points(ed.doc.path(w.centre).unwrap().curves.clone())
        .iter()
        .find(|p| ed.doc.point(**p).unwrap() == v(100.0, 40.0))
        .unwrap();
    ed.doc.begin();
    assert!(ed.drag(&[end], v(0.0, -20.0), &[]));
    assert_eq!(loops(&ed), (2, 0), "not there yet");
    assert!(ed.drag(&[end], v(0.0, -17.0), &[]));
    assert_eq!(loops(&ed), (1, 0), "inside the wall: a T, while dragging");
    ed.commit("Drag");
    assert_eq!(loops(&ed), (1, 0));
    assert_eq!(
        ed.doc.wall(branch).unwrap().ends[1].tee.map(|t| t.0),
        Some(ed.doc.walls().next().unwrap().0)
    );
    // Dragged back out, it lets go.
    ed.doc.begin();
    assert!(ed.drag(&[end], v(0.0, 40.0), &[]));
    ed.commit("Drag");
    assert_eq!(loops(&ed), (2, 0));
}

#[test]
fn the_preview_shows_the_join_before_the_click() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(200.0, 0.0)]);
    let mut tool = ToolKind::Wall.create();
    click(&mut ed, tool.as_mut(), v(100.0, 100.0));
    // Hovering inside the wall, below its centreline: the faces stop on its upper face.
    let p = ptr(&ed, v(100.0, -2.0));
    tool.handle(Input::Hover(p), &mut ed);
    let lowest = tool
        .editor_preview(&ed)
        .iter()
        .filter(|i| i.emphasis == Emphasis::Preview && i.kind != LineKind::Construction)
        .filter_map(|i| match i.shape {
            Shape::Line { a, b } => Some(a.y.min(b.y)),
            _ => None,
        })
        .fold(f64::INFINITY, f64::min);
    assert!((lowest - 5.0).abs() < 1e-9, "the faces end at y = {lowest}");
}

/// What the walls made, to compare two drawings: walls, face lines (curves the walls own),
/// curves nobody owns, fills, and the poché's area.
fn wall_summary(ed: &mut Editor) -> (usize, usize, usize, usize, i64) {
    let owners = ed.wall_owners();
    let curves: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect();
    let faces = curves.iter().filter(|id| owners.by_wall(**id)).count();
    let loose = curves
        .iter()
        .filter(|id| owners.get(**id).is_none())
        .count();
    let area = poche_area(ed);
    (
        ed.doc.walls().count(),
        faces,
        loose,
        ed.doc.fills().count(),
        area.round() as i64,
    )
}

/// One Trim drag through `pts`.
fn trim_drag(ed: &mut Editor, pts: &[DVec2]) {
    let mut tool = ToolKind::Trim.create();
    let m = Modifiers::default();
    tool.handle(Input::DragStart(Pointer::new(pts[0], TOL), m), ed);
    for at in &pts[1..] {
        tool.handle(Input::Drag(Pointer::new(*at, TOL)), ed);
    }
    let end = Pointer::new(*pts.last().unwrap(), TOL);
    tool.handle(Input::DragEnd(end, m), ed);
}

/// #338: a Trim drag across two opposite walls of a closed room trims the walls whole. Their
/// centrelines are cut (nothing else crosses them, so each crossed segment goes, corner to
/// corner) and the faces, poché and outline follow: what is left is what drawing the two
/// remaining walls from scratch makes, with no fill-less wall and no face lines left behind.
#[test]
fn a_trim_drag_across_walls_trims_them_whole() {
    let mut ed = editor();
    ed.show_centrelines = false;
    run(
        &mut ed,
        &[
            v(0.0, 0.0),
            v(100.0, 0.0),
            v(100.0, 60.0),
            v(0.0, 60.0),
            v(0.0, 0.0),
        ],
    );
    assert_eq!(ed.doc.walls().count(), 1);
    // Across the left wall and the right wall, in a few moves.
    trim_drag(
        &mut ed,
        &[
            v(-30.0, 30.0),
            v(-10.0, 31.0),
            v(20.0, 30.0),
            v(60.0, 30.0),
            v(130.0, 29.0),
        ],
    );
    let got = wall_summary(&mut ed);

    // The same two walls, drawn from scratch.
    let mut fresh = editor();
    run(&mut fresh, &[v(0.0, 0.0), v(100.0, 0.0)]);
    run(&mut fresh, &[v(100.0, 60.0), v(0.0, 60.0)]);
    let want = wall_summary(&mut fresh);
    assert_eq!(want, (2, 8, 0, 2, 2 * 100 * 10));
    assert_eq!(
        got, want,
        "(walls, face lines, loose curves, fills, poché area)"
    );
    // And a full rebuild of what is left changes nothing.
    let before = ed.doc.len();
    drift_geom::walls::regenerate(&mut ed.doc);
    assert_eq!(ed.doc.len(), before);
    assert_eq!(wall_summary(&mut ed), want);
    // One undo step puts the room back.
    ed.undo();
    assert_eq!(ed.doc.walls().count(), 1);
    assert_eq!(faces(&ed), 8);
}

/// A click-trim on the centreline of a closed room's side (with centrelines shown) opens the
/// room there: the wall is the U that is left, as drawn from scratch.
#[test]
fn a_trim_click_on_a_rooms_centreline_opens_the_room() {
    let mut ed = editor();
    run(
        &mut ed,
        &[
            v(0.0, 0.0),
            v(100.0, 0.0),
            v(100.0, 60.0),
            v(0.0, 60.0),
            v(0.0, 0.0),
        ],
    );
    let mut tool = ToolKind::Trim.create();
    click(&mut ed, tool.as_mut(), v(100.0, 30.0));
    let got = wall_summary(&mut ed);
    // The U, drawn from scratch.
    let mut u = editor();
    run(
        &mut u,
        &[v(100.0, 60.0), v(0.0, 60.0), v(0.0, 0.0), v(100.0, 0.0)],
    );
    assert_eq!(
        got,
        wall_summary(&mut u),
        "(walls, face lines, loose curves, fills, poché area)"
    );
    assert_eq!(got.0, 1);
}

/// A single click on a wall face with the Trim tool still says why it can't trim it.
#[test]
fn a_trim_click_on_a_wall_face_says_to_trim_the_centreline() {
    let mut ed = editor();
    ed.show_centrelines = false;
    run(&mut ed, &[v(0.0, 0.0), v(100.0, 0.0)]);
    let before = wall_summary(&mut ed);
    let mut tool = ToolKind::Trim.create();
    click(&mut ed, tool.as_mut(), v(50.0, -5.0));
    assert_eq!(wall_summary(&mut ed), before);
    assert_eq!(
        ed.out.notice.as_deref(),
        Some("Trim: Wall faces follow the wall: trim its centreline, or Explode it")
    );
}

/// A run's centreline points, start to end, as regeneration walks it.
fn run_points(ed: &Editor, w: WallId) -> Vec<DVec2> {
    let wall = ed.doc.wall(w).unwrap();
    let (_, points, _) = drift_geom::walls::regen::centreline(&ed.doc, wall).unwrap();
    points.iter().map(|p| ed.doc.point(*p).unwrap()).collect()
}

/// The wall whose run starts at `at`.
fn wall_starting_at(ed: &Editor, at: DVec2) -> WallId {
    ed.doc
        .walls()
        .map(|(id, _)| id)
        .find(|w| run_points(ed, *w)[0].distance(at) < 1e-6)
        .unwrap_or_else(|| panic!("a run starting at {at:?}"))
}

/// What is left is what the walls make again: a full rebuild changes nothing.
fn assert_rebuilt(ed: &mut Editor) {
    let before = (ed.doc.len(), wall_summary(ed));
    drift_geom::walls::regenerate(&mut ed.doc);
    assert_eq!((ed.doc.len(), wall_summary(ed)), before);
}

/// #338 review: a run cut in two by deleting a middle segment. The end teeing into a host
/// goes with the piece that ends there; the first piece keeps no end it no longer has.
#[test]
fn a_run_cut_in_two_keeps_each_end_with_its_piece() {
    let mut ed = editor();
    run(&mut ed, &[v(300.0, -50.0), v(300.0, 200.0)]);
    let host = ed.doc.walls().next().unwrap().0;
    run(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(295.0, 100.0)],
    );
    let w = ed
        .doc
        .walls()
        .map(|(id, _)| id)
        .find(|w| *w != host)
        .unwrap();
    assert_eq!(ed.doc.wall(w).unwrap().ends[1].tee.map(|t| t.0), Some(host));
    let end = *run_points(&ed, w).last().unwrap();
    let mut tool = ToolKind::Trim.create();
    click(&mut ed, tool.as_mut(), v(100.0, 50.0));
    assert_eq!(ed.doc.walls().count(), 3);
    let first = wall_starting_at(&ed, v(0.0, 0.0));
    assert_eq!(run_points(&ed, first), vec![v(0.0, 0.0), v(100.0, 0.0)]);
    let ends = &ed.doc.wall(first).unwrap().ends;
    assert_eq!(ends[1].tee, None, "the tee went with the other piece");
    let second = wall_starting_at(&ed, v(100.0, 100.0));
    assert_eq!(run_points(&ed, second), vec![v(100.0, 100.0), end]);
    assert_eq!(
        ed.doc.wall(second).unwrap().ends[1].tee.map(|t| t.0),
        Some(host)
    );
    assert_eq!(ed.doc.wall(second).unwrap().ends[0].tee, None);
    assert_rebuilt(&mut ed);
}

/// #338 review: a branch teeing into the piece a cut leaves on the far side stays where it
/// is, teeing into that piece's wall now.
#[test]
fn a_branch_follows_its_host_segment_to_the_new_piece() {
    let mut ed = editor();
    run(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(300.0, 100.0)],
    );
    let w = ed.doc.walls().next().unwrap().0;
    run(&mut ed, &[v(200.0, 105.0), v(200.0, 200.0)]);
    let branch = ed.doc.walls().map(|(id, _)| id).find(|b| *b != w).unwrap();
    assert_eq!(
        ed.doc.wall(branch).unwrap().ends[0].tee.map(|t| t.0),
        Some(w)
    );
    let before = run_points(&ed, branch);
    let mut tool = ToolKind::Trim.create();
    click(&mut ed, tool.as_mut(), v(100.0, 50.0));
    assert_eq!(run_points(&ed, branch), before, "the branch didn't move");
    let second = wall_starting_at(&ed, v(100.0, 100.0));
    assert_eq!(ed.doc.wall(branch).unwrap().ends[0].tee, Some((second, 0)));
    assert_rebuilt(&mut ed);
    // A branch whose host segment is deleted lets go where it is.
    let mut ed = editor();
    run(
        &mut ed,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0), v(300.0, 100.0)],
    );
    let w = ed.doc.walls().next().unwrap().0;
    run(&mut ed, &[v(105.0, 50.0), v(200.0, 50.0)]);
    let branch = ed.doc.walls().map(|(id, _)| id).find(|b| *b != w).unwrap();
    assert_eq!(
        ed.doc.wall(branch).unwrap().ends[0].tee.map(|t| t.0),
        Some(w)
    );
    let before = run_points(&ed, branch);
    let middle = ed.wall_curves(w)[1];
    ed.doc.begin();
    ed.doc.delete([middle]);
    ed.commit("Delete");
    assert_eq!(ed.doc.walls().count(), 3);
    assert_eq!(run_points(&ed, branch), before, "the branch didn't move");
    assert_eq!(ed.doc.wall(branch).unwrap().ends[0].tee, None);
    assert_rebuilt(&mut ed);
}

/// The highest point of the walls' faces.
fn top_of_faces(ed: &Editor) -> f64 {
    let owners = ed.wall_owners();
    ed.doc
        .iter()
        .filter(|(id, e)| !e.kind.is_point() && owners.by_wall(*id))
        .filter_map(|(id, _)| ed.doc.shape(id))
        .map(|s| s.bounds().max.y)
        .fold(f64::MIN, f64::max)
}

/// #338 review: a run whose first curve runs backwards (end to start) keeps its direction,
/// so a Left-justified wall stays on its side.
#[test]
fn a_run_starting_backwards_keeps_its_direction() {
    // From curves: the first line is drawn (100, 0) to (0, 0), but the run starts at (0, 50).
    let mut ed = editor();
    ed.prefs.wall_justify = Justify::Left;
    ed.doc.begin();
    let p: Vec<EntityId> = [v(100.0, 0.0), v(0.0, 0.0), v(100.0, 50.0), v(0.0, 50.0)]
        .iter()
        .map(|q| ed.doc.add_point(*q))
        .collect();
    let l1 = ed.doc.add_line(p[0], p[1]).unwrap();
    let l2 = ed.doc.add_line(p[0], p[2]).unwrap();
    let l3 = ed.doc.add_line(p[2], p[3]).unwrap();
    ed.commit("Lines");
    let w = ed.walls_from_curves(&[l1, l2, l3]).unwrap()[0];
    let points = run_points(&ed, w);
    assert_eq!(points[0], v(0.0, 50.0));
    // Left justified going (0, 50) → (100, 50): the wall is inside the U, below its top.
    assert!((top_of_faces(&ed) - 50.0).abs() < 1e-9);
    ed.set_wall_thickness(&[w], "20").unwrap();
    assert_eq!(run_points(&ed, w), points);
    assert!((top_of_faces(&ed) - 50.0).abs() < 1e-9);

    // The Wall tool: a clockwise arc (stored end to start), then a line.
    let mut ed = editor();
    ed.prefs.wall_justify = Justify::Left;
    ed.prefs.wall_arc = true;
    let mut tool = ToolKind::Wall.create();
    for at in [v(-50.0, 0.0), v(50.0, 0.0), v(0.0, 50.0)] {
        click(&mut ed, tool.as_mut(), at);
    }
    tool.key(ToolKey::LineNext, None, &mut ed);
    click(&mut ed, tool.as_mut(), v(50.0, -100.0));
    tool.handle(Input::Finish, &mut ed);
    let w = ed.doc.walls().next().unwrap().0;
    assert_eq!(run_points(&ed, w)[0], v(-50.0, 0.0));
    // Left justified along a clockwise arc: the wall is inside it.
    assert!((top_of_faces(&ed) - 50.0).abs() < 1e-9);
}

/// #338 re-review (R7): trimming away the end segment of a run that tees into a wall at each
/// end leaves the rest where it was: its new start is free, its far end keeps its tee.
#[test]
fn a_run_losing_its_end_segment_lets_that_tee_go() {
    let mut ed = editor();
    run(&mut ed, &[v(-50.0, -50.0), v(-50.0, 200.0)]);
    run(&mut ed, &[v(300.0, -50.0), v(300.0, 200.0)]);
    let hosts: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
    run(
        &mut ed,
        &[
            v(-45.0, 0.0),
            v(100.0, 0.0),
            v(100.0, 100.0),
            v(295.0, 100.0),
        ],
    );
    let w = ed
        .doc
        .walls()
        .map(|(id, _)| id)
        .find(|w| !hosts.contains(w))
        .unwrap();
    let ends = ed.doc.wall(w).unwrap().ends.clone();
    assert!(ends[0].tee.is_some() && ends[1].tee.is_some());
    let far = *run_points(&ed, w).last().unwrap();
    let mut tool = ToolKind::Trim.create();
    click(&mut ed, tool.as_mut(), v(30.0, 0.0));
    assert_eq!(
        run_points(&ed, w),
        vec![v(100.0, 0.0), v(100.0, 100.0), far]
    );
    let now = &ed.doc.wall(w).unwrap().ends;
    assert_eq!(now[0].tee, None, "the start's tee went with its segment");
    assert_eq!(now[1].tee.map(|t| t.0), ends[1].tee.map(|t| t.0));
    assert_rebuilt(&mut ed);
}

/// #338 re-review (R5): a trim that only shortens a run starting on a clockwise arc keeps the
/// run's direction, so a Left-justified wall stays on its side.
#[test]
fn shortening_a_run_starting_backwards_keeps_its_direction() {
    let mut ed = editor();
    ed.prefs.wall_justify = Justify::Left;
    ed.prefs.wall_arc = true;
    let mut tool = ToolKind::Wall.create();
    for at in [v(-50.0, 0.0), v(50.0, 0.0), v(0.0, 50.0)] {
        click(&mut ed, tool.as_mut(), at);
    }
    tool.key(ToolKey::LineNext, None, &mut ed);
    click(&mut ed, tool.as_mut(), v(50.0, -100.0));
    tool.handle(Input::Finish, &mut ed);
    let w = ed.doc.walls().next().unwrap().0;
    ed.doc.begin();
    let a = ed.doc.add_point(v(0.0, -60.0));
    let b = ed.doc.add_point(v(100.0, -60.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    let top = top_of_faces(&ed);
    let mut tool = ToolKind::Trim.create();
    click(&mut ed, tool.as_mut(), v(50.0, -80.0));
    let points = run_points(&ed, w);
    assert_eq!(points[0], v(-50.0, 0.0));
    assert!(points.last().unwrap().distance(v(50.0, -60.0)) < 1e-6);
    assert!(
        (top_of_faces(&ed) - top).abs() < 1e-9,
        "the faces kept their side"
    );
    assert_rebuilt(&mut ed);
}
