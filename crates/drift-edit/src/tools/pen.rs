//! The pen: paths of Bézier curves and straight lines, drawn as in Illustrator or Affinity.
//!
//! - Click places a corner point; press and drag places a smooth point, pulling out its
//!   handles (mirrored). Alt while dragging breaks the handle: only the outgoing one moves.
//! - The next segment follows the pointer (rubber band) until it's placed.
//! - Clicking (or dragging) on the first point closes the path; Enter, Esc or a right-click
//!   ends an open one.
//! - Alt-click on a point between two curves makes it smooth, or a corner again.
//! - Segments with no handles at either end are straight lines.

use super::*;

/// A point of the path being drawn.
#[derive(Clone, Copy, Debug)]
pub(super) struct PathPoint {
    pub at: Anchor,
    /// The point once it exists (the first one waits for the second).
    pub id: Option<EntityId>,
    /// Where its outgoing handle is, if it has one.
    pub out: Option<DVec2>,
    /// Its incoming handle position (kept for the first point, to close the path smoothly).
    pub in_pos: Option<DVec2>,
    /// The handle point of the curve arriving here, and of the one leaving.
    pub in_handle: Option<EntityId>,
    pub out_handle: Option<EntityId>,
    /// Its handles stay in line (a Smooth relation once both exist).
    pub smooth: bool,
}

/// A point being placed with the button held: dragging pulls its handles.
#[derive(Clone, Copy, Debug)]
pub(super) struct Press {
    pub at: Anchor,
    pub out: DVec2,
    /// The incoming handle once Alt has broken it from the outgoing one.
    pub frozen_in: Option<DVec2>,
    pub broken: bool,
}

impl Press {
    /// The incoming handle: mirrored from the outgoing one, unless broken.
    fn in_pos(&self) -> Option<DVec2> {
        if self.broken {
            self.frozen_in
        } else {
            Some(2.0 * self.at.pos - self.out)
        }
    }

    /// Too short a drag counts as a click.
    fn dragged(&self, tolerance: f64) -> bool {
        self.out.distance(self.at.pos) > tolerance * 0.5
    }
}

#[derive(Default)]
pub struct PenTool {
    pub(super) path: Vec<PathPoint>,
    /// The path's segments so far (to fill it when it closes).
    pub(super) curves: Vec<EntityId>,
    pub(super) press: Option<Press>,
    pub(super) pointer: Option<Pointer>,
}

/// Handles for a segment from `a` (outgoing handle `out`) to `b` (incoming `inn`): a missing
/// one sits a third of the way towards the other end's handle, as a retracted handle would.
fn segment_handles(
    a: DVec2,
    out: Option<DVec2>,
    b: DVec2,
    inn: Option<DVec2>,
) -> Option<(DVec2, DVec2)> {
    if out.is_none() && inn.is_none() {
        return None;
    }
    let c1 = out.unwrap_or_else(|| a + (inn.unwrap_or(b) - a) / 3.0);
    let c2 = inn.unwrap_or_else(|| b + (out.unwrap_or(a) - b) / 3.0);
    Some((c1, c2))
}

impl PenTool {
    fn reset(&mut self) {
        self.path.clear();
        self.press = None;
    }

    /// Whether `p` is on the path's first point (so placing there closes it).
    fn closes(&self, p: &Pointer) -> bool {
        let Some(first) = self.path.first() else {
            return false;
        };
        self.path.len() >= 2
            && (first.id.is_some_and(|id| p.snapped_point() == Some(id))
                || p.pos().distance(first.at.pos) <= p.tolerance)
    }

