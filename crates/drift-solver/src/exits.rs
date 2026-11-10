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
