//! Slots: straight (the two end centres), centre (the middle, then an end centre) and arc
//! (the arc's centre, then the two end centres), then the width. Built from lines and arcs
//! with tangent relations, so they stay slots when dimensioned or dragged.

use super::*;
use drift_core::LineKind;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SlotMode {
    #[default]
    Straight,
    Centre,
    Arc,
}

impl SlotMode {
    pub fn next(self) -> Self {
        match self {
            SlotMode::Straight => SlotMode::Centre,
            SlotMode::Centre => SlotMode::Arc,
            SlotMode::Arc => SlotMode::Straight,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SlotMode::Straight => "Straight slot",
            SlotMode::Centre => "Centre slot",
            SlotMode::Arc => "Arc slot",
        }
    }

    /// Points to click before the width.
    fn points(self) -> usize {
        match self {
            SlotMode::Straight | SlotMode::Centre => 2,
            SlotMode::Arc => 3,
        }
    }
}

#[derive(Default)]
pub struct SlotTool {
    pub(super) points: Vec<Anchor>,
    pub(super) typed: [Option<f64>; 2],
    /// A length typed for the second centre (dimensioned when the slot is made).
    pub(super) typed_length: Option<f64>,
    pub(super) pointer: Option<Pointer>,
}

/// The curves of a slot about to be made.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SlotShape {
    Straight {
        /// End centres, and (for a centre slot) the middle.
        ends: [DVec2; 2],
        middle: Option<DVec2>,
        half: f64,
    },
    Arc {
        center: DVec2,
        /// Start and end centres, counter-clockwise.
        ends: [DVec2; 2],
        half: f64,
    },
}

impl SlotShape {
    /// Its outline as shapes (for the preview).
    fn shapes(&self) -> Vec<Shape> {
        match *self {
            SlotShape::Straight {
                ends: [c1, c2],
                half,
                ..
            } => {
                let d = (c2 - c1).normalize_or_zero();
                let n = d.perp();
                let start = n.to_angle();
                vec![
                    Shape::Line {
                        a: c1 + n * half,
                        b: c2 + n * half,
                    },
                    Shape::Line {
                        a: c2 - n * half,
                        b: c1 - n * half,
                    },
                    Shape::Arc {
                        center: c2,
                        radius: half,
                        start: start + PI,
                        sweep: PI,
                    },
                    Shape::Arc {
                        center: c1,
                        radius: half,
                        start,
                        sweep: PI,
                    },
                ]
            }
            SlotShape::Arc {
                center,
                ends: [s, e],
                half,
            } => {
                let r = s.distance(center);
                let (a0, a1) = ((s - center).to_angle(), (e - center).to_angle());
                let sweep = (a1 - a0).rem_euclid(TAU);
                vec![
                    Shape::Arc {
                        center,
                        radius: r + half,
                        start: a0,
                        sweep,
                    },
                    Shape::Arc {
                        center,
                        radius: r - half,
                        start: a0,
                        sweep,
                    },
                    Shape::Arc {
                        center: e,
                        radius: half,
                        start: a1,
                        sweep: PI,
                    },
                    Shape::Arc {
                        center: s,
                        radius: half,
                        start: a0 + PI,
                        sweep: PI,
                    },
                ]
            }
        }
    }
}

impl SlotTool {
    fn mode(&self, ed: &Editor) -> SlotMode {
        ed.slot_mode
    }

