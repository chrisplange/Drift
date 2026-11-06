//! A whole move kept on release (`Editor::land_whole_move`), a drag that goes on without
//! looking at its points (`Editor::drag`'s session check), and the counters they lean on:
//! each against the long way round, digest for digest.

use drift_core::{Constraint, DVec2, EntityId, EntityKind, FillKind, Hatch};
use drift_geom::SnapOptions;
use drift_solver::Param;

use super::{Editor, Pointer};
use crate::tools::{Input, Modifiers, SelectTool, Tool, ToolKey};

const TOL: f64 = 0.5;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A snapped pointer at `at`, as the canvas would produce it.
fn ptr(ed: &Editor, tool: &dyn Tool, at: DVec2) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    if tool.wants_snap() {
        p.snap = ed.index.snap(
            at,
            SnapOptions {
                tolerance: TOL,
                grid: None,
                from: None,
            },
            |id| tool.snap_excludes(ed, id),
        );
    }
    tool.refine_pointer(&mut p);
    p
}

/// Everything the drawing holds, bit for bit (no revision or structure counters), and what
/// the editor kept about it: the undo history's last step and depth, the areas' key and an
/// edit's fills still to follow, the fills' shapes, which were found crossed, the picked
/// region, what overlaps, and the scene index's answers over a grid.
fn digest(ed: &mut Editor) -> String {
    let mut out = String::new();
    use std::fmt::Write;
    writeln!(out, "{}", drift_io::save(&ed.doc, &Default::default())).unwrap();
    for (id, e) in ed.doc.iter() {
        writeln!(out, "{id:?} {e:?}").unwrap();
    }
    for (id, c) in ed.doc.constraints() {
        writeln!(out, "{id:?} {c:?}").unwrap();
    }
    for (id, f) in ed.doc.fills() {
        writeln!(out, "{id:?} {f:?}").unwrap();
    }
    let last = ed.history.take_last();
    writeln!(out, "last step {last:?}").unwrap();
    if let Some(tx) = last {
        ed.history.put_back(tx);
    }
    writeln!(
        out,
        "history {} {} {:?} {:?}",
        ed.history.recorded(),
        ed.history.undo_depth(),
        ed.history.undo_label(),
        ed.history.redo_label()
    )
    .unwrap();
    writeln!(
        out,
        "pending {:?}",
        ed.areas.pending.as_ref().map(|p| (p.depth, p.recorded))
    )
    .unwrap();
    // (Its geometry counter is a value no other document ever has: the rest of it.)
    let key = format!("{:?}", ed.regions_key());
    let key = key.split_once(", ").map_or(key.as_str(), |(_, rest)| rest);
    writeln!(out, "areas key {key}").unwrap();
    let mut crossed: Vec<_> = ed.fill_crossed.iter().map(|(k, v)| (*k, *v)).collect();
    crossed.sort_unstable();
    writeln!(out, "crossed {crossed:?}").unwrap();
    writeln!(out, "region {:?}", ed.selected_region).unwrap();
    writeln!(out, "fill shapes {:?}", ed.fill_shapes()).unwrap();
    writeln!(out, "overlapping {:?}", ed.overlapping()).unwrap();
    writeln!(out, "regions {:?}", ed.regions()).unwrap();
    ed.sync();
    for x in -2..14 {
        for y in -2..10 {
            let at = v(x as f64 * 9.7, y as f64 * 9.3);
            let mut near: Vec<_> = ed.index.near(at, 4.0).map(|(id, ..)| id).collect();
            near.sort_unstable();
            writeln!(out, "near {x},{y} {near:?}").unwrap();
        }
    }
    out
}

/// `a` and `b` are the same digest; else the lines that differ.
#[track_caller]
fn same(a: &str, b: &str, what: &str) {
    if a == b {
        return;
    }
    let diff: Vec<String> = a
        .lines()
        .zip(b.lines())
        .filter(|(x, y)| x != y)
        .take(8)
        .map(|(x, y)| {
            format!(
                "  kept:   {x}
  redone: {y}"
            )
        })
        .collect();
    panic!(
        "{what}: digests differ ({} vs {} lines):
{}",
        a.lines().count(),
        b.lines().count(),
        diff.join(
            "
"
        )
    );
}

