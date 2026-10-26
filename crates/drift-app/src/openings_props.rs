//! Property tests for openings (docs/walls-design.md §9.2, the W3 rows): random openings on
//! the random wall networks of `walls_props`, then random edits (dragging centreline points,
//! sliding and resizing openings, flipping them, deleting walls). What the walls and the
//! openings made must always be what making everything again makes, keys must hold through
//! edits that keep them, each cut must take exactly width × thickness out of the poché, and
//! undo and saving must give the drawing back.
//!
//! Small case counts keep `cargo test` quick; the heavy run is behind `#[ignore]`
//! (`cargo test -p drift-app openings_props -- --ignored`).

use drift_core::{DVec2, EntityId, OpeningId, WallId};
use drift_geom::walls::regenerate;
use proptest::prelude::*;

use crate::editor::Editor;
use crate::walls_props::{Network, config, drawing, network};

const LIGHT: u32 = 16;

/// How many openings the cases placed (so a run that places none shows up).
static PLACED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[test]
fn openings_props_place_some() {
    // Two straight walls in an L: the tries land.
    let net = Network {
        n: 2,
        runs: vec![(vec![(0, 0), (1, 0), (1, 1)], 0, 0)],
        jitter: vec![(0.0, 0.0); 4],
    };
    openings_follow_edits(
        net,
        vec![(0, 0, 0.5, 800.0), (0, 1, 0.5, 600.0)],
        Vec::new(),
    );
    assert!(PLACED.load(std::sync::atomic::Ordering::Relaxed) >= 2);
}
const HEAVY: u32 = 300;

/// Openings to try: (wall, segment, place along it as a fraction, width).
type Tries = Vec<(usize, usize, f64, f64)>;

fn tries() -> impl Strategy<Value = Tries> {
    prop::collection::vec((0usize..8, 0usize..6, 0.1f64..0.9, 300.0f64..900.0), 1..6)
}

/// The walls' poché area, from what the generator makes of the document as it is, and the
/// notices it gives.
fn poche(ed: &Editor) -> (f64, Vec<String>) {
    let (inputs, _) = drift_geom::walls::regen::wall_inputs(&ed.doc);
    let openings = drift_geom::walls::opening_inputs(&ed.doc, &inputs);
    let g = drift_geom::walls::generate_cut(
        &inputs,
        &openings,
        drift_geom::walls::regen::snap(&ed.doc),
    );
    (g.regions().iter().map(|(_, _, r)| r.area).sum(), g.notices)
}

/// Whatever is in the document is what making it all again makes: walls and symbols.
fn matches_a_full_rebuild(ed: &Editor, what: &str) {
    let mut full = ed.doc.clone();
    let r = regenerate(&mut full);
    assert!(
        !r.changed,
        "{what}: the walls differ from a full rebuild: {r:?}"
    );
    let mut again = Editor::new(ed.doc.clone());
    again.doc.begin();
    let before = again.doc.clone();
    again.follow_openings(None);
    assert!(
        again.doc.same_sketch(&before),
        "{what}: the openings' symbols differ from a full rebuild"
    );
    again.doc.rollback();
}

