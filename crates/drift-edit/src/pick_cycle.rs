//! Click again to pick the next: a plain Select click on the spot of the last one picks the
//! next of what lies under the pointer there, round and round (PAPERCUTS 2026-11-11). The
//! candidates are the app's annotation under the pointer, if any, then the Select tool's
//! (see `tools::picking::candidates_at`), the first of them what a single click picks. A
//! first click picks as it always has: the candidates are only gathered when a click lands
//! on the spot of the last one. Moving the pointer away, a click anywhere else (Shift and
//! Ctrl clicks too), a drag, a double-click, Esc or another tool start again from the first.

use drift_core::DVec2;

use crate::tools::Pick;

/// How near a click must land to the last one to count as the same spot, as a share of the
/// pick radius (8 px on screen, so a few pixels).
pub const SAME_SPOT: f64 = 0.5;

/// Where the last plain Select click was and which of the candidates there it picked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickCycle {
    last: Option<Spot>,
    /// A click stepped the cycle this frame (see [`PickCycle::end_frame`]).
    fresh: bool,
    /// What the app's click on an annotation went on to, for the Select tool to pick this
    /// frame (see `tools::click_past_annotation`).
    handed: Option<Option<Pick>>,
    /// The tool's candidates at the spot, gathered for the hover after a click (each click
    /// gathers its own afresh).
    cached: Option<Vec<Pick>>,
    /// The hover shows one of the tool's candidates where the app's annotation is under the
    /// pointer: the next click goes past the annotation.
    hover_past: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spot {
    pub(crate) at: DVec2,
    /// The pick radius then, in world units.
    pub(crate) reach: f64,
    /// Which candidate this click picked, and how many there were (not counted on a first
    /// click, which picks the first).
    pub(crate) step: usize,
    pub(crate) len: Option<usize>,
    /// How many of them were the app's annotations, ahead of the tool's.
    pub(crate) lead: usize,
    /// The click went on from the one before (it didn't start the cycle).
    pub(crate) advanced: bool,
}

impl PickCycle {
    /// Start again from the first.
    pub fn reset(&mut self) {
        self.last = None;
        self.handed = None;
        self.cached = None;
        self.hover_past = false;
    }

    /// Whether the next click on the annotation under the pointer goes past it, to the
    /// geometry the hover shows: the app doesn't highlight the annotation then.
    pub fn hover_passes_annotation(&self) -> bool {
        self.hover_past
    }

    /// The pointer is at `at`: away from the last click's spot (out of the pick radius, so
    /// not on the way to a drag from it), the cycle starts again.
    pub fn pointer_at(&mut self, at: DVec2) {
        if self.last.is_some_and(|s| at.distance(s.at) > s.reach) {
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

    /// Which candidate the last click picked, and of how many once counted: for the tests.
    pub fn position(&self) -> Option<(usize, Option<usize>)> {
        self.last.map(|s| (s.step, s.len))
    }

    /// The last click, if `at` is on its spot and it had `lead` annotations ahead.
    pub(crate) fn same_spot(&self, at: DVec2, lead: usize) -> Option<Spot> {
        self.last
            .filter(|s| at.distance(s.at) <= SAME_SPOT * s.reach && s.lead == lead)
    }

    pub(crate) fn record(&mut self, spot: Spot) {
        self.last = Some(spot);
        self.fresh = true;
        self.cached = None;
    }

    /// The last click, if `at` is on its spot (whatever was ahead of the tool's).
    pub(crate) fn on_spot(&self, at: DVec2) -> Option<Spot> {
        self.last
            .filter(|s| at.distance(s.at) <= SAME_SPOT * s.reach)
    }

    pub(crate) fn cached(&self) -> Option<&Vec<Pick>> {
        self.cached.as_ref()
    }

    pub(crate) fn cache(&mut self, picks: Vec<Pick>) {
        self.cached = Some(picks);
    }

    pub(crate) fn set_hover_past(&mut self, past: bool) {
        self.hover_past = past;
    }

    pub(crate) fn hand(&mut self, pick: Option<Pick>) {
        self.handed = Some(pick);
    }

    /// What the app's click on an annotation went on to this frame.
    pub(crate) fn take_handed(&mut self) -> Option<Option<Pick>> {
        self.handed.take()
    }

    /// Which of the tool's candidates a click that went on picked, if `at` is on its spot:
    /// a drag from there takes that one.
    pub(crate) fn cycled_at(&self, at: DVec2) -> Option<usize> {
        let s = self
            .last
            .filter(|s| s.advanced && at.distance(s.at) <= SAME_SPOT * s.reach)?;
        s.step.checked_sub(s.lead)
    }

    /// The candidate a double-click's first click picked, of the tool's, when the second
    /// click (this frame) went on to the next: the double-click acts on that one, as before.
    pub(crate) fn step_back(&mut self) -> Option<usize> {
        let s = self.last.as_mut().filter(|_| self.fresh)?;
        let len = s.len.filter(|_| s.advanced)?;
        let prev = (s.step + len - 1) % len;
        let k = prev.checked_sub(s.lead)?;
        s.step = prev;
        Some(k)
    }
}

/// What the status bar says of a click again with several things under it.
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

    fn spot(at: DVec2, step: usize, lead: usize) -> Spot {
        Spot {
            at,
            reach: 1.0,
            step,
            len: Some(3),
            lead,
            advanced: step > 0,
        }
    }

    #[test]
    fn a_click_that_did_not_step_the_cycle_starts_it_again() {
        let mut c = PickCycle::default();
        c.record(spot(AT, 1, 0));
        c.end_frame(true);
        // A frame with no click keeps it.
        c.end_frame(false);
        assert!(c.same_spot(AT, 0).is_some());
        // A click that went elsewhere (a grip, a drag) doesn't.
        c.end_frame(true);
        assert_eq!(c.position(), None);
    }

    #[test]
    fn what_was_handed_goes_with_the_frame() {
        let mut c = PickCycle::default();
        c.hand(None);
        c.end_frame(false);
        assert_eq!(c.take_handed(), None);
    }

    #[test]
    fn the_spot_is_a_few_pixels_and_the_same_annotations() {
        let mut c = PickCycle::default();
        c.record(spot(AT, 1, 1));
        assert!(c.same_spot(AT + DVec2::new(0.4, 0.0), 1).is_some());
        assert!(c.same_spot(AT + DVec2::new(0.6, 0.0), 1).is_none());
        assert!(c.same_spot(AT, 0).is_none());
        assert_eq!(c.cycled_at(AT), Some(0));
        // Still on the way to a drag from it; then away.
        c.pointer_at(AT + DVec2::new(0.9, 0.0));
        assert!(c.position().is_some());
        c.pointer_at(AT + DVec2::new(1.1, 0.0));
        assert_eq!(c.position(), None);
    }
}
