//! The active element in relations added between picked elements (#333): the one picked
//! last stays exactly where it is when the relation can be met that way; otherwise the
//! relation solves exactly as it does without an active element.

use drift_core::{Constraint, DVec2};

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn line(ed: &mut Editor, a: DVec2, b: DVec2) -> (EntityId, EntityId, EntityId) {
    let pa = ed.doc.add_point(a);
    let pb = ed.doc.add_point(b);
    (ed.doc.add_line(pa, pb).unwrap(), pa, pb)
}

fn at(ed: &Editor, p: EntityId) -> DVec2 {
    ed.doc.point(p).unwrap()
}

/// Every point of the drawing, by id.
fn points(ed: &Editor) -> Vec<(EntityId, DVec2)> {
    ed.doc
        .iter()
        .filter(|(_, e)| e.kind.is_point())
        .map(|(id, _)| (id, at(ed, id)))
        .collect()
}

fn direction(ed: &Editor, a: EntityId, b: EntityId) -> DVec2 {
    (at(ed, b) - at(ed, a)).normalize()
}

/// Pick `ids` in order (the last is the active element, if `active`) and add the relation
/// labelled `label` as the app does.
fn relate(ed: &mut Editor, ids: &[EntityId], active: bool, label: &str) -> Result<(), EditError> {
    ed.selection = ids.iter().copied().collect();
    ed.set_active(if active { ids.last().copied() } else { None });
    let option = ed
        .relation_options()
        .into_iter()
        .find(|o| o.label == label)
        .unwrap_or_else(|| panic!("{label} is offered"));
    ed.add_picked_constraints(option.label, &option.constraints)
        .map(|_| ())
}

/// `a` and `b` alike to within one unit of floating-point rounding: for a point merged with
/// two or more others (a three-way Coincident, level or plumb points), which the confirming
/// solve and every commit's rebuild from the mean of its members, as every commit does today.
/// (Follow-up draft: a merged group whose members are all equal starts from that value, not
/// their mean; then this returns to `==`.)
fn same_to_rounding(a: DVec2, b: DVec2) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() <= f64::EPSILON * x.abs().max(1.0);
    near(a.x, b.x) && near(a.y, b.y)
}

/// The active element was kept, and the solve the relation was checked with took no step.
fn kept(ed: &Editor) -> bool {
    ed.relation_kept == Some((true, 0))
}

/// Two free lines apart: (editor, a and its ends, b and its ends).
fn two_lines() -> (Editor, [EntityId; 3], [EntityId; 3]) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = line(&mut ed, v(0.0, 0.0), v(10.0, 1.0));
    let b = line(&mut ed, v(20.0, 0.0), v(23.0, 10.0));
    ed.commit("Lines");
    (ed, [a.0, a.1, a.2], [b.0, b.1, b.2])
}

fn fix(ed: &mut Editor, ps: &[EntityId]) {
    let fixes: Vec<Constraint> = ps
        .iter()
        .map(|&p| Constraint::Fix {
            point: p,
            at: at(ed, p),
        })
        .collect();
    ed.add_constraints("Fix", &fixes).unwrap();
}

#[test]
fn perpendicular_turns_the_other_line_to_the_active_one() {
    for b_active in [true, false] {
        let (mut ed, a, b) = two_lines();
        let (keep, turn) = if b_active { (b, a) } else { (a, b) };
        let kept_at = [at(&ed, keep[1]), at(&ed, keep[2])];
        let turned = [at(&ed, turn[1]), at(&ed, turn[2])];
        relate(&mut ed, &[turn[0], keep[0]], true, "Perpendicular").unwrap();
        assert!(kept(&ed));
        assert_eq!([at(&ed, keep[1]), at(&ed, keep[2])], kept_at);
        assert_ne!([at(&ed, turn[1]), at(&ed, turn[2])], turned);
        let dot = direction(&ed, a[1], a[2]).dot(direction(&ed, b[1], b[2]));
        assert!(dot.abs() < 1e-9, "perpendicular: {dot}");
    }
}

