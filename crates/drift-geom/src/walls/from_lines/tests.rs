use super::*;

const T: f64 = 114.3;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Lines from end points, ids from 1.
fn lines(segs: &[(DVec2, DVec2)]) -> Vec<LineIn> {
    segs.iter()
        .enumerate()
        .map(|(n, &(a, b))| LineIn {
            id: EntityId(n as u32 + 1),
            group: 0,
            seg: Seg::Line { a, b },
            guide: false,
        })
        .collect()
}

fn settings() -> Settings {
    Settings::new(0.5, vec![T, 4.75 * 25.4, 100.0, 200.0], 1.0)
}

fn run(segs: &[(DVec2, DVec2)]) -> Found {
    find(&lines(segs), &settings())
}

/// A straight wall's faces from x0 to x1 along y = 0 (bottom face), with caps if asked.
fn straight(x0: f64, x1: f64, t: f64, caps: bool) -> Vec<(DVec2, DVec2)> {
    let mut v2 = vec![(v(x0, 0.0), v(x1, 0.0)), (v(x0, t), v(x1, t))];
    if caps {
        v2.push((v(x0, 0.0), v(x0, t)));
        v2.push((v(x1, 0.0), v(x1, t)));
    }
    v2
}

fn length(w: &FoundWall) -> f64 {
    w.segs.iter().map(|(s, _, _)| s.length()).sum()
}

#[test]
fn a_pair_of_parallel_lines_is_a_wall() {
    let f = run(&straight(0.0, 3000.0, T, true));
    assert_eq!(f.walls.len(), 1, "{f:?}");
    let w = &f.walls[0];
    assert_eq!(w.thickness, T);
    assert!((length(w) - 3000.0).abs() < 1e-6);
    let Seg::Line { a, b } = w.segs[0].0 else {
        panic!()
    };
    assert!((a.y - T / 2.0).abs() < 1e-9 && (b.y - T / 2.0).abs() < 1e-9);
    // Both faces and both caps are used.
    assert_eq!(w.lines.len(), 4);
    assert!(f.skipped.is_empty() && f.partly.is_empty());
}

#[test]
fn a_thickness_off_the_presets_is_rounded() {
    let f = run(&straight(0.0, 3000.0, 123.7, false));
    assert_eq!(f.walls[0].thickness, 124.0);
    // Close to a preset: snapped to it.
    let f = run(&straight(0.0, 3000.0, 199.0, false));
    assert_eq!(f.walls[0].thickness, 200.0);
}

#[test]
fn slightly_skewed_faces_still_pair() {
    let f = run(&[
        (v(0.0, 0.0), v(4000.0, 0.0)),
        (v(0.0, T), v(4000.0, T + 3.0)),
    ]);
    assert_eq!(f.walls.len(), 1, "{f:?}");
    assert!((f.walls[0].thickness - T).abs() < 1e-9);
}

#[test]
fn a_taper_is_skipped() {
    let f = run(&[
        (v(0.0, 0.0), v(4000.0, 0.0)),
        (v(0.0, T), v(4000.0, T + 60.0)),
    ]);
    assert!(f.walls.is_empty());
    assert_eq!(f.skipped.len(), 1);
    assert_eq!(f.skipped[0].why, Why::SpacingVaries);
}

#[test]
fn unequal_lengths_pair_over_their_overlap() {
    let f = run(&[(v(0.0, 0.0), v(6000.0, 0.0)), (v(1000.0, T), v(3000.0, T))]);
    assert_eq!(f.walls.len(), 1);
    assert!((length(&f.walls[0]) - 2000.0).abs() < 1e-6);
    // The long line is more than this wall: kept.
    assert_eq!(f.partly, vec![EntityId(1)]);
    assert_eq!(f.walls[0].lines, vec![EntityId(2)]);
}

#[test]
fn a_face_split_into_pieces_is_one_wall() {
    // One face in three pieces (as a polyline exploded), the other whole.
    let f = run(&[
        (v(0.0, 0.0), v(5000.0, 0.0)),
        (v(0.0, T), v(1000.0, T)),
        (v(1000.0, T), v(2500.0, T)),
        (v(2500.0, T), v(5000.0, T)),
    ]);
    assert_eq!(f.walls.len(), 1, "{f:?}");
    assert!((length(&f.walls[0]) - 5000.0).abs() < 1e-6);
    assert_eq!(f.walls[0].lines.len(), 4);
}

