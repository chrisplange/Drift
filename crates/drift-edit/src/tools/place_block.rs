//! Place block (I): copies of the block picked in the Blocks tab follow the pointer; click to
//! place. R turns the next copy 90°, X mirrors it.

use super::*;

#[derive(Default)]
pub struct BlockTool {
    angle: f64,
    mirror: bool,
    hover: Option<DVec2>,
}

impl BlockTool {
    fn copy_at(&self, ed: &Editor, at: DVec2) -> Option<drift_core::Instance> {
        let block = ed.place_block?;
        ed.doc.block(block)?;
        let mut i = drift_core::Instance::new(block, at);
        i.angle = self.angle;
        i.mirror = self.mirror;
        Some(i)
    }
}

impl Tool for BlockTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Block
    }

    fn hint(&self) -> String {
        "Click to place the block (snaps by its base point) · R turns 90° · X mirrors · \
         pick another in the Blocks tab · Esc to finish"
            .into()
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Hover(p) | Input::Drag(p) => self.hover = Some(p.pos()),
            Input::Click(p, _) => match self.copy_at(ed, p.pos()) {
                Some(i) => ed.place_instance(i.block, i.at, i.angle, i.mirror),
                None => ed.notice = Some("Pick a block in the Blocks tab first".into()),
            },
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn key(&mut self, key: ToolKey, _pointer: Option<&Pointer>, _ed: &mut Editor) -> bool {
        match key {
            ToolKey::R => self.angle = (self.angle + 90.0).rem_euclid(360.0),
            ToolKey::X => self.mirror = !self.mirror,
            _ => return false,
        }
        true
    }

    fn editor_preview(&self, ed: &Editor) -> Vec<SketchItem> {
        let (Some(at), Some(copy)) = (self.hover, self.hover.and_then(|h| self.copy_at(ed, h)))
        else {
            return Vec::new();
        };
        let Some(block) = ed.doc.block(copy.block) else {
            return Vec::new();
        };
        let mut items: Vec<SketchItem> = block
            .items
            .iter()
            .filter(|item| !matches!(item.shape, Shape::Point(_)))
            .map(|item| preview(copy.apply_shape(&item.shape)))
            .collect();
        items.push(preview_vertex(at));
        items
    }
}
