//! The node rule and the outlines it makes (docs/walls-design.md §9.1).

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn close(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-7
}

/// A wall through `pts` (points numbered from `first`), `t` thick, centred.
fn wall(id: u32, pts: &[DVec2], ids: &[u32], closed: bool, t: f64) -> WallIn {
    let n = if closed { pts.len() } else { pts.len() - 1 };
    WallIn {
        id: WallId(id),
        segs: (0..n)
            .map(|k| Seg::Line {
                a: pts[k],
                b: pts[(k + 1) % pts.len()],
            })
            .collect(),
        points: ids.iter().map(|i| EntityId(*i)).collect(),
        closed,
        offsets: (t / 2.0, -t / 2.0),
        cluster: 0,
        cross: true,
        ends: [EndIn::default(), EndIn::default()],
    }
}

fn outers(g: &Generated) -> Vec<&LoopOut> {
    g.loops.iter().filter(|l| l.key.hole == 0).collect()
}

fn area(g: &Generated) -> f64 {
    g.regions().iter().map(|(_, _, r)| r.area).sum()
}

fn assert_closed_and_simple(g: &Generated) {
    for l in &g.loops {
        let lp = g.loop_of(l).expect("a loop");
        for (i, e) in lp.edges.iter().enumerate() {
            let next = &lp.edges[(i + 1) % lp.edges.len()];
            assert!(close(e.end(), next.start()), "{:?}", l.key);
        }
    }
}

#[test]
fn an_l_wall_is_mitred_at_its_corner() {
    let w = wall(
        1,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 50.0)],
        &[1, 2, 3],
        false,
        10.0,
    );
    let g = generate(&[w], 0.25);
    assert!(g.notices.is_empty(), "{:?}", g.notices);
    // One outline: the faces mitred at (95, 5) inside and (105, -5) outside, and two caps.
    assert_eq!(g.loops.len(), 1);
    assert_eq!(g.curves.len(), 6);
    let pts: Vec<DVec2> = g.points.values().map(|(_, p)| *p).collect();
    assert!(pts.iter().any(|p| close(*p, v(95.0, 5.0))));
    assert!(pts.iter().any(|p| close(*p, v(105.0, -5.0))));
    assert!((area(&g) - 150.0 * 10.0).abs() < 1e-6, "{}", area(&g));
    assert_closed_and_simple(&g);
}

#[test]
fn a_closed_room_has_inner_and_outer_rectangles() {
    let w = wall(
        1,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0), v(0.0, 60.0)],
        &[1, 2, 3, 4],
        true,
        10.0,
    );
    let g = generate(&[w], 0.25);
    assert_eq!(outers(&g).len(), 1);
    assert_eq!(g.loops.len(), 2, "an outer loop and a hole");
    assert_eq!(g.curves.len(), 8, "no caps");
    let r = &g.regions()[0].2;
    assert!((r.area - (110.0 * 70.0 - 90.0 * 50.0)).abs() < 1e-6);
    assert!(r.contains(v(0.0, 30.0)) && !r.contains(v(50.0, 30.0)));
    assert_closed_and_simple(&g);
}

#[test]
fn a_room_whose_corner_touches_the_outer_loop_is_still_a_hole() {
    // A room in a network (found by walls_props, CI on #268): a left-justified wall turns at
    // a node where another left-justified wall ends, its face running on through the node,
    // so the room's corner there lies on the outer loop. The room is still a hole.
    let left = drift_core::Justify::Left.offsets(100.0);
    let node = v(2052.104547860062, 1000.0);
    let mut w0 = wall(
        1,
        &[node, v(2011.967682647122, -38.09280575616747)],
        &[7, 6],
        false,
        100.0,
    );
    w0.offsets = left;
    let mut w1 = wall(
        2,
        &[v(0.0, 1000.0), v(1000.0, 1000.0), node, v(2000.0, 2000.0)],
        &[1, 4, 7, 8],
        false,
        100.0,
    );
    w1.offsets = left;
    let w2 = wall(
        3,
        &[v(0.0, 2000.0), v(1000.0, 2000.0), v(2000.0, 2000.0)],
        &[2, 5, 8],
        false,
        100.0,
    );
    let w3 = wall(
        4,
        &[v(1000.0, 1000.0), v(1000.0, 2000.0)],
        &[4, 5],
        false,
        100.0,
    );
    let g = generate(&[w0, w1, w2, w3], 0.25);
    assert!(g.notices.is_empty(), "{:?}", g.notices);
    let keys: Vec<LoopKey> = g.loops.iter().map(|l| l.key).collect();
    assert_eq!(
        keys,
        [LoopKey { outer: 0, hole: 0 }, LoopKey { outer: 0, hole: 1 }]
    );
    let hole = g.loop_of(&g.loops[1]).expect("a loop");
    assert!(
        hole.edges.iter().any(|e| close(e.start(), node)),
        "the corner on the outer loop"
    );
    let r = &g.regions()[0].2;
    assert!(!r.contains(v(1500.0, 1500.0)), "the room isn't poché");
    assert_closed_and_simple(&g);
}

