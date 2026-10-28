//! Openings cut into walls: stations, jambs, the poché going round, keys, and the symbol's
//! frame (docs/walls-design.md §6, §9).

use std::collections::BTreeSet;

use drift_core::{
    DVec2, EntityId, FaceKey, Hand, OpeningId, Part, Shape, Station, WallFace, WallId,
};

use super::{EndIn, Frame, Generated, LoopKey, OpeningIn, Seg, WallIn, generate, generate_cut};

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn line_wall(id: u32, pts: &[DVec2], first: u32, closed: bool, t: f64) -> WallIn {
    let n = if closed { pts.len() } else { pts.len() - 1 };
    WallIn {
        id: WallId(id),
        segs: (0..n)
            .map(|k| Seg::Line {
                a: pts[k],
                b: pts[(k + 1) % pts.len()],
            })
            .collect(),
        points: (0..pts.len() as u32).map(|i| EntityId(first + i)).collect(),
        closed,
        offsets: (t / 2.0, -t / 2.0),
        cluster: 0,
        cross: true,
        ends: [EndIn::default(), EndIn::default()],
    }
}

fn opening(id: u32, wall: u32, seg: u16, u0: f64, u1: f64) -> OpeningIn {
    OpeningIn {
        id: OpeningId(id),
        wall: WallId(wall),
        seg,
        u0,
        u1,
    }
}

fn area(g: &Generated) -> f64 {
    g.regions().iter().map(|(_, _, r)| r.area).sum()
}

fn keys(g: &Generated) -> BTreeSet<(WallId, FaceKey)> {
    g.curves.keys().copied().collect()
}

#[test]
fn an_opening_cuts_the_wall_with_two_jambs() {
    let w = line_wall(1, &[v(0.0, 0.0), v(1000.0, 0.0)], 1, false, 100.0);
    let plain = generate(std::slice::from_ref(&w), 0.25);
    let g = generate_cut(&[w], &[opening(5, 1, 0, 300.0, 700.0)], 0.25);
    assert!(g.notices.is_empty(), "{:?}", g.notices);
    // Two pieces of wall either side: two loops, each closed by a cap and a jamb.
    assert_eq!(g.loops.len(), 2);
    let jambs: Vec<_> = g
        .curves
        .keys()
        .filter(|(_, k)| k.part == Part::Jamb)
        .collect();
    assert_eq!(jambs.len(), 2);
    assert!(
        jambs
            .iter()
            .any(|(_, k)| k.from == Station::Jamb(OpeningId(5), 0))
    );
    assert!(
        jambs
            .iter()
            .any(|(_, k)| k.from == Station::Jamb(OpeningId(5), 1))
    );
    // The poché is less by the opening's width × the thickness.
    assert!((area(&plain) - area(&g) - 400.0 * 100.0).abs() < 1e-6);
    // The jambs are square across the wall at the stations.
    for ((_, k), piece) in &g.curves {
        if k.part == Part::Jamb
            && let Some(Shape::Line { a, b }) = g.shape(piece)
        {
            assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() > 99.0);
            assert!((a.x - 300.0).abs() < 1e-9 || (a.x - 700.0).abs() < 1e-9);
        }
    }
    // The piece from the start keeps its key; the one after starts at the far jamb.
    let k = |part, from| (WallId(1), FaceKey { seg: 0, part, from });
    assert!(g.curves.contains_key(&k(Part::Left, Station::SegStart)));
    assert!(
        g.curves
            .contains_key(&k(Part::Left, Station::Jamb(OpeningId(5), 1)))
    );
    assert!(g.unfit.is_empty());
}