    /// Make the segment from the last point to `to` (with incoming handle `inn`), as one
    /// undo step. Returns the new point's id and incoming handle, or `None` if refused.
    fn segment(
        &mut self,
        ed: &mut Editor,
        to: Anchor,
        to_id: Option<EntityId>,
        inn: Option<DVec2>,
    ) -> Option<(EntityId, Option<EntityId>)> {
        let last = *self.path.last()?;
        if last.at.pos.distance(to.pos) <= 1e-9 * to.pos.abs().max_element().max(1.0) {
            return None;
        }
        ed.doc.begin();
        // The path's points are its own: one placed on another shape's point isn't joined
        // to it.
        let a = last.id.unwrap_or_else(|| last.at.own(&mut ed.doc));
        let b = to_id.unwrap_or_else(|| to.own(&mut ed.doc));
        let (made, c1, c2) = match segment_handles(last.at.pos, last.out, to.pos, inn) {
            None => (ed.doc.add_line(a, b), None, None),
            Some((h1, h2)) => {
                let c1 = ed.doc.add_point(h1);
                let c2 = ed.doc.add_point(h2);
                (ed.doc.add_bezier(a, c1, c2, b), Some(c1), Some(c2))
            }
        };
        let Ok(curve) = made else {
            ed.doc.rollback();
            ed.sync();
            return None;
        };
        // The point it leaves is smooth: its handles stay in line.
        if last.smooth
            && let (Some(h_in), Some(h_out)) = (last.in_handle, c1)
        {
            let _ = ed.doc.add_constraint(Constraint::Smooth {
                point: a,
                a: h_in,
                b: h_out,
            });
        }
        if !ed.commit(if c1.is_some() { "Curve" } else { "Line" }) {
            return None;
        }
        let n = self.path.len();
        self.path[n - 1].id = Some(a);
        self.path[n - 1].out_handle = c1;
        self.curves.push(curve);
        Some((b, c2))
    }

    /// Place a point: the first starts the path, later ones add a segment, and the first
    /// one again closes it.
    fn place(
        &mut self,
        ed: &mut Editor,
        at: Anchor,
        out: Option<DVec2>,
        inn: Option<DVec2>,
        smooth: bool,
        closing: bool,
    ) {
        if self.path.is_empty() {
            self.curves.clear();
            self.path.push(PathPoint {
                at,
                id: None,
                out,
                in_pos: inn,
                in_handle: None,
                out_handle: None,
                smooth,
            });
            return;
        }
        if closing {
            let first = self.path[0];
            // Closing onto a smooth first point comes in along its handle line.
            let inn = inn.or_else(|| {
                (first.smooth && first.out.is_some())
                    .then_some(first.in_pos)
                    .flatten()
            });
            if let Some((_, c2)) = self.segment(ed, first.at, first.id, inn) {
                // Closing smoothly, and filling the closed path, are one more step.
                ed.doc.begin();
                let mut changed = false;
                if (first.smooth || smooth)
                    && let (Some(h_in), Some(h_out), Some(id)) = (c2, first.out_handle, first.id)
                {
                    let _ = ed.doc.add_constraint(Constraint::Smooth {
                        point: id,
                        a: h_in,
                        b: h_out,
                    });
                    changed = true;
                }
                changed |= ed.add_shape(&self.curves).is_some();
                if changed {
                    ed.commit("Close path");
                } else {
                    ed.doc.rollback();
                    ed.sync();
                }
            }
            self.reset();
            return;
        }
        if let Some((id, in_handle)) = self.segment(ed, at, None, inn) {
            self.path.push(PathPoint {
                at: Anchor {
                    existing: Some(id),
                    ..Anchor::at(at.pos)
                },
                id: Some(id),
                out,
                in_pos: inn,
                in_handle,
                out_handle: None,
                smooth,
            });
        }
    }

    /// The segment being drawn, from the last point to the pointer (or the point being
    /// pressed), and its end.
    fn rubber_band(&self, p: &Pointer) -> Option<Shape> {
        let last = self.path.last()?;
        let (to, inn) = match &self.press {
            Some(press) => (press.at.pos, press.in_pos()),
            None if self.closes(p) => (self.path[0].at.pos, None),
            None => (p.pos(), None),
        };
        if to.distance(last.at.pos) <= 1e-9 {
            return None;
        }
        Some(match segment_handles(last.at.pos, last.out, to, inn) {
            None => Shape::Line {
                a: last.at.pos,
                b: to,
            },
            Some((c1, c2)) => Shape::Bezier {
                p: [last.at.pos, c1, c2, to],
            },
        })
    }
}

impl Tool for PenTool {
    fn is_busy(&self) -> bool {
        !self.path.is_empty() || self.press.is_some()
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Pen
    }