#[test]
fn without_an_active_element_both_lines_turn_as_before() {
    let (mut ed, a, b) = two_lines();
    let start = points(&ed);
    relate(&mut ed, &[a[0], b[0]], false, "Perpendicular").unwrap();
    assert_eq!(ed.relation_kept.map(|k| k.0), Some(false));
    // Both move (the even split).
    let moved = |p| at(&ed, p) != start.iter().find(|(q, _)| *q == p).unwrap().1;
    assert!((moved(a[1]) || moved(a[2])) && (moved(b[1]) || moved(b[2])));
    // And it is exactly what add_constraints does.
    let (mut plain, a, b) = two_lines();
    plain
        .add_constraints(
            "Perpendicular",
            &[Constraint::Perpendicular { a: a[0], b: b[0] }],
        )
        .unwrap();
    assert_eq!(points(&ed), points(&plain));
}

#[test]
fn a_corner_stays_and_the_other_line_turns_about_it() {
    // An L whose corner is shared: the corner is one of the active line's points, so it
    // stays too, and the other line turns about it.
    let mut ed = Editor::default();
    ed.doc.begin();
    let corner = ed.doc.add_point(v(0.0, 0.0));
    let far_a = ed.doc.add_point(v(10.0, 2.0));
    let far_b = ed.doc.add_point(v(1.0, 10.0));
    let a = ed.doc.add_line(corner, far_a).unwrap();
    let b = ed.doc.add_line(corner, far_b).unwrap();
    ed.commit("L");
    let (c0, fa0, fb0) = (at(&ed, corner), at(&ed, far_a), at(&ed, far_b));
    let dir = direction(&ed, corner, far_a);
    relate(&mut ed, &[b, a], true, "Perpendicular").unwrap();
    assert!(kept(&ed));
    assert_eq!((at(&ed, corner), at(&ed, far_a)), (c0, fa0));
    assert_eq!(direction(&ed, corner, far_a), dir);
    assert_ne!(at(&ed, far_b), fb0);
    assert!(
        direction(&ed, corner, far_a)
            .dot(direction(&ed, corner, far_b))
            .abs()
            < 1e-9
    );
}

#[test]
fn coincident_points_come_to_the_active_one() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let l1 = line(&mut ed, v(16.686, 1.6499999999999986), v(6.0, 0.0));
    let l2 = line(&mut ed, v(19.182000000000002, -27.504), v(30.0, -20.0));
    // (Values whose mean of three doesn't round back: the active point keeps its own.)
    let l3 = line(
        &mut ed,
        v(-10.842000000000002, -3.0420000000000016),
        v(-20.0, 5.0),
    );
    ed.commit("Lines");
    let target = at(&ed, l3.1);
    relate(&mut ed, &[l1.1, l2.1, l3.1], true, "Coincident").unwrap();
    assert!(kept(&ed));
    // (Three points merged: to within rounding, see `same_to_rounding`.)
    for p in [l1.1, l2.1, l3.1] {
        assert!(same_to_rounding(at(&ed, p), target), "{:?}", at(&ed, p));
        assert_eq!(at(&ed, p), at(&ed, l3.1), "one point");
    }
}

#[test]
fn points_merged_onto_the_active_one_keep_its_coordinates() {
    // Three free points merged onto the last (plumb, then coincident): the active point is
    // kept to within rounding, as the mean of three equal coordinates can round
    // (`same_to_rounding`).
    for label in ["Vertical", "Coincident"] {
        let (pa, pb, pc) = if label == "Vertical" {
            (
                v(23.586, 21.102000000000004),
                v(15.353999999999996, 25.194000000000003),
                v(15.353999999999996, -9.288),
            )
        } else {
            (
                v(16.686, 1.6499999999999986),
                v(19.182000000000002, -27.504),
                v(-10.842000000000002, -3.0420000000000016),
            )
        };
        let mut ed = Editor::default();
        ed.doc.begin();
        let [a, b, c] = [pa, pb, pc].map(|p| ed.doc.add_point(p));
        ed.commit("Points");
        relate(&mut ed, &[a, b, c], true, label).unwrap();
        assert!(kept(&ed), "{label}");
        assert!(
            same_to_rounding(at(&ed, c), pc),
            "{label}: {:?}",
            at(&ed, c)
        );
    }
}

#[test]
fn a_fixed_child_leaves_the_active_element_to_move() {
    let (mut ed, a, b) = two_lines();
    fix(&mut ed, &[a[1], a[2]]);
    let held = at(&ed, a[1]);
    relate(&mut ed, &[a[0], b[0]], true, "Perpendicular").unwrap();
    assert_eq!(ed.relation_kept.map(|k| k.0), Some(false));
    assert_eq!(at(&ed, a[1]), held);
    let dot = direction(&ed, a[1], a[2]).dot(direction(&ed, b[1], b[2]));
    assert!(dot.abs() < 1e-9);
    // As it does with no active element.
    let after = points(&ed);
    ed.undo();
    relate(&mut ed, &[a[0], b[0]], false, "Perpendicular").unwrap();
    assert_eq!(points(&ed), after);
}

