//! Where dimension and annotation labels sit, worked out from the document alone: the
//! points a dimension's label is measured from, a note's label in the world, the next free
//! mark label. The app lays the labels out and paints them (`annotations`, `notes`); the
//! editor and the tools need only these.

use drift_core::{
    Anchor, Annotation, Constraint, DVec2, Document, EntityId, EntityKind, OrdinateAxis, Shape,
};

/// The world point a dimension's label offset is measured from.
pub fn dimension_anchor(doc: &Document, c: &Constraint) -> Option<DVec2> {
    // Points, or lines standing for their midpoints.
    let p = |id| doc.ref_point(id);
    Some(match *c {
        Constraint::Distance { a, b, .. }
        | Constraint::HorizontalDistance { a, b, .. }
        | Constraint::VerticalDistance { a, b, .. } => (p(a)? + p(b)?) * 0.5,
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point)?;
            (q + foot(doc, q, line)?) * 0.5
        }
        Constraint::Radius { curve, .. } | Constraint::Diameter { curve, .. } => {
            center_of(doc, curve)?
        }
        Constraint::Angle { a, b, .. } => angle_vertex(doc, a, b)?,
        _ => return None,
    })
}

/// A label offset for a newly placed dimension: as far from the geometry as the pointer is,
/// with the number in the middle of the dimension line. (Dragging the label afterwards moves
/// it along the line too.)
pub fn centred_offset(doc: &Document, c: &Constraint, offset: DVec2) -> DVec2 {
    let p = |id| doc.ref_point(id);
    let across = |a: Option<DVec2>, b: Option<DVec2>| match (a, b) {
        (Some(a), Some(b)) if a != b => {
            let n = (b - a).normalize().perp();
            n * offset.dot(n)
        }
        _ => offset,
    };
    match *c {
        Constraint::Distance { a, b, .. } => across(p(a), p(b)),
        Constraint::PointLineDistance { point, line, .. } => {
            let q = p(point);
            across(q, q.and_then(|q| foot(doc, q, line)))
        }
        Constraint::HorizontalDistance { .. } => DVec2::new(0.0, offset.y),
        Constraint::VerticalDistance { .. } => DVec2::new(offset.x, 0.0),
        _ => offset,
    }
}

pub fn center_of(doc: &Document, curve: EntityId) -> Option<DVec2> {
    match doc.get(curve)?.kind {
        EntityKind::Circle { center, .. } | EntityKind::Arc { center, .. } => {
            doc.point(center).ok()
        }
        _ => None,
    }
}

pub fn line_points(doc: &Document, line: EntityId) -> Option<(DVec2, DVec2)> {
    match doc.get(line)?.kind {
        EntityKind::Line { a, b } => Some((doc.point(a).ok()?, doc.point(b).ok()?)),
        _ => None,
    }
}

/// Foot of the perpendicular from `q` onto the (infinite) line.
pub fn foot(doc: &Document, q: DVec2, line: EntityId) -> Option<DVec2> {
    let (a, b) = line_points(doc, line)?;
    let d = b - a;
    let len2 = d.length_squared();
    (len2 > 0.0).then(|| a + d * ((q - a).dot(d) / len2))
}

/// Intersection of two lines' supporting lines, or the midpoint between them if parallel.
pub fn angle_vertex(doc: &Document, l1: EntityId, l2: EntityId) -> Option<DVec2> {
    let (a1, b1) = line_points(doc, l1)?;
    let (a2, b2) = line_points(doc, l2)?;
    Some(
        drift_geom::intersect::line_line(a1, b1 - a1, a2, b2 - a2)
            .map_or((a1 + b1 + a2 + b2) * 0.25, |x| x.at),
    )
}

/// Unit direction along a line from `vertex` (where it meets the other line of an angle)
/// toward the line itself: its end farther from the vertex.
pub fn ray_from(doc: &Document, line: EntityId, vertex: DVec2) -> Option<DVec2> {
    let (a, b) = line_points(doc, line)?;
    let far = if a.distance_squared(vertex) >= b.distance_squared(vertex) {
        a
    } else {
        b
    };
    (far - vertex)
        .try_normalize()
        .or_else(|| (b - a).try_normalize())
}