#[test]
fn justification_puts_the_path_on_a_face() {
    let mut w = wall(1, &[v(0.0, 0.0), v(100.0, 0.0)], &[1, 2], false, 10.0);
    w.offsets = drift_core::Justify::Left.offsets(10.0);
    let g = generate(&[w], 0.25);
    let pts: Vec<DVec2> = g.points.values().map(|(_, p)| *p).collect();
    assert!(pts.iter().any(|p| close(*p, v(0.0, 0.0))));
    assert!(pts.iter().any(|p| close(*p, v(0.0, -10.0))));
    assert!((area(&g) - 1000.0).abs() < 1e-6);
}

#[test]
fn a_curved_wall_has_concentric_faces() {
    // A half circle of radius 50 round the origin, counter-clockwise from (50, 0).
    let seg = Seg::arc_through(v(50.0, 0.0), v(0.0, 50.0), v(-50.0, 0.0)).unwrap();
    let w = WallIn {
        segs: vec![seg],
        ..wall(1, &[v(50.0, 0.0), v(-50.0, 0.0)], &[1, 2], false, 10.0)
    };
    let g = generate(&[w], 0.25);
    let arcs: Vec<&Piece> = g.curves.values().filter(|p| p.arc.is_some()).collect();
    assert_eq!(arcs.len(), 2);
    let half_ring = PI * (55.0f64.powi(2) - 45.0f64.powi(2)) / 2.0;
    assert!((area(&g) - half_ring).abs() < 1e-6, "{}", area(&g));
    // Too tight: an error, not a panic.
    let w = WallIn {
        segs: vec![seg],
        ..wall(1, &[v(50.0, 0.0), v(-50.0, 0.0)], &[1, 2], false, 120.0)
    };
    let g = generate(&[w], 0.25);
    assert!(g.curves.is_empty() && !g.notices.is_empty());
}

#[test]
fn a_tangent_arc_runs_smoothly_into_a_line() {
    // Along x to (100, 0), then a half circle up and back.
    let arc = Seg::arc_through(v(100.0, 0.0), v(150.0, 50.0), v(100.0, 100.0)).unwrap();
    let mut w = wall(
        1,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 100.0)],
        &[1, 2, 3],
        false,
        10.0,
    );
    w.segs[1] = arc;
    let g = generate(&[w], 0.25);
    assert!(g.notices.is_empty(), "{:?}", g.notices);
    // The corners at the join are the offset points, with no step between.
    let pts: Vec<DVec2> = g.points.values().map(|(_, p)| *p).collect();
    assert!(pts.iter().any(|p| close(*p, v(100.0, 5.0))));
    assert!(pts.iter().any(|p| close(*p, v(100.0, -5.0))));
    let expected = 100.0 * 10.0 + PI * (55.0f64.powi(2) - 45.0f64.powi(2)) / 2.0;
    assert!((area(&g) - expected).abs() < 1e-6);
}

