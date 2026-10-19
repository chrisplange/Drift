//! Adding a relation to a loosely held sketch: the solve should make the least change that
//! meets it, not reshape the rest of the sketch.

use drift_core::EntityKind;

use super::*;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// A U-shaped outline: two arms (outer edges y = ±20, inner edges y = ±10) open to the
/// left, joined on the right by an inner semicircle (centre (30, 0), radius 10, on a small
/// circle's centre) and by a right side x = `right` with quarter-circle fillets of radius 10
/// at its corners. Everything is tangent-joined; nothing is horizontal, vertical or sized.
struct Outline {
    ed: Editor,
    inner: EntityId,
    top_fillet: EntityId,
    /// Every point of the outline, with where it started.
    points: Vec<(EntityId, DVec2)>,
}

fn outline(right: f64) -> Outline {
    let mut ed = Editor::new(drift_core::Document::new());
    ed.doc.begin();
    let mut points = Vec::new();
    let mut pt = |ed: &mut Editor, at: DVec2| {
        let id = ed.doc.add_point(at);
        points.push((id, at));
        id
    };
    // Going round: top arm's outer edge, top-right fillet, right side, bottom-right fillet,
    // bottom arm's outer edge, bottom left end, bottom inner edge, inner arc, top inner edge,
    // top left end.
    let top_left = pt(&mut ed, v(0.0, 20.0));
    let top_out = pt(&mut ed, v(right - 10.0, 20.0));
    let tr_center = pt(&mut ed, v(right - 10.0, 10.0));
    let right_top = pt(&mut ed, v(right, 10.0));
    let right_bottom = pt(&mut ed, v(right, -10.0));
    let br_center = pt(&mut ed, v(right - 10.0, -10.0));
    let bottom_out = pt(&mut ed, v(right - 10.0, -20.0));
    let bottom_left = pt(&mut ed, v(0.0, -20.0));
    let bottom_inner_left = pt(&mut ed, v(0.0, -10.0));
    let bottom_inner = pt(&mut ed, v(30.0, -10.0));
    let inner_center = pt(&mut ed, v(30.0, 0.0));
    let top_inner = pt(&mut ed, v(30.0, 10.0));
    let top_inner_left = pt(&mut ed, v(0.0, 10.0));

    let doc = &mut ed.doc;
    let top = doc.add_line(top_left, top_out).unwrap();
    let top_fillet = doc.add_arc(tr_center, right_top, top_out).unwrap();
    let right = doc.add_line(right_top, right_bottom).unwrap();
    let bottom_fillet = doc.add_arc(br_center, bottom_out, right_bottom).unwrap();
    let bottom = doc.add_line(bottom_out, bottom_left).unwrap();
    doc.add_line(bottom_left, bottom_inner_left).unwrap();
    let bottom_in = doc.add_line(bottom_inner_left, bottom_inner).unwrap();
    let inner = doc.add_arc(inner_center, bottom_inner, top_inner).unwrap();
    let top_in = doc.add_line(top_inner, top_inner_left).unwrap();
    doc.add_line(top_inner_left, top_left).unwrap();
    doc.add_circle(inner_center, 3.0).unwrap();
    for (a, b) in [
        (top, top_fillet),
        (right, top_fillet),
        (right, bottom_fillet),
        (bottom, bottom_fillet),
        (bottom_in, inner),
        (top_in, inner),
    ] {
        doc.add_constraint(Constraint::Tangent { a, b }).unwrap();
    }
    assert!(ed.commit("Outline"));
    assert!(ed.analysis().conflicts.is_empty());
    Outline {
        ed,
        inner,
        top_fillet,
        points,
    }
}

fn center(ed: &Editor, arc: EntityId) -> DVec2 {
    match ed.doc.get(arc).map(|e| e.kind) {
        Some(EntityKind::Arc { center, .. }) => ed.doc.point(center).unwrap(),
        _ => panic!("an arc"),
    }
}

/// Each line's direction, in degrees.
fn directions(ed: &Editor) -> Vec<(EntityId, f64)> {
    ed.doc
        .iter()
        .filter_map(|(id, e)| match e.kind {
            EntityKind::Line { a, b } => {
                let d = ed.doc.point(b).unwrap() - ed.doc.point(a).unwrap();
                Some((id, d.y.atan2(d.x).to_degrees()))
            }
            _ => None,
        })
        .collect()
}

/// Make the inner arc and the top-right fillet concentric, and check that only they and
/// what they have to take along moved: every line keeps its direction, the arms' far ends
/// stay where they were and the bottom-right corner is untouched. Returns how far each
/// point moved.
fn make_concentric(right: f64) -> Vec<(DVec2, f64)> {
    let Outline {
        mut ed,
        inner,
        top_fillet,
        points,
    } = outline(right);
    let before = directions(&ed);
    ed.add_constraints(
        "Concentric",
        &[Constraint::Concentric {
            a: inner,
            b: top_fillet,
        }],
    )
    .unwrap();
    assert!(center(&ed, inner).distance(center(&ed, top_fillet)) < 1e-6);
    for ((id, was), (_, now)) in before.iter().zip(directions(&ed)) {
        assert!(
            ((now - was + 180.0).rem_euclid(360.0) - 180.0).abs() < 0.5,
            "line {id:?} turned from {was:.1}° to {now:.1}°"
        );
    }
    let moved: Vec<(DVec2, f64)> = points
        .iter()
        .map(|&(id, at)| (at, ed.doc.point(id).unwrap().distance(at)))
        .collect();
    for &(at, d) in &moved {
        if at.x == 0.0 || at.y < -5.0 && at.x > 30.0 {
            assert!(d < 0.1, "{at} moved {d:.2}: {moved:?}");
        }
    }
    moved
}

/// Issue #75: making the inner arc and the top-right fillet concentric should move the
/// fillet, not swing the arms into a wedge. Here the fillet can do it alone: centred on
/// the inner arc with radius 20 it still touches the top edge and the right side.
#[test]
fn concentric_moves_the_fillet() {
    let moved = make_concentric(50.0);
    for &(at, d) in &moved {
        // The fillet's centre and ends move; the inner arc stays.
        let fillet = at.x >= 40.0 && at.y >= 10.0;
        if !fillet {
            assert!(d < 0.1, "{at} moved {d:.2}: {moved:?}");
        }
    }
}

/// The same with a wider right side: the fillet can't touch both its lines from the inner
/// arc's centre, so something else has to give, but the lines still keep their directions.
#[test]
fn concentric_keeps_lines_square() {
    make_concentric(55.0);
}