/// A drawing of free curves (lines, an arc, a circle, a pen curve), a hatched rectangle and
/// loose lines, every point at an inexact coordinate.
fn free_drawing() -> Editor {
    let mut ed = Editor::default();
    ed.doc.begin();
    let mut lines = Vec::new();
    for k in 0..3 {
        let x = k as f64 * 31.7 + 0.1;
        let p = [(0.0, 0.3), (23.1, 0.0), (23.1, 17.3), (0.0, 17.3)]
            .map(|(a, b)| ed.doc.add_point(v(x + a, b)));
        for i in 0..4 {
            lines.push(ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
        }
    }
    let c = ed.doc.add_point(v(110.3, 40.1));
    ed.doc.add_circle(c, 7.7).unwrap();
    let (ac, a0, a1) = (
        ed.doc.add_point(v(20.0, 50.0)),
        ed.doc.add_point(v(31.3, 50.0)),
        ed.doc.add_point(v(20.0, 61.30000001)),
    );
    ed.doc.add_arc(ac, a0, a1).unwrap();
    let b =
        [v(50.0, 50.0), v(55.0, 60.0), v(65.0, 40.0), v(70.0, 50.0)].map(|p| ed.doc.add_point(p));
    ed.doc.add_bezier(b[0], b[1], b[2], b[3]).unwrap();
    for k in 0..5 {
        let a = ed.doc.add_point(v(k as f64 * 7.3, 75.1));
        let b = ed.doc.add_point(v(k as f64 * 7.3 + 5.1, 83.9));
        ed.doc.add_line(a, b).unwrap();
    }
    ed.commit("Setup");
    ed.prefs.fill_style.kind = FillKind::Hatch(Hatch::new("ANSI37"));
    ed.add_fill_at(v(5.0, 5.0)).unwrap();
    ed.settle_areas();
    let _ = lines;
    ed
}

/// The modal move of everything picked by `pick`: G, the pointer through `path`, a click at
/// its last point. Returns how many moves were kept open on release.
fn modal_move(ed: &mut Editor, pick: &dyn Fn(&mut Editor), path: &[DVec2]) -> u32 {
    modal_move_from(ed, pick, v(1.0, 1.0), path)
}

/// [`modal_move`] with G pressed at `from`.
fn modal_move_from(
    ed: &mut Editor,
    pick: &dyn Fn(&mut Editor),
    from: DVec2,
    path: &[DVec2],
) -> u32 {
    pick(ed);
    let mut tool = SelectTool::default();
    let before = ed.landed_whole;
    let p = ptr(ed, &tool, from);
    tool.key(ToolKey::G, Some(&p), ed);
    for &at in path {
        let p = ptr(ed, &tool, at);
        tool.handle(Input::Hover(p), ed);
    }
    let last = *path.last().unwrap();
    let p = ptr(ed, &tool, last);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
    ed.landed_whole - before
}

/// The same move on two copies of a drawing, one keeping the open move where it can, the
/// other the long way round: the same drawing, history and caches after it, after its fills
/// settle, after undo and after redo. Returns whether the move was kept open.
fn kept_lands_as_redone(
    build: &dyn Fn() -> Editor,
    pick: &dyn Fn(&mut Editor),
    path: &[DVec2],
) -> bool {
    let mut kept = build();
    let mut redone = build();
    redone.land_whole_off = true;
    let landed = modal_move(&mut kept, pick, path);
    assert_eq!(modal_move(&mut redone, pick, path), 0);
    same(&digest(&mut kept), &digest(&mut redone), "landed");
    kept.settle_areas();
    redone.settle_areas();
    same(&digest(&mut kept), &digest(&mut redone), "settled");
    assert!(kept.undo() && redone.undo());
    same(&digest(&mut kept), &digest(&mut redone), "undone");
    assert!(kept.redo() && redone.redo());
    same(&digest(&mut kept), &digest(&mut redone), "redone");
    landed == 1
}

/// A pointer path of inexact steps (none a round number of millimetres).
fn wobbly_path() -> Vec<DVec2> {
    (1..=23)
        .map(|k| v(1.0 + 0.37 * k as f64, 1.0 + 0.113 * (k * k) as f64 / 7.0))
        .collect()
}

#[test]
fn a_whole_move_kept_open_lands_as_one_redone_from_the_start() {
    let all = |ed: &mut Editor| ed.select_all();
    assert!(kept_lands_as_redone(&free_drawing, &all, &wobbly_path()));
    // The hatched rectangle alone (its hatch carried along).
    let first_rect = |ed: &mut Editor| {
        let ids: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .take(4)
            .collect();
        ed.selection = ids.into_iter().collect();
    };
    assert!(kept_lands_as_redone(
        &free_drawing,
        &first_rect,
        &wobbly_path()
    ));
    // The round curves alone.
    let rounds = |ed: &mut Editor| {
        let ids: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| {
                matches!(
                    e.kind,
                    EntityKind::Arc { .. } | EntityKind::Circle { .. } | EntityKind::Bezier { .. }
                )
            })
            .map(|(id, _)| id)
            .collect();
        ed.selection = ids.into_iter().collect();
    };
    assert!(kept_lands_as_redone(&free_drawing, &rounds, &wobbly_path()));
    // Far from the origin, where the frames' offsets added up round differently.
    let far = || {
        let mut ed = free_drawing();
        ed.doc.begin();
        let all: Vec<EntityId> = ed.doc.iter().map(|(id, _)| id).collect();
        ed.doc.translate(all, v(1.0e6 + 0.1, -1.0e6 + 0.3));
        ed.commit("Far");
        ed
    };
    // (The pointer path is the same: a move is relative to where the G began.)
    let all_far = |ed: &mut Editor| ed.select_all();
    assert!(kept_lands_as_redone(&far, &all_far, &wobbly_path()));
}

