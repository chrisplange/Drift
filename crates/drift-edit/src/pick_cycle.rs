//! Click again to pick the next: a plain Select click on the spot of the last one picks the
//! next of what lies under the pointer there, round and round (PAPERCUTS 2026-11-11). The
//! candidates are the app's annotation under the pointer, if any, then the Select tool's
//! (see `tools::picking::candidates_at`), the first of them what a single click picks.
//! Moving the pointer away, a click anywhere else (Shift and Ctrl clicks too), a drag, Esc
//! or another tool start again from the first.

use drift_core::DVec2;

use crate::editor::Editor;

/// How near a click must land to the last one to count as the same spot, as a share of the
/// pick radius (8 px on screen, so a few pixels).
pub const SAME_SPOT: f64 = 0.5;

/// Where the last plain Select click was and which of the candidates there it picked.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PickCycle {
    last: Option<Spot>,
    /// A click stepped the cycle this frame (see [`PickCycle::end_frame`]).
    fresh: bool,
    /// The step the app worked out this frame, past its annotation, for the Select tool to
    /// pick (see [`Editor::hand_pick_to_tool`]).
    handed: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Spot {
    at: DVec2,
    /// The pick radius then, in world units.
    reach: f64,
    /// Which candidate this click picked, and how many there were.
    step: usize,
    len: usize,
    /// How many of them were the app's annotations, ahead of the tool's.
    lead: usize,
    /// The click went on from the one before (it didn't start the cycle).
    advanced: bool,
}

impl PickCycle {
    /// Start again from the first.
    pub fn reset(&mut self) {
        self.last = None;
        self.handed = None;
    }

    /// The pointer is at `at`: away from the last click's spot, the cycle starts again.
    pub fn pointer_at(&mut self, at: DVec2) {
        if self
            .last
            .is_some_and(|s| at.distance(s.at) > SAME_SPOT * s.reach)
        {
            self.reset();
        }
    }

    /// The app's end of a frame, `clicked` if a click (or the start of a drag) came in: one
    /// that didn't step the cycle went somewhere else, so the next starts again.
    pub fn end_frame(&mut self, clicked: bool) {
        if clicked && !self.fresh {
            self.reset();
        }
        self.fresh = false;
        self.handed = None;
    }

    /// Which candidate the last click picked, of how many: for the status bar and the tests.
    pub fn position(&self) -> Option<(usize, usize)> {
        self.last.map(|s| (s.step, s.len))
    }

    /// The step the app handed to the Select tool this frame, past its annotations.
    pub(crate) fn take_handed(&mut self) -> Option<usize> {
        self.handed.take()
    }

    /// The candidate a double-click's first click picked, of the tool's, when the second
    /// click (this frame) went on to the next: the double-click acts on that one, as before.
    pub(crate) fn step_back(&mut self) -> Option<usize> {
        let s = self.last.as_mut().filter(|_| self.fresh)?;
        if !s.advanced {
            return None;
        }
        let prev = (s.step + s.len - 1) % s.len;
        let k = prev.checked_sub(s.lead)?;
        s.step = prev;
        Some(k)
    }
}

impl Editor {
    /// A plain Select click at `at` (pick radius `reach`) with `len` candidates under it, the
    /// first `lead` of them the app's annotations: which one it picks. The first, unless the
    /// last click was on the same spot with the same candidates: then the one after the last
    /// picked, round to the first again. With more than one, the status bar says which.
    pub fn cycle_pick(&mut self, at: DVec2, reach: f64, lead: usize, len: usize) -> usize {
        let same = self
            .pick_cycle
            .last
            .filter(|s| at.distance(s.at) <= SAME_SPOT * s.reach && s.len == len && s.lead == lead);
        let step = match same {
            Some(s) if len > 0 => (s.step + 1) % len,
            _ => 0,
        };
        self.pick_cycle.last = Some(Spot {
            // (The spot stays where the cycle began, so it can't creep away.)
            at: same.map_or(at, |s| s.at),
            reach,
            step,
            len,
            lead,
            advanced: same.is_some(),
        });
        self.pick_cycle.fresh = true;
        if len > 1 {
            self.out.inform(cycle_status(step, len));
        }
        step
    }

    /// The app's annotation under the pointer was passed over: the Select tool picks its
    /// candidate `k` this frame.
    pub fn hand_pick_to_tool(&mut self, k: usize) {
        self.pick_cycle.handed = Some(k);
    }
}

/// What the status bar says of a click with several things under it.
pub fn cycle_status(step: usize, len: usize) -> String {
    format!(
        "Picked {} of {len} here (click again for the next)",
        step + 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: DVec2 = DVec2::new(1.0, 2.0);

    #[test]
    fn a_click_that_did_not_step_the_cycle_starts_it_again() {
        let mut ed = Editor::default();
        assert_eq!(ed.cycle_pick(AT, 1.0, 0, 3), 0);
        ed.pick_cycle.end_frame(true);
        assert_eq!(ed.cycle_pick(AT, 1.0, 0, 3), 1);
        ed.pick_cycle.end_frame(true);
        // A frame with no click keeps it; a click that went elsewhere (a grip, a panel's
        // button on the canvas, a drag) doesn't.
        ed.pick_cycle.end_frame(false);
        assert_eq!(ed.cycle_pick(AT, 1.0, 0, 3), 2);
        ed.pick_cycle.end_frame(true);
        ed.pick_cycle.end_frame(true);
        assert_eq!(ed.cycle_pick(AT, 1.0, 0, 3), 0);
    }

    #[test]
    fn an_annotation_leads_and_the_tool_takes_the_steps_after_it() {
        let mut ed = Editor::default();
        // The dimension under the pointer, then two of the tool's.
        assert_eq!(ed.cycle_pick(AT, 1.0, 1, 3), 0);
        ed.pick_cycle.end_frame(true);
        let step = ed.cycle_pick(AT, 1.0, 1, 3);
        assert_eq!(step, 1);
        ed.hand_pick_to_tool(step - 1);
        assert_eq!(ed.pick_cycle.take_handed(), Some(0));
        assert_eq!(ed.pick_cycle.take_handed(), None);
        ed.pick_cycle.end_frame(true);
        // Handed but not taken (another handler had the click): gone with the frame.
        ed.hand_pick_to_tool(5);
        ed.pick_cycle.end_frame(false);
        assert_eq!(ed.pick_cycle.take_handed(), None);
        assert_eq!(ed.pick_cycle.position(), Some((1, 3)));
    }

    #[test]
    fn a_different_count_or_spot_starts_again() {
        let mut ed = Editor::default();
        ed.cycle_pick(AT, 1.0, 0, 3);
        assert_eq!(ed.cycle_pick(AT, 1.0, 0, 2), 0);
        assert_eq!(ed.cycle_pick(AT + DVec2::new(0.4, 0.0), 1.0, 0, 2), 1);
        assert_eq!(ed.cycle_pick(AT + DVec2::new(0.6, 0.0), 1.0, 0, 2), 0);
        ed.pick_cycle.pointer_at(AT + DVec2::new(3.0, 0.0));
        assert_eq!(ed.pick_cycle.position(), None);
    }
}
