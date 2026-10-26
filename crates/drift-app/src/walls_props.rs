//! Property tests for walls (docs/walls-design.md §9.2, the W1 rows): random wall networks on
//! a jittered grid (runs along grid lines, joined in L, T and n-way nodes where they share
//! grid points, crossing in X where they cross), with mixed thicknesses and justifications,
//! are regenerated and edited, and what the walls make is checked against what it should be.
//!
//! The case counts are small so `cargo test` stays quick; each property also has a heavier run
//! behind `#[ignore]` (`cargo test -p drift-app walls_props -- --ignored`).

use std::collections::BTreeMap;

use drift_core::{DVec2, Document, EntityId, Justify, LayerId, LineKind, Part, Path, Wall, WallId};
use drift_geom::walls::{Seg, generate, regen, regenerate};
use proptest::prelude::*;

use crate::editor::Editor;

const LIGHT: u32 = 24;
const HEAVY: u32 = 400;
/// Grid spacing: walls are well clear of each other's thickness.
const GRID: f64 = 1000.0;
const THICK: [f64; 3] = [100.0, 150.0, 200.0];

/// A run: its grid nodes, its thickness (an index into [`THICK`]) and justification.
type Run = (Vec<(usize, usize)>, usize, u8);

/// A random network: grid size, runs (each a list of grid nodes, consecutive ones
/// neighbours), and a jitter for each node.
#[derive(Clone, Debug)]
pub(crate) struct Network {
    pub(crate) n: usize,
    pub(crate) runs: Vec<Run>,
    pub(crate) jitter: Vec<(f64, f64)>,
}

pub(crate) fn network() -> impl Strategy<Value = Network> {
    (2usize..5).prop_flat_map(|n| {
        let node = (0..n, 0..n);
        let run = (
            node,
            prop::collection::vec(0u8..4, 1..5),
            0usize..THICK.len(),
            0u8..3,
        );
        (
            Just(n),
            prop::collection::vec(run, 1..6),
            prop::collection::vec((-60.0f64..60.0, -60.0f64..60.0), n * n),
        )
            .prop_map(|(n, runs, jitter)| Network {
                n,
                runs: runs
                    .into_iter()
                    .map(|(start, steps, t, j)| {
                        // A walk on the grid that never steps back on itself.
                        let mut at = start;
                        let mut path = vec![at];
                        for s in steps {
                            let (dx, dy): (isize, isize) = match s {
                                0 => (1, 0),
                                1 => (0, 1),
                                2 => (-1, 0),
                                _ => (0, -1),
                            };
                            let (x, y) = (at.0 as isize + dx, at.1 as isize + dy);
                            if x < 0 || y < 0 || x >= n as isize || y >= n as isize {
                                continue;
                            }
                            let next = (x as usize, y as usize);
                            if path.contains(&next) {
                                continue;
                            }
                            path.push(next);
                            at = next;
                        }
                        (path, t, j)
                    })
                    .filter(|(p, ..)| p.len() >= 2)
                    .collect(),
                jitter,
            })
    })
}

/// The network as a drawing: shared grid points, one wall per run (a grid edge already
/// walled is left out of later runs, so walls don't run along each other), regenerated.
pub(crate) fn drawing(net: &Network) -> Document {
    let mut doc = Document::new();
    let mut points: BTreeMap<(usize, usize), EntityId> = BTreeMap::new();
    let mut used: std::collections::BTreeSet<((usize, usize), (usize, usize))> = Default::default();
    for (path, t, j) in &net.runs {
        let mut curves = Vec::new();
        for w in path.windows(2) {
            let edge = (w[0].min(w[1]), w[0].max(w[1]));
            if !used.insert(edge) {
                break;
            }
            let mut pt = |doc: &mut Document, g: (usize, usize)| {
                *points.entry(g).or_insert_with(|| {
                    let (jx, jy) = net.jitter[g.0 * net.n + g.1];
                    doc.add_point(DVec2::new(g.0 as f64 * GRID + jx, g.1 as f64 * GRID + jy))
                })
            };
            let (a, b) = (pt(&mut doc, w[0]), pt(&mut doc, w[1]));
            let l = doc.add_line(a, b).unwrap();
            doc.set_style(l, LineKind::Construction);
            curves.push(l);
        }
        if curves.is_empty() {
            continue;
        }
        let path = doc
            .add_path(Path {
                curves,
                closed: false,
            })
            .unwrap();
        let mut wall = Wall::new(path, THICK[*t], LayerId::DEFAULT);
        wall.justify = [Justify::Centre, Justify::Left, Justify::Right][usize::from(*j)];
        doc.add_wall(wall).unwrap();
    }
    regenerate(&mut doc);
    doc
}

pub(crate) fn config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Regenerating again changes nothing (the same ids, the same geometry).
fn idempotent(net: Network) {
    let mut doc = drawing(&net);
    let before = doc.clone();
    let report = regenerate(&mut doc);
    assert!(!report.changed, "{report:?}");
    assert_eq!(doc, before);
}

