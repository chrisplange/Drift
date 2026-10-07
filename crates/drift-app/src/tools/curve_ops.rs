//! Tools that act on the curve under the pointer: Trim (click or drag across pieces), Extend
//! (to the next curve) and Split (at a point).

use super::*;

/// The line, arc or circle under the pointer.
pub(super) fn curve_under(ed: &Editor, p: &Pointer) -> Option<EntityId> {
    ed.index.pick(p.raw, p.tolerance, |id| {
        ed.doc.get(id).is_some_and(|e| !e.kind.is_point()) && !ed.is_locked(id)
    })
}

fn report(ed: &mut Editor, what: &str, result: Result<(), crate::editor::EditError>) {
    if let Err(e) = result {
        ed.notice = Some(format!("{what}: {e}"));
    }
}

/// Click a piece of a curve to remove it up to the nearest curves that cut it; drag across
/// pieces to trim each one the pointer passes over (power trim).
#[derive(Default)]
pub struct TrimTool {
    hover: Option<(EntityId, DVec2)>,
    dragging: bool,
}

impl Tool for TrimTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Trim
    }

    fn hint(&self) -> String {
        "Click a piece to remove it · drag across pieces to trim several".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                // The removal preview is the highlight; the whole curve isn't hovered.
                self.hover = curve_under(ed, &p).map(|id| (id, p.raw));
            }
            Input::Click(p, _) => {
                if let Some(id) = curve_under(ed, &p) {
                    let r = ed.trim_at(id, p.raw);
                    report(ed, "Trim", r);
                }
                self.hover = None;
            }
            Input::DragStart(p, _) | Input::Drag(p) => {
                self.dragging = true;
                if let Some(id) = curve_under(ed, &p) {
                    // Each piece crossed is its own undo step; failures are skipped quietly.
                    let _ = ed.trim_at(id, p.raw);
                }
                self.hover = None;
            }
            Input::DragEnd(..) => self.dragging = false,
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn is_busy(&self) -> bool {
        self.dragging
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        Vec::new()
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        self.hover
            .and_then(|(id, at)| ed.trim_preview(id, at))
            .map(|shape| vec![SketchItem::new(shape).with_emphasis(Emphasis::Remove)])
            .unwrap_or_default()
    }
}

/// Click near the end of a line or arc to extend it to the next curve in its way.
#[derive(Default)]
pub struct ExtendTool {
    preview: Option<Shape>,
}

impl ExtendTool {
    fn target(ed: &Editor, p: &Pointer) -> Option<Shape> {
        let id = curve_under(ed, p)?;
        let (end, at, ..) = ed.extend_target(id, p.raw).ok()?;
        let from = ed.doc.point(end).ok()?;
        Some(match ed.doc.shape(id)? {
            Shape::Arc { center, .. } => {
                // The extension follows the circle from the end.
                let (s, e) =
                    if ed.doc.get(id).is_some_and(
                        |x| matches!(x.kind, EntityKind::Arc { end: e, .. } if e == end),
                    ) {
                        (from, at)
                    } else {
                        (at, from)
                    };
                arc_shape(center, s, e)
            }
            _ => Shape::Line { a: from, b: at },
        })
    }
}

impl Tool for ExtendTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Extend
    }

    fn hint(&self) -> String {
        "Click near the end of a line or arc to extend it to the next curve".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) => {
                self.preview = Self::target(ed, &p);
                ed.hovered = curve_under(ed, &p);
            }
            Input::Click(p, _) => {
                if let Some(id) = curve_under(ed, &p) {
                    let r = ed.extend_at(id, p.raw);
                    report(ed, "Extend", r);
                }
                self.preview = None;
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        self.preview.map(preview).into_iter().collect()
    }
}

/// Click a line or arc to split it in two at that point.
#[derive(Default)]
pub struct SplitTool {
    at: Option<DVec2>,
}

impl Tool for SplitTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Split
    }

    fn hint(&self) -> String {
        "Click a line or arc where it should be split".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let place = |p: &Pointer, ed: &Editor| {
            let id = curve_under(ed, p)?;
            let shape = ed.doc.shape(id)?;
            let t = crate::modify::param_of(&shape, p.pos());
            Some((id, crate::modify::point_at(&shape, t)))
        };
        match input {
            Input::Hover(p) => {
                self.at = place(&p, ed).map(|(_, at)| at);
                ed.hovered = curve_under(ed, &p);
            }
            Input::Click(p, _) => {
                if let Some((id, at)) = place(&p, ed) {
                    let r = ed.split_at(id, at).map(|_| ());
                    report(ed, "Split", r);
                }
            }
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn preview(&self, _pointer: Option<&Pointer>) -> Vec<SketchItem> {
        self.at.map(preview_vertex).into_iter().collect()
    }
}
