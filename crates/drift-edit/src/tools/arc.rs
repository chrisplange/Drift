//! Centre-point arc: centre, start, end; the direction follows the pointer.

use super::*;

#[derive(Default)]
pub struct ArcTool {
    pub(super) center: Option<Anchor>,
    pub(super) start: Option<Anchor>,
    /// Signed angle the pointer has swept around the centre since the start was placed.
    pub(super) swept: f64,
    pub(super) last_angle: f64,
    pub(super) typed: [Option<f64>; 2],
    /// A radius typed for the start point, to add as a dimension when the arc is created.
    pub(super) radius_dimension: Option<f64>,
    /// The typed radius expression, if it used variables.
    pub(super) radius_expr: Option<String>,
}

impl ArcTool {
    pub(super) fn angle_of(&self, at: DVec2) -> f64 {
        let c = self.center.map_or(DVec2::ZERO, |c| c.pos);
        (at - c).y.atan2((at - c).x)
    }

    pub(super) fn track(&mut self, at: DVec2) {
        if self.start.is_some() {
            let angle = self.angle_of(at);
            let mut delta = angle - self.last_angle;
            if delta > PI {
                delta -= TAU;
            } else if delta < -PI {
                delta += TAU;
            }
            self.swept += delta;
            self.last_angle = angle;
        }
    }

    /// Where the start point goes for a pointer position (typed radius, pointer direction).
    pub(super) fn start_for(&self, p: &Pointer) -> DVec2 {
        let center = self.center.map_or(DVec2::ZERO, |c| c.pos);
        match self.typed[0] {
            Some(r) => {
                let d = p.pos() - center;
                let dir = if d.length() > 0.0 {
                    d.normalize()
                } else {
                    DVec2::X
                };
                center + dir * r.abs()
            }
            None => p.pos(),
        }
    }

    /// Centre, start and end of the arc for the pointer position, ordered counter-clockwise.
    pub(super) fn geometry(&self, p: &Pointer) -> Option<(DVec2, DVec2, DVec2)> {
        let center = self.center?.pos;
        let start = self.start?.pos;
        let radius = start.distance(center);
        let end = match self.typed[0] {
            // Typed sweep (degrees, negative = clockwise) from the start.
            Some(sweep) => {
                let a = (start - center).y.atan2((start - center).x) + sweep.to_radians();
                center + radius * DVec2::from_angle(a)
            }
            None => {
                let dir = p.pos() - center;
                if dir == DVec2::ZERO {
                    return None;
                }
                center + dir.normalize() * radius
            }
        };
        let clockwise = match self.typed[0] {
            Some(sweep) => sweep < 0.0,
            None => self.swept < 0.0,
        };
        Some(if clockwise {
            (center, end, start)
        } else {
            (center, start, end)
        })
    }

    pub(super) fn create(&mut self, p: &Pointer, ed: &mut Editor) {
        let (Some(center), Some(start)) = (self.center, self.start) else {
            return;
        };
        let Some((_, s, e)) = self.geometry(p) else {
            return;
        };
        let clockwise = s != start.pos;
        let radius = start.pos.distance(center.pos);
        // Reuse a snapped point for the free end only if it lies exactly on the circle.
        let free = Anchor {
            pos: if clockwise { s } else { e },
            existing: p
                .snapped_point()
                .filter(|_| self.typed[0].is_none())
                .filter(|_| {
                    (p.pos().distance(center.pos) - radius).abs() <= 1e-9 * radius.max(1.0)
                }),
            midpoint_of: None,
            on_curve: None,
        };
        ed.doc.begin();
        let c = center.resolve(&mut ed.doc);
        let st = start.resolve(&mut ed.doc);
        let fr = free.resolve(&mut ed.doc);
        let (first, last) = if clockwise { (fr, st) } else { (st, fr) };
        match ed.doc.add_arc(c, first, last) {
            Ok(arc) => {
                if let Some(value) = self.radius_dimension
                    && ed.prefs.auto_relations
                    && let Ok(id) = ed
                        .doc
                        .add_constraint(Constraint::Radius { curve: arc, value })
                    && let Some(expr) = self.radius_expr.take()
                {
                    ed.doc.set_expression(id, Some(expr));
                }
                ed.commit("Arc");
            }
            Err(_) => {
                ed.doc.rollback();
                ed.sync();
            }
        }
        self.reset();
    }

