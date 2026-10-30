//! What the perf probes and the tests in other crates read of the editor's insides (drift-app's
//! whole-app probes in `perf_big`, its wall gallery). Test-only: compiled for drift-edit's
//! tests and, through the `test-support` feature, for drift-app's. Read-only, except
//! [`Editor::forget_fill_shapes`].

use super::*;

impl Editor {
    /// Whether the fills of an edit still wait for their areas (found on a thread).
    pub fn areas_pending(&self) -> bool {
        self.areas.pending.is_some()
    }

    /// How many solves the last drag step took.
    pub fn drag_solves(&self) -> u32 {
        self.drag_solves
    }

    /// The entities a drag of `ids` can move (`drag_scope`).
    pub fn scope_of_drag(
        &self,
        ids: &[EntityId],
    ) -> std::rc::Rc<std::collections::HashSet<EntityId>> {
        self.drag_scope(ids)
    }

    /// The crossed fills' areas as remembered: how many, and their size in bytes.
    pub fn crossed_fill_memo(&self) -> (usize, usize) {
        let memo = self.uncrossed_memo.borrow();
        (memo.len(), memo.values().map(|m| m.bytes()).sum())
    }

    /// Forget the crossed fills' remembered areas and the fill shapes, so the next
    /// [`Editor::fill_shapes`] finds them all afresh. The one probe that changes the editor:
    /// it exists to check the remembered areas against a fresh look, and for nothing else.
    pub fn forget_fill_shapes(&mut self) {
        self.uncrossed_memo.borrow_mut().clear();
        self.fill_shapes = None;
    }

    /// Why a relation on walls' generated geometry can't hold it
    /// (`refused_on_walls`).
    pub fn refusal_on_walls(&self, c: &Constraint) -> Option<String> {
        self.refused_on_walls(c)
    }
}
