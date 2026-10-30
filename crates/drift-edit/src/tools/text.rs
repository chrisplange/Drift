//! Text: click where a line of text starts (on its baseline) and type; it shows live on
//! the canvas. Return or a click elsewhere finishes it. A click on a text types into it.

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
        "Text: click where it starts (its baseline) or in a text, then type · Enter finishes · Shift+Enter: new line".into()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        let Input::Click(p, _) = input else {
            return;
        };
        let height = if ed.prefs.text_height > 0.0 {
            ed.prefs.text_height
        } else {
            10.0
        };
        ed.selection.clear();
        // On a text: type into it, from where it was clicked.
        if let Some(t) = ed.text_at(p.raw, p.tolerance) {
            ed.selected_texts = [t].into();
            ed.editing_text = Some(t);
            ed.text_caret_at = Some(p.raw);
            return;
        }
        ed.add_text(drift_core::SketchText::new("", p.pos(), height));
        ed.editing_text = ed.sole_text();
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }
}