#[test]
fn a_whole_move_lands_at_its_offset_not_the_frames_added_up() {
    // G pressed and moved where nothing snaps: each frame moves by the pointer's step, and
    // those steps added up miss the pointer's offset in the last bit. Kept open, the move
    // still lands where one move by that offset puts it (a point at 0 shows the bit).
    let from = v(150.0, 150.0);
    // (Out a long way and back: a step back from far off is rounded, so the steps no
    // longer add up to the offset exactly.)
    let path = vec![
        from + v(1.1, 0.3),
        from + v(733.77, 512.25),
        from + v(0.013, 0.1 + 1.0e-3 / 3.0),
    ];
    let mut added = DVec2::ZERO;
    let mut last = DVec2::ZERO;
    for &p in &path {
        added += (p - from) - last;
        last = p - from;
    }
    assert_ne!(
        added, last,
        "the steps add up to the offset exactly: no bit to show"
    );
    let all = |ed: &mut Editor| ed.select_all();
    let mut kept = free_drawing();
    let mut redone = free_drawing();
    redone.land_whole_off = true;
    assert_eq!(modal_move_from(&mut kept, &all, from, &path), 1);
    assert_eq!(modal_move_from(&mut redone, &all, from, &path), 0);
    same(&digest(&mut kept), &digest(&mut redone), "landed");
}

#[test]
fn a_move_that_solved_or_was_restarted_goes_the_long_way() {
    // A part that solves (a horizontal line held at a point): no whole move to keep.
    let pinned = || {
        let mut ed = free_drawing();
        ed.doc.begin();
        let a = ed.doc.add_point(v(0.0, 100.0));
        let b = ed.doc.add_point(v(20.0, 100.0));
        let l = ed.doc.add_line(a, b).unwrap();
        ed.doc
            .add_constraint(Constraint::Horizontal { line: l })
            .unwrap();
        ed.commit("Pinned");
        ed
    };
    let all = |ed: &mut Editor| ed.select_all();
    assert!(!kept_lands_as_redone(&pinned, &all, &wobbly_path()));

    // A drag restarted once by an edit between its frames, then whole to the end.
    let mut kept = free_drawing();
    let mut redone = free_drawing();
    redone.land_whole_off = true;
    for ed in [&mut kept, &mut redone] {
        ed.select_all();
        let mut tool = SelectTool::default();
        let p = ptr(ed, &tool, v(1.0, 1.0));
        tool.key(ToolKey::G, Some(&p), ed);
        let path = wobbly_path();
        for (k, &at) in path.iter().enumerate() {
            if k == 5 {
                // (Something else moves a point the drag moves: the drag starts again.)
                let id = ed.selection.iter().copied().next().unwrap();
                let p = ed.doc.defining_points([id]).into_iter().next().unwrap();
                let at = ed.doc.point(p).unwrap() + v(0.0, 1.0e-3);
                ed.doc.set_point(p, at).unwrap();
            }
            let p = ptr(ed, &tool, at);
            tool.handle(Input::Hover(p), ed);
        }
        let p = ptr(ed, &tool, *path.last().unwrap());
        tool.handle(Input::Click(p, Modifiers::default()), ed);
    }
    assert_eq!(kept.landed_whole, 0, "a restarted drag isn't kept open");
    same(&digest(&mut kept), &digest(&mut redone), "after");
}

