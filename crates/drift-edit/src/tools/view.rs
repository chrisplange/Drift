//! A view of the model: drag (or click two corners of) its crop.

use drift_core::{Document, View};

use super::*;

/// `View n`, for the first `n` no view of `doc` is called.
pub(crate) fn next_view_name(doc: &Document) -> String {
    (1..)
        .map(|n| format!("View {n}"))
        .find(|name| doc.views().all(|(_, v)| v.name != *name))
        .unwrap_or_default()
}

/// The scale a new view gets: the last view's, or the drawing's scale.
pub(crate) fn new_view_scale(doc: &Document) -> f64 {
    doc.views()
        .last()
        .map_or_else(|| doc.draft_style().scale, |(_, v)| v.scale)
}

#[derive(Default)]
pub struct ViewTool {
    first: Option<DVec2>,
}

impl ViewTool {
    fn create(&mut self, second: DVec2, ed: &mut Editor) {
        let Some(first) = self.first.take() else {
            return;
        };
        let crop = Bounds {
            min: first.min(second),
            max: first.max(second),
        };
        if crop.size().min_element() <= 0.0 {
            return;
        }
        let view = View::new(next_view_name(&ed.doc), crop, new_view_scale(&ed.doc));
        ed.doc.begin();
        let id = ed.doc.add_view(view);
        ed.commit("View");
        ed.new_view = Some(id);
    }

    fn corners(&self, p: &Pointer) -> Option<[DVec2; 4]> {
        let (a, c) = (self.first?, p.pos());
        Some([a, DVec2::new(c.x, a.y), c, DVec2::new(a.x, c.y)])
    }
}

impl Tool for ViewTool {
    fn kind(&self) -> ToolKind {
        ToolKind::View
    }

    fn hint(&self) -> String {
        match self.first {
            None => "Drag the view's crop, or click its first corner".into(),
            Some(_) => "Click the opposite corner of the crop".into(),
        }
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let p = match input {
            Input::Click(p, _) | Input::DragEnd(p, _) => p,
            Input::DragStart(p, _) if self.first.is_none() => p,
            _ => return,
        };
        if self.first.is_none() {
            self.first = Some(p.pos());
        } else {
            self.create(p.pos(), ed);
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        self.first.take().is_some()
    }

    fn is_busy(&self) -> bool {
        self.first.is_some()
    }

    fn preview(&self, pointer: Option<&Pointer>) -> Vec<SketchItem> {
        let Some(c) = pointer.and_then(|p| self.corners(p)) else {
            return Vec::new();
        };
        (0..4)
            .map(|i| {
                preview(Shape::Line {
                    a: c[i],
                    b: c[(i + 1) % 4],
                })
                .with_kind(drift_core::LineKind::Construction)
            })
            .collect()
    }

    fn measurement(&self, p: &Pointer) -> Option<String> {
        let first = self.first?;
        let d = (p.pos() - first).abs();
        Some(format!(
            "{} × {}",
            p.units.format_readout(d.x),
            p.units.format_readout(d.y)
        ))
    }
}
