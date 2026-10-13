//! Example documents.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use glam::DVec2;

use crate::{Constraint, Document, LineKind, Shape, SketchItem};

/// A 120 × 80 mm mounting plate with filleted corners, a central bore, a bolt circle and a
/// slot, fully defined by relations and dimensions (tangent fillets, symmetric bolt holes, a
/// slot built from tangent arcs). The outline and slot are closed chains that share their end
/// points.
pub fn mounting_plate() -> Document {
    let mut doc = Document::new();
    let v = DVec2::new;
    let (w, h, r) = (120.0, 80.0, 10.0);
    let constrain = |doc: &mut Document, c: Constraint| -> crate::ConstraintId {
        doc.add_constraint(c).expect("valid example constraint")
    };
    // Dimension label positions (offsets from each dimension's anchor), for a tidy drawing.
    let mut labels: Vec<(crate::ConstraintId, DVec2)> = Vec::new();

    // Outline: four lines joined by four fillet arcs, counter-clockwise from the bottom edge.
    let corners = [
        (v(w - r, r), -FRAC_PI_2),
        (v(w - r, h - r), 0.0),
        (v(r, h - r), FRAC_PI_2),
        (v(r, r), 2.0 * FRAC_PI_2),
    ];
    let mut tangent_points = Vec::new();
    let mut fillets = Vec::new();
    for (center, start_angle) in corners {
        let start = center + r * DVec2::from_angle(start_angle);
        let end = center + r * DVec2::from_angle(start_angle + FRAC_PI_2);
        let c = doc.add_point(center);
        let s = doc.add_point(start);
        let e = doc.add_point(end);
        fillets.push(doc.add_arc(c, s, e).expect("valid fillet"));
        doc.set_style(c, LineKind::Construction);
        tangent_points.push((s, e));
    }
    let mut edges = Vec::new();
    for i in 0..4 {
        let from = tangent_points[(i + 3) % 4].1;
        let to = tangent_points[i].0;
        edges.push(doc.add_line(from, to).expect("valid edge"));
    }
    // Bottom, right, top, left; each edge is tangent to the fillets at its ends.
    constrain(&mut doc, Constraint::Horizontal { line: edges[0] });
    constrain(&mut doc, Constraint::Vertical { line: edges[1] });
    constrain(&mut doc, Constraint::Horizontal { line: edges[2] });
    constrain(&mut doc, Constraint::Vertical { line: edges[3] });
    for i in 0..4 {
        for arc in [fillets[(i + 3) % 4], fillets[i]] {
            constrain(
                &mut doc,
                Constraint::Tangent {
                    a: edges[i],
                    b: arc,
                },
            );
        }
    }
    let id = constrain(
        &mut doc,
        Constraint::Radius {
            curve: fillets[0],
            value: r,
        },
    );
    labels.push((id, v(16.0, -16.0)));
    for &f in &fillets[1..] {
        constrain(
            &mut doc,
            Constraint::Equal {
                a: fillets[0],
                b: f,
            },
        );
    }
    // The left edge's lower end anchors the plate; width and height span the edges.
    let anchor = tangent_points[3].0;
    constrain(
        &mut doc,
        Constraint::Fix {
            point: anchor,
            at: v(0.0, r),
        },
    );
    let id = constrain(
        &mut doc,
        Constraint::HorizontalDistance {
            a: anchor,
            b: tangent_points[0].1,
            value: w,
        },
    );
    labels.push((id, v(0.0, -36.0)));
    let id = constrain(
        &mut doc,
        Constraint::VerticalDistance {
            a: tangent_points[3].1,
            b: tangent_points[2].0,
            value: h,
        },
    );
    labels.push((id, v(-36.0, 0.0)));

    // Central bore and bolt circle, positioned from the anchor.
    let bore = v(70.0, 40.0);
    let bolt_radius = 26.0;
    let bore_center = doc.add_point(bore);
    let bore_circle = doc.add_circle(bore_center, 14.0).expect("valid bore");
    let bolt = doc
        .add_circle(bore_center, bolt_radius)
        .expect("valid bolt circle");
    doc.set_style(bolt, LineKind::Construction);
    let id = constrain(
        &mut doc,
        Constraint::HorizontalDistance {
            a: anchor,
            b: bore_center,
            value: bore.x,
        },
    );
    labels.push((id, v(0.0, -41.0)));
    let id = constrain(
        &mut doc,
        Constraint::VerticalDistance {
            a: anchor,
            b: bore_center,
            value: bore.y - r,
        },
    );
    labels.push((id, v(-51.0, 0.0)));
    let id = constrain(
        &mut doc,
        Constraint::Diameter {
            curve: bore_circle,
            value: 28.0,
        },
    );
    labels.push((id, v(-9.0, -8.0)));
    let id = constrain(
        &mut doc,
        Constraint::Diameter {
            curve: bolt,
            value: 2.0 * bolt_radius,
        },
    );
    labels.push((id, v(-22.0, -24.0)));

    // Centre lines through the bore: centred on it, horizontal / vertical, 68 long.
    let reach = bolt_radius + 8.0;
    let mut center_lines = Vec::new();
    for dir in [v(1.0, 0.0), v(0.0, 1.0)] {
        let a = doc.add_point(bore - dir * reach);
        let b = doc.add_point(bore + dir * reach);
        let line = doc.add_line(a, b).expect("valid centre line");
        doc.set_style(line, LineKind::Center);
        constrain(
            &mut doc,
            if dir.x != 0.0 {
                Constraint::Horizontal { line }
            } else {
                Constraint::Vertical { line }
            },
        );
        constrain(
            &mut doc,
            Constraint::Midpoint {
                point: bore_center,
                line,
            },
        );
        let id = constrain(
            &mut doc,
            Constraint::Distance {
                a,
                b,
                value: 2.0 * reach,
            },
        );
        // Beside each centre line, past the bolt circle.
        labels.push((
            id,
            if dir.x != 0.0 {
                v(0.0, -31.0)
            } else {
                v(38.0, 0.0)
            },
        ));
        center_lines.push(line);
    }
    let (horizontal_cl, vertical_cl) = (center_lines[0], center_lines[1]);

    // Bolt holes at 45°, 135°, 225° and 315°: the first on the bolt circle at 45°, the others
    // mirrored about the centre lines; all the same size.
    let mut holes = Vec::new();
    let mut hole_centers = Vec::new();
    for i in 0..4 {
        let at = bore + bolt_radius * DVec2::from_angle(FRAC_PI_4 + f64::from(i) * FRAC_PI_2);
        let center = doc.add_point(at);
        holes.push(doc.add_circle(center, 3.2).expect("valid hole"));
        hole_centers.push(center);
    }
    constrain(
        &mut doc,
        Constraint::PointOnCurve {
            point: hole_centers[0],
            curve: bolt,
        },
    );
    // The first hole sits at 45° on a construction line from the bore centre (an exact
    // angle, rather than a rounded 26·cos 45° offset).
    let diagonal = doc
        .add_line(bore_center, hole_centers[0])
        .expect("valid construction line");
    doc.set_style(diagonal, LineKind::Construction);
    let id = constrain(
        &mut doc,
        Constraint::Angle {
            a: horizontal_cl,
            b: diagonal,
            degrees: 45.0,
        },
    );
    labels.push((id, v(17.0, 7.0)));
    for (a, b, axis) in [
        (0, 1, vertical_cl),
        (0, 3, horizontal_cl),
        (1, 2, horizontal_cl),
    ] {
        constrain(
            &mut doc,
            Constraint::Symmetric {
                a: hole_centers[a],
                b: hole_centers[b],
                axis,
            },
        );
    }
    let id = constrain(
        &mut doc,
        Constraint::Diameter {
            curve: holes[0],
            value: 6.4,
        },
    );
    labels.push((id, v(9.0, 9.0)));
    for &hole in &holes[1..] {
        constrain(
            &mut doc,
            Constraint::Equal {
                a: holes[0],
                b: hole,
            },
        );
    }

    // Vertical slot on the left: two lines and two semicircular ends sharing points. Vertical
    // sides tangent to both ends make the ends equal; position, length and radius define it.
    let (x, lo, hi, sr) = (20.0, 25.0, 55.0, 4.0);
    let bl = doc.add_point(v(x - sr, lo));
    let br = doc.add_point(v(x + sr, lo));
    let tr = doc.add_point(v(x + sr, hi));
    let tl = doc.add_point(v(x - sr, hi));
    let bc = doc.add_point(v(x, lo));
    let tc = doc.add_point(v(x, hi));
    let right = doc.add_line(br, tr).expect("valid slot side");
    let left = doc.add_line(tl, bl).expect("valid slot side");
    let bottom = doc.add_arc(bc, bl, br).expect("valid slot end");
    let top = doc.add_arc(tc, tr, tl).expect("valid slot end");
    for side in [right, left] {
        constrain(&mut doc, Constraint::Vertical { line: side });
        for end in [bottom, top] {
            constrain(&mut doc, Constraint::Tangent { a: side, b: end });
        }
    }
    let id = constrain(
        &mut doc,
        Constraint::Radius {
            curve: bottom,
            value: sr,
        },
    );
    labels.push((id, v(9.0, -9.0)));
    let id = constrain(
        &mut doc,
        Constraint::HorizontalDistance {
            a: anchor,
            b: bc,
            value: x,
        },
    );
    labels.push((id, v(0.0, -23.5)));
    let id = constrain(
        &mut doc,
        Constraint::VerticalDistance {
            a: anchor,
            b: bc,
            value: lo - r,
        },
    );
    labels.push((id, v(-16.0, 0.0)));
    let id = constrain(
        &mut doc,
        Constraint::VerticalDistance {
            a: bc,
            b: tc,
            value: hi - lo,
        },
    );
    labels.push((id, v(12.0, 0.0)));
    // The slot centre line, through both end centres with equal overhangs.
    let a = doc.add_point(v(x, lo - sr - 4.0));
    let b = doc.add_point(v(x, hi + sr + 4.0));
    let slot_axis = doc.add_line(a, b).expect("valid slot axis");
    doc.set_style(slot_axis, LineKind::Center);
    constrain(&mut doc, Constraint::Vertical { line: slot_axis });
    constrain(
        &mut doc,
        Constraint::PointOnCurve {
            point: bc,
            curve: slot_axis,
        },
    );
    for (p, q) in [(a, bc), (tc, b)] {
        let id = constrain(
            &mut doc,
            Constraint::VerticalDistance {
                a: p,
                b: q,
                value: sr + 4.0,
            },
        );
        labels.push((id, v(-9.0, 0.0)));
    }

    for (id, offset) in labels {
        doc.set_placement(id, offset).expect("label on a dimension");
    }
    doc
}