#[test]
fn an_area_move_goes_the_long_way() {
    // A picked area lets go of its neighbours: its points are new ones on each try.
    let mut kept = free_drawing();
    kept.change_rules(|r| r.regions_need_fill = false);
    let mut redone = free_drawing();
    redone.change_rules(|r| r.regions_need_fill = false);
    redone.land_whole_off = true;
    for ed in [&mut kept, &mut redone] {
        let mut tool = SelectTool::default();
        let p = ptr(ed, &tool, v(40.0, 5.0));
        tool.handle(Input::Hover(p), ed);
        tool.handle(Input::Click(p, Modifiers::default()), ed);
        assert!(ed.region().is_some());
        let p = ptr(ed, &tool, v(40.0, 5.0));
        tool.key(ToolKey::G, Some(&p), ed);
        for &at in &wobbly_path() {
            let p = ptr(ed, &tool, at + v(40.0, 5.0));
            tool.handle(Input::Hover(p), ed);
        }
        let p = ptr(ed, &tool, *wobbly_path().last().unwrap() + v(40.0, 5.0));
        tool.handle(Input::Click(p, Modifiers::default()), ed);
    }
    assert_eq!(kept.landed_whole, 0);
    same(&digest(&mut kept), &digest(&mut redone), "after");
}

/// The dragged parameters of moving `ids` whole.
fn dragged(ed: &Editor, ids: &[EntityId]) -> Vec<Param> {
    ed.doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect()
}

/// Every entity and fill, bit for bit.
fn drawing(ed: &Editor) -> String {
    let mut out = String::new();
    for (id, e) in ed.doc.iter() {
        out += &format!("{id:?} {e:?}\n");
    }
    for (id, f) in ed.doc.fills() {
        out += &format!("{id:?} {f:?}\n");
    }
    out
}

/// Frames of a drag of `ids` on two copies of a drawing, one telling where its points are
/// without looking when nothing changed, the other always looking, with `between` done to
/// both before each frame: the same drawing after every frame.
fn goes_on_as_looking(
    build: &dyn Fn() -> Editor,
    ids: &dyn Fn(&Editor) -> Vec<EntityId>,
    between: &dyn Fn(&mut Editor, usize),
) {
    let mut fast = build();
    let mut looking = build();
    looking.drag_always_look = true;
    let ids_f = ids(&fast);
    let d = dragged(&fast, &ids_f);
    fast.doc.begin();
    looking.doc.begin();
    for k in 0..12 {
        between(&mut fast, k);
        between(&mut looking, k);
        let delta = v(0.37, -0.113 * k as f64);
        let a = fast.drag(&ids_f, delta, &d);
        let b = looking.drag(&ids_f, delta, &d);
        assert_eq!(a, b, "frame {k}");
        same(&drawing(&fast), &drawing(&looking), &format!("frame {k}"));
    }
    fast.commit("Move");
    looking.commit("Move");
    same(&digest(&mut fast), &digest(&mut looking), "after");
}

#[test]
fn a_drag_tells_where_its_points_are_as_looking_would() {
    let curves = |ed: &Editor| -> Vec<EntityId> {
        ed.doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .take(6)
            .collect()
    };
    // Nothing between frames.
    goes_on_as_looking(&free_drawing, &curves, &|_, _| {});
    // A point the drag moves, moved by something else between frames.
    goes_on_as_looking(&free_drawing, &curves, &|ed, k| {
        if k == 4 {
            let id = curves(ed)[0];
            let p = ed.doc.defining_points([id]).into_iter().next().unwrap();
            let at = ed.doc.point(p).unwrap() + v(0.5, 0.0);
            ed.doc.set_point(p, at).unwrap();
        }
    });
    // A point elsewhere moved (as the live link or the assistant may), and moved back.
    goes_on_as_looking(&free_drawing, &curves, &|ed, k| {
        let far = ed
            .doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| id)
            .last()
            .unwrap();
        let at = ed.doc.point(far).unwrap();
        if k == 3 {
            ed.doc.set_point(far, at + v(1.0, 0.0)).unwrap();
        }
        if k == 6 {
            ed.doc.set_point(far, at - v(1.0, 0.0)).unwrap();
        }
    });
    // A relation added between frames (it moves no point): what looking does, then.
    goes_on_as_looking(&free_drawing, &curves, &|ed, k| {
        if k == 5 {
            let line = curves(ed)[1];
            let _ = ed.doc.add_constraint(Constraint::Horizontal { line });
        }
    });
    // An undo between frames.
    goes_on_as_looking(&free_drawing, &curves, &|ed, k| {
        if k == 7 {
            ed.undo();
        }
    });
}