    /// The end centres (and middle) from the clicked points, with the pointer standing in for
    /// the next one.
    fn centres(&self, mode: SlotMode, next: DVec2) -> Option<SlotShape> {
        let pts: Vec<DVec2> = self
            .points
            .iter()
            .map(|a| a.pos)
            .chain(std::iter::once(next))
            .collect();
        match mode {
            SlotMode::Straight if pts.len() >= 2 => Some(SlotShape::Straight {
                ends: [pts[0], pts[1]],
                middle: None,
                half: 0.0,
            }),
            SlotMode::Centre if pts.len() >= 2 => Some(SlotShape::Straight {
                ends: [2.0 * pts[0] - pts[1], pts[1]],
                middle: Some(pts[0]),
                half: 0.0,
            }),
            SlotMode::Arc if pts.len() >= 3 => {
                let (o, s) = (pts[0], pts[1]);
                let r = s.distance(o);
                if r <= 1e-9 {
                    return None;
                }
                let e = o + (pts[2] - o).normalize_or_zero() * r;
                // Arcs run counter-clockwise: going round the other way swaps the ends.
                let ends = if (s - o).perp_dot(e - o) < 0.0 {
                    [e, s]
                } else {
                    [s, e]
                };
                Some(SlotShape::Arc {
                    center: o,
                    ends,
                    half: 0.0,
                })
            }
            _ => None,
        }
    }

    /// The slot for the pointer at `p` once all its centres are placed.
    fn shape(&self, p: &Pointer, ed: &Editor) -> Option<SlotShape> {
        let mode = self.mode(ed);
        if self.points.len() < mode.points() {
            return None;
        }
        let last = self.points.last()?.pos;
        let mut shape = self.centres(mode, last)?;
        // The pointer's distance from the slot's centre line sets the width.
        let half = match self.typed[0] {
            Some(w) => w.abs() / 2.0,
            None => match &shape {
                SlotShape::Straight { ends: [c1, c2], .. } => {
                    let d = (*c2 - *c1).normalize_or_zero();
                    (p.pos() - *c1).dot(d.perp()).abs()
                }
                SlotShape::Arc {
                    center,
                    ends: [s, _],
                    ..
                } => (p.pos().distance(*center) - s.distance(*center)).abs(),
            },
        };
        match &mut shape {
            SlotShape::Straight { half: h, ends, .. } => {
                if ends[0].distance(ends[1]) <= 1e-9 {
                    return None;
                }
                *h = half;
            }
            SlotShape::Arc {
                center,
                ends: [s, e],
                half: h,
            } => {
                if half >= s.distance(*center) || s.distance(*e) <= 1e-9 {
                    return None;
                }
                *h = half;
            }
        }
        (half > 1e-9).then_some(shape)
    }