#[test]
fn keys_elsewhere_stay_when_an_opening_is_added_or_slid() {
    let room = line_wall(
        1,
        &[
            v(0.0, 0.0),
            v(4000.0, 0.0),
            v(4000.0, 3000.0),
            v(0.0, 3000.0),
        ],
        1,
        true,
        100.0,
    );
    let none = generate(std::slice::from_ref(&room), 0.25);
    let one = generate_cut(
        std::slice::from_ref(&room),
        &[opening(9, 1, 1, 1000.0, 1900.0)],
        0.25,
    );
    let slid = generate_cut(
        std::slice::from_ref(&room),
        &[opening(9, 1, 1, 1200.0, 2100.0)],
        0.25,
    );
    // Adding it changes only segment 1's pieces; sliding it changes no key.
    let off = |g: &Generated| -> BTreeSet<_> {
        keys(g).into_iter().filter(|(_, k)| k.seg != 1).collect()
    };
    assert_eq!(off(&none), off(&one));
    assert_eq!(keys(&one), keys(&slid));
    // A room with a door: the outer and inner loops become one loop round.
    assert_eq!(none.loops.len(), 2);
    assert_eq!(one.loops.len(), 1);
    assert_eq!(one.loops[0].key, LoopKey::default());
    assert!((area(&none) - area(&one) - 900.0 * 100.0).abs() < 1e-6);
}

#[test]
fn an_opening_past_a_corner_isnt_cut() {
    let l = line_wall(
        1,
        &[v(0.0, 0.0), v(1000.0, 0.0), v(1000.0, 800.0)],
        1,
        false,
        100.0,
    );
    // Its far jamb is past the inside corner (at 950 along).
    let g = generate_cut(
        std::slice::from_ref(&l),
        &[opening(3, 1, 0, 600.0, 980.0)],
        0.25,
    );
    assert!(g.unfit.contains(&OpeningId(3)));
    assert!(
        g.notices.iter().any(|n| n.contains("o3")),
        "{:?}",
        g.notices
    );
    let mut plain = generate(std::slice::from_ref(&l), 0.25);
    plain.unfit = g.unfit.clone();
    plain.notices = g.notices.clone();
    assert_eq!(
        g, plain,
        "an opening that isn't cut leaves the wall as it was"
    );
    // Clear of it: cut.
    let g = generate_cut(&[l], &[opening(3, 1, 0, 500.0, 940.0)], 0.25);
    assert!(g.unfit.is_empty(), "{:?}", g.notices);
}

#[test]
fn a_tee_inside_an_opening_leaves_it_uncut() {
    let host = line_wall(1, &[v(0.0, 0.0), v(2000.0, 0.0)], 1, false, 100.0);
    let mut branch = line_wall(2, &[v(1000.0, 0.0), v(1000.0, 900.0)], 10, false, 100.0);
    branch.ends[0].tee = Some((WallId(1), 0));
    let g = generate_cut(
        &[host.clone(), branch.clone()],
        &[opening(4, 1, 0, 800.0, 1200.0)],
        0.25,
    );
    assert!(g.unfit.contains(&OpeningId(4)));
    // Beside the T: cut, and the T is untouched.
    let g = generate_cut(&[host, branch], &[opening(4, 1, 0, 200.0, 800.0)], 0.25);
    assert!(g.unfit.is_empty(), "{:?}", g.notices);
    assert!(
        g.curves
            .keys()
            .any(|(_, k)| matches!(k.from, Station::Tee(..)))
    );
}

#[test]
fn an_opening_in_an_arc_wall_has_radial_jambs() {
    let arc = WallIn {
        id: WallId(1),
        segs: vec![Seg::Arc {
            center: v(0.0, 0.0),
            radius: 2000.0,
            a: v(2000.0, 0.0),
            b: v(0.0, 2000.0),
            ccw: true,
        }],
        points: vec![EntityId(1), EntityId(2)],
        closed: false,
        offsets: (50.0, -50.0),
        cluster: 0,
        cross: true,
        ends: Default::default(),
    };
    let plain = generate(std::slice::from_ref(&arc), 0.25);
    let g = generate_cut(&[arc], &[opening(1, 1, 0, 1000.0, 1800.0)], 0.25);
    assert!(g.notices.is_empty(), "{:?}", g.notices);
    assert_eq!(g.loops.len(), 2);
    for ((_, k), piece) in &g.curves {
        if k.part == Part::Jamb {
            let Some(Shape::Line { a, b }) = g.shape(piece) else {
                panic!("a jamb is a line");
            };
            // Along a radius: both ends at the same angle.
            assert!((a.y.atan2(a.x) - b.y.atan2(b.x)).abs() < 1e-9);
        }
    }
    // Less by the ring sector: its width along the centreline × the thickness.
    assert!((area(&plain) - area(&g) - 800.0 * 100.0).abs() < 1e-3);
}