/// The order of two lines for an angle dimension that measures the angle between them as
/// they are drawn (at their corner), rather than its supplement.
pub fn corner_order(doc: &Document, a: EntityId, b: EntityId) -> (EntityId, EntityId) {
    let corner = (|| {
        let vertex = angle_vertex(doc, a, b)?;
        let (r1, r2) = (ray_from(doc, a, vertex)?, ray_from(doc, b, vertex)?);
        Some(r1.perp_dot(r2).atan2(r1.dot(r2)).abs().to_degrees())
    })();
    let measured = drift_solver::measure(doc, &Constraint::Angle { a, b, degrees: 0.0 });
    match (corner, measured) {
        // The solver measures a → b; the other order measures the supplement.
        (Some(corner), Some(m)) if (m - corner).abs() > (180.0 - m - corner).abs() => (b, a),
        _ => (a, b),
    }
}

/// The centre of a circle or arc, or a point's position.
pub fn centre(doc: &Document, id: EntityId) -> Option<(DVec2, f64)> {
    match doc.get(id)?.kind {
        EntityKind::Circle { center, radius } => Some((doc.point(center).ok()?, radius)),
        EntityKind::Arc { center, start, .. } => {
            let c = doc.point(center).ok()?;
            Some((c, c.distance(doc.point(start).ok()?)))
        }
        EntityKind::Ellipse { center, major, .. }
        | EntityKind::EllipseArc { center, major, .. } => {
            let c = doc.point(center).ok()?;
            Some((c, c.distance(doc.point(major).ok()?)))
        }
        EntityKind::Point(p) => Some((p, 0.0)),
        EntityKind::Line { .. } | EntityKind::Bezier { .. } => None,
    }
}

pub fn line_ends(doc: &Document, id: EntityId) -> Option<(DVec2, DVec2)> {
    match doc.shape(id)? {
        Shape::Line { a, b } => Some((a, b)),
        _ => None,
    }
}

/// Where an annotation's label is anchored in the world (what dragging moves).
pub fn label_world(doc: &Document, a: &Annotation) -> Option<DVec2> {
    match a {
        Annotation::Note { at, leaders, .. } => doc.note_text_pos(*at, leaders),
        Annotation::HoleCallout { curve, at, .. } => Some(centre(doc, *curve)?.0 + *at),
        Annotation::ChamferCallout { line, at } => {
            let (p, q) = line_ends(doc, *line)?;
            Some((p + q) * 0.5 + *at)
        }
        Annotation::Ordinate {
            origin,
            point,
            axis,
            at,
        } => {
            let (o, p) = (doc.ref_point(*origin)?, doc.ref_point(*point)?);
            Some(match axis {
                OrdinateAxis::X => DVec2::new(p.x, o.y + at),
                OrdinateAxis::Y => DVec2::new(o.x + at, p.y),
            })
        }
        Annotation::CentreMark { .. } | Annotation::Centreline { .. } => None,
        // A mark is dragged whole, by its first end.
        Annotation::Section { a, .. } => Some(*a),
        Annotation::Elevation { at, .. } => Some(*at),
    }
}

/// The annotation with its label moved to `to` (world).
pub fn with_label_at(doc: &Document, a: &Annotation, to: DVec2) -> Option<Annotation> {
    let mut a = a.clone();
    match &mut a {
        Annotation::Note { at, leaders, .. } => *at = doc.note_at(to, leaders)?,
        Annotation::HoleCallout { curve, at, .. } => *at = to - centre(doc, *curve)?.0,
        Annotation::ChamferCallout { line, at } => {
            let (p, q) = line_ends(doc, *line)?;
            *at = to - (p + q) * 0.5;
        }
        Annotation::Ordinate {
            origin, axis, at, ..
        } => {
            let o = doc.ref_point(*origin)?;
            *at = match axis {
                OrdinateAxis::X => to.y - o.y,
                OrdinateAxis::Y => to.x - o.x,
            };
        }
        Annotation::CentreMark { .. } | Annotation::Centreline { .. } => return None,
        Annotation::Section { a, b, .. } => {
            *b += to - *a;
            *a = to;
        }
        Annotation::Elevation { at, .. } => *at = to,
    }
    Some(a)
}

