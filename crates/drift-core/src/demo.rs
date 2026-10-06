//! Example documents.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use glam::DVec2;

use crate::{Document, LineKind, Shape, SketchItem};

/// A 120 × 80 mm mounting plate with filleted corners, a central bore, a bolt circle and a slot.
/// The outline and slot are closed chains that share their end points.
pub fn mounting_plate() -> Document {
    let mut doc = Document::new();
    let v = DVec2::new;
    let (w, h, r) = (120.0, 80.0, 10.0);

    // Outline: four lines joined by four fillet arcs, counter-clockwise from the bottom edge.
    let corners = [
        (v(w - r, r), -FRAC_PI_2),
        (v(w - r, h - r), 0.0),
        (v(r, h - r), FRAC_PI_2),
        (v(r, r), 2.0 * FRAC_PI_2),
    ];
    let mut tangent_points = Vec::new();
    for (center, start_angle) in corners {
        let start = center + r * DVec2::from_angle(start_angle);
        let end = center + r * DVec2::from_angle(start_angle + FRAC_PI_2);
        let c = doc.add_point(center);
        let s = doc.add_point(start);
        let e = doc.add_point(end);
        doc.add_arc(c, s, e).expect("valid fillet");
        doc.set_style(c, LineKind::Construction);
        tangent_points.push((s, e));
    }
    for i in 0..4 {
        let from = tangent_points[(i + 3) % 4].1;
        let to = tangent_points[i].0;
        doc.add_line(from, to).expect("valid edge");
    }

    // Central bore, bolt circle and holes.
    let bore = v(70.0, 40.0);
    let bolt_radius = 26.0;
    let bore_center = doc.add_point(bore);
    doc.add_circle(bore_center, 14.0).expect("valid bore");
    let bolt = doc
        .add_circle(bore_center, bolt_radius)
        .expect("valid bolt circle");
    doc.set_style(bolt, LineKind::Construction);
    for i in 0..4 {
        let at = bore + bolt_radius * DVec2::from_angle(FRAC_PI_4 + f64::from(i) * FRAC_PI_2);
        let center = doc.add_point(at);
        doc.add_circle(center, 3.2).expect("valid hole");
    }

    // Centre lines through the bore.
    let reach = bolt_radius + 8.0;
    for dir in [v(1.0, 0.0), v(0.0, 1.0)] {
        let a = doc.add_point(bore - dir * reach);
        let b = doc.add_point(bore + dir * reach);
        let line = doc.add_line(a, b).expect("valid centre line");
        doc.set_style(line, LineKind::Center);
    }

    // Vertical slot on the left: two lines and two semicircular ends sharing points.
    let (x, lo, hi, sr) = (20.0, 25.0, 55.0, 4.0);
    let bl = doc.add_point(v(x - sr, lo));
    let br = doc.add_point(v(x + sr, lo));
    let tr = doc.add_point(v(x + sr, hi));
    let tl = doc.add_point(v(x - sr, hi));
    let bc = doc.add_point(v(x, lo));
    let tc = doc.add_point(v(x, hi));
    doc.add_line(br, tr).expect("valid slot side");
    doc.add_line(tl, bl).expect("valid slot side");
    doc.add_arc(bc, bl, br).expect("valid slot end");
    doc.add_arc(tc, tr, tl).expect("valid slot end");
    let a = doc.add_point(v(x, lo - sr - 4.0));
    let b = doc.add_point(v(x, hi + sr + 4.0));
    let slot_axis = doc.add_line(a, b).expect("valid slot axis");
    doc.set_style(slot_axis, LineKind::Center);

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
