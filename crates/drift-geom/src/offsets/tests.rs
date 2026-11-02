//! The offset pieces of every curve kind, and their regeneration by key.

use std::f64::consts::TAU;

use drift_core::{
    Constraint, Document, EntityId, EntityKind, Offset, OffsetId, PieceKey, PointKey, Shape, Side,
};

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn close(a: DVec2, b: DVec2) -> bool {
    a.distance(b) < 1e-9
}

/// Lines through `points` in order, closed back to the first when asked.
fn polyline(doc: &mut Document, points: &[DVec2], closed: bool) -> Vec<EntityId> {
    let ps: Vec<EntityId> = points.iter().map(|p| doc.add_point(*p)).collect();
    let n = ps.len();
    let mut out = Vec::new();
    for i in 0..n {
        if i + 1 == n && !closed {
            break;
        }
        out.push(doc.add_line(ps[i], ps[(i + 1) % n]).unwrap());
    }
    out
}

fn line_ends(doc: &Document, id: EntityId) -> (DVec2, DVec2) {
    match doc.shape(id) {
        Some(Shape::Line { a, b }) => (a, b),
        other => panic!("not a line: {other:?}"),
    }
}

/// The ellipse of `with_ellipse`: semi-axes 20 (along x) and 10.
fn off_ellipse(p: DVec2) -> f64 {
    (0..20_000)
        .map(|k| {
            let a = TAU * f64::from(k) / 20_000.0;
            v(20.0 * a.cos(), 10.0 * a.sin()).distance(p)
        })
        .fold(f64::INFINITY, f64::min)
}

fn with_ellipse(doc: &mut Document) -> EntityId {
    let c = doc.add_point(v(0.0, 0.0));
    let major = doc.add_point(v(20.0, 0.0));
    let minor = doc.add_point(v(0.0, 10.0));
    doc.add_ellipse(c, major, minor).unwrap()
}

const ARCH: [DVec2; 4] = [
    DVec2::new(0.0, 0.0),
    DVec2::new(0.0, 20.0),
    DVec2::new(40.0, 20.0),
    DVec2::new(40.0, 0.0),
];

fn with_arch(doc: &mut Document) -> EntityId {
    let [a, c1, c2, b] = ARCH.map(|p| doc.add_point(p));
    doc.add_bezier(a, c1, c2, b).unwrap()
}

fn offset_of(doc: &Document, x: OffsetId) -> Offset {
    doc.offset(x).unwrap().clone()
}

/// Every piece's shape is `d` from the source chain (sampled), for lines and arcs exactly.
fn gap_to(doc: &Document, source: &[EntityId], p: DVec2) -> f64 {
    source
        .iter()
        .filter_map(|c| doc.shape(*c))
        .map(|s| crate::distance(&s, p))
        .fold(f64::INFINITY, f64::min)
}

// ----- Pieces ---------------------------------------------------------------------------------

#[test]
fn an_open_chain_rejoins_its_corner_and_a_closed_one_every_corner() {
    let mut doc = Document::default();
    let ids = polyline(&mut doc, &[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0)], false);
    let (chain, closed) = chain_through(&doc, ids[1]);
    assert_eq!(chain.len(), 2);
    assert!(!closed);
    // To the left of travel (above the first line, left of the second).
    let p = pieces(&doc, &chain, closed, 2.0).unwrap();
    assert_eq!(p.len(), 2);
    let (Shape::Line { a: a0, b: b0 }, Shape::Line { a: a1, b: b1 }) = (p[0].shape, p[1].shape)
    else {
        panic!("lines")
    };
    assert!(
        close(a0, v(0.0, 2.0)) && close(b0, v(18.0, 2.0)),
        "{a0} {b0}"
    );
    assert!(
        close(a1, v(18.0, 2.0)) && close(b1, v(18.0, 10.0)),
        "{a1} {b1}"
    );
    // The side a point is on.
    assert_eq!(side_of(&doc, &chain, v(10.0, 5.0)), Side::Left);
    assert_eq!(side_of(&doc, &chain, v(10.0, -5.0)), Side::Right);
    // A closed rectangle, inwards (its travel is counter-clockwise: inwards is left).
    let mut doc = Document::default();
    let ids = polyline(
        &mut doc,
        &[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)],
        true,
    );
    let (chain, closed) = chain_through(&doc, ids[0]);
    assert!(closed);
    let p = pieces(&doc, &chain, closed, 1.0).unwrap();
    assert_eq!(p.len(), 4);
    let corners: std::collections::BTreeSet<(i64, i64)> = p
        .iter()
        .flat_map(|piece| {
            let Shape::Line { a, b } = piece.shape else {
                panic!("line")
            };
            [
                (a.x.round() as i64, a.y.round() as i64),
                (b.x.round() as i64, b.y.round() as i64),
            ]
        })
        .collect();
    assert_eq!(corners, [(1, 1), (19, 1), (19, 9), (1, 9)].into());
    // Too far: the short sides would run backwards.
    assert_eq!(
        pieces(&doc, &chain, closed, 6.0),
        Err(OffsetError::TooLarge)
    );
}

