//! Three-point arc: start, end, then a point on the arc.

use super::*;

/// Centre of the circle through three points, or `None` if they are (nearly) collinear.
/// Computed relative to `a` to keep precision far from the origin.
pub(super) fn circumcenter(a: DVec2, b: DVec2, c: DVec2) -> Option<DVec2> {
    let (b, c) = (b - a, c - a);
    let d = 2.0 * b.perp_dot(c);
    if d.abs() <= 1e-12 * b.length() * c.length() || d == 0.0 {
        return None;
    }
    let (bb, cc) = (b.length_squared(), c.length_squared());
    Some(a + DVec2::new(c.y * bb - b.y * cc, b.x * cc - c.x * bb) / d)
}

/// Order a start/end pair counter-clockwise for an arc from `s` to `e` that passes `m`'s side
/// of the chord: (centre, first, last), and whether they were swapped.
pub(super) fn order_through(
    center: DVec2,
    s: DVec2,
    e: DVec2,
    m: DVec2,
) -> (DVec2, DVec2, DVec2, bool) {
    // Counter-clockwise from s to e runs on the right of the chord s → e.
    if (e - s).perp_dot(m - s) < 0.0 {
        (center, s, e, false)
    } else {
        (center, e, s, true)
    }
}

/// Start point, end point, then a point on the arc (sets the bulge and radius).
#[derive(Default)]
pub struct ThreePointArcTool {
    pub(super) start: Option<Anchor>,
    pub(super) end: Option<Anchor>,
    pub(super) typed: [Option<f64>; 2],
}

impl ThreePointArcTool {
    /// End point for the pointer: typed length/angle from the start, else the pointer.
    pub(super) fn end_for(&self, p: &Pointer) -> DVec2 {
        let Some(start) = self.start else {
            return p.pos();
        };
        if !any_typed(self.typed) {
            return p.pos();
        }
        let d = p.pos() - start.pos;
        let len = self.typed[0].unwrap_or(d.length());
        let dir = match self.typed[1] {
            Some(degrees) => exact_direction(degrees),
            None if d.length() > 0.0 => d.normalize(),
            None => DVec2::X,
        };
        start.pos + dir * len
    }

    /// (centre, first, last, swapped) for the pointer, honouring a typed radius.
    pub(super) fn geometry(&self, p: &Pointer) -> Option<(DVec2, DVec2, DVec2, bool)> {
        let (s, e) = (self.start?.pos, self.end?.pos);
        let m = p.pos();
        match self.typed[0] {
            Some(r) => {
                // The shorter arc of radius r, bulging towards the pointer.
                let chord = e - s;
                let half = chord.length() / 2.0;
                let r = r.abs();
                if r < half || half == 0.0 {
                    return None;
                }
                let mut n = chord.perp().normalize();
                if n.dot(m - s) < 0.0 {
                    n = -n;
                }
                let h = (r * r - half * half).sqrt();
                let mid = s + chord / 2.0;
                let center = mid - n * h;
                Some(order_through(center, s, e, mid + n * (r - h)))
            }
            None => Some(order_through(circumcenter(s, e, m)?, s, e, m)),
        }
    }

    pub(super) fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let (Some(start), Some(end)) = (self.start, self.end) else {
            return;
        };
        let Some((center, _, _, swapped)) = self.geometry(p) else {
            ed.notice = Some("The three points are in line: move off the line".into());
            return;
        };
        let radius = self.typed[0].map(f64::abs);
        // A third point picked on an existing point stays on the arc.
        let through = p.snapped_point().filter(|_| radius.is_none());
        ed.doc.begin();
        let c = ed.doc.add_point(center);
        let s = start.resolve(&mut ed.doc);
        let e = end.resolve(&mut ed.doc);
        let (first, last) = if swapped { (e, s) } else { (s, e) };
        match ed.doc.add_arc(c, first, last) {
            Ok(arc) => {
                if let Some(value) = radius {
                    ed.add_typed_dimension(0, Constraint::Radius { curve: arc, value });
                }
                if let Some(point) = through
                    && point != s
                    && point != e
                {
                    ed.relate(Constraint::PointOnCurve { point, curve: arc });
                }
                ed.commit("3-point arc");
            }
            Err(_) => {
                ed.doc.rollback();
                ed.sync();
            }
        }
        *self = Self::default();
    }
}

impl Tool for ThreePointArcTool {
    fn is_busy(&self) -> bool {
        self.start.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::ThreePointArc
    }

    fn hint(&self) -> String {
        match (self.start, self.end) {
            (None, _) => "Click the arc's start point, or type X / Y".into(),
            (Some(_), None) => "Click the end point, or type the chord length (Tab: angle)".into(),
            _ => "Click a point on the arc (sets its size and side), or type the radius".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.start.is_none() => p,
            _ => return,
        };
        match (self.start, self.end) {
            (None, _) => self.start = Some(p.anchor()),
            (Some(start), None) => {
                if p.pos() != start.pos {
                    self.end = Some(p.anchor());
                }
            }
            _ => self.create(&p, ed),
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.start.is_some();
        *self = Self::default();
        had
    }

    fn fields(&self) -> &'static [&'static str] {
        match (self.start, self.end) {
            (None, _) => XY,
            (Some(_), None) => &["Length", "Angle"],
            _ => &["Radius"],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        match (self.start, self.end) {
            (None, _) => self.start = Some(exact(p, typed_point(self.typed, p)).anchor()),
            (Some(start), None) => {
                let at = self.end_for(p);
                if at != start.pos {
                    self.end = Some(Anchor::at(at));
                }
            }
            _ => self.create(p, ed),
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(start), Some(p)) = (self.start, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(start.pos)];
        match self.end {
            None => items.push(preview(Shape::Line {
                a: start.pos,
                b: self.end_for(p),
            })),
            Some(end) => {
                items.push(preview_vertex(end.pos));
                match self.geometry(p) {
                    Some((c, s, e, _)) => items.push(preview(arc_shape(c, s, e))),
                    None => items.push(preview(Shape::Line {
                        a: start.pos,
                        b: end.pos,
                    })),
                }
            }
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let start = self.start?.pos;
        if self.end.is_none() {
            let d = self.end_for(p) - start;
            return Some(format!(
                "Chord {}  A {:.2}°",
                p.units.format_readout(d.length()),
                d.y.atan2(d.x).to_degrees()
            ));
        }
        let (c, s, e, _) = self.geometry(p)?;
        let Shape::Arc { sweep, .. } = arc_shape(c, s, e) else {
            return None;
        };
        Some(format!(
            "R {}  {:.2}°",
            p.units.format_readout(c.distance(s)),
            sweep.to_degrees()
        ))
    }
}