    pub(super) fn set_start(&mut self, anchor: Anchor) {
        self.start = Some(anchor);
        self.swept = 0.0;
        self.last_angle = self.angle_of(anchor.pos);
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

impl Tool for ArcTool {
    fn is_busy(&self) -> bool {
        self.center.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Arc
    }

    fn hint(&self) -> String {
        match (self.center, self.start) {
            (None, _) => "Click the arc centre, or type X / Y".into(),
            (Some(_), None) => "Click the start point (sets the radius), or type the radius".into(),
            _ => "Click the end point, or type the sweep angle · move around the centre to choose \
                  the direction"
                .into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Hover(p) | Input::Drag(p) => {
                self.track(p.pos());
                return;
            }
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.start.is_none() => p,
            _ => return,
        };
        self.track(p.pos());
        match (self.center, self.start) {
            (None, _) => self.center = Some(p.anchor()),
            (Some(center), None) => {
                if p.pos() != center.pos {
                    self.set_start(p.anchor());
                }
            }
            _ => self.create(&p, ed),
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let had = self.center.is_some();
        self.reset();
        had
    }

    fn refine_pointer(&self, p: &mut Pointer) {
        // The end goes on the circle, along the pointer's direction from the centre. A point
        // snapped off the circle is marked with where the click lands, as an axis lock does
        // (the snap marker alone said the arc would end on it).
        let (Some(center), Some(start), None) = (self.center, self.start, self.typed[0]) else {
            return;
        };
        let Some(snap) = p.exact_snap() else {
            return;
        };
        let radius = start.pos.distance(center.pos);
        let d = snap.at - center.pos;
        if (d.length() - radius).abs() <= 1e-9 * radius.max(1.0) || d == DVec2::ZERO {
            return;
        }
        p.overridden = Some(snap);
        p.snap = None;
        p.guide = Some(Guide {
            at: center.pos + d.normalize() * radius,
            kind: GuideKind::Aligned,
            from: [Some(snap.at), None],
        });
    }

    fn fields(&self) -> &'static [&'static str] {
        match (self.center, self.start) {
            (None, _) => XY,
            (Some(_), None) => &["Radius"],
            _ => &["Sweep"],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        match (self.center, self.start) {
            (None, _) => self.center = Some(exact(p, typed_point(self.typed, p)).anchor()),
            (Some(_), None) => {
                let at = self.start_for(p);
                self.radius_dimension = self.typed[0].map(f64::abs).filter(|r| *r > 0.0);
                self.radius_expr = ed.typed_exprs[0]
                    .clone()
                    .filter(|_| self.typed[0].is_some_and(|r| r > 0.0));
                self.typed = [None; 2];
                self.set_start(Anchor::at(at));
            }
            _ => self.create(p, ed),
        }
        self.typed = [None; 2];
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let (Some(center), Some(p)) = (self.center, pointer) else {
            return Vec::new();
        };
        let mut items = vec![preview_vertex(center.pos)];
        if self.start.is_none() {
            items.push(preview(Shape::Line {
                a: center.pos,
                b: self.start_for(p),
            }));
            return items;
        }
        if let Some((c, s, e)) = self.geometry(p)
            && s != e
        {
            items.push(preview(arc_shape(c, s, e)));
            items.push(preview_vertex(s));
            items.push(preview_vertex(e));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let center = self.center?.pos;
        match self.start {
            None => Some(format!(
                "R {}",
                p.units.format_readout(self.start_for(p).distance(center))
            )),
            Some(start) => {
                let r = start.pos.distance(center);
                let (c, s, e) = self.geometry(p)?;
                let Shape::Arc { sweep, .. } = arc_shape(c, s, e) else {
                    return None;
                };
                Some(format!(
                    "R {}  {:.2}°",
                    p.units.format_readout(r),
                    sweep.to_degrees()
                ))
            }
        }
    }
}