#[test]
fn both_fixed_conflict_as_before() {
    let (mut ed, a, b) = two_lines();
    fix(&mut ed, &[a[1], a[2], b[1], b[2]]);
    let start = points(&ed);
    for active in [true, false] {
        let r = relate(&mut ed, &[a[0], b[0]], active, "Perpendicular");
        assert!(matches!(r, Err(EditError::Conflict)), "{r:?}");
        assert_eq!(points(&ed), start);
    }
}

#[test]
fn a_relation_that_adds_nothing_is_redundant_as_before() {
    // Two level lines: Parallel holds already.
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = line(&mut ed, v(0.0, 0.0), v(10.0, 0.0));
    let b = line(&mut ed, v(0.0, 5.0), v(7.0, 5.0));
    ed.commit("Lines");
    for l in [a.0, b.0] {
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: l }])
            .unwrap();
    }
    let start = points(&ed);
    for active in [true, false] {
        ed.selection = [a.0, b.0].into();
        ed.set_active(active.then_some(b.0));
        let r = ed.add_picked_constraints("Parallel", &[Constraint::Parallel { a: a.0, b: b.0 }]);
        assert!(matches!(r, Err(EditError::Redundant)), "{r:?}");
        assert_eq!(points(&ed), start);
    }
}

#[test]
fn a_child_held_at_one_end_pivots_and_the_active_stays() {
    let (mut ed, a, b) = two_lines();
    fix(&mut ed, &[a[1]]);
    let held = at(&ed, a[1]);
    let kept_at = [at(&ed, b[1]), at(&ed, b[2])];
    relate(&mut ed, &[a[0], b[0]], true, "Perpendicular").unwrap();
    assert!(kept(&ed));
    assert_eq!(at(&ed, a[1]), held);
    assert_eq!([at(&ed, b[1]), at(&ed, b[2])], kept_at);
    let dot = direction(&ed, a[1], a[2]).dot(direction(&ed, b[1], b[2]));
    assert!(dot.abs() < 1e-9);
}

#[test]
fn a_chain_beyond_the_child_follows_and_the_active_stays() {
    // a's far end is shared with c, which goes on to a free end: a turns, c follows.
    let (mut ed, a, b) = two_lines();
    ed.doc.begin();
    let end = ed.doc.add_point(v(10.0, -8.0));
    ed.doc.add_line(a[2], end).unwrap();
    ed.commit("Line");
    let kept_at = [at(&ed, b[1]), at(&ed, b[2])];
    relate(&mut ed, &[a[0], b[0]], true, "Perpendicular").unwrap();
    assert!(kept(&ed));
    assert_eq!([at(&ed, b[1]), at(&ed, b[2])], kept_at);
    let dot = direction(&ed, a[1], a[2]).dot(direction(&ed, b[1], b[2]));
    assert!(dot.abs() < 1e-9);
}

#[test]
fn equal_lines_take_the_active_ones_length() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let l1 = line(&mut ed, v(0.0, 0.0), v(4.0, 0.0));
    let l2 = line(&mut ed, v(0.0, 5.0), v(7.0, 5.0));
    let l3 = line(&mut ed, v(0.0, 10.0), v(10.0, 10.0));
    ed.commit("Lines");
    let kept_at = [at(&ed, l3.1), at(&ed, l3.2)];
    relate(&mut ed, &[l1.0, l2.0, l3.0], true, "Equal").unwrap();
    assert!(kept(&ed));
    assert_eq!([at(&ed, l3.1), at(&ed, l3.2)], kept_at);
    for l in [l1, l2] {
        let len = at(&ed, l.1).distance(at(&ed, l.2));
        assert!((len - 10.0).abs() < 1e-9, "{len}");
    }
}

#[test]
fn a_dimension_between_picks_moves_nothing() {
    let (mut ed, a, b) = two_lines();
    let start = points(&ed);
    ed.selection = [a[1], b[1]].into();
    ed.set_active(Some(b[1]));
    let option = ed.dimension_options().remove(0);
    ed.add_picked_constraints(option.label, &option.constraints)
        .unwrap();
    assert_eq!(points(&ed), start);
}

