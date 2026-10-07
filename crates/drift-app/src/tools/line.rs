//! Line: a polyline chain, with typed length/angle and X/Y axis locks.

use super::*;

#[derive(Default)]
pub struct LineTool {
    pub(super) last: Option<Anchor>,
    pub(super) chain_start: Option<EntityId>,
    pub(super) typed: [Option<f64>; 2],
    /// Axis the current segment is locked to; cleared when the segment is placed.
    pub(super) lock: Option<Axis>,
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
            self.last = Some(anchor);
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
        match self.lock {
            // An explicit axis lock always gets its relation.
            Some(axis) => {
                let _ = ed.doc.add_constraint(match axis {
                    Axis::X => Constraint::Horizontal { line },
                    Axis::Y => Constraint::Vertical { line },
                });
            }
            None => auto_relation(ed, relation, line),
        }
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
        } else {
            self.last = Some(Anchor {
                pos: ed.doc.point(b).unwrap_or(anchor.pos),
                existing: Some(b),
                midpoint_of: None,
                on_curve: None,
            });
        }
    }

    pub(super) fn finish(&mut self) {
        self.last = None;
        self.chain_start = None;
        self.lock = None;
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
        let Some(last) = self.last else { return };
        if let Some(axis) = self.lock {
            // Project onto the axis through the last point. A snap target off the axis still
            // sets how far along it goes (shown with a guide line from the target); it is only
            // reused if it lies exactly on the axis.
            let u = axis.dir();
            let target = p.pos();
            let at = last.pos + u * (target - last.pos).dot(u);
            let reference = p.snap.map(|s| s.at).filter(|q| *q != at);
            if reference.is_some() {
                p.snap = None;
            }
            p.guide = Some(Guide {
                at,
                kind: axis.relation(),
                from: [Some(last.pos), reference],
            });
            return;
        }
        if p.snapped_point().is_some() {
            return;
        }
        let at = p.pos();
        let d = at - last.pos;
        let tol = p.tolerance;
        let (kind, at) = if d.y.abs() <= tol && d.x.abs() > tol {
            (GuideKind::Horizontal, DVec2::new(at.x, last.pos.y))
        } else if d.x.abs() <= tol && d.y.abs() > tol {
            (GuideKind::Vertical, DVec2::new(last.pos.x, at.y))
        } else {
            return;
        };
        let other = p.guide.and_then(|g| g.from[0]);
        p.guide = Some(Guide {
            at,
            kind,
            from: [Some(last.pos), other],
        });
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
            self.last = Some(Anchor {
                pos,
                existing: Some(point),
                midpoint_of: None,
                on_curve: None,
            });
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