#[test]
fn the_inspector_and_drag_scope_keys_ignore_fills_only() {
    // Frames of a drag carrying a hatch keep the drag's scope; after a relation, a layer
    // lock or a wall edit, the scope kept is the one found afresh.
    let mut ed = free_drawing();
    let ids: Vec<EntityId> = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .take(4)
        .collect();
    let d = dragged(&ed, &ids);
    ed.doc.begin();
    ed.drag(&ids, v(0.1, 0.0), &d);
    let (scope, _) = ed.drag_scope_and_parts(&ids);
    let hatch = |ed: &Editor| format!("{:?}", ed.doc.fills().next().unwrap().1);
    let was = hatch(&ed);
    for k in 1..5 {
        ed.drag(&ids, v(0.1 * k as f64, 0.2), &d);
        let (kept, _) = ed.drag_scope_and_parts(&ids);
        assert!(std::rc::Rc::ptr_eq(&kept, &scope), "frame {k}: found again");
    }
    assert_ne!(hatch(&ed), was, "the hatch was carried along");
    ed.commit("Move");
    let fresh = |ed: &Editor| -> std::collections::HashSet<EntityId> {
        drift_solver::connected_parts_with(&ed.doc, ids.iter().copied(), ed.lowering().as_deref())
            .into_iter()
            .flatten()
            .collect()
    };
    let other = ids[0];
    let far: EntityId = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .last()
        .unwrap();
    ed.doc.begin();
    ed.doc
        .add_constraint(Constraint::Parallel { a: other, b: far })
        .unwrap();
    ed.commit("Parallel");
    assert_eq!(
        *ed.drag_scope_and_parts(&ids).0,
        fresh(&ed),
        "after a relation"
    );
    let layer = ed.add_layer("Locked");
    ed.doc.begin();
    ed.doc.set_entity_layer(far, layer).unwrap();
    ed.commit("Layer");
    assert_eq!(
        *ed.drag_scope_and_parts(&ids).0,
        fresh(&ed),
        "after a layer change"
    );
    let mut locked = ed.doc.layer(layer).unwrap().clone();
    locked.locked = true;
    ed.update_layer(layer, locked);
    assert_eq!(
        *ed.drag_scope_and_parts(&ids).0,
        fresh(&ed),
        "after a layer lock"
    );
}