/// Every point of a drawing, and whether the last relation kept the active element.
type Solved = (Vec<(EntityId, DVec2)>, Option<(bool, usize)>);

/// Add `label` between (a, b) of a fresh drawing made by `build`, with b active or not:
/// every point after, and whether it was kept.
fn solved(build: impl Fn() -> (Editor, EntityId, EntityId), label: &str, active: bool) -> Solved {
    let (mut ed, a, b) = build();
    relate(&mut ed, &[a, b], active, label).expect("added");
    (points(&ed), ed.relation_kept)
}

#[test]
fn a_child_that_cannot_turn_leaves_the_relation_to_solve_as_before() {
    // a is held level: Parallel can only be met by turning b, the active one. Keeping it
    // where it is can't land, so the relation solves exactly as with no active element.
    let build = || {
        let (mut ed, a, b) = two_lines();
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: a[0] }])
            .unwrap();
        (ed, a[0], b[0])
    };
    let (kept_pts, k) = solved(build, "Parallel", true);
    assert_eq!(k.map(|k| k.0), Some(false));
    assert_eq!(kept_pts, solved(build, "Parallel", false).0);
}

#[test]
fn where_the_active_line_must_turn_its_neighbours_move_as_before() {
    // As above, with a free line hanging from each end of b: they move exactly as they do
    // with no active element (at the carried rank alone, they would be pushed instead of b:
    // 2 and 5.7 mm against 0.02).
    let build = || {
        let (mut ed, a, b) = two_lines();
        ed.doc.begin();
        let p = ed.doc.add_point(v(30.0, -5.0));
        let q = ed.doc.add_point(v(15.0, 18.0));
        ed.doc.add_line(b[1], p).unwrap();
        ed.doc.add_line(b[2], q).unwrap();
        ed.commit("Hangers");
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: a[0] }])
            .unwrap();
        (ed, a[0], b[0])
    };
    let (kept_pts, k) = solved(build, "Parallel", true);
    assert_eq!(k.map(|k| k.0), Some(false));
    assert_eq!(kept_pts, solved(build, "Parallel", false).0);
}

#[test]
fn a_relation_the_even_split_cannot_finish_may_be_added_keeping_the_active_one() {
    // An L whose second line is held level: Collinear shared between the two collapses
    // geometry, and is refused; with the level line active it stays, and the other turns
    // onto its line, which meets the relation. (The one way the rule changes what is added:
    // a relation keeping the active element can finish where the even split can't.)
    let build = || {
        let mut ed = Editor::default();
        ed.doc.begin();
        let far = ed.doc.add_point(v(29.4, 3.9));
        let corner = ed.doc.add_point(v(8.4, 19.5));
        let end = ed.doc.add_point(v(24.4, 0.5));
        let a = ed.doc.add_line(far, corner).unwrap();
        let b = ed.doc.add_line(corner, end).unwrap();
        ed.commit("L");
        ed.add_constraints("Horizontal", &[Constraint::Horizontal { line: b }])
            .unwrap();
        (ed, a, b, [far, corner, end])
    };
    let (mut today, a, b, _) = build();
    let start = points(&today);
    let r = relate(&mut today, &[a, b], false, "Collinear");
    assert!(matches!(r, Err(EditError::Degenerate)), "{r:?}");
    assert_eq!(points(&today), start);
    let (mut ed, a, b, [far, corner, end]) = build();
    relate(&mut ed, &[a, b], true, "Collinear").unwrap();
    assert!(kept(&ed));
    let line = start.iter().map(|(_, p)| *p).collect::<Vec<_>>();
    assert_eq!((at(&ed, corner), at(&ed, end)), (line[1], line[2]));
    assert!((at(&ed, far).y - line[1].y).abs() < 1e-9, "on b's line");
}

/// A chain of `bars` bars held at their lengths from a fixed end, zigzagging: (editor, its
/// lines).
fn held_chain(bars: usize) -> (Editor, Vec<EntityId>) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let points: Vec<EntityId> = (0..=bars)
        .map(|k| {
            let y = if k % 2 == 0 { 0.0 } else { 2.0 };
            ed.doc.add_point(v(k as f64 * 10.0, y))
        })
        .collect();
    ed.doc
        .add_constraint(Constraint::Fix {
            point: points[0],
            at: DVec2::ZERO,
        })
        .unwrap();
    let mut lines = Vec::new();
    for pair in points.windows(2) {
        lines.push(ed.doc.add_line(pair[0], pair[1]).unwrap());
        let value = at(&ed, pair[0]).distance(at(&ed, pair[1]));
        ed.doc
            .add_constraint(Constraint::Distance {
                a: pair[0],
                b: pair[1],
                value,
            })
            .unwrap();
    }
    ed.commit("Chain");
    (ed, lines)
}

