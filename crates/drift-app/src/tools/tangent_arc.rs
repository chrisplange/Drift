//! Tangent arc, continuing from the end of a line or arc.

use super::*;

/// The directions a curve leaves `point` in, for each line or arc ending there: (curve, unit
/// direction continuing past the end).
pub(super) fn tangents_at(doc: &drift_core::Document, point: EntityId) -> Vec<(EntityId, DVec2)> {
    let Ok(p) = doc.point(point) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (id, e) in doc.iter() {
        let dir = match e.kind {
            EntityKind::Line { a, b } if a == point || b == point => {
                let other = if a == point { b } else { a };
                doc.point(other).ok().map(|q| p - q)
            }
            EntityKind::Arc { center, start, end } if start == point || end == point => {
                doc.point(center).ok().map(|c| {
                    // Counter-clockwise travel direction at the end; reversed at the start.
                    let along = (p - c).perp();
                    if end == point { along } else { -along }
                })
            }
            _ => None,
        };
        if let Some(d) = dir.filter(|d| d.length() > 0.0) {
            out.push((id, d.normalize()));
        }
    }
    out
}

/// An arc leaving `from` along `dir`: centre, end and whether it turns left (counter-clockwise),
/// from a target end point and/or a typed radius and sweep (degrees, positive turns left).
pub(super) fn tangent_arc(
    from: DVec2,
    dir: DVec2,
    target: DVec2,
    radius: Option<f64>,
    sweep: Option<f64>,
) -> Option<(DVec2, DVec2, bool)> {
    let n = dir.perp(); // left of the travel direction
    let d = target - from;
    let nd = n.dot(d);
    // Radius and side through the target point (centre on the normal, equidistant).
    let through = (nd.abs() > 1e-12 * d.length()).then(|| {
        let s = d.length_squared() / (2.0 * nd);
        (s.abs(), s > 0.0)
    });
    let (r, left) = match (radius, sweep) {
        (Some(r), Some(sweep)) => (r.abs(), sweep > 0.0),
        (Some(r), None) => (r.abs(), nd >= 0.0),
        (None, Some(sweep)) => (through?.0, sweep > 0.0),
        (None, None) => through?,
    };
    if r <= 0.0 {
        return None;
    }
    let center = from + n * if left { r } else { -r };
    let end = match sweep {
        Some(sweep) => center + DVec2::from_angle(sweep.to_radians()).rotate(from - center),
        None if radius.is_some() => {
            let v = target - center;
            if v == DVec2::ZERO {
                return None;
            }
            center + v.normalize() * r
        }
        None => target,
    };
    (end != from).then_some((center, end, left))
}

/// An arc tangent to the line or arc it starts from. Chains: each arc starts where the last
/// ended. Pressing L continues with lines.
#[derive(Default)]
pub struct TangentArcTool {
    /// The existing point the arc starts at, and where it is.
    pub(super) start: Option<(EntityId, DVec2)>,
    /// Curves ending at the start, with the direction each leaves it in.
    pub(super) tangents: Vec<(EntityId, DVec2)>,
    pub(super) typed: [Option<f64>; 2],
}

impl TangentArcTool {
    pub(super) fn begin_at(&mut self, point: EntityId, ed: &Editor) -> bool {
        let tangents = tangents_at(&ed.doc, point);
        let Ok(pos) = ed.doc.point(point) else {
            return false;
        };
        if tangents.is_empty() {
            return false;
        }
        self.start = Some((point, pos));
        self.tangents = tangents;
        true
    }

    /// The curve to be tangent to and its direction: the one best aligned with the pointer.
    pub(super) fn tangent_for(&self, target: DVec2) -> Option<(EntityId, DVec2)> {
        let (_, from) = self.start?;
        let towards = (target - from).normalize_or_zero();
        self.tangents
            .iter()
            .copied()
            .max_by(|a, b| a.1.dot(towards).total_cmp(&b.1.dot(towards)))
    }

