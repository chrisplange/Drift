//! Line: a polyline chain, with typed length/angle and X/Y axis locks.

use super::*;

#[derive(Default)]
pub struct LineTool {
    pub(super) last: Option<Anchor>,
    pub(super) chain_start: Option<EntityId>,
    pub(super) typed: [Option<f64>; 2],
    /// Axis the current segment is locked to; cleared when the segment is placed.
    pub(super) lock: Option<Axis>,
    /// Guides from the curves ending at the last point: square to a line, along an arc's
    /// tangent (the relation and the guide's direction).
    pub(super) guides: Vec<(GuideKind, DVec2)>,
}

impl LineTool {
    /// Where the next point goes: typed length/angle from the last point, else the pointer.
    pub(super) fn target(&self, p: &Pointer) -> DVec2 {
        let Some(last) = self.last else {
            return typed_point(self.typed, p);
        };
        if !any_typed(self.typed) {
            return p.pos();
        }
        let d = p.pos() - last.pos;
        let len = self.typed[0].unwrap_or(d.length());
        let dir = match (self.typed[1], self.lock) {
            (Some(degrees), _) => exact_direction(degrees),
            // Locked: along the axis, towards the pointer's side.
            (None, Some(axis)) => {
                let u = axis.dir();
                if d.dot(u) < 0.0 { -u } else { u }
            }
            (None, None) if d.length() > 0.0 => d.normalize(),
            (None, None) => DVec2::X,
        };
        last.pos + dir * len
    }

    /// Place the next point at `anchor`. `length` adds a length dimension; `relation` an
    /// automatic horizontal/vertical relation.
    pub(super) fn place(
        &mut self,
        anchor: Anchor,
        relation: Option<GuideKind>,
        length: Option<f64>,
        ed: &mut Editor,
    ) {
        let Some(last) = self.last else {
            self.set_last(anchor, ed);
            self.chain_start = None;
            return;
        };
        if anchor.pos == last.pos {
            return;
        }
        ed.doc.begin();
        let a = last.resolve(&mut ed.doc);
        let b = anchor.resolve(&mut ed.doc);
        let Ok(line) = ed.doc.add_line(a, b) else {
            ed.doc.rollback();
            ed.sync();
            return;
        };
        // A locked segment is drawn exactly along its axis either way; it is only kept there
        // by a relation when automatic relations are on.
        let relation = self.lock.map(Axis::relation).or(relation);
        auto_relation(ed, relation, line);
        if let Some(value) = length {
            ed.add_typed_dimension(0, Constraint::Distance { a, b, value });
        }
        if !ed.commit("Line") {
            return;
        }
        // Locks are per segment.
        self.lock = None;
        let start = *self.chain_start.get_or_insert(a);
        if b == start {
            // Closed the loop.
            self.last = None;
            self.chain_start = None;
            self.guides.clear();
        } else {
            let pos = ed.doc.point(b).unwrap_or(anchor.pos);
            self.set_last(Anchor::existing(pos, b), ed);
        }
    }

    /// Draw on from `anchor`, with guides from the lines and arcs that end there.
    fn set_last(&mut self, anchor: Anchor, ed: &Editor) {
        self.last = Some(anchor);
        self.guides = anchor
            .existing
            .map(|point| super::tangent_arc::tangents_at(&ed.doc, point))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(curve, dir)| match ed.doc.get(curve)?.kind {
                EntityKind::Line { .. } => Some((GuideKind::Perpendicular(curve), dir.perp())),
                EntityKind::Arc { .. } => Some((GuideKind::Tangent(curve), dir)),
                _ => None,
            })
            .collect();
    }

    pub(super) fn finish(&mut self) {
        self.last = None;
        self.chain_start = None;
        self.lock = None;
        self.guides.clear();
    }

    /// Lock (or, pressed again, unlock) the segment in progress to an axis. False before the
    /// first point.
    pub(super) fn lock_axis(&mut self, axis: Axis) -> bool {
        if self.last.is_none() {
            return false;
        }
        self.lock = if self.lock == Some(axis) {
            None
        } else {
            Some(axis)
        };
        true
    }
}

impl Tool for LineTool {
    fn is_busy(&self) -> bool {
        self.last.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Line
    }