#[test]
fn the_free_end_of_a_chain_turns_to_the_active_bar() {
    // The last bar turns to the one before it, which stays: the rest of the chain moves no
    // more than with no active element.
    let (mut ed, lines) = held_chain(30);
    let start = points(&ed);
    relate(&mut ed, &[lines[29], lines[28]], true, "Perpendicular").unwrap();
    assert!(kept(&ed));
    let kept_moves: Vec<f64> = start
        .iter()
        .map(|(p, s)| at(&ed, *p).distance(*s))
        .collect();
    let (mut today, lines) = held_chain(30);
    relate(&mut today, &[lines[29], lines[28]], false, "Perpendicular").unwrap();
    let today_moves: Vec<f64> = start
        .iter()
        .map(|(p, s)| at(&today, *p).distance(*s))
        .collect();
    // (Points 0 to 27; 28 and 29 are the active bar's, and don't move at all.)
    for (k, (kept, today)) in kept_moves.iter().zip(&today_moves).enumerate().take(28) {
        assert!(*kept <= *today + 1e-9, "point {k}: {kept} against {today}");
    }
    assert_eq!(kept_moves[28..30], [0.0, 0.0]);
}

#[test]
fn a_relation_the_active_one_can_stay_through_by_bending_the_chain_keeps_it() {
    // Two bars near the free end of a long held chain, the second active: it can stay where
    // it is, the chain bending to meet the relation, so it does (#333's rule). Before PR 3
    // that solve ran out of iterations, and the relation was shared as with no active element
    // (the fallback the other tests here still see where keeping can't land). Undo and redo
    // take the drawing back and forth exactly.
    let (mut ed, lines) = held_chain(500);
    let start = points(&ed);
    relate(&mut ed, &[lines[497], lines[498]], true, "Perpendicular").expect("added");
    assert_eq!(ed.relation_kept.map(|k| k.0), Some(true));
    let after = points(&ed);
    assert_eq!(after[498..500], start[498..500]);
    assert!(ed.undo());
    assert_eq!(points(&ed), start);
    assert!(ed.redo());
    assert_eq!(points(&ed), after);
}