/// The next unused label for a new section (`A`, `B` … then `A1` …) or elevation (`1`,
/// `2` …).
pub fn next_mark_label(doc: &Document, section: bool) -> String {
    let used: Vec<&str> = doc
        .annotations()
        .filter_map(|(_, a)| match a {
            Annotation::Section { label, .. } if section => Some(label.as_str()),
            Annotation::Elevation { label, .. } if !section => Some(label.as_str()),
            _ => None,
        })
        .collect();
    let free = |label: &String| !used.contains(&label.as_str());
    if section {
        (0..)
            .flat_map(|round: u32| {
                ('A'..='Z').map(move |c| {
                    if round == 0 {
                        c.to_string()
                    } else {
                        format!("{c}{round}")
                    }
                })
            })
            .find(free)
            .expect("an unbounded supply")
    } else {
        (1u32..)
            .map(|n| n.to_string())
            .find(free)
            .expect("an unbounded supply")
    }
}

/// The anchor for a leader tip at the pointer: a snapped point, a spot on a curve, or free.
pub fn anchor_at(
    doc: &Document,
    point: Option<EntityId>,
    curve: Option<EntityId>,
    at: DVec2,
) -> Anchor {
    if let Some(p) = point {
        return Anchor::Point(p);
    }
    if let Some(c) = curve {
        match doc.shape(c) {
            Some(Shape::Line { a, b }) => {
                let d = b - a;
                let t = if d.length_squared() > 0.0 {
                    ((at - a).dot(d) / d.length_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return Anchor::Curve { curve: c, t };
            }
            Some(Shape::Circle { center, .. } | Shape::Arc { center, .. }) => {
                let v = at - center;
                return Anchor::Curve {
                    curve: c,
                    t: v.y.atan2(v.x),
                };
            }
            _ => {}
        }
    }
    Anchor::Free(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_dimensions_are_centred_on_their_line() {
        let mut doc = Document::new();
        doc.begin();
        let a = doc.add_point(DVec2::new(0.0, 0.0));
        let b = doc.add_point(DVec2::new(40.0, 0.0));
        let c = doc.add_point(DVec2::new(40.0, 30.0));
        doc.commit("Points");
        let off = DVec2::new(13.0, 7.0);
        let value = 0.0;
        // Along the measured direction the offset goes; across it, it is kept.
        let d = centred_offset(&doc, &Constraint::Distance { a, b, value }, off);
        assert!(d.distance(DVec2::new(0.0, 7.0)) < 1e-12);
        let d = centred_offset(&doc, &Constraint::Distance { a, b: c, value }, off);
        assert!(d.dot(DVec2::new(40.0, 30.0)).abs() < 1e-9 && d.length() > 1.0);
        let h = Constraint::HorizontalDistance { a, b: c, value };
        assert_eq!(centred_offset(&doc, &h, off), DVec2::new(0.0, 7.0));
        let v = Constraint::VerticalDistance { a, b: c, value };
        assert_eq!(centred_offset(&doc, &v, off), DVec2::new(13.0, 0.0));
    }

    #[test]
    fn marks_take_the_next_free_label() {
        let mut doc = Document::new();
        assert_eq!(next_mark_label(&doc, true), "A");
        assert_eq!(next_mark_label(&doc, false), "1");
        for label in ["A", "C"] {
            doc.add_annotation(Annotation::Section {
                a: DVec2::ZERO,
                b: DVec2::X,
                label: label.into(),
                sheet: String::new(),
                flip: false,
                view: None,
            })
            .unwrap();
        }
        doc.add_annotation(Annotation::Elevation {
            at: DVec2::ZERO,
            angle: 0.0,
            label: "1".into(),
            sheet: String::new(),
            view: None,
        })
        .unwrap();
        // Gaps are filled; sections and elevations are numbered apart.
        assert_eq!(next_mark_label(&doc, true), "B");
        assert_eq!(next_mark_label(&doc, false), "2");
    }
}