    fn hint(&self) -> String {
        match self.last {
            None => "Click the start point, or type X / Y".into(),
            Some(_) => match self.lock {
                Some(axis) => format!(
                    "Locked to the {axis:?} axis ({axis:?} again to unlock) · click the next point \
                     or type a length"
                ),
                None => "Click the next point or type a length (Tab: angle) · X/Y: lock axis · Enter: finish"
                    .into(),
            },
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => {
                self.place(p.anchor(), p.auto_relation(), None, ed)
            }
            Input::DragStart(p, _) if self.last.is_none() => self.place(p.anchor(), None, None, ed),
            Input::DoubleClick(_) | Input::Finish => self.finish(),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.last.is_some();
        self.finish();
        had
    }

    fn refine_pointer(&self, p: &mut Pointer) {
        if let Some(last) = self.last {
            if self.lock.is_none() && follow_guides(last.pos, &self.guides, p) {
                return;
            }
            steer(last.pos, self.lock, p);
        }
    }

    fn snap_from(&self) -> Option<DVec2> {
        self.last.map(|last| last.pos)
    }

    fn fields(&self) -> &'static [&'static str] {
        if self.last.is_some() {
            &["Length", "Angle"]
        } else {
            XY
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        match key {
            ToolKey::X => self.lock_axis(Axis::X),
            ToolKey::Y => self.lock_axis(Axis::Y),
            _ => false,
        }
    }

    fn axis_lock(&self) -> Option<(DVec2, Axis)> {
        Some((self.last?.pos, self.lock?))
    }

    fn chain_end(&self) -> Option<EntityId> {
        self.last?.existing
    }

    fn continue_from(&mut self, point: EntityId, ed: &mut Editor) {
        if let Ok(pos) = ed.doc.point(point) {
            self.set_last(Anchor::existing(pos, point), ed);
            self.chain_start = None;
        }
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let target = self.target(p);
        let anchor = Anchor::at(target);
        if self.last.is_none() {
            self.place(anchor, None, None, ed);
        } else {
            if self.typed[1].is_some() {
                // A typed angle overrides the lock.
                self.lock = None;
            }
            let relation = match self.typed[1] {
                Some(degrees) => relation_for_angle(degrees),
                None => p.auto_relation(),
            };
            let length = self.typed[0].filter(|v| *v > 0.0);
            self.place(anchor, relation, length, ed);
        }
        self.typed = [None; 2];
    }
    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        match (self.last, pointer) {
            (Some(last), Some(p)) => vec![
                preview(Shape::Line {
                    a: last.pos,
                    b: self.target(p),
                }),
                preview_vertex(last.pos),
            ],
            _ => Vec::new(),
        }
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let last = self.last?;
        let d = self.target(p) - last.pos;
        let angle = d.y.atan2(d.x).to_degrees();
        let lock = self
            .lock
            .map_or(String::new(), |axis| format!("  [{axis:?} locked]"));
        Some(format!(
            "L {}  A {angle:.2}°{lock}",
            p.units.format_readout(d.length())
        ))
    }
}

/// Put the pointer on a perpendicular or tangent guide through `from` when it is near one (and
/// no exact snap decides where it goes). Square to a horizontal or vertical line is left to
/// the horizontal / vertical guides, whose relations hold the same. True if a guide applied.
pub(super) fn follow_guides(from: DVec2, guides: &[(GuideKind, DVec2)], p: &mut Pointer) -> bool {
    if !p.perp_tangent || p.exact_snap().is_some() {
        return false;
    }
    let target = p.snap.map_or(p.raw, |grid| grid.at);
    let d = target - from;
    let tol = p.tolerance;
    let mut best: Option<(f64, DVec2, GuideKind)> = None;
    for &(kind, dir) in guides {
        let along = d.dot(dir);
        let off = (d - dir * along).length();
        let level = dir.x.abs() <= 1e-12 || dir.y.abs() <= 1e-12;
        if along.abs() <= tol || off > tol || (level && matches!(kind, GuideKind::Perpendicular(_)))
        {
            continue;
        }
        if best.is_none_or(|(o, ..)| off < o) {
            best = Some((off, from + dir * along, kind));
        }
    }
    let Some((_, at, kind)) = best else {
        return false;
    };
    p.guide = Some(Guide {
        at,
        kind,
        from: [Some(from), None],
    });
    true
}

/// Steer the pointer for a segment from `from`: along a locked axis (X / Y), else in the
/// document's angle steps, else onto the horizontal or vertical when it is nearly there.
/// An exact snap still decides where an unlocked segment ends.
pub(super) fn steer(from: DVec2, lock: Option<Axis>, p: &mut Pointer) {
    if let Some(axis) = lock {
        // Project onto the axis through `from`. A snap target off the axis still
        // sets how far along it goes (shown with a guide line from the target); it is only
        // reused if it lies exactly on the axis.
        let u = axis.dir();
        let target = p.pos();
        let at = from + u * (target - from).dot(u);
        let reference = p.snap.map(|s| s.at).filter(|q| *q != at);
        if reference.is_some() {
            p.snap = None;
        }
        p.guide = Some(Guide {
            at,
            kind: axis.relation(),
            from: [Some(from), reference],
        });
        return;
    }
    if p.exact_snap().is_some() {
        return;
    }
    // Angle snapping: the segment turns in steps, and is as long as the pointer is far
    // along that direction. (An exact snap, above, still wins: the line goes to it.)
    if let Some(step) = p.angle_step.filter(|s| *s > 0.0) {
        let target = p.snap.map_or(p.raw, |grid| grid.at);
        let d = target - from;
        if d.length() <= p.tolerance {
            return;
        }
        let degrees = (d.y.atan2(d.x).to_degrees() / step).round() * step;
        let u = exact_direction(degrees);
        p.guide = Some(Guide {
            at: from + u * d.dot(u).max(0.0),
            kind: relation_for_angle(degrees).unwrap_or(GuideKind::Aligned),
            from: [Some(from), None],
        });
        return;
    }
    let at = p.pos();
    let d = at - from;
    let tol = p.tolerance;
    let (kind, at) = if d.y.abs() <= tol && d.x.abs() > tol {
        (GuideKind::Horizontal, DVec2::new(at.x, from.y))
    } else if d.x.abs() <= tol && d.y.abs() > tol {
        (GuideKind::Vertical, DVec2::new(from.x, at.y))
    } else {
        return;
    };
    let other = p.guide.and_then(|g| g.from[0]);
    p.guide = Some(Guide {
        at,
        kind,
        from: [Some(from), other],
    });
}