/// Run `f` `n` times; the median, in milliseconds.
fn median_ms(n: usize, mut f: impl FnMut()) -> f64 {
    let mut each: Vec<f64> = (0..n)
        .map(|_| {
            let t = std::time::Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    each.sort_by(f64::total_cmp);
    each[n / 2]
}

/// Adding Perpendicular between two picked lines, added and undone, with no active element
/// and with the second picked active: a 2,000-bar chain where the free end turns (kept) and
/// where keeping would swing the chain (solved again as with none), a 200-bar chain, and an
/// L corner; five rounds alternating.
#[test]
#[ignore = "torture test: run with --release --ignored --nocapture"]
fn perf_add_relation_active() {
    let l_corner = || {
        let mut ed = Editor::default();
        ed.doc.begin();
        let corner = ed.doc.add_point(v(0.0, 0.0));
        let far_a = ed.doc.add_point(v(10.0, 2.0));
        let far_b = ed.doc.add_point(v(1.0, 10.0));
        let a = ed.doc.add_line(corner, far_a).unwrap();
        let b = ed.doc.add_line(corner, far_b).unwrap();
        ed.commit("L");
        (ed, b, a)
    };
    let mut cases: Vec<(&str, Editor, EntityId, EntityId)> = Vec::new();
    let (ed, lines) = held_chain(2_000);
    cases.push((
        "2000 bars, the free end turns",
        ed,
        lines[1_999],
        lines[1_998],
    ));
    let (ed, lines) = held_chain(2_000);
    cases.push((
        "2000 bars, the chain would swing",
        ed,
        lines[1_997],
        lines[1_998],
    ));
    let (ed, lines) = held_chain(200);
    cases.push(("200 bars, the free end turns", ed, lines[199], lines[198]));
    let (ed, a, b) = l_corner();
    cases.push(("an L corner", ed, a, b));
    for round in 1..=5 {
        for (label, ed, a, b) in &mut cases {
            let (a, b) = (*a, *b);
            let mut line = format!("round {round}  {label:<34}");
            for active in [false, true] {
                ed.selection = [a, b].into();
                ed.set_active(active.then_some(b));
                let ms = median_ms(5, || {
                    ed.add_picked_constraints(
                        "Perpendicular",
                        &[Constraint::Perpendicular { a, b }],
                    )
                    .expect("added");
                    ed.undo();
                });
                let who = if active { "active" } else { "none" };
                line += &format!("  {who} {ms:>9.3} ms");
            }
            println!("{line}");
        }
    }
}

/// Perpendicular between two picked lines of the big drawing `DRIFT_BIG` names (a PDF,
/// imported as geometry), at ten places across it: how long adding it takes (and undoing,
/// where it was added) with no active element and with the second picked active, whether
/// each was added, and whether the active one was kept.
#[test]
#[ignore = "big-drawing probe: needs DRIFT_BIG; run with --release --ignored --nocapture"]
fn perf_big_add_relation_active() {
    let Ok(path) = std::env::var("DRIFT_BIG") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let art = drift_io::pdf_import::read_artwork(&bytes, 1, "sheet").unwrap();
    let options = drift_io::vector::ImportOptions {
        scale: 1.0,
        construction_edges: true,
    };
    let import = drift_io::vector::to_import(&art, &options);
    let mut ed = Editor::new(drift_core::Document::new());
    ed.doc.begin();
    drift_io::dxf::add_to_document(&mut ed.doc, &import);
    assert!(ed.commit("Import"));
    // Slanted lines, each paired with one half the list on at an angle to it.
    let slanted: Vec<(EntityId, DVec2)> = ed
        .doc
        .iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Line { a, b } => {
                let d = ed.doc.point(b).ok()? - ed.doc.point(a).ok()?;
                let len = d.length();
                (len > 1.0 && d.x.abs() > 0.1 * len && d.y.abs() > 0.1 * len).then(|| (id, d / len))
            }
            _ => None,
        })
        .collect();
    let curves = ed.doc.iter().filter(|(_, e)| !e.kind.is_point()).count();
    println!(
        "
{curves} curves, {} slanted lines",
        slanted.len()
    );
    let n = slanted.len();
    for k in 0..10 {
        let (a, da) = slanted[(k * n / 10 + 3) % n];
        let Some((b, _)) = slanted[(k * n / 10 + n / 2) % n..]
            .iter()
            .copied()
            .find(|(_, d)| d.dot(da).abs() < 0.9)
        else {
            continue;
        };
        let c = Constraint::Perpendicular { a, b };
        let mut line = format!("place {k}");
        for active in [false, true] {
            ed.selection = [a, b].into();
            ed.set_active(active.then_some(b));
            let t = std::time::Instant::now();
            let added = ed.add_picked_constraints("Perpendicular", &[c]).is_ok();
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let kept = ed.relation_kept.is_some_and(|k| k.0);
            if added {
                ed.undo();
            }
            let who = if active { "active" } else { "none" };
            line += &format!("   {who} {ms:>6.0} ms, added {added:<5} kept {kept:<5}");
        }
        println!("{line}");
    }
}

/// Probe (#349): Perpendicular between neighbouring bars of held chains, with the solver's
/// exits. Run with `--ignored --nocapture`.
#[test]
#[ignore = "probe"]
fn zz_probe_349() {
    let sizes: Vec<usize> = std::env::var("CHAIN_BARS")
        .ok()
        .map(|s| s.split(',').map(|x| x.parse().unwrap()).collect())
        .unwrap_or_else(|| vec![10, 30, 100, 300, 1000, 2000]);
    for bars in sizes {
        for at_k in [bars / 2, bars - 2] {
            let (mut ed, lines) = held_chain(bars);
            let _ = drift_solver::exits::take();
            let t = std::time::Instant::now();
            let r = relate(
                &mut ed,
                &[lines[at_k - 1], lines[at_k]],
                false,
                "Perpendicular",
            );
            let ms = t.elapsed().as_secs_f64() * 1e3;
            let st = drift_solver::exits::take();
            println!(
                "P349 bars {bars} at {at_k}: {} {ms:.1} ms drag {:?} other {:?} it {}",
                if r.is_ok() { "added" } else { "REFUSED" },
                st.drag,
                st.other,
                st.other_iterations
            );
        }
    }
}