    fn create(&mut self, shape: SlotShape, ed: &mut Editor) {
        let width_typed = self.typed[0].is_some();
        let length = self.typed_length;
        ed.doc.begin();
        let anchors = std::mem::take(&mut self.points);
        let made = (|| -> Result<(), drift_core::DocumentError> {
            match shape {
                SlotShape::Straight {
                    ends: [c1, c2],
                    middle,
                    half,
                } => {
                    let d = (c2 - c1).normalize();
                    let n = d.perp();
                    // Clicked points are the slot's own (snapping to another shape's point
                    // places them there without joining).
                    let (e1, e2, m) = match middle {
                        Some(_) => {
                            let m = anchors[0].own(&mut ed.doc);
                            let e2 = anchors[1].own(&mut ed.doc);
                            (ed.doc.add_point(c1), e2, Some(m))
                        }
                        None => (
                            anchors[0].own(&mut ed.doc),
                            anchors[1].own(&mut ed.doc),
                            None,
                        ),
                    };
                    let p1 = ed.doc.add_point(c1 + n * half);
                    let p2 = ed.doc.add_point(c2 + n * half);
                    let p3 = ed.doc.add_point(c2 - n * half);
                    let p4 = ed.doc.add_point(c1 - n * half);
                    let top = ed.doc.add_line(p1, p2)?;
                    let bottom = ed.doc.add_line(p3, p4)?;
                    let end2 = ed.doc.add_arc(e2, p3, p2)?;
                    let end1 = ed.doc.add_arc(e1, p1, p4)?;
                    let axis = ed.doc.add_line(e1, e2)?;
                    ed.doc.set_style(axis, LineKind::Construction);
                    ed.fill_new_shape(&[top, end2, bottom, end1]);
                    for (line, arc) in [(top, end1), (top, end2), (bottom, end1), (bottom, end2)] {
                        ed.relate(Constraint::Tangent { a: line, b: arc });
                    }
                    ed.relate(Constraint::Equal { a: end1, b: end2 });
                    if let Some(m) = m {
                        ed.relate(Constraint::Midpoint {
                            point: m,
                            line: axis,
                        });
                    }
                    let scale = c1.distance(c2).max(1.0) * 1e-9;
                    if (c1.y - c2.y).abs() < scale {
                        auto_relation(ed, Some(GuideKind::Horizontal), axis);
                    } else if (c1.x - c2.x).abs() < scale {
                        auto_relation(ed, Some(GuideKind::Vertical), axis);
                    }
                    if let Some(len) = length {
                        ed.add_typed_dimension(
                            0,
                            Constraint::Distance {
                                a: e1,
                                b: e2,
                                value: len,
                            },
                        );
                    }
                    if width_typed {
                        ed.add_typed_dimension(
                            0,
                            Constraint::PointLineDistance {
                                point: p4,
                                line: top,
                                value: 2.0 * half,
                            },
                        );
                    }
                }
                SlotShape::Arc {
                    center,
                    ends: [s, e],
                    half,
                } => {
                    let us = (s - center).normalize();
                    let ue = (e - center).normalize();
                    let o = anchors[0].own(&mut ed.doc);
                    // The clicked start centre is one of the ends (which, depends on the
                    // direction it was drawn in).
                    let (cs, ce) = if anchors[1].pos == s {
                        (anchors[1].own(&mut ed.doc), ed.doc.add_point(e))
                    } else {
                        (ed.doc.add_point(s), anchors[1].own(&mut ed.doc))
                    };
                    let s_out = ed.doc.add_point(s + us * half);
                    let s_in = ed.doc.add_point(s - us * half);
                    let e_out = ed.doc.add_point(e + ue * half);
                    let e_in = ed.doc.add_point(e - ue * half);
                    let outer = ed.doc.add_arc(o, s_out, e_out)?;
                    let inner = ed.doc.add_arc(o, s_in, e_in)?;
                    let cap_e = ed.doc.add_arc(ce, e_out, e_in)?;
                    let cap_s = ed.doc.add_arc(cs, s_in, s_out)?;
                    ed.fill_new_shape(&[outer, cap_e, inner, cap_s]);
                    for (cap, arc) in [
                        (cap_e, outer),
                        (cap_e, inner),
                        (cap_s, outer),
                        (cap_s, inner),
                    ] {
                        ed.relate(Constraint::Tangent { a: cap, b: arc });
                    }
                    if width_typed {
                        ed.add_typed_dimension(
                            0,
                            Constraint::Diameter {
                                curve: cap_s,
                                value: 2.0 * half,
                            },
                        );
                    }
                }
            }
            Ok(())
        })();
        match made {
            Ok(()) => {
                ed.commit("Slot");
            }
            Err(e) => {
                ed.doc.rollback();
                ed.sync();
                ed.notice = Some(format!("Slot: {e}"));
            }
        }
        self.typed_length = None;
    }
}

impl Tool for SlotTool {
    fn is_busy(&self) -> bool {
        !self.points.is_empty()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Slot
    }

