//! A sample drawing used until the document model and drawing tools land (M1).

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use glam::DVec2;

use crate::{LineKind, Shape, SketchItem};

/// A 120 × 80 mm mounting plate with filleted corners, a central bore, a bolt circle and a slot.
pub fn mounting_plate() -> Vec<SketchItem> {
    let v = DVec2::new;
    let line = |a: DVec2, b: DVec2| SketchItem::new(Shape::Line { a, b });
    let arc = |center: DVec2, radius: f64, start: f64, sweep: f64| {
        SketchItem::new(Shape::Arc {
            center,
            radius,
            start,
            sweep,
        })
    };
    let circle = |center: DVec2, radius: f64| SketchItem::new(Shape::Circle { center, radius });

    let (w, h, r) = (120.0, 80.0, 10.0);
    let mut items = vec![
        // Outline with filleted corners.
        line(v(r, 0.0), v(w - r, 0.0)),
        line(v(w, r), v(w, h - r)),
        line(v(w - r, h), v(r, h)),
        line(v(0.0, h - r), v(0.0, r)),
        arc(v(w - r, r), r, -FRAC_PI_2, FRAC_PI_2),
        arc(v(w - r, h - r), r, 0.0, FRAC_PI_2),
        arc(v(r, h - r), r, FRAC_PI_2, FRAC_PI_2),
        arc(v(r, r), r, PI, FRAC_PI_2),
    ];

    // Central bore and bolt circle.
    let bore = v(70.0, 40.0);
    let bolt_radius = 26.0;
    items.push(circle(bore, 14.0));
    items.push(circle(bore, bolt_radius).with_kind(LineKind::Construction));
    for i in 0..4 {
        let center = bore + bolt_radius * DVec2::from_angle(FRAC_PI_4 + f64::from(i) * FRAC_PI_2);
        items.push(circle(center, 3.2));
        items.push(SketchItem::new(Shape::Point(center)));
    }
    items.push(SketchItem::new(Shape::Point(bore)));

    // Centre lines through the bore.
    let reach = bolt_radius + 8.0;
    items.push(line(bore - v(reach, 0.0), bore + v(reach, 0.0)).with_kind(LineKind::Center));
    items.push(line(bore - v(0.0, reach), bore + v(0.0, reach)).with_kind(LineKind::Center));

    // Vertical slot on the left.
    let (slot_x, slot_lo, slot_hi, slot_r) = (20.0, 25.0, 55.0, 4.0);
    items.push(line(
        v(slot_x - slot_r, slot_hi),
        v(slot_x - slot_r, slot_lo),
    ));
    items.push(line(
        v(slot_x + slot_r, slot_lo),
        v(slot_x + slot_r, slot_hi),
    ));
    items.push(arc(v(slot_x, slot_lo), slot_r, PI, PI));
    items.push(arc(v(slot_x, slot_hi), slot_r, 0.0, PI));
    items.push(
        line(
            v(slot_x, slot_lo - slot_r - 4.0),
            v(slot_x, slot_hi + slot_r + 4.0),
        )
        .with_kind(LineKind::Center),
    );

    items
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
