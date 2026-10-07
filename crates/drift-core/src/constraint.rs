//! Geometric relations and driving dimensions between entities.

use glam::DVec2;

use crate::EntityId;

/// Stable identifier of a constraint within a document. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstraintId(pub u32);

impl std::fmt::Display for ConstraintId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}", self.0)
    }
}

/// What kind of entity a constraint slot accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Point,
    Line,
    /// A circle or an arc.
    Circular,
    /// Two lines, or two circles/arcs (for [`Constraint::Equal`]).
    LineOrCircular,
}

impl Slot {
    pub fn describe(self) -> &'static str {
        match self {
            Slot::Point => "a point",
            Slot::Line => "a line",
            Slot::Circular => "a circle or arc",
            Slot::LineOrCircular => "a line, circle or arc",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    /// Two points at the same location.
    Coincident {
        a: EntityId,
        b: EntityId,
    },
    Horizontal {
        line: EntityId,
    },
    Vertical {
        line: EntityId,
    },
    /// Two points on the same horizontal.
    HorizontalPoints {
        a: EntityId,
        b: EntityId,
    },
    /// Two points on the same vertical.
    VerticalPoints {
        a: EntityId,
        b: EntityId,
    },
    Parallel {
        a: EntityId,
        b: EntityId,
    },
    Perpendicular {
        a: EntityId,
        b: EntityId,
    },
    /// Two lines of equal length, or two circles/arcs of equal radius.
    Equal {
        a: EntityId,
        b: EntityId,
    },
    /// A point locked at a location.
    Fix {
        point: EntityId,
        at: DVec2,
    },
    /// A circle's or arc's radius locked (used when fixing circular geometry).
    FixRadius {
        curve: EntityId,
        radius: f64,
    },
    /// Driving dimension: distance between two points (a line's length uses its end points).
    Distance {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    /// Driving dimension: radius of a circle or arc.
    Radius {
        curve: EntityId,
        value: f64,
    },
    /// Driving dimension: angle between two lines, in degrees, measured from `a` to `b`
    /// counter-clockwise. Lines are undirected, so the angle is taken modulo 180°.
    Angle {
        a: EntityId,
        b: EntityId,
        degrees: f64,
    },
}

impl Constraint {
    /// The entities this constraint refers to, with the kind each must be.
    pub fn slots(&self) -> Vec<(EntityId, Slot)> {
        use Slot::*;
        match *self {
            Constraint::Coincident { a, b }
            | Constraint::HorizontalPoints { a, b }
            | Constraint::VerticalPoints { a, b }
            | Constraint::Distance { a, b, .. } => vec![(a, Point), (b, Point)],
            Constraint::Horizontal { line } | Constraint::Vertical { line } => {
                vec![(line, Line)]
            }
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Angle { a, b, .. } => vec![(a, Line), (b, Line)],
            Constraint::Equal { a, b } => vec![(a, LineOrCircular), (b, LineOrCircular)],
            Constraint::Fix { point, .. } => vec![(point, Point)],
            Constraint::FixRadius { curve, .. } | Constraint::Radius { curve, .. } => {
                vec![(curve, Circular)]
            }
        }
    }

    /// The entities this constraint refers to.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> {
        self.slots().into_iter().map(|(id, _)| id)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Constraint::Coincident { .. } => "Coincident",
            Constraint::Horizontal { .. } | Constraint::HorizontalPoints { .. } => "Horizontal",
            Constraint::Vertical { .. } | Constraint::VerticalPoints { .. } => "Vertical",
            Constraint::Parallel { .. } => "Parallel",
            Constraint::Perpendicular { .. } => "Perpendicular",
            Constraint::Equal { .. } => "Equal",
            Constraint::Fix { .. } | Constraint::FixRadius { .. } => "Fix",
            Constraint::Distance { .. } => "Distance",
            Constraint::Radius { .. } => "Radius",
            Constraint::Angle { .. } => "Angle",
        }
    }

    /// The driving value of a dimension (mm, or degrees for angles).
    pub fn value(&self) -> Option<f64> {
        match *self {
            Constraint::Distance { value, .. } | Constraint::Radius { value, .. } => Some(value),
            Constraint::Angle { degrees, .. } => Some(degrees),
            _ => None,
        }
    }

    /// Unit suffix for [`value`](Self::value).
    pub fn unit(&self) -> &'static str {
        match self {
            Constraint::Angle { .. } => "°",
            _ => "mm",
        }
    }

    /// A copy with a new dimension value. Non-dimensions are returned unchanged.
    pub fn with_value(mut self, new: f64) -> Self {
        match &mut self {
            Constraint::Distance { value, .. } | Constraint::Radius { value, .. } => *value = new,
            Constraint::Angle { degrees, .. } => *degrees = new,
            _ => {}
        }
        self
    }

    /// Whether a dimension value is acceptable.
    pub fn value_is_valid(&self) -> bool {
        match *self {
            Constraint::Distance { value, .. } | Constraint::Radius { value, .. } => {
                value.is_finite() && value > 0.0
            }
            Constraint::Angle { degrees, .. } => degrees.is_finite(),
            Constraint::Fix { at, .. } => at.is_finite(),
            Constraint::FixRadius { radius, .. } => radius.is_finite() && radius > 0.0,
            _ => true,
        }
    }

    /// Redirect references from point `from` to point `to`.
    pub(crate) fn replace_point(&mut self, from: EntityId, to: EntityId) {
        let swap = |id: &mut EntityId| {
            if *id == from {
                *id = to;
            }
        };
        match self {
            Constraint::Coincident { a, b }
            | Constraint::HorizontalPoints { a, b }
            | Constraint::VerticalPoints { a, b }
            | Constraint::Distance { a, b, .. } => {
                swap(a);
                swap(b);
            }
            Constraint::Fix { point, .. } => swap(point),
            _ => {}
        }
    }

    /// True if the constraint refers to the same entity twice where that makes no sense.
    pub(crate) fn is_degenerate(&self) -> bool {
        match *self {
            Constraint::Coincident { a, b }
            | Constraint::HorizontalPoints { a, b }
            | Constraint::VerticalPoints { a, b }
            | Constraint::Distance { a, b, .. }
            | Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Equal { a, b }
            | Constraint::Angle { a, b, .. } => a == b,
            _ => false,
        }
    }
}
