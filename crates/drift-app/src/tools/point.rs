//! Point.

use super::*;

#[derive(Default)]
pub struct PointTool {
    pub(super) typed: [Option<f64>; 2],
}

impl Tool for PointTool {
    fn is_busy(&self) -> bool {
        false
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Point
    }

    fn hint(&self) -> String {
        "Click to place a point, or type X / Y".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if let Input::Click(p, _) = input
            && p.snapped_point().is_none()
        {
            ed.doc.begin();
            ed.doc.add_point(p.pos());
            ed.commit("Point");
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn fields(&self) -> &'static [&'static str] {
        XY
    }

    fn set_typed(&mut self, values: [Option<f64>; 2]) {
        self.typed = values;
    }

    fn commit_typed(&mut self, p: &Pointer, ed: &mut Editor) {
        let at = typed_point(self.typed, p);
        ed.doc.begin();
        ed.doc.add_point(at);
        ed.commit("Point");
        self.typed = [None; 2];
    }
}