/// The same network built in another order makes the same outlines (by geometry), and
/// every loop closes.
fn order_independent(net: Network) {
    let doc = drawing(&net);
    let (mut inputs, _) = regen::wall_inputs(&doc);
    let snap = regen::snap(&doc);
    let a = generate(&inputs, snap);
    inputs.reverse();
    let b = generate(&inputs, snap);
    let corners = |g: &drift_geom::walls::Generated| {
        let mut p: Vec<(u64, u64)> = g
            .points
            .values()
            .map(|(_, p)| (p.x.to_bits(), p.y.to_bits()))
            .collect();
        p.sort_unstable();
        p
    };
    assert_eq!(corners(&a), corners(&b));
    assert_eq!(a.loops.len(), b.loops.len());
    let area = |g: &drift_geom::walls::Generated| -> f64 {
        g.regions().iter().map(|(_, _, r)| r.area).sum()
    };
    assert!((area(&a) - area(&b)).abs() <= 1e-6 * area(&a).max(1.0));
    for l in &a.loops {
        let lp = a.loop_of(l).expect("a loop");
        for (i, e) in lp.edges.iter().enumerate() {
            let next = &lp.edges[(i + 1) % lp.edges.len()];
            assert!(e.end().distance(next.start()) < 1e-6, "an open loop");
        }
    }
}

/// Every straight face piece lies at exactly its offset from its centreline.
fn parallel_at_thickness(net: Network) {
    let doc = drawing(&net);
    for (_, w) in doc.walls() {
        let Ok((segs, ..)) = regen::centreline(&doc, w) else {
            continue;
        };
        let (ol, or) = w.offsets();
        for (k, id) in &w.made.curves {
            let o = match k.part {
                Part::Left => ol,
                Part::Right => or,
                _ => continue,
            };
            let Seg::Line { a, b } = segs[usize::from(k.seg)] else {
                continue;
            };
            let n = (b - a).normalize().perp();
            let Some(drift_core::Shape::Line { a: p, b: q }) = doc.shape(*id) else {
                panic!("a face is a line");
            };
            for x in [p, q] {
                let off = (x - a).dot(n);
                assert!(
                    (off - o).abs() <= 1e-9 * GRID * 10.0,
                    "{k:?} lies {off} off, not {o}"
                );
            }
        }
    }
}

/// Moving centreline points a little keeps every generated entity's id where the joins stay
/// what they were (the same keys), makes and deletes nothing, and journals only what moved.
fn stable_keys(net: Network, nudge: (f64, f64)) {
    let mut doc = drawing(&net);
    let made: BTreeMap<WallId, drift_core::Made> =
        doc.walls().map(|(id, w)| (id, w.made.clone())).collect();
    let centre: Vec<EntityId> = doc
        .walls()
        .flat_map(|(_, w)| doc.path(w.centre).unwrap().curves.clone())
        .collect();
    let Some(p) = doc
        .defining_points(centre.iter().copied())
        .into_iter()
        .next()
    else {
        return;
    };
    doc.begin();
    let at = doc.point(p).unwrap();
    doc.set_point(p, at + DVec2::new(nudge.0, nudge.1)).unwrap();
    let report = regenerate(&mut doc);
    let tx = doc.commit("Nudge");
    let now: BTreeMap<WallId, drift_core::Made> =
        doc.walls().map(|(id, w)| (id, w.made.clone())).collect();
    // (A nudge can change what a join is: two walls of different thickness meeting nearly
    // straight on go from a bevel to a corner. Then the keys say so; the property is for the
    // edits that keep them.)
    let keys = |m: &BTreeMap<WallId, drift_core::Made>| -> Vec<(WallId, Vec<drift_core::FaceKey>)> {
        m.iter()
            .map(|(w, made)| (*w, made.curves.keys().copied().collect()))
            .collect()
    };
    if keys(&now) != keys(&made) {
        return;
    }
    assert_eq!((report.created, report.deleted), (0, 0), "{report:?}");
    assert_eq!(now, made);
    // Only what moved is journalled: the point, and generated entities whose geometry
    // changed (no more than every generated point).
    let generated = doc.generated_entities().len();
    assert!(tx.entities().count() <= generated + 1);
}

/// Any edit sequence (moving nodes, re-thickening, flipping, deleting a wall), fully undone,
/// gives the drawing back; and free straight walls need no solve. (A drag of one still
/// solves where it carries a joined wall's free end along at its length, as a drag of
/// lines does.)
fn undo_restores(net: Network, edits: Vec<(u8, usize, (f64, f64))>) {
    let mut ed = Editor::new(drawing(&net));
    ed.auto_relations = false;
    let start = ed.doc.clone();
    let walls: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
    if walls.is_empty() {
        return;
    }
    let mut done = 0;
    for (kind, which, (dx, dy)) in edits {
        let w = walls[which % walls.len()];
        if ed.doc.wall(w).is_none() {
            continue;
        }
        let before = ed.history.undo_depth();
        match kind % 4 {
            0 => {
                let curves = ed.wall_curves(w);
                let scope = drift_solver::connected(&ed.doc, curves.iter().copied());
                assert!(
                    !drift_solver::needs_solve(&ed.doc, &scope),
                    "free straight walls need no solve"
                );
                ed.doc.begin();
                ed.drag(&curves, DVec2::new(dx, dy), &[]);
                ed.commit("Move");
            }
            1 => {
                let _ = ed.set_wall_thickness(&[w], "120");
            }
            2 => {
                let j = ed.doc.wall(w).unwrap().justify.flipped();
                ed.set_wall_justify(&[w], j);
            }
            _ => {
                ed.selection = ed.wall_curves(w).into_iter().collect();
                ed.delete_selection();
            }
        }
        done += ed.history.undo_depth() - before;
    }
    for _ in 0..done {
        ed.undo();
    }
    assert!(ed.doc.same_sketch(&start.sketch_copy()));
}