/// What the editor's index answers, against an index built afresh from the drawing: every
/// search over a grid of probes across the drawing, and every point's role and users.
#[track_caller]
fn index_answers_as_fresh(ed: &Editor, what: &str) {
    let fresh = drift_geom::SceneIndex::build(&ed.doc);
    let points: Vec<DVec2> = ed
        .doc
        .iter()
        .filter_map(|(id, e)| e.kind.is_point().then(|| ed.doc.point(id).ok()).flatten())
        .collect();
    let (lo, hi) = points.iter().fold(
        (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    let span = (hi - lo).max(DVec2::splat(1.0));
    let sorted = |mut v: Vec<EntityId>| {
        v.sort_unstable();
        v
    };
    for i in 0..9 {
        for j in 0..9 {
            let p =
                lo + span * DVec2::new(i as f64 / 8.0, j as f64 / 8.0) + DVec2::new(0.013, 0.007);
            for tol in [0.5, span.max_element() / 7.0] {
                let near = |x: &drift_geom::SceneIndex| {
                    sorted(x.near(p, tol).map(|(id, ..)| id).collect())
                };
                assert_eq!(near(&ed.index), near(&fresh), "{what}: near {p} {tol}");
                assert_eq!(
                    ed.index.pick(p, tol, |_| true),
                    fresh.pick(p, tol, |_| true),
                    "{what}: pick {p} {tol}"
                );
                assert_eq!(
                    ed.index.pick_midpoint(p, tol, |_| true),
                    fresh.pick_midpoint(p, tol, |_| true),
                    "{what}: midpoint {p} {tol}"
                );
                let rect = drift_core::Bounds::from_center_half_size(p, DVec2::splat(tol));
                for crossing in [true, false] {
                    assert_eq!(
                        sorted(ed.index.select_in_rect(&rect, crossing)),
                        sorted(fresh.select_in_rect(&rect, crossing)),
                        "{what}: box {p} {tol} {crossing}"
                    );
                }
                assert_eq!(
                    sorted(ed.index.query(rect).collect()),
                    sorted(fresh.query(rect).collect()),
                    "{what}: query {p} {tol}"
                );
            }
        }
    }
    // Boxes just touching each shape's box, on each side: found only if it is known where
    // that shape is now, to the last bit.
    for (id, _) in ed.doc.iter() {
        let Some(b) = fresh.shape(id).map(|s| s.bounds()) else {
            continue;
        };
        for (min, max) in [
            (
                DVec2::new(b.max.x, b.min.y),
                DVec2::new(b.max.x + 1.0, b.max.y),
            ),
            (
                DVec2::new(b.min.x - 1.0, b.min.y),
                DVec2::new(b.min.x, b.max.y),
            ),
            (
                DVec2::new(b.min.x, b.max.y),
                DVec2::new(b.max.x, b.max.y + 1.0),
            ),
            (
                DVec2::new(b.min.x, b.min.y - 1.0),
                DVec2::new(b.max.x, b.min.y),
            ),
        ] {
            let rect = drift_core::Bounds { min, max };
            assert_eq!(
                sorted(ed.index.query(rect).collect()),
                sorted(fresh.query(rect).collect()),
                "{what}: touching {id:?}"
            );
        }
    }
    for (id, _) in ed.doc.iter() {
        assert_eq!(ed.index.role(id), fresh.role(id), "{what}: role of {id:?}");
        assert_eq!(
            sorted(ed.index.users_of(id).collect()),
            sorted(fresh.users_of(id).collect()),
            "{what}: users of {id:?}"
        );
    }
}

/// A random drawing: lines, arcs whose ends are a hair off their circle (as imported ones
/// are), circles, pen curves; some far from the origin; some held by relations (parts that
/// solve) among the free ones.
fn random_drawing(rnd: &mut impl FnMut(usize) -> usize) -> Editor {
    let mut ed = Editor::default();
    ed.doc.begin();
    let far = match rnd(3) {
        0 => DVec2::ZERO,
        1 => DVec2::new(1.0e6 + 0.3, -1.0e6 + 0.7),
        _ => DVec2::new(-0.7e6, 2.1e5 + 0.1),
    };
    let at = |rnd: &mut dyn FnMut(usize) -> usize| {
        far + DVec2::new(rnd(4000) as f64 / 37.0, rnd(3000) as f64 / 41.0)
    };
    for _ in 0..12 + rnd(20) {
        match rnd(5) {
            0 | 1 => {
                let (a, b) = (at(rnd), at(rnd));
                let (a, b) = (ed.doc.add_point(a), ed.doc.add_point(b));
                let l = ed.doc.add_line(a, b).unwrap();
                if rnd(5) == 0 {
                    let _ = ed.doc.add_constraint(Constraint::Horizontal { line: l });
                }
            }
            2 => {
                let c = at(rnd);
                let r = 3.0 + rnd(40) as f64 / 7.0;
                let off = 1.0 + (rnd(9) as f64 - 4.0) * 1.0e-6;
                let (c, s, e) = (
                    ed.doc.add_point(c),
                    ed.doc.add_point(c + DVec2::new(r, 0.0)),
                    ed.doc.add_point(c + DVec2::new(0.0, r * off)),
                );
                let _ = ed.doc.add_arc(c, s, e);
            }
            3 => {
                let c = ed.doc.add_point(at(rnd));
                let _ = ed.doc.add_circle(c, 1.0 + rnd(30) as f64 / 3.0);
            }
            _ => {
                let p0 = at(rnd);
                let b = [
                    p0,
                    p0 + DVec2::new(3.0, 5.0),
                    p0 + DVec2::new(9.0, -4.0),
                    p0 + DVec2::new(12.0, 1.0),
                ]
                .map(|p| ed.doc.add_point(p));
                let _ = ed.doc.add_bezier(b[0], b[1], b[2], b[3]);
            }
        }
    }
    ed.commit("Setup");
    ed.sync();
    ed
}

#[test]
fn a_drag_s_index_answers_as_one_built_afresh() {
    let mut seed: u64 = 0x5eed_1dea_0000_0253;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    for case in 0..16 {
        let mut ed = random_drawing(&mut rnd);
        let curves: Vec<EntityId> = ed
            .doc
            .iter()
            .filter(|(_, e)| !e.kind.is_point())
            .map(|(id, _)| id)
            .collect();
        let mut ids: Vec<EntityId> = curves.iter().copied().filter(|_| rnd(3) != 0).collect();
        if ids.is_empty() {
            ids.push(curves[0]);
        }
        let d = dragged(&ed, &ids);
        ed.doc.begin();
        let mut to = DVec2::ZERO;
        for frame in 0..20 + rnd(10) {
            let step = DVec2::new(0.1 + rnd(97) as f64 / 31.0, -0.3 + rnd(53) as f64 / 17.0);
            match rnd(14) {
                0 => {
                    // Something else moves a point the drag moves: it starts again.
                    let p = ed.doc.defining_points([ids[0]]).into_iter().next().unwrap();
                    let at = ed.doc.point(p).unwrap() + DVec2::new(1.0e-3, 0.0);
                    ed.doc.set_point(p, at).unwrap();
                }
                1 => {
                    // Let go of and begun again.
                    ed.doc.rollback();
                    ed.sync();
                    index_answers_as_fresh(&ed, &format!("case {case}: rolled back"));
                    ed.doc.begin();
                    to = DVec2::ZERO;
                }
                _ => {}
            }
            if ed.drag(&ids, step, &d) {
                to += step;
            }
            index_answers_as_fresh(&ed, &format!("case {case}, frame {frame}"));
        }
        let _ = to;
        ed.commit("Move");
        index_answers_as_fresh(&ed, &format!("case {case}: committed"));
        assert!(ed.undo());
        index_answers_as_fresh(&ed, &format!("case {case}: undone"));
        assert!(ed.redo());
        index_answers_as_fresh(&ed, &format!("case {case}: redone"));
    }
}

/// Two walls, the second pointing at the first's middle and run on to it (a T), and the
/// free drawing beside them.
fn walls_drawing() -> Editor {
    let mut ed = free_drawing();
    ed.doc.begin();
    let mut walls = Vec::new();
    for (a, b) in [
        (v(0.0, 200.0), v(200.0, 200.0)),
        (v(100.0, 300.0), v(100.0, 230.0)),
    ] {
        let (a, b) = (ed.doc.add_point(a), ed.doc.add_point(b));
        let l = ed.doc.add_line(a, b).unwrap();
        ed.doc.set_style(l, drift_core::LineKind::Construction);
        let path = ed
            .doc
            .add_path(drift_core::Path {
                curves: vec![l],
                closed: false,
            })
            .unwrap();
        walls.push(
            ed.doc
                .add_wall(drift_core::Wall::new(
                    path,
                    10.0,
                    drift_core::LayerId::DEFAULT,
                ))
                .unwrap(),
        );
    }
    ed.commit("Walls");
    assert!(ed.join_walls(&walls), "a T");
    ed
}

/// The centreline of the wall the T runs on to.
fn through_wall(ed: &Editor) -> Vec<EntityId> {
    let (_, w) = ed.doc.walls().next().unwrap();
    ed.doc.path(w.centre).unwrap().curves.clone()
}

#[test]
fn a_drag_whose_walls_or_offsets_follow_goes_on_as_looking_would() {
    // The wall a T runs on to dragged: the T's end follows it after each frame took its
    // end (the next frame looks).
    goes_on_as_looking(&walls_drawing, &through_wall, &|_, _| {});
    // A line with an offset made from it: the offset's pieces follow it.
    let offset = || {
        let mut ed = free_drawing();
        let line = ed
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .last()
            .unwrap();
        ed.offset(line, 2.0, v(40.0, 90.0)).unwrap();
        assert!(ed.doc.has_offsets());
        ed
    };
    let source = |ed: &Editor| -> Vec<EntityId> {
        let (_, o) = ed.doc.offsets().next().unwrap();
        o.source.clone()
    };
    goes_on_as_looking(&offset, &source, &|_, _| {});
}

#[test]
fn a_whole_move_with_walls_by_or_text_along_lands_as_redone() {
    // Walls in the drawing, not moved: kept open, it lands as redone.
    let free_lines = |ed: &mut Editor| {
        let walls: std::collections::BTreeSet<EntityId> = ed
            .doc
            .walls()
            .flat_map(|(_, w)| ed.doc.path(w.centre).unwrap().curves.clone())
            .collect();
        let made: std::collections::BTreeSet<EntityId> =
            ed.doc.wall_owners().of.into_keys().collect();
        ed.selection = ed
            .doc
            .iter()
            .filter(|(id, e)| {
                matches!(e.kind, EntityKind::Line { .. })
                    && !walls.contains(id)
                    && !made.contains(id)
            })
            .map(|(id, _)| id)
            .collect();
    };
    assert!(kept_lands_as_redone(
        &walls_drawing,
        &free_lines,
        &wobbly_path()
    ));
    // A text picked with the curves rides along: moving it changes the make-up, so the
    // move goes the long way (as the same drawing).
    let with_text = || {
        let mut ed = free_drawing();
        ed.add_text(drift_core::SketchText::new("Note", v(30.0, 30.0), 3.5));
        ed
    };
    let all_and_text = |ed: &mut Editor| {
        ed.select_all();
        ed.selected_texts = ed.doc.texts().map(|(id, _)| id).collect();
    };
    assert!(!kept_lands_as_redone(
        &with_text,
        &all_and_text,
        &wobbly_path()
    ));
}

/// Something done to the drawing between two frames of a move.
type Between = dyn Fn(&mut Editor);

/// When something else changes the drawing during a move.
#[derive(Clone, Copy, Debug)]
enum When {
    /// Before this frame.
    Frame(usize),
    /// After the last frame, before the release.
    Release,
}

/// The modal move of the hatched rectangle, with `between` done to the drawing `when`: what
/// the release leaves, kept open or rolled back and moved again.
fn released_after(ed: &mut Editor, between: &dyn Fn(&mut Editor), when: When) {
    ed.selection = ed
        .doc
        .iter()
        .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
        .map(|(id, _)| id)
        .take(4)
        .collect();
    let mut tool = SelectTool::default();
    let p = ptr(ed, &tool, v(1.0, 1.0));
    tool.key(ToolKey::G, Some(&p), ed);
    let path = wobbly_path();
    for (k, &at) in path.iter().enumerate() {
        if matches!(when, When::Frame(f) if f == k) {
            between(ed);
        }
        let p = ptr(ed, &tool, at);
        tool.handle(Input::Hover(p), ed);
    }
    if matches!(when, When::Release) {
        between(ed);
    }
    let p = ptr(ed, &tool, *path.last().unwrap());
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

#[test]
fn a_move_with_anything_else_changed_during_it_goes_the_long_way() {
    // Each change during the move by something other than the drag: the release rolls back
    // and moves again, as it always did (what changed goes as it always went).
    let far_point = |ed: &mut Editor| {
        // A point the drag doesn't move (the drawing's last: a loose line's end) moved: it
        // moves the geometry, not the make-up.
        let p = ed
            .doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| id)
            .last()
            .unwrap();
        let at = ed.doc.point(p).unwrap() + v(3.0, 0.0);
        ed.doc.set_point(p, at).unwrap();
    };
    let radius = |ed: &mut Editor| {
        // The circle (the drag doesn't move it) made bigger.
        let circle = ed
            .doc
            .iter()
            .find(|(_, e)| matches!(e.kind, EntityKind::Circle { .. }))
            .map(|(id, _)| id)
            .unwrap();
        ed.doc.set_radius(circle, 9.5).unwrap();
    };
    let fill = |ed: &mut Editor| {
        let (id, mut f) = ed
            .doc
            .fills()
            .next()
            .map(|(id, f)| (id, f.clone()))
            .unwrap();
        f.opacity = 0.9;
        ed.doc.set_fill(id, f);
    };
    let relation = |ed: &mut Editor| {
        let line = ed
            .doc
            .iter()
            .filter(|(_, e)| matches!(e.kind, EntityKind::Line { .. }))
            .map(|(id, _)| id)
            .last()
            .unwrap();
        let _ = ed.doc.add_constraint(Constraint::Horizontal { line });
    };
    let cases: Vec<(&str, &Between)> = vec![
        ("a point elsewhere", &far_point),
        ("a radius elsewhere", &radius),
        ("a fill", &fill),
        ("a relation", &relation),
    ];
    // Before the first frame, between two frames, and after the last frame but before the
    // release (where only the release's own look at the revision can tell).
    for when in [When::Frame(0), When::Frame(6), When::Release] {
        for (what, between) in &cases {
            let mut kept = free_drawing();
            let mut redone = free_drawing();
            redone.land_whole_off = true;
            released_after(&mut kept, between, when);
            released_after(&mut redone, between, when);
            assert_eq!(kept.landed_whole, 0, "{what}, {when:?}: kept open");
            same(
                &digest(&mut kept),
                &digest(&mut redone),
                &format!("{what}, {when:?}"),
            );
        }
    }
}
