//! How each component's Newton solve ended, counted per thread: test-only instrumentation
//! for the solver's acceptance bar (`docs/solver-audit.md`, section 5). Built only with the
//! `exit-counts` feature, which only drift-edit's tests turn on (as a dev-dependency), so a
//! release build of the app has none of it. Counting changes nothing a solve does.
//!
//! A census reads [`take`] before and after each drag: the share of solves that end other
//! than by the stop test is the audit's hypothesis (b) in numbers.

use std::cell::Cell;

/// How a component's solve ended (see `newton`'s two stages).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// The stop test: its rows held, and its kept shapes' projected gradient, or the most a
    /// step could still gain them, was below the bar.
    Settled,
    /// Its rows were off at a least-squares stationary point: the best step brought them no
    /// more than a bar closer (a conflict, judged by rank).
    Conflict,
    /// It ran to [`crate::MAX_ITERATIONS`]: the safety net.
    Cap,
    /// A residual or gradient wasn't a number.
    NoStep,
}

impl Exit {
    /// Every exit, in the order [`ExitCounts`] keeps them.
    pub const ALL: [Exit; 4] = [Exit::Settled, Exit::Conflict, Exit::Cap, Exit::NoStep];

    /// A short name (the census's column heading).
    pub fn name(self) -> &'static str {
        match self {
            Exit::Settled => "settled",
            Exit::Conflict => "conflict",
            Exit::Cap => "cap",
            Exit::NoStep => "nostep",
        }
    }
}

/// Component solves by how they ended, drag solves apart from the rest (commits, relation
/// adding, analysis's solves), with the iterations they ran.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExitCounts {
    /// Drag solves, indexed as [`Exit::ALL`].
    pub drag: [u64; 4],
    /// Every other solve, indexed as [`Exit::ALL`].
    pub other: [u64; 4],
    /// Iterations the drag solves ran, summed.
    pub drag_iterations: u64,
    /// Iterations the other solves ran, summed.
    pub other_iterations: u64,
}

impl ExitCounts {
    /// Solves of either kind that ended `exit`.
    pub fn of(&self, exit: Exit) -> u64 {
        let i = exit as usize;
        self.drag[i] + self.other[i]
    }
}

thread_local! {
    static COUNTS: Cell<ExitCounts> = Cell::new(ExitCounts::default());
}

/// The counts so far on this thread, reset to zero.
pub fn take() -> ExitCounts {
    COUNTS.with(|c| c.take())
}

/// Count one solve.
pub(crate) fn record(exit: Exit, dragging: bool, iterations: usize) {
    COUNTS.with(|c| {
        let mut n = c.get();
        let i = exit as usize;
        if dragging {
            n.drag[i] += 1;
            n.drag_iterations += iterations as u64;
        } else {
            n.other[i] += 1;
            n.other_iterations += iterations as u64;
        }
        c.set(n);
    });
}

/// How the second stage (the preferences, among the rows' solutions) ended, by its stop: every
/// shape met; the model's whole step (its minimum among the rows' solutions) within the bar;
/// the most a step could gain below half the bar squared; that gain below what the shapes'
/// offsets can resolve (round-off); the region below the bar; a factor that failed; the
/// iteration cap. The region and the cap are stalls: the stage stopped without a test saying
/// the shapes are as near as the rows let them be.
pub const STAGE2: [&str; 7] = [
    "met",
    "step",
    "promised",
    "round-off",
    "region",
    "factor",
    "cap",
];

/// Second-stage endings, drag solves apart from the rest, indexed as [`STAGE2`], and of those
/// how many were stalls (the region or the cap).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stage2Counts {
    pub drag: [u64; 7],
    pub other: [u64; 7],
    pub drag_stalled: [u64; 7],
    pub other_stalled: [u64; 7],
}

thread_local! {
    static STAGE2_COUNTS: Cell<Stage2Counts> = Cell::new(Stage2Counts::default());
}

/// The second-stage counts so far on this thread, reset to zero.
pub fn take_stage2() -> Stage2Counts {
    STAGE2_COUNTS.with(|c| c.take())
}

/// Count one second-stage ending.
pub(crate) fn record_stage2(stop: usize, dragging: bool) {
    let stalled = matches!(STAGE2[stop], "region" | "cap");
    STAGE2_COUNTS.with(|c| {
        let mut n = c.get();
        if dragging {
            n.drag[stop] += 1;
            n.drag_stalled[stop] += u64::from(stalled);
        } else {
            n.other[stop] += 1;
            n.other_stalled[stop] += u64::from(stalled);
        }
        c.set(n);
    });
}
