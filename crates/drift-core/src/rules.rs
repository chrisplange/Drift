//! Rules saved with a drawing that change what it is: how its closed areas are found and
//! picked, how its fills stack and follow edits, and when ends count as a gap. They travel
//! with the file, so it fills, picks and prints the same on every computer.

/// How a drawing's closed areas and fills behave (saved with it, apart from the app's
/// preferences).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawingRules {
    /// Closed areas are found from each layer's curves separately.
    pub regions_by_layer: bool,
    /// A click inside a closed area picks it only when a fill or hatch is in it (an empty
    /// outline's inside picks nothing).
    pub regions_need_fill: bool,
    /// Each fill or hatch is drawn over everything below it, with its outline on top (as a
    /// drawing program stacks objects). Off: every fill is drawn under every curve.
    pub stacked_fills: bool,
    /// An edit that divides a fill's area moves the fill to the area around its seed; off,
    /// the fill is kept whole and flagged.
    pub follow_divided_fills: bool,
    /// Ends closer than this (mm) are a gap to Check sketch.
    pub gap_tolerance: f64,
}

impl DrawingRules {
    /// The gap tolerance a drawing starts with (mm).
    pub const DEFAULT_GAP_TOLERANCE: f64 = 0.5;
}

impl Default for DrawingRules {
    fn default() -> Self {
        Self {
            regions_by_layer: false,
            regions_need_fill: true,
            stacked_fills: true,
            follow_divided_fills: false,
            gap_tolerance: Self::DEFAULT_GAP_TOLERANCE,
        }
    }
}