#[test]
fn a_gap_at_an_opening_leaves_two_walls_and_an_opening() {
    let mut segs = straight(0.0, 1000.0, T, true);
    segs.extend(straight(1900.0, 4000.0, T, true));
    let f = run(&segs);
    assert_eq!(f.walls.len(), 2, "{f:?}");
    assert_eq!(f.openings.len(), 1);
    assert!((f.openings[0].1 - 900.0).abs() < 1e-6);
    assert!((f.openings[0].0 - v(1450.0, T / 2.0)).length() < 1e-6);
    // The jamb lines go with the walls.
    assert_eq!(f.used().len(), 8);
}

/// An L: outer faces along the axes, inner faces `t` in, caps at the far ends.
fn l_corner(t: f64) -> Vec<(DVec2, DVec2)> {
    vec![
        (v(0.0, 0.0), v(3000.0, 0.0)),
        (v(0.0, 0.0), v(0.0, 2000.0)),
        (v(t, t), v(3000.0, t)),
        (v(t, t), v(t, 2000.0)),
        (v(3000.0, 0.0), v(3000.0, t)),
        (v(0.0, 2000.0), v(t, 2000.0)),
    ]
}

#[test]
fn an_l_corner_is_one_run_through_a_shared_point() {
    let f = run(&l_corner(T));
    assert_eq!(f.walls.len(), 1, "{f:?}");
    let w = &f.walls[0];
    assert_eq!(w.segs.len(), 2);
    assert!(!w.closed);
    // The runs meet where the centrelines cross.
    let corner = v(T / 2.0, T / 2.0);
    let shared = w.segs[0].2;
    assert_eq!(shared, w.segs[1].1);
    assert!((f.points[shared] - corner).length() < 1e-9);
    assert_eq!(w.lines.len(), 6);
}

#[test]
fn an_l_of_two_thicknesses_meets_at_the_centrelines() {
    // Horizontal 200 thick along y = 0..200, vertical 114.3 thick along x = 0..T.
    let f = run(&[
        (v(0.0, 0.0), v(3000.0, 0.0)),
        (v(0.0, 0.0), v(0.0, 2000.0)),
        (v(T, 200.0), v(3000.0, 200.0)),
        (v(T, 200.0), v(T, 2000.0)),
    ]);
    assert_eq!(f.walls.len(), 2, "{f:?}");
    let corner = v(T / 2.0, 100.0);
    assert!(f.points.iter().any(|p| (*p - corner).length() < 1e-9));
    let ts: BTreeSet<u64> = f.walls.iter().map(|w| w.thickness.to_bits()).collect();
    assert_eq!(ts.len(), 2);
}

#[test]
fn a_t_ends_on_the_host_centreline() {
    // Host along y = 0..T, its top face broken where the stem comes in.
    let (x0, x1) = (1500.0 - T / 2.0, 1500.0 + T / 2.0);
    let f = run(&[
        (v(0.0, 0.0), v(3000.0, 0.0)),
        (v(0.0, T), v(x0, T)),
        (v(x1, T), v(3000.0, T)),
        (v(x0, T), v(x0, 2000.0)),
        (v(x1, T), v(x1, 2000.0)),
    ]);
    assert_eq!(f.walls.len(), 2, "{f:?}");
    let host = f
        .walls
        .iter()
        .find(|w| (length(w) - 3000.0).abs() < 1e-6)
        .expect("the host runs through");
    assert_eq!(host.segs.len(), 1);
    let stem = f.walls.iter().find(|w| !std::ptr::eq(*w, host)).unwrap();
    let ends = [f.points[stem.segs[0].1], f.points[stem.segs[0].2]];
    assert!(
        ends.iter()
            .any(|p| (*p - v(1500.0, T / 2.0)).length() < 1e-9),
        "{ends:?}"
    );
    assert_eq!(f.used().len(), 5);
}

#[test]
fn an_x_crossing_keeps_both_walls_whole() {
    let h = T / 2.0;
    let f = run(&[
        // Horizontal, centred on y = 0, broken at x = ±h.
        (v(-2000.0, -h), v(-h, -h)),
        (v(h, -h), v(2000.0, -h)),
        (v(-2000.0, h), v(-h, h)),
        (v(h, h), v(2000.0, h)),
        // Vertical, centred on x = 0, broken at y = ±h.
        (v(-h, -2000.0), v(-h, -h)),
        (v(-h, h), v(-h, 2000.0)),
        (v(h, -2000.0), v(h, -h)),
        (v(h, h), v(h, 2000.0)),
    ]);
    assert_eq!(f.walls.len(), 2, "{f:?}");
    for w in &f.walls {
        assert!((length(w) - 4000.0).abs() < 1e-6);
    }
    assert!(f.openings.is_empty());
}

