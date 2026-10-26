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
    ed.auto_relations = false;
    ed.wall_thickness = 10.0;
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
    ed.wall_arc = true;
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
    ed.wall_justify = Justify::Left;
    tool.key(ToolKey::Space, None, &mut ed);
    assert_eq!(ed.doc.wall(w).unwrap().justify, Justify::Right);
    // W again: arcs; L: straight again.
    tool.key(ToolKey::Again, None, &mut ed);
    assert!(ed.wall_arc);
    tool.key(ToolKey::LineNext, None, &mut ed);
    assert!(!ed.wall_arc);
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
        ed.notice
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

#[test]
fn walls_print_their_faces_not_their_centrelines_or_butts() {
    let mut ed = editor();
    run(&mut ed, &[v(0.0, 0.0), v(200.0, 0.0)]);
    // A thinner wall of another poché teed into it: it butts against the host's face.
    // (Another material: faces only, no poché.)
    ed.wall_thickness = 6.0;
    ed.wall_poche.on = false;
    run(&mut ed, &[v(100.0, 80.0), v(100.0, 5.0)]);
    assert_eq!(ed.doc.fills().count(), 1, "the poché of the host");
    let ctx = eframe::egui::Context::default();
    let painter = eframe::egui::Painter::new(
        ctx,
        eframe::egui::LayerId::background(),
        eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            eframe::egui::vec2(800.0, 800.0),
        ),
    );
    let d =
        crate::output::model_drawing(&mut ed, &crate::fills::PatternLibrary::default(), &painter);
    let lines = d
        .items
        .iter()
        .filter(|i| matches!(i.prim, drift_draft::page::Prim::Line(..)))
        .count();
    // The host's two faces and caps, the branch's two faces and its far cap: not the
    // centrelines (construction) nor the butt edge along the host's face.
    assert_eq!(lines, 7);
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