#[test]
fn circles_and_arcs_offset_about_their_centre() {
    let mut doc = Document::default();
    let c = doc.add_point(v(0.0, 0.0));
    let circle = doc.add_circle(c, 5.0).unwrap();
    let (chain, closed) = chain_through(&doc, circle);
    assert!(closed);
    // Left is outward for a whole circle.
    assert_eq!(side_of(&doc, &chain, v(10.0, 0.0)), Side::Left);
    assert_eq!(side_of(&doc, &chain, v(1.0, 0.0)), Side::Right);
    let out = pieces(&doc, &chain, closed, 2.0).unwrap();
    assert!(matches!(out[0].shape, Shape::Circle { radius, .. } if (radius - 7.0).abs() < 1e-12));
    let inner = pieces(&doc, &chain, closed, -2.0).unwrap();
    assert!(matches!(inner[0].shape, Shape::Circle { radius, .. } if (radius - 3.0).abs() < 1e-12));
    assert_eq!(
        pieces(&doc, &chain, closed, -5.0),
        Err(OffsetError::LargerThanRadius)
    );
    // A quarter arc: left of counter-clockwise travel is the centre.
    let s = doc.add_point(v(5.0, 0.0));
    let e = doc.add_point(v(0.0, 5.0));
    let arc = doc.add_arc(c, s, e).unwrap();
    let (chain, closed) = chain_through(&doc, arc);
    assert!(!closed);
    let inner = pieces(&doc, &chain, closed, 1.0).unwrap();
    assert!(matches!(inner[0].shape, Shape::Arc { radius, .. } if (radius - 4.0).abs() < 1e-12));
    assert_eq!(
        pieces(&doc, &chain, closed, 5.0),
        Err(OffsetError::LargerThanRadius)
    );
}

#[test]
fn an_ellipse_offsets_to_a_closed_smooth_run_of_pen_curves() {
    let mut doc = Document::default();
    let ellipse = with_ellipse(&mut doc);
    let (chain, closed) = chain_through(&doc, ellipse);
    assert!(closed);
    // Outside (left is outward for a whole ellipse): 3 from the ellipse all round.
    let out = pieces(&doc, &chain, closed, 3.0).unwrap();
    assert!(out.len() >= 4);
    for piece in &out {
        let Shape::Bezier { p } = piece.shape else {
            panic!("a pen curve")
        };
        for k in 0..=8 {
            let q = drift_core::shape::bezier_point(&p, f64::from(k) / 8.0);
            assert!((off_ellipse(q) - 3.0).abs() < 5e-3, "{}", off_ellipse(q));
            assert!(q.length() > 10.0, "outside");
        }
    }
    // Closed: every end is another's start.
    for piece in &out {
        let Shape::Bezier { p } = piece.shape else {
            unreachable!()
        };
        let starts = out
            .iter()
            .filter(|o| matches!(o.shape, Shape::Bezier { p: q } if q[0].distance(p[3]) < 1e-9))
            .count();
        assert_eq!(starts, 1);
    }
    // Inside works up to the tightest bend (10² / 20 = 5), then says why not.
    let inner = pieces(&doc, &chain, closed, -4.0).unwrap();
    let Shape::Bezier { p } = inner[0].shape else {
        panic!("a pen curve")
    };
    assert!((off_ellipse(p[0]) - 4.0).abs() < 5e-3 && p[0].length() < 20.0);
    assert_eq!(
        pieces(&doc, &chain, closed, -6.0),
        Err(OffsetError::TooTight)
    );
}

