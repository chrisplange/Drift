//! Dragging filleted sketches: the shape has to survive (arcs keep their side and sweep,
//! tangent lines stay on their side, nothing collapses or crosses).

use drift_core::EntityKind;
use drift_geom::SnapOptions;

use super::*;

const TOL: f64 = 0.5;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

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

/// The L-shaped band of the bug report: an outer and an inner corner, both filleted (inner
/// radius 6, outer 16, about one centre), with horizontal / vertical relations, tangents and
/// radius dimensions. `concentric` adds a concentric relation between the two arcs;
/// without `outer_sized` the outer arc has no radius dimension.
struct Band {
    ed: Editor,
    inner: EntityId,
    outer: EntityId,
}

fn band(concentric: bool, outer_sized: bool) -> Band {
    let mut ed = Editor::new(drift_core::Document::new());
    ed.doc.begin();
    let at = [
        v(-20.0, 16.0),
        v(16.0, 16.0),
        v(16.0, -14.0),
        v(6.0, -14.0),
        v(6.0, 6.0),
        v(-20.0, 6.0),
    ];
    let p = at.map(|q| ed.doc.add_point(q));
    for i in 0..6 {
        let l = ed.doc.add_line(p[i], p[(i + 1) % 6]).unwrap();
        let c = if i % 2 == 0 {
            Constraint::Horizontal { line: l }
        } else {
            Constraint::Vertical { line: l }
        };
        ed.doc.add_constraint(c).unwrap();
    }
    ed.commit("Band");
    let inner = ed.fillet(p[4], 6.0).unwrap();
    let outer = ed.fillet(p[1], 16.0).unwrap();
    if concentric {
        ed.doc.begin();
        ed.doc
            .add_constraint(Constraint::Concentric { a: inner, b: outer })
            .unwrap();
        ed.commit("Concentric");
    }
    if !outer_sized {
        let sized: Vec<_> = ed
            .doc
            .constraints_on(outer)
            .filter(|id| matches!(ed.doc.constraint(*id), Some(Constraint::Radius { .. })))
            .collect();
        ed.doc.begin();
        for id in sized {
            ed.doc.remove_constraint(id);
        }
        ed.commit("Free outer radius");
    }
    assert!(ed.analysis().conflicts.is_empty());
    Band { ed, inner, outer }
}

/// An arc's centre and its counter-clockwise sweep from start to end, in degrees.
fn sweep(ed: &Editor, arc: EntityId) -> (DVec2, f64) {
    let Some(EntityKind::Arc { center, start, end }) = ed.doc.get(arc).map(|e| e.kind) else {
        panic!("an arc");
    };
    let c = ed.doc.point(center).unwrap();
    let (s, e) = (
        ed.doc.point(start).unwrap() - c,
        ed.doc.point(end).unwrap() - c,
    );
    (
        c,
        s.perp_dot(e).atan2(s.dot(e)).to_degrees().rem_euclid(360.0),
    )
}

/// Everything that makes up the band's shape: each line's direction (unit), and for each
/// line–arc tangent which side of the line the arc's centre is on.
fn shape_signature(ed: &Editor) -> Vec<(String, f64, f64)> {
    let mut out = Vec::new();
    for (id, e) in ed.doc.iter() {
        match e.kind {
            EntityKind::Line { a, b } => {
                let d = ed.doc.point(b).unwrap() - ed.doc.point(a).unwrap();
                let n = d.normalize_or_zero();
                out.push((format!("line {id:?}"), n.x, n.y));
            }
            EntityKind::Arc { .. } => {
                let (_, s) = sweep(ed, id);
                out.push((format!("arc {id:?}"), s, 0.0));
            }
            _ => {}
        }
    }
    for (_, c) in ed.doc.constraints() {
        if let Constraint::Tangent { a, b } = *c
            && let Some(EntityKind::Line { a: la, b: lb }) = ed.doc.get(a).map(|e| e.kind)
        {
            let (c, _) = sweep(ed, b);
            let (pa, pb) = (ed.doc.point(la).unwrap(), ed.doc.point(lb).unwrap());
            let side = (pb - pa).perp_dot(c - pa).signum();
            out.push((format!("side {a:?}/{b:?}"), side, 0.0));
        }
    }
    out
}