    /// (curve, centre, end, left) for the pointer and typed values.
    pub(super) fn geometry(&self, p: &Pointer) -> Option<(EntityId, DVec2, DVec2, bool)> {
        let (_, from) = self.start?;
        let (curve, dir) = self.tangent_for(p.pos())?;
        let (center, end, left) = tangent_arc(from, dir, p.pos(), self.typed[0], self.typed[1])?;
        Some((curve, center, end, left))
    }

    pub(super) fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let Some((point, _)) = self.start else {
            return;
        };
        let Some((curve, center, end, left)) = self.geometry(p) else {
            ed.notice = Some("Move off the tangent line to bend the arc".into());
            return;
        };
        let typed = any_typed(self.typed);
        let anchor = if typed {
            Anchor::at(end)
        } else {
            // Reuse a snapped point only if the arc ends exactly on it.
            let mut a = p.anchor();
            a.pos = end;
            a.existing = a.existing.filter(|_| p.pos() == end);
            a
        };
        let radius = self.typed[0].map(f64::abs);
        ed.doc.begin();
        let c = ed.doc.add_point(center);
        let e = anchor.resolve(&mut ed.doc);
        let (first, last) = if left { (point, e) } else { (e, point) };
        let Ok(arc) = ed.doc.add_arc(c, first, last) else {
            ed.doc.rollback();
            ed.sync();
            return;
        };
        ed.relate(Constraint::Tangent { a: curve, b: arc });
        if let Some(value) = radius {
            ed.add_typed_dimension(0, Constraint::Radius { curve: arc, value });
        }
        if !ed.commit("Tangent arc") {
            return;
        }
        // Chain: the next arc continues from this one's end.
        self.typed = [None; 2];
        if !self.begin_at(e, ed) {
            self.start = None;
        }
        // Only the new arc counts as the curve to continue from.
        self.tangents.retain(|(id, _)| *id == arc);
    }
}

impl Tool for TangentArcTool {
    fn is_busy(&self) -> bool {
        self.start.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::TangentArc
    }

    fn hint(&self) -> String {
        match self.start {
            None => "Click the end of a line or arc to start a tangent arc".into(),
            Some(_) => "Click the arc's end, or type radius (Tab: sweep) · L: lines · Enter / right-click to finish"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => match self.start {
                None => match p.snapped_point() {
                    Some(point) if self.begin_at(point, ed) => {}
                    _ => {
                        ed.notice = Some("Start a tangent arc on the end of a line or arc".into());
                    }
                },
                Some(_) => self.create(&p, ed),
            },
            Input::DragStart(p, _) if self.start.is_none() => {
                if let Some(point) = p.snapped_point() {
                    self.begin_at(point, ed);
                }
            }
            Input::DoubleClick(_) | Input::Finish => *self = Self::default(),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.start.is_some();
        *self = Self::default();
        had
    }

    fn chain_end(&self) -> Option<EntityId> {
        self.start.map(|(point, _)| point)
    }

    fn continue_from(&mut self, point: EntityId, ed: &mut Editor) {
        self.begin_at(point, ed);
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.start.is_some() {
            &["Radius", "Sweep"]
        } else {
            &[]
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        self.create(p, ed);
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some((_, from)), Some(p)) = (self.start, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(from)];
        if let Some((_, center, end, left)) = self.geometry(p) {
            let (s, e) = if left { (from, end) } else { (end, from) };
            items.push(preview(arc_shape(center, s, e)));
            items.push(preview_vertex(end));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let (_, from) = self.start?;
        let (_, center, end, left) = self.geometry(p)?;
        let (s, e) = if left { (from, end) } else { (end, from) };
        let Shape::Arc { sweep, .. } = arc_shape(center, s, e) else {
            return None;
        };
        let sweep = if left { sweep } else { -sweep };
        Some(format!(
            "R {}  {:.2}°",
            p.units.format_readout(center.distance(from)),
            sweep.to_degrees()
        ))
    }
}