#[test]
fn a_wall_ending_on_another_makes_a_t() {
    let host = wall(1, &[v(0.0, 0.0), v(200.0, 0.0)], &[1, 2], false, 10.0);
    let mut branch = wall(2, &[v(100.0, 80.0), v(100.0, 0.0)], &[3, 4], false, 10.0);
    branch.ends[1].tee = Some((WallId(1), 0));
    let g = generate(&[host, branch], 0.25);
    assert!(g.notices.is_empty(), "{:?}", g.notices);
    assert_eq!(g.loops.len(), 1, "one T-shaped outline");
    assert!((area(&g) - (200.0 * 10.0 + 75.0 * 10.0)).abs() < 1e-6);
    // The host's near face is in two pieces, the second keyed by the branch.
    let host_left: Vec<&FaceKey> = g
        .curves
        .keys()
        .filter(|(w, k)| *w == WallId(1) && k.part == Part::Left)
        .map(|(_, k)| k)
        .collect();
    assert_eq!(host_left.len(), 2);
    assert!(
        host_left
            .iter()
            .any(|k| matches!(k.from, Station::Tee(WallId(2), _)))
    );
    let host_right = g
        .curves
        .keys()
        .filter(|(w, k)| *w == WallId(1) && k.part == Part::Right)
        .count();
    assert_eq!(host_right, 1, "the far face runs straight through");
    assert_closed_and_simple(&g);
}

#[test]
fn crossing_walls_make_an_x() {
    let a = wall(1, &[v(0.0, 0.0), v(200.0, 0.0)], &[1, 2], false, 10.0);
    let b = wall(
        2,
        &[v(100.0, -100.0), v(100.0, 100.0)],
        &[3, 4],
        false,
        10.0,
    );
    let g = generate(&[a.clone(), b.clone()], 0.25);
    assert_eq!(g.loops.len(), 1);
    assert!((area(&g) - (200.0 * 10.0 + 200.0 * 10.0 - 100.0)).abs() < 1e-6);
    // Each face is in two pieces.
    assert_eq!(g.curves.values().filter(|p| p.stroke).count(), 8 + 4);
    // Not when one of them doesn't join walls it crosses: two separate outlines.
    let mut b2 = b;
    b2.cross = false;
    let g = generate(&[a, b2], 0.25);
    assert_eq!(g.loops.len(), 2);
}

#[test]
fn y_and_star_nodes_have_a_corner_between_each_pair() {
    for n in [3usize, 5] {
        let mut walls = Vec::new();
        for k in 0..n {
            let a = TAU * k as f64 / n as f64 + 0.1;
            walls.push(wall(
                k as u32 + 1,
                &[v(0.0, 0.0), v(100.0 * a.cos(), 100.0 * a.sin())],
                &[1, 10 + k as u32],
                false,
                10.0,
            ));
        }
        let g = generate(&walls, 0.25);
        assert!(g.notices.is_empty(), "{:?}", g.notices);
        assert_eq!(g.loops.len(), 1);
        let at_node = g
            .points
            .keys()
            .filter(|k| matches!(k, CornerKey::Pair(..)))
            .count();
        assert_eq!(at_node, n);
        assert_closed_and_simple(&g);
    }
}

#[test]
fn straight_on_walls_meet_on_the_normal_or_step() {
    // Equal thickness: the faces line up, no step.
    let a = wall(1, &[v(0.0, 0.0), v(100.0, 0.0)], &[1, 2], false, 10.0);
    let b = wall(2, &[v(100.0, 0.0), v(200.0, 0.0)], &[2, 3], false, 10.0);
    let g = generate(&[a.clone(), b], 0.25);
    assert!(
        g.curves
            .keys()
            .all(|(_, k)| !matches!(k.part, Part::Step(_)))
    );
    assert!((area(&g) - 2000.0).abs() < 1e-6);
    // A thicker one: a step on each side.
    let b = wall(2, &[v(100.0, 0.0), v(200.0, 0.0)], &[2, 3], false, 20.0);
    let g = generate(&[a.clone(), b], 0.25);
    let steps = g
        .curves
        .keys()
        .filter(|(_, k)| matches!(k.part, Part::Step(_)))
        .count();
    assert_eq!(steps, 2);
    assert!((area(&g) - (1000.0 + 2000.0)).abs() < 1e-6);
    // Just off straight (1e-7 rad): still straight on.
    let tiny: f64 = 1e-7;
    let b = wall(
        2,
        &[
            v(100.0, 0.0),
            v(100.0 + 100.0 * tiny.cos(), 100.0 * tiny.sin()),
        ],
        &[2, 3],
        false,
        10.0,
    );
    let g = generate(&[a, b], 0.25);
    assert!(g.notices.is_empty() && g.loops.len() == 1);
}