#[test]
fn a_room_is_one_closed_ring() {
    // Outer rectangle 4000 × 3000 and inner one T in.
    let (w, h) = (4000.0, 3000.0);
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
        vec![
            (v(x0, y0), v(x1, y0)),
            (v(x1, y0), v(x1, y1)),
            (v(x1, y1), v(x0, y1)),
            (v(x0, y1), v(x0, y0)),
        ]
    };
    let mut segs = rect(0.0, 0.0, w, h);
    segs.extend(rect(T, T, w - T, h - T));
    let f = run(&segs);
    assert_eq!(f.walls.len(), 1, "{f:?}");
    assert!(f.walls[0].closed);
    assert_eq!(f.walls[0].segs.len(), 4);
    assert!((length(&f.walls[0]) - 2.0 * ((w - T) + (h - T))).abs() < 1e-6);
    assert_eq!(f.points.len(), 4);
}

#[test]
fn concentric_arcs_are_a_curved_wall() {
    let c = v(0.0, 0.0);
    let arc = |r: f64| Seg::Arc {
        center: c,
        radius: r,
        a: v(r, 0.0),
        b: v(0.0, r),
        ccw: true,
    };
    let ls = vec![
        LineIn {
            id: EntityId(1),
            group: 0,
            seg: arc(2000.0),
            guide: false,
        },
        LineIn {
            id: EntityId(2),
            group: 0,
            seg: arc(2200.0),
            guide: false,
        },
    ];
    let f = find(&ls, &settings());
    assert_eq!(f.walls.len(), 1, "{f:?}");
    let w = &f.walls[0];
    assert_eq!(w.thickness, 200.0);
    let Seg::Arc { radius, .. } = w.segs[0].0 else {
        panic!("not an arc")
    };
    assert!((radius - 2100.0).abs() < 1e-9);
    assert!((length(w) - 2100.0 * PI / 2.0).abs() < 1e-6);
}

#[test]
fn a_curved_wall_meets_a_straight_one() {
    // A quarter-circle wall from (r, 0) to (0, r) and a straight wall carrying on from its
    // end along y = r (its faces at r ± 100).
    let c = v(0.0, 0.0);
    let arc = |r: f64| Seg::Arc {
        center: c,
        radius: r,
        a: v(r, 0.0),
        b: v(0.0, r),
        ccw: true,
    };
    let mut ls = vec![
        LineIn {
            id: EntityId(1),
            group: 0,
            seg: arc(1900.0),
            guide: false,
        },
        LineIn {
            id: EntityId(2),
            group: 0,
            seg: arc(2100.0),
            guide: false,
        },
    ];
    for (n, y) in [(3, 1900.0), (4, 2100.0)] {
        ls.push(LineIn {
            id: EntityId(n),
            group: 0,
            seg: Seg::Line {
                a: v(0.0, y),
                b: v(-3000.0, y),
            },
            guide: false,
        });
    }
    let f = find(&ls, &settings());
    // Tangent where they meet, on the same point: one run.
    assert_eq!(f.walls.len(), 1, "{f:?}");
    assert_eq!(f.walls[0].segs.len(), 2);
}

/// A grid of rooms' walls as face lines, broken at every junction (an exploded or imported
/// plan): outer faces whole, the rest between the walls they meet.
fn grid(rows: usize, cols: usize, w: f64, h: f64) -> Vec<(DVec2, DVec2)> {
    let t = T / 2.0;
    let mut out = Vec::new();
    for (count, other, step, other_step, flip) in
        [(rows, cols, h, w, false), (cols, rows, w, h, true)]
    {
        let end = other as f64 * other_step;
        for j in 0..=count {
            let at = j as f64 * step;
            for (side, outer) in [(-t, j == 0), (t, j == count)] {
                let mut spans = Vec::new();
                if outer {
                    spans.push((-t, end + t));
                } else {
                    for i in 0..other {
                        spans.push((i as f64 * other_step + t, (i + 1) as f64 * other_step - t));
                    }
                }
                for (a, b) in spans {
                    let (p, q) = (v(a, at + side), v(b, at + side));
                    out.push(if flip {
                        (v(p.y, p.x), v(q.y, q.x))
                    } else {
                        (p, q)
                    });
                }
            }
        }
    }
    out
}

#[test]
fn a_grid_of_rooms_comes_back_as_its_walls() {
    let f = run(&grid(2, 3, 4000.0, 3000.0));
    // The outside is one ring; each inner grid line is one wall through its crossings.
    assert_eq!(f.walls.len(), 1 + 1 + 2, "{f:?}");
    assert!(f.skipped.is_empty() && f.openings.is_empty() && f.partly.is_empty());
    assert_eq!(f.walls.iter().filter(|w| w.closed).count(), 1);
}