fn assert_same_shape(before: &[(String, f64, f64)], ed: &Editor, what: &str) {
    let after = shape_signature(ed);
    for ((name, x0, y0), (_, x1, y1)) in before.iter().zip(&after) {
        let tol = if name.starts_with("arc") { 60.0 } else { 1e-3 };
        assert!(
            (x0 - x1).abs() < tol && (y0 - y1).abs() < tol,
            "{what}: {name} went from ({x0:.3}, {y0:.3}) to ({x1:.3}, {y1:.3})\n{}",
            dump(ed)
        );
    }
    for (name, s, _) in &after {
        if name.starts_with("arc") {
            assert!(*s < 180.0, "{what}: {name} sweeps {s:.1}°\n{}", dump(ed));
        }
    }
    assert!(
        ed.analysis().conflicts.is_empty(),
        "{what}: conflicts {:?}",
        ed.analysis().conflicts
    );
}

fn dump(ed: &Editor) -> String {
    let mut s = String::new();
    for (id, e) in ed.doc.iter() {
        if let Some(shape) = ed.doc.shape(id)
            && !e.kind.is_point()
        {
            s += &format!("{id:?}: {shape:?}\n");
        }
    }
    s
}

/// Drag the centre point of `arc` with the select tool, in small steps along `by`.
fn drag_centre(ed: &mut Editor, arc: EntityId, by: DVec2, steps: usize) {
    let (c, _) = sweep(ed, arc);
    let mut tool = SelectTool::default();
    // Pick the arc's centre (both arcs may have one there: make sure it's this one's).
    let Some(EntityKind::Arc { center, .. }) = ed.doc.get(arc).map(|e| e.kind) else {
        unreachable!()
    };
    ed.selection = [center].into();
    let start = ptr(ed, &tool, c);
    tool.handle(Input::DragStart(start, Modifiers::default()), ed);
    for i in 1..=steps {
        let p = Pointer::new(c + by * (i as f64 / steps as f64), TOL);
        tool.handle(Input::Drag(p), ed);
    }
    let p = Pointer::new(c + by, TOL);
    tool.handle(Input::DragEnd(p, Modifiers::default()), ed);
}

fn centre_of(ed: &Editor, arc: EntityId) -> EntityId {
    match ed.doc.get(arc).map(|e| e.kind) {
        Some(EntityKind::Arc { center, .. }) => center,
        _ => panic!("an arc"),
    }
}

fn radius(ed: &Editor, arc: EntityId) -> f64 {
    drift_solver::radius_of(&ed.doc, arc).unwrap()
}

/// The sketches of the bug report: with and without a concentric relation, and with the
/// outer arc's size set by a dimension or only by its lines.
fn bands() -> impl Iterator<Item = (bool, bool)> {
    [(true, false), (true, true), (false, false), (false, true)].into_iter()
}

#[test]
fn dragging_a_fillet_centre_keeps_the_band_in_shape() {
    for (concentric, outer_sized) in bands() {
        // Out to where the inner arm would reach the outer one (when they aren't tied, that
        // crossing is real).
        let reach = if concentric { 18.0 } else { 8.0 };
        for by in [v(reach, 0.0), v(-8.0, 0.0), v(0.0, 8.0), v(4.0, -5.0)] {
            let Band {
                mut ed,
                inner,
                outer,
            } = band(concentric, outer_sized);
            let before = shape_signature(&ed);
            drag_centre(&mut ed, inner, by, 40);
            let what = format!("concentric {concentric}, sized {outer_sized}, centre by {by}");
            assert_same_shape(&before, &ed, &what);
            // The rounds keep their sizes: the concentric one follows with its lines.
            assert!((radius(&ed, inner) - 6.0).abs() < 1e-6, "{what}");
            let r = radius(&ed, outer);
            assert!((r - 16.0).abs() < 0.01, "{what}: outer radius {r}");
            // The centre under the pointer followed it (without the concentric relation the
            // two centres are separate points, one on the other: either may be the one).
            let centres = [inner, outer].map(|arc| ed.doc.point(centre_of(&ed, arc)).unwrap());
            assert!(
                centres.iter().any(|c| c.distance(by) < 1e-3),
                "{what}: centres at {centres:?}"
            );
        }
    }
}