    fn hint(&self) -> String {
        if let Some(press) = &self.press {
            return if press.broken {
                "Drag the outgoing handle; release to place the point".into()
            } else {
                "Drag to shape the curve (Alt breaks the handle); release to place".into()
            };
        }
        match (
            self.path.len(),
            self.pointer.as_ref().is_some_and(|p| self.closes(p)),
        ) {
            (0, _) => "Pen: click for a corner, drag for a smooth point. Alt-click a point to \
                       make it smooth or a corner"
                .into(),
            (_, true) => "Click to close the path".into(),
            _ => "Click or drag the next point; Enter or Esc to finish".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => self.pointer = Some(p),
            Input::Click(p, _) => {
                self.pointer = Some(p);
                // Alt-click on a point between two curves: smooth ↔ corner.
                if p.alt
                    && self.path.is_empty()
                    && let Some(id) = ed.index.pick(p.raw, p.tolerance, |id| {
                        ed.doc.get(id).is_some_and(|e| e.kind.is_point())
                    })
                {
                    match ed.toggle_smooth(id) {
                        Ok(true) => ed.out.notice = Some("Smooth point".into()),
                        Ok(false) => ed.out.notice = Some("Corner point".into()),
                        Err(e) => ed.out.notice = Some(e),
                    }
                    return;
                }
                let closing = self.closes(&p);
                self.place(ed, p.anchor(), None, None, false, closing);
            }
            Input::DragStart(p, _) => {
                self.pointer = Some(p);
                let at = if self.closes(&p) {
                    let first = self.path[0].at;
                    Anchor {
                        pos: first.pos,
                        ..first
                    }
                } else {
                    p.anchor()
                };
                self.press = Some(Press {
                    at,
                    out: at.pos,
                    frozen_in: None,
                    broken: p.alt,
                });
            }
            Input::Drag(p) => {
                self.pointer = Some(p);
                if let Some(press) = &mut self.press {
                    if p.alt && !press.broken {
                        // Broken before any handle was pulled: no incoming handle at all.
                        press.frozen_in = (press.out.distance(press.at.pos) > 1e-9)
                            .then(|| press.in_pos())
                            .flatten();
                        press.broken = true;
                    }
                    press.out = p.raw;
                }
            }
            Input::DragEnd(p, _) => {
                self.pointer = Some(p);
                let Some(mut press) = self.press.take() else {
                    return;
                };
                press.out = p.raw;
                let closing =
                    self.path.len() >= 2 && press.at.pos.distance(self.path[0].at.pos) <= 1e-9;
                if press.dragged(p.tolerance) {
                    let inn = press.in_pos();
                    self.place(ed, press.at, Some(press.out), inn, !press.broken, closing);
                } else {
                    self.place(ed, press.at, None, None, false, closing);
                }
            }
            Input::Finish => self.reset(),
            Input::DoubleClick(_) => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        let busy = self.is_busy();
        self.reset();
        busy
    }

    fn editor_preview(&self, _ed: &Editor) -> Vec<SketchItem> {
        let Some(p) = self.pointer.as_ref() else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if let Some(shape) = self.rubber_band(p) {
            items.push(preview(shape));
        }
        // The last point's outgoing handle, and the handles being pulled.
        let mut handle = |from: DVec2, to: DVec2| {
            items.push(
                preview(Shape::Line { a: from, b: to })
                    .with_kind(drift_core::LineKind::Construction),
            );
            items.push(preview_vertex(to));
        };
        if let Some(out) = self.path.last().and_then(|last| last.out) {
            handle(self.path[self.path.len() - 1].at.pos, out);
        }
        if let Some(press) = &self.press
            && press.dragged(p.tolerance)
        {
            handle(press.at.pos, press.out);
            if let Some(inn) = press.in_pos() {
                handle(press.at.pos, inn);
            }
        }
        for point in &self.path {
            items.push(preview_vertex(point.at.pos));
        }
        // Closing: ring the first point.
        if self.press.is_none() && self.closes(p) {
            items.push(preview(Shape::Circle {
                center: self.path[0].at.pos,
                radius: p.tolerance * 0.8,
            }));
        }
        items
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let last = self.path.last()?;
        Some(p.units.format_readout(p.pos().distance(last.at.pos)))
    }
}