#[test]
fn a_table_is_not_a_wall() {
    // A 30″ × 60″ table: its sides are too far apart.
    let (w, d) = (60.0 * 25.4, 30.0 * 25.4);
    let f = run(&[
        (v(0.0, 0.0), v(w, 0.0)),
        (v(w, 0.0), v(w, d)),
        (v(w, d), v(0.0, d)),
        (v(0.0, d), v(0.0, 0.0)),
    ]);
    assert!(f.walls.is_empty() && f.skipped.is_empty(), "{f:?}");
}

#[test]
fn a_shelf_on_its_own_is_skipped() {
    // 12″ × 36″: wall-like spacing, but short and standing alone.
    let (w, d) = (36.0 * 25.4, 12.0 * 25.4);
    let f = run(&[
        (v(0.0, 0.0), v(w, 0.0)),
        (v(w, 0.0), v(w, d)),
        (v(w, d), v(0.0, d)),
        (v(0.0, d), v(0.0, 0.0)),
    ]);
    assert!(f.walls.is_empty(), "{f:?}");
    assert_eq!(f.skipped.len(), 1);
    assert_eq!(f.skipped[0].why, Why::Object);
}

#[test]
fn a_hatch_is_not_walls() {
    let segs: Vec<(DVec2, DVec2)> = (0..10)
        .map(|k| {
            let y = k as f64 * 100.0;
            (v(0.0, y), v(3000.0, y))
        })
        .collect();
    let f = run(&segs);
    assert!(f.walls.is_empty(), "{f:?}");
    assert!(f.skipped.iter().all(|s| s.why == Why::Pattern));
    assert!(!f.skipped.is_empty());
}

#[test]
fn two_walls_side_by_side_pair_with_their_own_faces() {
    // Two 4½″ walls with a 6″ gap between them: each line pairs with its own wall's face.
    let gap = 152.4;
    let mut segs = straight(0.0, 3000.0, T, false);
    segs.extend(
        straight(0.0, 3000.0, T, false)
            .into_iter()
            .map(|(a, b)| (a + v(0.0, T + gap), b + v(0.0, T + gap))),
    );
    let f = run(&segs);
    assert_eq!(f.walls.len(), 2, "{f:?}");
    assert!(f.walls.iter().all(|w| w.thickness == T));
}

#[test]
fn lines_of_different_groups_never_pair() {
    let mut ls = lines(&straight(0.0, 3000.0, T, false));
    ls[1].group = 1;
    assert!(find(&ls, &settings()).walls.is_empty());
}

#[test]
fn an_old_script_walls_centreline_goes_with_it() {
    let mut ls = lines(&straight(0.0, 3000.0, T, true));
    ls.push(LineIn {
        id: EntityId(9),
        group: 0,
        seg: Seg::Line {
            a: v(0.0, T / 2.0),
            b: v(3000.0, T / 2.0),
        },
        guide: true,
    });
    let f = find(&ls, &settings());
    assert_eq!(f.walls.len(), 1);
    assert!(f.walls[0].lines.contains(&EntityId(9)));
}

#[test]
fn far_from_the_origin_too() {
    // Imported plans often sit in survey coordinates.
    let o = v(6.0e6, 2.0e6);
    let segs: Vec<(DVec2, DVec2)> = l_corner(T)
        .into_iter()
        .map(|(a, b)| (a + o, b + o))
        .collect();
    let f = run(&segs);
    assert_eq!(f.walls.len(), 1, "{f:?}");
    assert_eq!(f.walls[0].segs.len(), 2);
}

#[test]
fn a_solid_centreline_between_the_faces_is_taken_as_one() {
    // An L whose centrelines are drawn as solid lines too.
    let mut segs = l_corner(T);
    segs.push((v(T / 2.0, T / 2.0), v(3000.0, T / 2.0)));
    segs.push((v(T / 2.0, T / 2.0), v(T / 2.0, 2000.0)));
    let f = run(&segs);
    assert_eq!(f.walls.len(), 1, "{f:?}");
    assert!(f.skipped.is_empty(), "{f:?}");
    assert_eq!(f.walls[0].lines.len(), 8);
}

#[test]
fn specks_on_a_face_are_left_alone() {
    // Tiny segments on a face (a PDF's dotted line or text strokes) aren't the wall's.
    let mut segs = straight(0.0, 3000.0, T, false);
    for k in 0..20 {
        let x = 100.0 + k as f64 * 100.0;
        segs.push((v(x, 0.0), v(x + 0.5, 0.0)));
    }
    let f = run(&segs);
    assert_eq!(f.walls.len(), 1);
    assert_eq!(f.walls[0].lines.len(), 2);
}
