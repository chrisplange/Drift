//! Base point: click a snap point to be a block's base point (Set base point), or the base
//! point of a block being made (Make block). Enter takes the block's centre. One pick, then
//! back to the select tool.

use super::*;
use crate::blocks::BasePointPick;

#[derive(Default)]
pub struct BasePointTool;

impl BasePointTool {
    /// Use `at` (or, with `None`, the centre) for what the pick is for.
    fn pick(ed: &mut Editor, at: Option<DVec2>) {
        ed.out.base_point_done = true;
        let Some(pick) = ed.base_point_pick.take() else {
            return;
        };
        let done = match pick {
            BasePointPick::Make(name) => ed.make_block_at(&name, at).map(|_| ()),
            BasePointPick::Block(copy) => match at.or_else(|| ed.block_centre(copy)) {
                Some(at) => ed.set_base_point(copy, at),
                None => Err("That block has nothing in it".into()),
            },
        };
        if let Err(e) = done {
            ed.out.notice = Some(e);
        }
    }
}

impl Tool for BasePointTool {
    fn kind(&self) -> ToolKind {
        ToolKind::BasePoint
    }

    fn hint(&self) -> String {
        "Click a snap point for the base point (an end, corner, middle or centre) · \
         Enter: the centre · Esc cancels"
            .into()
    }

    fn is_busy(&self) -> bool {
        false
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        match input {
            Input::Click(p, _) => Self::pick(ed, Some(p.pos())),
            Input::Finish => Self::pick(ed, None),
            _ => {}
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }
}