#[test]
fn a_symbol_frame_sits_on_the_face_it_opens_to() {
    let s = Seg::Line {
        a: v(0.0, 0.0),
        b: v(1000.0, 0.0),
    };
    // Left face (y = +50), hinged on the left looking from that face: the jamb at u1.
    let f = Frame::of(
        &s,
        (50.0, -50.0),
        (300.0, 700.0),
        Hand::Left,
        WallFace::Left,
    )
    .expect("a frame");
    assert!(f.origin.distance(v(700.0, 50.0)) < 1e-9);
    assert!(f.x.distance(v(-1.0, 0.0)) < 1e-9);
    // Into the wall: down, from the left face.
    assert!(f.y.distance(v(0.0, -1.0)) < 1e-9);
    assert_eq!((f.width, f.thickness), (400.0, 100.0));
    let f = Frame::of(
        &s,
        (50.0, -50.0),
        (300.0, 700.0),
        Hand::Left,
        WallFace::Right,
    )
    .expect("a frame");
    assert!(f.origin.distance(v(300.0, -50.0)) < 1e-9);
    assert!(f.y.distance(v(0.0, 1.0)) < 1e-9);
    // A library arc comes out an arc of the same radius round the same centre.
    let swing = Shape::Arc {
        center: DVec2::ZERO,
        radius: 400.0,
        start: -std::f64::consts::FRAC_PI_2,
        sweep: std::f64::consts::FRAC_PI_2,
    };
    let Some(Shape::Arc { center, radius, .. }) = f.shape(&swing) else {
        panic!("an arc");
    };
    assert!(center.distance(f.origin) < 1e-9 && (radius - 400.0).abs() < 1e-9);
}

#[test]
fn a_symbol_frame_on_a_vertical_wall_places_by_exact_quarter_turns() {
    let s = Seg::Line {
        a: v(2500.0, 100.0),
        b: v(2500.0, 4100.0),
    };
    for hand in [Hand::Left, Hand::Right] {
        for facing in [WallFace::Left, WallFace::Right] {
            let f = Frame::of(&s, (50.0, -50.0), (300.0, 1100.0), hand, facing).expect("a frame");
            let what = format!("{hand:?} {facing:?}");
            assert!(drift_core::blocks::is_quarter_turn(f.place.angle), "{what}");
            // Its axes are exactly along the wall and across it.
            assert_eq!(f.x.x, 0.0, "{what}");
            assert_eq!(f.y.y, 0.0, "{what}");
            // A symbol's line ends are whole numbers here, and come out so.
            let Some(Shape::Line { a, b }) = f.shape(&Shape::Line {
                a: v(0.0, 0.0),
                b: v(800.0, 100.0),
            }) else {
                panic!("a line ({what})");
            };
            for p in [a, b] {
                assert_eq!(p, p.round(), "{what}: {p}");
            }
            // An arc's centre is placed exactly, and it runs between its placed ends.
            let swing = Shape::Arc {
                center: DVec2::ZERO,
                radius: 800.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            };
            let Some(arc @ Shape::Arc { center, sweep, .. }) = f.shape(&swing) else {
                panic!("an arc ({what})");
            };
            assert_eq!(center, f.place.apply(DVec2::ZERO), "{what}");
            assert_eq!(center, center.round(), "{what}");
            let ends = [arc.point_at(0.0), arc.point_at(sweep)];
            for local in [v(800.0, 0.0), v(0.0, 800.0)] {
                let want = f.place.apply(local);
                assert!(
                    ends.iter().any(|e| e.distance(want) < 1e-9),
                    "{what}: {ends:?}"
                );
            }
        }
    }
}
