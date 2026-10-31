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
        "Click inside a closed area to fill or hatch it · Alt+click a fill to pick up its style"
            .into()
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
            Input::Click(p, mods) if mods.alt || ed.fill_eyedropper => {
                if let Err(e) = ed.pick_up_fill_style(p.raw) {
                    ed.out.notice = Some(e);
                }
            }
            Input::Click(p, _) => {
                let result = match ed.repick_fill.take() {
                    Some(id) => ed.repick_fill_at(id, p.raw),
                    None => ed.add_fill_at(p.raw).map(|_| ()),
                };
                if let Err(e) = result {
                    ed.out.notice = Some(e);
                }
            }
            _ => {}
        }
    }

    fn cancel(&mut self, ed: &mut Editor) -> bool {
        let eyedropper = std::mem::take(&mut ed.fill_eyedropper);
        ed.repick_fill.take().is_some() || eyedropper
    }

    fn overlay(&self) -> Option<Overlay> {
        self.hover.map(|at| Overlay::Region { at })
    }
}
