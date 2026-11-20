//! Tangent arc, continuing from the end of a line, arc, elliptical arc or curve.

use super::*;

/// The directions a curve leaves `point` in, for each line, arc, elliptical arc or Bézier
/// ending there: (curve, unit direction continuing past the end).
pub(super) fn tangents_at(doc: &drift_core::Document, point: EntityId) -> Vec<(EntityId, DVec2)> {
    doc.iter()
        .filter_map(|(id, _)| Some((id, doc.end_direction(id, point)?)))
        .collect()
}

/// A Tangent relation between a new line or arc and the curve it continues from. An
/// elliptical arc or Bézier goes second (tangent at the end they share).
pub(super) fn tangent_relation(
    doc: &drift_core::Document,
    new: EntityId,
    from: EntityId,
) -> Constraint {
    match doc.get(from).map(|e| e.kind) {
        Some(EntityKind::EllipseArc { .. } | EntityKind::Bezier { .. }) => {
            Constraint::Tangent { a: new, b: from }
        }
        _ => Constraint::Tangent { a: from, b: new },
    }
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

/// How an arc continuing a chain leaves the curve before it (Space cycles, #500).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArcMode {
    /// Tangent, bulging along the last segment's direction.
    #[default]
    Tangent,
    /// Still tangent, but leaving back along the last segment: it bulges to the other side.
    Reverse,
    /// No tangency: placed by its end, then a point it passes through.
    Free,
}

impl ArcMode {
    /// The next mode Space steps to.
    pub fn next(self) -> Self {
        match self {
            ArcMode::Tangent => ArcMode::Reverse,
            ArcMode::Reverse => ArcMode::Free,
            ArcMode::Free => ArcMode::Tangent,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ArcMode::Tangent => "Tangent",
            ArcMode::Reverse => "Reverse tangent",
            ArcMode::Free => "Free",
        }
    }
}

/// An arc tangent to the line or arc it starts from (or, with Space, reverse tangent or
/// free). Chains: each arc starts where the last ended, in the same mode. Pressing L
/// continues with lines.
#[derive(Default)]
pub struct TangentArcTool {
    /// The existing point the arc starts at, and where it is.
    pub(super) start: Option<(EntityId, DVec2)>,
    /// Curves ending at the start, with the direction each leaves it in.
    pub(super) tangents: Vec<(EntityId, DVec2)>,
    pub(super) typed: [Option<f64>; 2],
    /// Tangent, reverse tangent or free: kept for the next arc in the chain.
    pub(super) mode: ArcMode,
    /// A free arc's end, once clicked: the next click is a point the arc passes through.
    pub(super) free_end: Option<Anchor>,
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

    /// The curve to be tangent to and the direction the arc leaves in: the one best aligned
    /// with the pointer (back along the curve in reverse tangent mode).
    pub(super) fn tangent_for(&self, target: DVec2) -> Option<(EntityId, DVec2)> {
        let (_, from) = self.start?;
        let towards = (target - from).normalize_or_zero();
        let flip = if self.mode == ArcMode::Reverse {
            -1.0
        } else {
            1.0
        };
        self.tangents
            .iter()
            .map(|&(id, dir)| (id, dir * flip))
            .max_by(|a, b| a.1.dot(towards).total_cmp(&b.1.dot(towards)))
    }

    /// A free arc from the start through the pointer to the clicked end: (centre, first,
    /// last, swapped) as [`order_through`](super::three_point_arc::order_through) gives.
    pub(super) fn free_geometry(&self, p: &Pointer) -> Option<(DVec2, DVec2, DVec2, bool)> {
        let (_, from) = self.start?;
        let end = self.free_end?.pos;
        let m = p.pos();
        let (center, _) = drift_core::shape::circle_through(from, end, m)?;
        Some(super::three_point_arc::order_through(center, from, end, m))
    }