#[test]
fn a_sharp_fork_is_bevelled_at_the_mitre_limit() {
    // Two walls leaving one point 1° apart.
    let a = wall(1, &[v(0.0, 0.0), v(1000.0, 0.0)], &[1, 2], false, 10.0);
    let d = 1f64.to_radians();
    let b = wall(
        2,
        &[v(0.0, 0.0), v(1000.0 * d.cos(), 1000.0 * d.sin())],
        &[1, 3],
        false,
        10.0,
    );
    let g = generate(&[a, b], 0.25);
    assert!(
        g.curves
            .keys()
            .any(|(_, k)| matches!(k.part, Part::Bevel(_)))
    );
    for (_, p) in g.points.values() {
        assert!(
            p.length() <= 1000.0 + 20.0,
            "no corner runs off into the distance: {p}"
        );
    }
}

#[test]
fn a_segment_shorter_than_its_thickness_is_absorbed() {
    // A 4 mm jog in a 20 mm wall.
    let w = wall(
        1,
        &[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 4.0), v(200.0, 4.0)],
        &[1, 2, 3, 4],
        false,
        20.0,
    );
    let g = generate(&[w], 0.25);
    assert_eq!(g.loops.len(), 1, "{:?}", g.notices);
    assert_closed_and_simple(&g);
    let lp = g.loop_of(&g.loops[0]).unwrap();
    assert!(lp.signed_area() > 0.0);
}

#[test]
fn different_materials_butt_against_the_thicker_wall() {
    let host = wall(1, &[v(0.0, 0.0), v(200.0, 0.0)], &[1, 2], false, 20.0);
    let mut branch = wall(2, &[v(100.0, 80.0), v(100.0, 0.0)], &[3, 4], false, 10.0);
    branch.ends[1].tee = Some((WallId(1), 0));
    branch.cluster = 1;
    let g = generate(&[host, branch], 0.25);
    assert_eq!(g.loops.len(), 2, "one outline per material");
    // The host's faces are whole; the branch ends on its near face, unstroked.
    assert_eq!(
        g.curves
            .keys()
            .filter(|(w, k)| *w == WallId(1) && matches!(k.part, Part::Left | Part::Right))
            .count(),
        2
    );
    let butt = g
        .curves
        .iter()
        .find(|((_, k), _)| matches!(k.part, Part::Butt(_)))
        .expect("a butt edge");
    assert!(!butt.1.stroke);
    assert!(
        (area(&g) - (4000.0 + 70.0 * 10.0)).abs() < 1e-6,
        "{}",
        area(&g)
    );
    // Two materials at an L, neither running through: a seam, drawn once.
    let a = wall(1, &[v(0.0, 0.0), v(100.0, 0.0)], &[1, 2], false, 10.0);
    let mut b = wall(2, &[v(100.0, 0.0), v(100.0, 100.0)], &[2, 3], false, 10.0);
    b.cluster = 1;
    let g = generate(&[a, b], 0.25);
    let seams: Vec<&Piece> = g
        .curves
        .iter()
        .filter(|((_, k), _)| matches!(k.part, Part::Seam(..)))
        .map(|(_, p)| p)
        .collect();
    assert_eq!(seams.len(), 2);
    assert_eq!(seams.iter().filter(|p| p.stroke).count(), 1);
    assert_eq!(g.loops.len(), 2);
}

#[test]
fn a_wall_runs_straight_through_an_n_way_node() {
    // A wall through a point another wall ends at, both centred.
    let a = wall(
        1,
        &[v(0.0, 0.0), v(100.0, 0.0), v(200.0, 0.0)],
        &[1, 2, 3],
        false,
        10.0,
    );
    let b = wall(2, &[v(100.0, 0.0), v(100.0, 100.0)], &[2, 4], false, 10.0);
    let g = generate(&[a, b], 0.25);
    assert_eq!(g.loops.len(), 1);
    assert!((area(&g) - (2000.0 + 950.0)).abs() < 1e-6);
}