/// A tiny feature a kilometre from the origin, for checking precision at large coordinates.
pub fn far_feature() -> Vec<SketchItem> {
    let far = DVec2::new(1.0e6, 1.0e6);
    vec![
        SketchItem::new(Shape::Circle {
            center: far,
            radius: 0.5,
        }),
        SketchItem::new(Shape::Arc {
            center: far,
            radius: 0.25,
            start: 0.0,
            sweep: TAU * 0.75,
        }),
        SketchItem::new(Shape::Line {
            a: far - DVec2::new(0.75, 0.0),
            b: far + DVec2::new(0.75, 0.0),
        })
        .with_kind(LineKind::Center),
        SketchItem::new(Shape::Point(far)),
    ]
}

/// Plain shapes of a document, for rendering without interaction state.
pub fn document_items(doc: &Document) -> Vec<SketchItem> {
    let referenced = doc.referenced_points();
    doc.iter()
        .filter_map(|(id, entity)| {
            let item = SketchItem::new(doc.shape(id)?).with_kind(entity.style);
            Some(if referenced.contains(&id) {
                item.as_vertex()
            } else {
                item
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EntityKind;

    #[test]
    fn plate_outline_is_connected() {
        let doc = mounting_plate();
        // Every outline/slot end point is shared by exactly two curves.
        let mut uses = std::collections::HashMap::new();
        for (_, e) in doc.iter() {
            if matches!(e.kind, EntityKind::Line { .. } | EntityKind::Arc { .. }) {
                let ends: Vec<_> = match e.kind {
                    EntityKind::Line { a, b } => vec![a, b],
                    EntityKind::Arc { start, end, .. } => vec![start, end],
                    _ => unreachable!(),
                };
                for p in ends {
                    *uses.entry(p).or_insert(0) += 1;
                }
            }
        }
        let shared = uses.values().filter(|&&n| n == 2).count();
        assert_eq!(shared, 8 + 4, "8 outline joints and 4 slot joints");
    }
}