    fn hint(&self) -> String {
        // The mode lives in the editor; the hint names the steps of all three.
        match self.points.len() {
            0 => "Slot (U again: straight / centre / arc): click the first centre (centre slot: \
                  the middle; arc slot: the arc's centre)"
                .into(),
            1 => "Click the end centre, or type the length".into(),
            _ => "Click to set the width (arc slot: the end centre first), or type it".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if let Input::Hover(p) | Input::Drag(p) = input {
            self.pointer = Some(p);
        }
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.points.is_empty() => p,
            _ => return,
        };
        let mode = self.mode(ed);
        if self.points.len() < mode.points() {
            let at = p.anchor();
            if self
                .points
                .last()
                .is_some_and(|q| q.pos.distance(at.pos) <= 1e-9)
            {
                return;
            }
            self.points.push(at);
            return;
        }
        if let Some(shape) = self.shape(&p, ed) {
            self.create(shape, ed);
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.typed_length = None;
        !std::mem::take(&mut self.points).is_empty()
    }

    fn fields(&self) -> &'static [&'static str] {
        match self.points.len() {
            0 => XY,
            1 => &["Length"],
            _ => &["Width"],
        }
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let mode = self.mode(ed);
        match self.points.len() {
            0 => self
                .points
                .push(exact(p, typed_point(self.typed, p)).anchor()),
            1 => {
                if let Some(len) = self.typed[0].map(f64::abs).filter(|l| *l > 0.0) {
                    let first = self.points[0].pos;
                    let dir = (p.pos() - first).normalize_or(DVec2::X);
                    // A centre slot's length is end to end, so its end is half of it away.
                    let reach = if mode == SlotMode::Centre {
                        len / 2.0
                    } else {
                        len
                    };
                    if mode != SlotMode::Arc {
                        self.typed_length = Some(len);
                    }
                    self.points.push(exact(p, first + dir * reach).anchor());
                }
            }
            _ => {
                if self.points.len() >= mode.points()
                    && let Some(shape) = self.shape(p, ed)
                {
                    self.create(shape, ed);
                }
            }
        }
        self.typed = [None; 2];
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let Some(p) = self.pointer.as_ref() else {
            return Vec::new();
        };
        let mode = self.mode(ed);
        let mut items: Vec<SketchItem> =
            self.points.iter().map(|a| preview_vertex(a.pos)).collect();
        if let Some(shape) = self.shape(p, ed) {
            items.extend(shape.shapes().into_iter().map(preview));
        } else if !self.points.is_empty()
            && let Some(centres) = self.centres(mode, p.pos())
        {
            // Still placing centres: the centre line (or arc) so far.
            match centres {
                SlotShape::Straight { ends: [a, b], .. } => {
                    items.push(preview(Shape::Line { a, b }).with_kind(LineKind::Construction));
                }
                SlotShape::Arc {
                    center,
                    ends: [s, e],
                    ..
                } => {
                    let (a0, a1) = ((s - center).to_angle(), (e - center).to_angle());
                    items.push(
                        preview(Shape::Arc {
                            center,
                            radius: s.distance(center),
                            start: a0,
                            sweep: (a1 - a0).rem_euclid(TAU),
                        })
                        .with_kind(LineKind::Construction),
                    );
                }
            }
        } else if mode == SlotMode::Arc && self.points.len() == 1 {
            let o = self.points[0].pos;
            items.push(preview(Shape::Line { a: o, b: p.pos() }).with_kind(LineKind::Construction));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let first = self.points.first()?.pos;
        Some(if self.points.len() == 1 {
            p.units.format_readout(p.pos().distance(first))
        } else {
            format!("Width {}", p.units.format_readout(2.0 * self.width_hint(p)))
        })
    }
}

impl SlotTool {
    /// Half the width the pointer sets, for the readout (straight slots measure from their
    /// centre line, arc slots from their centre arc).
    fn width_hint(&self, p: &Pointer) -> f64 {
        if let Some(w) = self.typed[0] {
            return w.abs() / 2.0;
        }
        match self.points.as_slice() {
            [a, b] => {
                let d = (b.pos - a.pos).normalize_or_zero();
                (p.pos() - a.pos).dot(d.perp()).abs()
            }
            [o, s, ..] => (p.pos().distance(o.pos) - s.pos.distance(o.pos)).abs(),
            _ => 0.0,
        }
    }
}
