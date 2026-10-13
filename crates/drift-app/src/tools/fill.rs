//! Fill / hatch: click inside a closed area to fill it with the current fill style.

use super::*;

#[derive(Default)]
pub struct FillTool {
    hover: Option<DVec2>,
}

impl Tool for FillTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Fill
    }

    fn hint(&self) -> String {
        "Click inside a closed area to fill or hatch it; set the style in the inspector".into()
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn wants_snap(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => self.hover = Some(p.raw),
            Input::Click(p, _) => {
                let result = match ed.repick_fill.take() {
                    Some(id) => ed.repick_fill_at(id, p.raw),
                    None => ed.add_fill_at(p.raw).map(|_| ()),
                };
                if let Err(e) = result {
                    ed.notice = Some(e);
                }
            }
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        ed.repick_fill.take().is_some()
    }

    fn overlay(&self) -> Option<Overlay> {
        self.hover.map(|at| Overlay::Region { at })
    }
}