#[test]
fn a_pen_curve_between_lines_bridges_an_opened_corner_and_cuts_a_closed_one() {
    // A line running into an arch (a corner between them), then a line out of it.
    let mut doc = Document::default();
    let arch = with_arch(&mut doc);
    let EntityKind::Bezier { a, b, .. } = doc.get(arch).unwrap().kind else {
        unreachable!()
    };
    let start = doc.add_point(v(-30.0, 0.0));
    let end = doc.add_point(v(70.0, 0.0));
    let lead_in = doc.add_line(start, a).unwrap();
    doc.add_line(b, end).unwrap();
    let (chain, closed) = chain_through(&doc, lead_in);
    assert_eq!(chain.len(), 3);
    assert!(!closed);
    // Above the path (outside the arch): the corners open, so they're bridged.
    assert_eq!(side_of(&doc, &chain, v(-20.0, 10.0)), Side::Left);
    let made = pieces(&doc, &chain, closed, 4.0).unwrap();
    let on_offset = |p: DVec2| {
        let to_arch = crate::nearest_on_bezier(&ARCH, p).0.distance(p);
        let to_lines = if p.x < 0.0 || p.x > 40.0 {
            p.y.abs()
        } else {
            f64::INFINITY
        };
        to_arch.min(to_lines)
    };
    let mut curves_made = 0;
    for piece in made.iter().filter(|p| !p.bridge) {
        match piece.shape {
            Shape::Line { a, b } => {
                assert!((a.y - 4.0).abs() < 1e-6 && (b.y - 4.0).abs() < 1e-6);
            }
            Shape::Bezier { p } => {
                curves_made += 1;
                for k in 0..=8 {
                    let q = drift_core::shape::bezier_point(&p, f64::from(k) / 8.0);
                    assert!((on_offset(q) - 4.0).abs() < 5e-3, "{}", on_offset(q));
                }
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(curves_made >= 1);
    // One connected run: every piece's end meets the next piece's start.
    for w in made.windows(2) {
        let e = piece_end(&w[0].shape, w[0].forward, true);
        let s = piece_end(&w[1].shape, w[1].forward, false);
        assert!(e.distance(s) < 1e-6, "{e} {s}");
    }
    // Below (inside the arch) too: one connected run.
    let made = pieces(&doc, &chain, closed, -2.0).unwrap();
    for w in made.windows(2) {
        let e = piece_end(&w[0].shape, w[0].forward, true);
        let s = piece_end(&w[1].shape, w[1].forward, false);
        assert!(e.distance(s) < 1e-6, "{e} {s}");
    }
    // A lone arch offsets to curves ending square to its ends.
    let mut doc = Document::default();
    let arch = with_arch(&mut doc);
    let (chain, closed) = chain_through(&doc, arch);
    let made = pieces(&doc, &chain, closed, 5.0).unwrap();
    let first = piece_end(&made[0].shape, true, false);
    let last = piece_end(&made[made.len() - 1].shape, true, true);
    assert!(first.distance(v(-5.0, 0.0)) < 1e-6, "{first}");
    assert!(last.distance(v(45.0, 0.0)) < 1e-6, "{last}");
}

// ----- Regeneration by key -------------------------------------------------------------------

/// A 40 × 30 rectangle offset 2 outward (travel counter-clockwise: outward is right).
fn rectangle_offset(doc: &mut Document) -> (Vec<EntityId>, OffsetId) {
    let ids = polyline(
        doc,
        &[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)],
        true,
    );
    let x = doc
        .add_offset(Offset::new(ids.clone(), Side::Right, 2.0))
        .unwrap();
    let r = regenerate(doc);
    assert_eq!(r.created, 8, "four lines and four corners");
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    (ids, x)
}

#[test]
fn a_rectangles_offset_is_four_lines_at_the_distance_sharing_corners() {
    let mut doc = Document::default();
    let (ids, x) = rectangle_offset(&mut doc);
    let o = offset_of(&doc, x);
    assert_eq!(o.made.curves.len(), 4);
    assert_eq!(o.made.points.len(), 4);
    for c in o.made.curves.values() {
        let (a, b) = line_ends(&doc, *c);
        let mid = (a + b) / 2.0;
        assert!((gap_to(&doc, &ids, mid) - 2.0).abs() < 1e-9);
        assert!(
            mid.x < 0.0 || mid.x > 40.0 || mid.y < 0.0 || mid.y > 30.0,
            "outside"
        );
    }
    // Every piece's end is another's start: four points, each used twice.
    for p in o.made.points.values() {
        assert_eq!(doc.users_of(*p).count(), 2);
    }
    // Nothing to do a second time.
    let r = regenerate(&mut doc);
    assert!(!r.changed, "{r:?}");
    assert_eq!(offset_of(&doc, x).made, o.made, "same keys, same ids");
}

#[test]
fn moving_the_source_keeps_every_id_and_moves_the_pieces() {
    let mut doc = Document::default();
    let (ids, x) = rectangle_offset(&mut doc);
    let before = offset_of(&doc, x).made;
    // Stretch the rectangle: move its right side's two corners.
    let EntityKind::Line { a, b } = doc.get(ids[1]).unwrap().kind else {
        unreachable!()
    };
    doc.set_point(a, v(60.0, 0.0)).unwrap();
    doc.set_point(b, v(60.0, 30.0)).unwrap();
    let r = regenerate(&mut doc);
    assert!(r.changed && r.created == 0 && r.deleted == 0, "{r:?}");
    let after = offset_of(&doc, x).made;
    assert_eq!(after, before, "same keys, same ids");
    for c in after.curves.values() {
        let (a, b) = line_ends(&doc, *c);
        assert!((gap_to(&doc, &ids, (a + b) / 2.0) - 2.0).abs() < 1e-9);
    }
    let right = after.curves[&PieceKey::Of { of: ids[1], n: 0 }];
    let (a, _) = line_ends(&doc, right);
    assert!((a.x - 62.0).abs() < 1e-9, "{a}");
}

#[test]
fn a_corner_that_opens_gains_a_bridge_and_nothing_else_changes_id() {
    // A line into an arch: offset outside, where the corner opens.
    let mut doc = Document::default();
    let arch = with_arch(&mut doc);
    let EntityKind::Bezier { a, .. } = doc.get(arch).unwrap().kind else {
        unreachable!()
    };
    let start = doc.add_point(v(-30.0, 0.0));
    let lead_in = doc.add_line(start, a).unwrap();
    // On the side where the corner is cut (the offsets meet when extended) there is no
    // bridge; on the other it opens and is bridged.
    let has_bridge = |doc: &Document, x| {
        offset_of(doc, x)
            .made
            .curves
            .keys()
            .any(|k| matches!(k, PieceKey::Bridge { .. }))
    };
    let x = doc
        .add_offset(Offset::new(vec![lead_in, arch], Side::Right, 2.0))
        .unwrap();
    let r = regenerate(&mut doc);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    let (cut, open) = if has_bridge(&doc, x) {
        (Side::Left, Side::Right)
    } else {
        (Side::Right, Side::Left)
    };
    let mut o = offset_of(&doc, x);
    o.side = cut;
    doc.set_offset(x, o).unwrap();
    regenerate(&mut doc);
    assert!(!has_bridge(&doc, x), "cut on the {cut:?}");
    let before = offset_of(&doc, x).made;
    // Flip to the other side: the corner opens and is bridged; the line's piece and the
    // fitted pieces keep their ids.
    let mut o = offset_of(&doc, x);
    o.side = open;
    doc.set_offset(x, o).unwrap();
    let r = regenerate(&mut doc);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    let after = offset_of(&doc, x).made;
    let bridge = PieceKey::Bridge { after: lead_in };
    assert!(after.curves.contains_key(&bridge));
    assert_eq!(after.curves.len(), before.curves.len() + 1);
    for (k, id) in &before.curves {
        assert_eq!(after.curves.get(k), Some(id), "{k:?} kept its id");
    }
    assert!(after.points.contains_key(&PointKey::Start(bridge)));
    for (k, id) in &before.points {
        assert_eq!(after.points.get(k), Some(id), "{k:?} kept its id");
    }
}

#[test]
fn too_far_keeps_what_it_had_and_says_why() {
    let mut doc = Document::default();
    let (_, x) = rectangle_offset(&mut doc);
    let before = offset_of(&doc, x);
    let mut o = before.clone();
    o.distance = 20.0;
    o.side = Side::Left; // inwards: the short sides would run backwards
    doc.set_offset(x, o).unwrap();
    let r = regenerate(&mut doc);
    assert_eq!(r.notices.len(), 1, "{:?}", r.notices);
    assert!(r.notices[0].contains("too large"), "{}", r.notices[0]);
    assert_eq!(offset_of(&doc, x).made, before.made);
    let right = before.made.curves[&PieceKey::Of {
        of: before.source[1],
        n: 0,
    }];
    assert!((line_ends(&doc, right).0.x - 42.0).abs() < 1e-9);
}

#[test]
fn a_split_source_line_is_followed_and_a_deleted_one_shortens_the_chain() {
    let mut doc = Document::default();
    let ids = polyline(
        &mut doc,
        &[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(40.0, 10.0)],
        false,
    );
    let x = doc
        .add_offset(Offset::new(ids.clone(), Side::Left, 1.0))
        .unwrap();
    regenerate(&mut doc);
    let before = offset_of(&doc, x).made;
    assert_eq!(before.curves.len(), 3);
    // Split the first line at its middle: two ids, the second new.
    let EntityKind::Line { a, b } = doc.get(ids[0]).unwrap().kind else {
        unreachable!()
    };
    let m = doc.add_point(v(10.0, 0.0));
    doc.set_entity_kind(ids[0], EntityKind::Line { a, b: m })
        .unwrap();
    let second_half = doc.add_line(m, b).unwrap();
    let r = regenerate(&mut doc);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    let o = offset_of(&doc, x);
    assert_eq!(
        o.source,
        vec![ids[0], second_half, ids[1], ids[2]],
        "re-chained"
    );
    assert_eq!(o.made.curves.len(), 4);
    for (k, id) in &before.curves {
        assert_eq!(o.made.curves.get(k), Some(id), "{k:?} kept its id");
    }
    // Delete the last line: the chain ends at the second.
    doc.delete([ids[2]]);
    let r = regenerate(&mut doc);
    assert!(r.notices.is_empty(), "{:?}", r.notices);
    let o = offset_of(&doc, x);
    assert_eq!(o.source, vec![ids[0], second_half, ids[1]]);
    assert_eq!(o.made.curves.len(), 3);
    assert!(
        !o.made
            .curves
            .contains_key(&PieceKey::Of { of: ids[2], n: 0 })
    );
}

/// Condition A: a re-chain never adopts curves beyond the first and last old source curve.
#[test]
fn a_rechain_stays_between_the_first_and_last_old_curve() {
    let mut doc = Document::default();
    let ids = polyline(
        &mut doc,
        &[v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(40.0, 10.0)],
        false,
    );
    let x = doc
        .add_offset(Offset::new(ids.clone(), Side::Left, 1.0))
        .unwrap();
    regenerate(&mut doc);
    // A fourth line drawn onto the chain's end, then the first line split.
    let EntityKind::Line { b: end, .. } = doc.get(ids[2]).unwrap().kind else {
        unreachable!()
    };
    let far = doc.add_point(v(40.0, 30.0));
    let fourth = doc.add_line(end, far).unwrap();
    let EntityKind::Line { a, b } = doc.get(ids[0]).unwrap().kind else {
        unreachable!()
    };
    let m = doc.add_point(v(10.0, 0.0));
    doc.set_entity_kind(ids[0], EntityKind::Line { a, b: m })
        .unwrap();
    let half = doc.add_line(m, b).unwrap();
    regenerate(&mut doc);
    let o = offset_of(&doc, x);
    assert_eq!(o.source, vec![ids[0], half, ids[1], ids[2]]);
    assert!(!o.source.contains(&fourth), "never the fourth");
    assert!(
        !o.made
            .curves
            .contains_key(&PieceKey::Of { of: fourth, n: 0 })
    );
    // A closed loop with every old curve still on it keeps the loop whole (the split
    // half included).
    let mut doc = Document::default();
    let (ids, x) = rectangle_offset(&mut doc);
    let EntityKind::Line { a, b } = doc.get(ids[0]).unwrap().kind else {
        unreachable!()
    };
    let m = doc.add_point(v(20.0, 0.0));
    doc.set_entity_kind(ids[0], EntityKind::Line { a, b: m })
        .unwrap();
    let half = doc.add_line(m, b).unwrap();
    regenerate(&mut doc);
    let o = offset_of(&doc, x);
    assert_eq!(o.source.len(), 5);
    assert!(o.source.contains(&half));
    assert_eq!(o.made.curves.len(), 5);
    assert_eq!(o.made.points.len(), 5, "still a closed loop");
}

#[test]
fn a_mirrored_chains_side_flips_and_a_circles_does_not() {
    assert_eq!(Side::Left.flipped(), Side::Right);
    // (The copy rule lives in drift-edit; here the sign the generator takes.)
    assert_eq!(Side::Left.sign(), 1.0);
    assert_eq!(Side::Right.sign(), -1.0);
    let o = Offset::new(vec![EntityId(1)], Side::Right, 3.0);
    assert_eq!(o.signed(), -3.0);
}

#[test]
fn pieces_are_never_solver_unknowns() {
    let mut doc = Document::default();
    let (ids, x) = rectangle_offset(&mut doc);
    let generated = doc.generated_entities();
    let o = offset_of(&doc, x);
    assert!(o.made.entities().all(|e| generated.contains(&e)));
    assert!(ids.iter().all(|c| !generated.contains(c)));
    let owners = doc.wall_owners();
    assert!(o.made.entities().all(|e| owners.offset(e) == Some(x)));
    assert!(
        o.made
            .entities()
            .all(|e| owners.generated(e) && !owners.by_wall(e))
    );
    // A relation on a piece is the solver's business only through the generated set; the
    // document itself takes it (the editor refuses it before it gets here).
    let piece = *o.made.curves.values().next().unwrap();
    assert!(
        doc.add_constraint(Constraint::Horizontal { line: piece })
            .is_ok()
    );
}