/// Place the tries, each checked as it goes: a placed one cuts exactly its width × its
/// wall's thickness out of the poché (where the cut fits), and keeps every other wall's and
/// opening's ids.
fn place(ed: &mut Editor, tries: &Tries) -> Vec<OpeningId> {
    let walls: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
    let mut placed = Vec::new();
    if walls.is_empty() {
        return placed;
    }
    for &(w, k, along, width) in tries {
        let w = walls[w % walls.len()];
        let Some(segs) = ed
            .doc
            .wall(w)
            .and_then(|x| drift_geom::walls::regen::centreline(&ed.doc, x).ok())
            .map(|(s, ..)| s)
        else {
            continue;
        };
        let k = (k % segs.len()) as u16;
        let len = segs[usize::from(k)].length();
        ed.opening.family = "door.single".into();
        ed.opening.width = width;
        ed.opening.leaf = width * 0.9;
        let (before_area, troubled) = poche(ed);
        // (Faces and corners: a cut can join or split the cluster's outline loops, which
        // its lead wall keeps.)
        let others: Vec<_> = ed
            .doc
            .walls()
            .filter(|(id, _)| *id != w)
            .map(|(id, x)| (id, (x.made.curves.clone(), x.made.points.clone())))
            .collect();
        let symbols: Vec<_> = ed
            .doc
            .openings()
            .map(|(id, o)| (id, o.made.clone()))
            .collect();
        let Some(o) = ed.place_opening(w, k, len * along - width / 2.0) else {
            continue;
        };
        placed.push(o);
        let t = ed.doc.wall(w).unwrap().thickness;
        let (after_area, notices) = poche(ed);
        // (On a network the walls can't make whole (a segment shorter than its thickness at a
        // sharp join, say), there is no poché area to check against.)
        if !troubled.is_empty() {
            continue;
        }
        assert!(notices.is_empty(), "the cut made trouble: {notices:?}");
        let cut = before_area - after_area;
        assert!(
            (cut - width * t).abs() <= 1e-6 * before_area.max(1.0),
            "an opening {width} wide in a wall {t} thick took {cut} out of the poché ({:?},              of {before_area}; {:?})",
            ed.notice,
            ed.doc
                .opening(o)
                .map(|x| (x.wall, x.seg, ed.opening_span(o)))
        );
        for (id, made) in others {
            assert_eq!(
                ed.doc
                    .wall(id)
                    .map(|x| (x.made.curves.clone(), x.made.points.clone())),
                Some(made),
                "another wall's ids changed"
            );
        }
        for (id, made) in symbols {
            assert_eq!(
                ed.doc.opening(id).map(|x| &x.made),
                Some(&made),
                "another opening's symbol changed"
            );
        }
        matches_a_full_rebuild(ed, "after placing");
    }
    placed
}

