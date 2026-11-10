//! Rules saved with a drawing that change what it is: how its closed areas are found and
//! picked, how its fills stack and follow edits, and when ends count as a gap. They travel
//! with the file, so it fills, picks and prints the same on every computer. Also the window
//! within which a sketched line is taken as level or plumb.

use crate::{Constraint, DVec2, EntityId};

/// Lines drawn within this many degrees of level or plumb get a Horizontal or Vertical
/// relation when automatic relations are on. Wide enough for loosely sketched input (#242).
pub const AXIS_INFERENCE_DEGREES: f64 = 3.0;

/// The Horizontal or Vertical relation inferred for `line`, running along `d` (from one end
/// to the other), if it is within [`AXIS_INFERENCE_DEGREES`] of level or plumb.
pub fn axis_relation(line: EntityId, d: DVec2) -> Option<Constraint> {
    let slope = AXIS_INFERENCE_DEGREES.to_radians().tan();
    if d.y.abs() <= slope * d.x.abs() {
        Some(Constraint::Horizontal { line })
    } else if d.x.abs() <= slope * d.y.abs() {
        Some(Constraint::Vertical { line })
    } else {
        None
    }
}

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
    /// Which door and window size set (and wall thickness presets) the drawing uses; None:
    /// by its units (US imperial for imperial drawings, generic metric for metric ones).
    pub size_set: Option<crate::SizeSetId>,
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
            size_set: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_degrees(degrees: f64) -> DVec2 {
        let a = degrees.to_radians();
        DVec2::new(a.cos(), a.sin()) * 100.0
    }

    #[test]
    fn axis_relation_takes_sketched_lines_within_three_degrees() {
        let line = EntityId(1);
        assert_eq!(
            axis_relation(line, at_degrees(2.5)),
            Some(Constraint::Horizontal { line })
        );
        assert_eq!(axis_relation(line, at_degrees(8.0)), None);
        // Plumb, and a side about 1.05° off plumb as in #242, drawn either way.
        assert_eq!(
            axis_relation(line, at_degrees(92.5)),
            Some(Constraint::Vertical { line })
        );
        assert_eq!(
            axis_relation(line, at_degrees(-88.95)),
            Some(Constraint::Vertical { line })
        );
        assert_eq!(axis_relation(line, at_degrees(98.0)), None);
    }
}