/// Whatever the edits (dragging centreline points, the fast path when joins stay, the full
/// rebuild when they don't), what the walls made is what making them all again makes: in a
/// drag frame, with the joins the drag keeps; once it is let go, with the joins worked out
/// afresh.
fn edits_match_a_full_rebuild(net: Network, drags: Vec<(usize, (f64, f64))>) {
    let mut ed = Editor::new(drawing(&net));
    ed.auto_relations = false;
    let points: Vec<EntityId> = {
        let centre: Vec<EntityId> = ed
            .doc
            .walls()
            .flat_map(|(_, w)| ed.doc.path(w.centre).unwrap().curves.clone())
            .collect();
        ed.doc.defining_points(centre).into_iter().collect()
    };
    if points.is_empty() {
        return;
    }
    for (which, (dx, dy)) in drags {
        let p = points[which % points.len()];
        if !ed.doc.contains(p) {
            continue;
        }
        // A drag in a few frames, then let go.
        ed.doc.begin();
        for k in 1..=3 {
            let step = DVec2::new(dx, dy) * (f64::from(k) / 3.0);
            let _ = ed.drag(
                &[p],
                step - DVec2::new(dx, dy) * (f64::from(k - 1) / 3.0),
                &[],
            );
            let mut full = ed.doc.clone();
            let kept = ed.drag_kept_joins();
            assert!(
                !drift_geom::walls::regenerate_with(&mut full, kept.as_deref()).changed,
                "a drag frame left the walls other than a full rebuild makes them"
            );
        }
        ed.commit("Drag");
        let mut full = ed.doc.clone();
        let r = regenerate(&mut full);
        assert!(!r.changed, "after a drag: {r:?}");
    }
}

/// Saved and opened again, nothing needs regenerating.
fn round_trip(net: Network) {
    let doc = drawing(&net);
    let text = drift_io::save(&doc, &drift_io::Metadata::default());
    let (mut again, _) = drift_io::load(&text).unwrap();
    assert_eq!(again, doc);
    assert!(!regenerate(&mut again).changed);
}

proptest! {
    #![proptest_config(config(LIGHT))]

    #[test]
    fn walls_props_regenerating_is_idempotent(net in network()) {
        idempotent(net);
    }

    #[test]
    fn walls_props_outlines_dont_depend_on_order(net in network()) {
        order_independent(net);
    }

    #[test]
    fn walls_props_faces_are_parallel_at_thickness(net in network()) {
        parallel_at_thickness(net);
    }

    #[test]
    fn walls_props_small_moves_keep_ids(net in network(), nudge in (-30.0f64..30.0, -30.0f64..30.0)) {
        stable_keys(net, nudge);
    }

    #[test]
    fn walls_props_undo_restores(
        net in network(),
        edits in prop::collection::vec((0u8..4, 0usize..8, (-200.0f64..200.0, -200.0f64..200.0)), 1..5),
    ) {
        undo_restores(net, edits);
    }

    #[test]
    fn walls_props_round_trip(net in network()) {
        round_trip(net);
    }

    #[test]
    fn walls_props_edits_match_a_full_rebuild(
        net in network(),
        drags in prop::collection::vec((0usize..64, (-300.0f64..300.0, -300.0f64..300.0)), 1..5),
    ) {
        edits_match_a_full_rebuild(net, drags);
    }
}

proptest! {
    #![proptest_config(config(HEAVY))]

    #[test]
    #[ignore = "heavy: cargo test -p drift-app walls_props -- --ignored"]
    fn walls_props_heavy_idempotent_and_ordered(net in network()) {
        idempotent(net.clone());
        order_independent(net.clone());
        parallel_at_thickness(net);
    }

    #[test]
    #[ignore = "heavy: cargo test -p drift-app walls_props -- --ignored"]
    fn walls_props_heavy_edits(
        net in network(),
        nudge in (-30.0f64..30.0, -30.0f64..30.0),
        edits in prop::collection::vec((0u8..4, 0usize..8, (-200.0f64..200.0, -200.0f64..200.0)), 1..8),
    ) {
        stable_keys(net.clone(), nudge);
        undo_restores(net.clone(), edits);
        round_trip(net.clone());
        edits_match_a_full_rebuild(net, vec![(0, nudge), (5, (nudge.1 * 7.0, nudge.0 * 7.0))]);
    }
}