#[test]
fn a_wall_can_end_against_a_curve() {
    let mut w = wall(1, &[v(0.0, 0.0), v(0.0, 47.0)], &[1, 2], false, 10.0);
    w.ends[1].against = Some(Shape::Line {
        a: v(-100.0, 50.0),
        b: v(100.0, 50.0),
    });
    let g = generate(&[w], 0.25);
    let pts: Vec<DVec2> = g.points.values().map(|(_, p)| *p).collect();
    assert!(pts.iter().any(|p| close(*p, v(-5.0, 50.0))));
    assert!(pts.iter().any(|p| close(*p, v(5.0, 50.0))));
    assert!((area(&g) - 500.0).abs() < 1e-6);
}

#[test]
fn joins_are_symmetric_and_order_independent() {
    let a = wall(1, &[v(0.0, 0.0), v(100.0, 0.0)], &[1, 2], false, 10.0);
    let b = wall(2, &[v(100.0, 0.0), v(130.0, 70.0)], &[2, 3], false, 16.0);
    let g1 = generate(&[a.clone(), b.clone()], 0.25);
    let g2 = generate(&[b, a], 0.25);
    let mut p1: Vec<DVec2> = g1.points.values().map(|(_, p)| *p).collect();
    let mut p2: Vec<DVec2> = g2.points.values().map(|(_, p)| *p).collect();
    p1.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    p2.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    assert_eq!(p1, p2);
    assert_eq!(g1.curves, g2.curves);
}

// ---------------------------------------------------------------------------------------------
// Writing into a document

mod regen {
    use super::v;
    use crate::walls::regenerate;
    use drift_core::{Document, EntityId, History, LayerId, LineKind, Path, Wall, WallId};

    /// A wall along `pts` (a ring if `closed`), 10 thick, regenerated.
    pub(super) fn drawn(pts: &[drift_core::DVec2], closed: bool) -> (Document, WallId) {
        let mut doc = Document::new();
        let ids: Vec<EntityId> = pts.iter().map(|p| doc.add_point(*p)).collect();
        let n = if closed { ids.len() } else { ids.len() - 1 };
        let lines: Vec<EntityId> = (0..n)
            .map(|k| doc.add_line(ids[k], ids[(k + 1) % ids.len()]).unwrap())
            .collect();
        for l in &lines {
            doc.set_style(*l, LineKind::Construction);
        }
        let path = doc
            .add_path(Path {
                curves: lines,
                closed,
            })
            .unwrap();
        let w = doc
            .add_wall(Wall::new(path, 10.0, LayerId::DEFAULT))
            .unwrap();
        regenerate(&mut doc);
        (doc, w)
    }