    /// Space: the next mode; the arc being placed starts over in it.
    pub(super) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
        self.free_end = None;
        self.typed = [None; 2];
    }

    /// The free arc: its end first, then a point it passes through. No tangent relation.
    fn create_free(&mut self, p: &Pointer, ed: &mut Editor) {
        let Some((point, from)) = self.start else {
            return;
        };
        let Some(end) = self.free_end else {
            // First click: the end.
            let a = p.anchor();
            if a.pos.distance(from) > 1e-9 {
                self.free_end = Some(a);
            }
            return;
        };
        let Some((center, _, _, swapped)) = self.free_geometry(p) else {
            ed.out.notice = Some("The three points are in line: move off the line".into());
            return;
        };
        let through = p.snapped_point();
        ed.doc.begin();
        let c = ed.doc.add_point(center);
        let e = end.resolve(&mut ed.doc);
        let (first, last) = if swapped { (e, point) } else { (point, e) };
        let Ok(arc) = ed.doc.add_arc(c, first, last) else {
            ed.doc.rollback();
            ed.sync();
            return;
        };
        if let Some(t) = through
            && t != point
            && t != e
        {
            ed.relate(Constraint::PointOnCurve {
                point: t,
                curve: arc,
            });
        }
        if !ed.commit("Arc") {
            return;
        }
        self.free_end = None;
        if !self.begin_at(e, ed) {
            self.start = None;
        }
        self.tangents.retain(|(id, _)| *id == arc);
    }

    /// (curve, centre, end, left) for the pointer and typed values.
    pub(super) fn geometry(&self, p: &Pointer) -> Option<(EntityId, DVec2, DVec2, bool)> {
        let (_, from) = self.start?;
        let (curve, dir) = self.tangent_for(p.pos())?;
        let (center, end, left) = tangent_arc(from, dir, p.pos(), self.typed[0], self.typed[1])?;
        Some((curve, center, end, left))
    }

    pub(super) fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        if self.mode == ArcMode::Free {
            return self.create_free(p, ed);
        }
        let Some((point, _)) = self.start else {
            return;
        };
        let Some((curve, center, end, left)) = self.geometry(p) else {
            ed.out.notice = Some("Move off the tangent line to bend the arc".into());
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
        ed.relate(tangent_relation(&ed.doc, arc, curve));
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

    fn joins_points(&self) -> bool {
        // The start always continues from the curve's end; the end joins with Auto merge.
        true
    }

    fn kind(&self) -> ToolKind {
        ToolKind::TangentArc
    }

    fn hint(&self) -> String {
        match self.start {
            None => "Click the end of a line, arc or curve to start a tangent arc".into(),
            Some(_) => {
                let next = self.mode.next().label().to_lowercase();
                match (self.mode, self.free_end) {
                    (ArcMode::Free, None) => format!(
                        "Free arc: click its end · Space: {next} · L: lines · Enter / right-click to finish"
                    ),
                    (ArcMode::Free, Some(_)) => format!(
                        "Free arc: click a point it passes through · Space: {next} · L: lines"
                    ),
                    (mode, _) => format!(
                        "{} arc: click its end, or type radius (Tab: sweep) · Space: {next} · L: lines · Enter / right-click to finish",
                        mode.label()
                    ),
                }
            }
        }
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        match key {
            ToolKey::Space if self.start.is_some() => {
                self.cycle_mode();
                true
            }
            _ => false,
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => match self.start {
                None => match p.snapped_point() {
                    Some(point) if self.begin_at(point, ed) => {}
                    _ => {
                        ed.out.notice =
                            Some("Start a tangent arc on the end of a line, arc or curve".into());
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
        if self.start.is_some() && self.mode != ArcMode::Free {
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
        if self.mode == ArcMode::Free {
            match self.free_end {
                None => {
                    items.push(preview(Shape::Line {
                        a: from,
                        b: p.pos(),
                    }));
                    items.push(preview_vertex(p.pos()));
                }
                Some(end) => {
                    if let Some((center, s, e, _)) = self.free_geometry(p) {
                        items.push(preview(arc_shape(center, s, e)));
                    }
                    items.push(preview_vertex(end.pos));
                }
            }
            return items;
        }
        if let Some((_, center, end, left)) = self.geometry(p) {
            let (s, e) = if left { (from, end) } else { (end, from) };
            items.push(preview(arc_shape(center, s, e)));
            items.push(preview_vertex(end));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let (_, from) = self.start?;
        if self.mode == ArcMode::Free {
            let (center, _, _, _) = self.free_geometry(p)?;
            return Some(format!(
                "R {}",
                p.units.format_readout(center.distance(from))
            ));
        }
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