/// A modal move (G, then X to lock the axis) of `selection`, from `from`, out to `by` in
/// small pointer steps, then a click to confirm.
fn grab_along_x(ed: &mut Editor, selection: &[EntityId], from: DVec2, by: f64, steps: usize) {
    let mut tool = SelectTool::default();
    ed.selection = selection.iter().copied().collect();
    let p = Pointer::new(from, TOL);
    assert!(tool.key(ToolKey::G, Some(&p), ed));
    assert!(tool.key(ToolKey::X, Some(&p), ed));
    for i in 1..=steps {
        let p = Pointer::new(from + v(by * i as f64 / steps as f64, 0.3), TOL);
        tool.handle(Input::Hover(p), ed);
    }
    let p = Pointer::new(from + v(by, 0.3), TOL);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

#[test]
fn a_modal_move_of_a_fillet_centre_keeps_the_band_in_shape() {
    // The move is confirmed in one solve from where it began: the centre jumps past the
    // arc's ends, which used to leave the arc turned inside out (270° the long way round).
    for (concentric, outer_sized) in bands() {
        for by in [6.0, -6.0, 12.0, 18.0] {
            if !concentric && by > 9.0 {
                continue;
            }
            for pick_arc in [false, true] {
                let Band {
                    mut ed,
                    inner,
                    outer,
                } = band(concentric, outer_sized);
                let before = shape_signature(&ed);
                let picked = if pick_arc {
                    inner
                } else {
                    centre_of(&ed, inner)
                };
                grab_along_x(&mut ed, &[picked], v(0.0, 0.0), by, 30);
                let what = format!(
                    "concentric {concentric}, sized {outer_sized}, arc {pick_arc}, by {by}"
                );
                assert_same_shape(&before, &ed, &what);
                let r = radius(&ed, outer);
                assert!((r - 16.0).abs() < 0.01, "{what}: outer radius {r}");
                let centre = ed.doc.point(centre_of(&ed, inner)).unwrap();
                assert!(
                    centre.distance(v(by, 0.0)) < 1e-3,
                    "{what}: centre at {centre}"
                );
            }
        }
    }
}

/// Drag the geometry under `from` with the select tool to `to`, in small steps.
fn drag_at(ed: &mut Editor, from: DVec2, to: DVec2, steps: usize) {
    let mut tool = SelectTool::default();
    ed.selection.clear();
    let start = ptr(ed, &tool, from);
    tool.handle(Input::Hover(start), ed);
    tool.handle(Input::DragStart(start, Modifiers::default()), ed);
    for i in 1..=steps {
        let p = Pointer::new(from + (to - from) * (i as f64 / steps as f64), TOL);
        tool.handle(Input::Drag(p), ed);
    }
    let end = Pointer::new(to, TOL);
    tool.handle(Input::DragEnd(end, Modifiers::default()), ed);
}

#[test]
fn dragging_lines_ends_and_the_fillet_itself_keeps_the_band_in_shape() {
    for (concentric, outer_sized) in bands() {
        // (where to grab, where to let go)
        let drags = [
            // The inner vertical line, sideways.
            (v(6.0, -8.0), v(9.0, -8.0)),
            (v(6.0, -8.0), v(2.0, -8.0)),
            // The inner horizontal line, up and down.
            (v(-10.0, 6.0), v(-10.0, 9.0)),
            (v(-10.0, 6.0), v(-10.0, 2.0)),
            // The outer vertical line, in towards the inner one and out.
            (v(16.0, -8.0), v(12.0, -8.0)),
            (v(16.0, -8.0), v(22.0, -8.0)),
            // The inner fillet's end point, along its line.
            (v(6.0, 0.0), v(6.0, -3.0)),
            // The inner fillet itself (resizes it, from its middle).
            (v(6.0, 6.0) - DVec2::splat(6.0 / 2f64.sqrt()), v(1.0, 1.0)),
            // The far corner of the bottom arm.
            (v(16.0, -14.0), v(19.0, -18.0)),
        ];
        for (from, to) in drags {
            let Band { mut ed, .. } = band(concentric, outer_sized);
            let before = shape_signature(&ed);
            drag_at(&mut ed, from, to, 30);
            let what = format!("concentric {concentric}, sized {outer_sized}, {from} to {to}");
            assert_same_shape(&before, &ed, &what);
        }
    }
}

#[test]
fn changing_the_fillet_radius_keeps_the_band_in_shape() {
    for (concentric, outer_sized) in bands() {
        for r in [2.0, 4.0, 9.0, 12.0] {
            let Band {
                mut ed,
                inner,
                outer,
            } = band(concentric, outer_sized);
            let before = shape_signature(&ed);
            let dim = ed
                .doc
                .constraints_on(inner)
                .find(|id| matches!(ed.doc.constraint(*id), Some(Constraint::Radius { .. })))
                .unwrap();
            let what = format!("concentric {concentric}, sized {outer_sized}, radius {r}");
            match ed.set_dimension(dim, r) {
                Ok(()) => {
                    assert!((radius(&ed, inner) - r).abs() < 1e-6, "{what}");
                    assert_same_shape(&before, &ed, &what);
                }
                Err(e) => panic!("{what}: refused ({e:?})\n{}", dump(&ed)),
            }
            let _ = outer;
        }
    }
}

/// Random small drags of points and lines (seeded, so failures repeat): whatever happens,
/// the fillets stay quarter turns on their side of their lines, and no relation breaks.
#[test]
fn random_drags_never_turn_the_band_inside_out() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let sides = |ed: &Editor| -> Vec<(String, f64, f64)> {
        shape_signature(ed)
            .into_iter()
            .filter(|(name, ..)| name.starts_with("side"))
            .collect()
    };
    for (concentric, outer_sized) in bands() {
        let Band { mut ed, .. } = band(concentric, outer_sized);
        let start = sides(&ed);
        for round in 0..60 {
            let targets: Vec<EntityId> = ed
                .doc
                .iter()
                .filter(|(_, e)| e.kind.is_point() || matches!(e.kind, EntityKind::Line { .. }))
                .map(|(id, _)| id)
                .collect();
            let target = targets[(random() * targets.len() as f64) as usize % targets.len()];
            let delta = v(random() - 0.5, random() - 0.5) * 6.0;
            let dragged: Vec<Param> = ed
                .doc
                .defining_points([target])
                .into_iter()
                .flat_map(Param::point)
                .collect();
            ed.doc.begin();
            for _ in 0..4 {
                ed.drag(&[target], delta / 4.0, &dragged);
            }
            ed.commit("Drag");
            let what = format!("concentric {concentric}, sized {outer_sized}, round {round}");
            for (name, s, _) in shape_signature(&ed) {
                if name.starts_with("arc") {
                    assert!(
                        (s - 90.0).abs() < 1e-3,
                        "{what}: {name} sweeps {s}\n{}",
                        dump(&ed)
                    );
                }
            }
            assert_eq!(start, sides(&ed), "{what}\n{}", dump(&ed));
            assert!(ed.analysis().conflicts.is_empty(), "{what}");
        }
    }
}
