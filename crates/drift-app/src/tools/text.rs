//! Text: click where a line of text starts (on its baseline); type it in the inspector,
//! where it shows live on the canvas.

use super::*;

#[derive(Default)]
pub struct TextTool;

impl Tool for TextTool {
    fn is_busy(&self) -> bool {
        false
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Text
    }

    fn hint(&self) -> String {
        "Text: click where it starts (its baseline), then type it in the inspector".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let Input::Click(p, _) = input else {
            return;
        };
        let height = if ed.text_height > 0.0 {
            ed.text_height
        } else {
            10.0
        };
        ed.selection.clear();
        ed.add_text(drift_core::SketchText::new("", p.pos(), height));
        ed.editing_text = ed.selected_text;
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }
}