/// Openings on random networks: placed, then the walls dragged about (in drag frames and
/// commits) and the openings slid, resized and flipped; every step matches a full rebuild,
/// a slide keeps every key, and undoing it all gives the drawing back.
fn openings_follow_edits(net: Network, tries: Tries, edits: Vec<(u8, usize, (f64, f64))>) {
    let mut ed = Editor::new(drawing(&net));
    ed.auto_relations = false;
    let start = ed.doc.clone();
    let depth = ed.history.undo_depth();
    let placed = place(&mut ed, &tries);
    PLACED.fetch_add(placed.len(), std::sync::atomic::Ordering::Relaxed);
    let points: Vec<EntityId> = {
        let centre: Vec<EntityId> = ed
            .doc
            .walls()
            .flat_map(|(_, w)| ed.doc.path(w.centre).unwrap().curves.clone())
            .collect();
        ed.doc.defining_points(centre).into_iter().collect()
    };
    for (kind, which, (dx, dy)) in edits {
        match kind % 4 {
            0 if !points.is_empty() => {
                let p = points[which % points.len()];
                if !ed.doc.contains(p) {
                    continue;
                }
                ed.doc.begin();
                for k in 1..=3 {
                    let _ = ed.drag(&[p], DVec2::new(dx, dy) / 3.0, &[]);
                    matches_a_full_rebuild(&ed, &format!("drag frame {k}"));
                }
                ed.commit("Drag");
                matches_a_full_rebuild(&ed, "after a drag");
            }
            1 if !placed.is_empty() => {
                let o = placed[which % placed.len()];
                let Some((u0, _)) = ed.opening_span(o) else {
                    continue;
                };
                let walls: Vec<_> = ed.doc.walls().map(|(id, w)| (id, w.made.clone())).collect();
                let symbol = ed.doc.opening(o).unwrap().made.clone();
                let cut = |ed: &Editor| {
                    let w = ed.doc.opening(o).unwrap().wall;
                    ed.doc.wall(w).is_some_and(|x| {
                        x.made
                            .curves
                            .keys()
                            .any(|k| k.from == drift_core::Station::Jamb(o, 0))
                    })
                };
                let was_cut = cut(&ed);
                ed.doc.begin();
                ed.slide_opening(o, u0 + dx / 4.0);
                ed.commit("Move opening");
                matches_a_full_rebuild(&ed, "after a slide");
                // Sliding keeps every key: no id anywhere changes (unless the slide took it
                // in or out of where it can be cut: an edit had left it across a corner).
                if !(was_cut && cut(&ed)) {
                    continue;
                }
                for (id, made) in walls {
                    let now = ed.doc.wall(id).map(|w| w.made.clone()).unwrap_or_default();
                    let differ: Vec<String> = made
                        .curves
                        .iter()
                        .filter(|(k, e)| now.curves.get(k) != Some(e))
                        .map(|(k, e)| format!("{k:?} {e:?} -> {:?}", now.curves.get(k)))
                        .chain(
                            made.points
                                .iter()
                                .filter(|(k, e)| now.points.get(k) != Some(e))
                                .map(|(k, e)| format!("{k:?} {e:?} -> {:?}", now.points.get(k))),
                        )
                        .collect();
                    assert!(
                        differ.is_empty() && now.outlines == made.outlines,
                        "slide: ids of {id}: {differ:?} ({:?})",
                        ed.notice
                    );
                }
                assert_eq!(ed.doc.opening(o).unwrap().made, symbol, "slide: symbol ids");
            }
            2 if !placed.is_empty() => {
                let o = placed[which % placed.len()];
                if ed.doc.opening(o).is_none() {
                    continue;
                }
                let _ = ed.set_opening_width(o, &format!("{}", 300.0 + dy.abs()));
                ed.flip_openings(&[o], crate::openings::Flip::Facing);
                matches_a_full_rebuild(&ed, "after a resize and a flip");
            }
            _ => {
                let walls: Vec<WallId> = ed.doc.walls().map(|(id, _)| id).collect();
                if walls.is_empty() {
                    continue;
                }
                let w = walls[which % walls.len()];
                ed.selection = ed.wall_curves(w).into_iter().collect();
                ed.delete_selection();
                assert!(
                    ed.doc
                        .openings()
                        .all(|(_, o)| ed.doc.wall(o.wall).is_some()),
                    "an opening outlived its wall"
                );
                matches_a_full_rebuild(&ed, "after deleting a wall");
            }
        }
    }
    // Saved and opened again: the same, and nothing to make again.
    let text = drift_io::save(&ed.doc, &drift_io::Metadata::default());
    let (loaded, _) = drift_io::load(&text).unwrap();
    assert_eq!(loaded, ed.doc, "round trip");
    matches_a_full_rebuild(&Editor::new(loaded), "after opening the file");
    // All undone: the drawing as it was.
    while ed.history.undo_depth() > depth {
        ed.undo();
    }
    assert!(ed.doc.same_sketch(&start.sketch_copy()));
}

proptest! {
    #![proptest_config(config(LIGHT))]

    #[test]
    fn openings_props_follow_edits_and_match_a_full_rebuild(
        net in network(),
        tries in tries(),
        edits in prop::collection::vec((0u8..4, 0usize..16, (-300.0f64..300.0, -300.0f64..300.0)), 1..5),
    ) {
        openings_follow_edits(net, tries, edits);
    }
}

proptest! {
    #![proptest_config(config(HEAVY))]

    #[test]
    #[ignore = "heavy: cargo test -p drift-app openings_props -- --ignored"]
    fn openings_props_heavy(
        net in network(),
        tries in tries(),
        edits in prop::collection::vec((0u8..4, 0usize..16, (-300.0f64..300.0, -300.0f64..300.0)), 1..8),
    ) {
        openings_follow_edits(net, tries, edits);
    }
}

/// A case the heavy run found: an inside corner past the end of its segment (a wall justified
/// to its face meeting at an angle). The opening must stay within the segment to be cut.
#[test]
fn openings_props_stay_within_their_segment() {
    let net = Network {
        n: 2,
        runs: vec![
            (vec![(1, 0), (0, 0), (0, 1)], 0, 0),
            (vec![(0, 1), (1, 1)], 0, 2),
        ],
        jitter: vec![
            (0.0, 0.0),
            (0.0, -28.059_272_800_599_55),
            (0.0, 0.0),
            (0.0, 0.0),
        ],
    };
    openings_follow_edits(
        net,
        vec![(2, 5, 0.825_049_468_269_934_7, 789.436_478_530_000_3)],
        Vec::new(),
    );
}