    #[test]
    fn regenerating_writes_faces_once_and_is_idempotent() {
        let (mut doc, w) = drawn(&[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0)], false);
        let wall = doc.wall(w).unwrap().clone();
        assert_eq!(wall.made.curves.len(), 6);
        assert_eq!(wall.made.fills.len(), 1);
        let before = doc.clone();
        let again = regenerate(&mut doc);
        assert!(!again.changed, "{again:?}");
        assert_eq!(doc, before);
        // The poché is the wall's area.
        let f = doc
            .fill(wall.made.fills.values().copied().next().unwrap())
            .unwrap();
        assert!(f.path.is_some());
        assert_eq!(f.kind, drift_core::FillKind::Solid);
    }

    #[test]
    fn moving_a_corner_keeps_every_id_and_undo_restores() {
        let (mut doc, w) = drawn(&[v(0.0, 0.0), v(100.0, 0.0), v(100.0, 60.0)], false);
        let mut history = History::default();
        let before = doc.clone();
        let made = doc.wall(w).unwrap().made.clone();
        let corner = doc.path(doc.wall(w).unwrap().centre).unwrap().curves[0];
        let b = match doc.get(corner).unwrap().kind {
            drift_core::EntityKind::Line { b, .. } => b,
            _ => unreachable!(),
        };
        doc.begin();
        doc.set_point(b, v(120.0, 10.0)).unwrap();
        let r = regenerate(&mut doc);
        assert!(r.changed && r.created == 0 && r.deleted == 0, "{r:?}");
        history.push(doc.commit("Move"));
        assert_eq!(doc.wall(w).unwrap().made, made, "same keys, same ids");
        history.undo(&mut doc);
        assert_eq!(doc, before);
    }

    #[test]
    fn a_move_regenerates_only_round_it_and_matches_a_full_rebuild() {
        use crate::walls::regenerate_moved;
        // A T and an L: the branch's far end moves.
        let (mut doc, host) = drawn(&[v(0.0, 0.0), v(200.0, 0.0), v(200.0, 100.0)], false);
        let a = doc.add_point(v(100.0, 0.0));
        let b = doc.add_point(v(100.0, 80.0));
        let l = doc.add_line(b, a).unwrap();
        let path = doc
            .add_path(Path {
                curves: vec![l],
                closed: false,
            })
            .unwrap();
        let mut branch = Wall::new(path, 10.0, LayerId::DEFAULT);
        branch.ends[1].tee = Some((host, 0));
        let bw = doc.add_wall(branch).unwrap();
        regenerate(&mut doc);
        let made = doc.wall(host).unwrap().made.clone();
        doc.begin();
        doc.set_point(b, v(130.0, 90.0)).unwrap();
        let r = regenerate_moved(&mut doc, &[(bw, 0)].into()).expect("the joins stayed");
        assert!(r.changed);
        assert_eq!(doc.wall(host).unwrap().made, made);
        let mut full = doc.clone();
        assert!(!regenerate(&mut full).changed, "the same as a full rebuild");
        // Moving the T's foot along the host also stays a move.
        doc.set_point(a, v(60.0, 0.0)).unwrap();
        regenerate_moved(&mut doc, &[(bw, 0), (host, 0)].into()).expect("the joins stayed");
        let mut full = doc.clone();
        assert!(!regenerate(&mut full).changed);
        doc.commit("Move");
        // A move that changes a join (the branch pulled off its host's end) isn't one.
        doc.begin();
        doc.set_point(a, v(260.0, 0.0)).unwrap();
        assert!(regenerate_moved(&mut doc, &[(bw, 0)].into()).is_none());
        doc.rollback();
    }

    #[test]
    fn a_t_keeps_the_hosts_ids_and_deleting_the_branch_heals_it() {
        let (mut doc, host) = drawn(&[v(0.0, 0.0), v(200.0, 0.0)], false);
        let host_made = doc.wall(host).unwrap().made.clone();
        // A branch up from the host's centreline.
        let a = doc.add_point(v(100.0, 0.0));
        let b = doc.add_point(v(100.0, 80.0));
        let l = doc.add_line(b, a).unwrap();
        let path = doc
            .add_path(Path {
                curves: vec![l],
                closed: false,
            })
            .unwrap();
        let mut branch = Wall::new(path, 10.0, LayerId::DEFAULT);
        branch.ends[1].tee = Some((host, 0));
        let bw = doc.add_wall(branch).unwrap();
        regenerate(&mut doc);
        let now = &doc.wall(host).unwrap().made;
        // The pieces that were there keep their ids; the near face gained one after the T.
        for (k, id) in &host_made.curves {
            assert_eq!(now.curves.get(k), Some(id), "{k:?}");
        }
        assert_eq!(now.curves.len(), host_made.curves.len() + 1);
        assert_eq!(doc.fills().count(), 1, "one poché for the T");
        // Deleting the branch's centreline deletes the branch and heals the host.
        doc.delete([l]);
        assert!(doc.wall(bw).is_none());
        regenerate(&mut doc);
        assert_eq!(doc.wall(host).unwrap().made.curves.len(), 4);
        assert_eq!(doc.fills().count(), 1);
    }
}

#[test]
fn an_arc_through_three_points_holds_far_from_the_origin() {
    // A 2 m radius bow 40 km out: the old absolute cut-off called it a line.
    let far = DVec2::new(4.0e7, -3.0e7);
    let (a, m, b) = (
        far + DVec2::new(-2000.0, 0.0),
        far + DVec2::new(0.0, 2000.0),
        far + DVec2::new(2000.0, 0.0),
    );
    let Some(Seg::Arc {
        center,
        radius,
        ccw,
        ..
    }) = Seg::arc_through(a, m, b)
    else {
        panic!("an arc");
    };
    assert!(center.distance(far) < 1e-6, "{center}");
    assert!((radius - 2000.0).abs() < 1e-6, "{radius}");
    assert!(!ccw);
    // Three points in a line there are still not an arc.
    assert!(Seg::arc_through(a, far, b).is_none());
}
