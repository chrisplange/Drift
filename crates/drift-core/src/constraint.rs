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
    /// A point, or a line standing for its midpoint (dimensions can measure to midpoints).
    PointOrMidpoint,
    Line,
    /// A circle or an arc.
    Circular,
    /// Two lines, or two circles/arcs (for [`Constraint::Equal`]).
    LineOrCircular,
    /// Any curve, ellipses included.
    Curve,
}

impl Slot {
    pub fn describe(self) -> &'static str {
        match self {
            Slot::Point => "a point",
            Slot::PointOrMidpoint => "a point or a line's midpoint",
            Slot::Line => "a line",
            Slot::Circular => "a circle or arc",
            Slot::LineOrCircular => "a line, circle or arc",
            Slot::Curve => "a curve",
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
    /// A line or circle/arc (`a`) tangent to a circle or arc (`b`). Which side (and, for two
    /// circles, inside or outside) follows the geometry when the relation is added. `b` may
    /// also be an elliptical arc or a Bézier sharing an end point with `a`: the two are then
    /// tangent at that point.
    Tangent {
        a: EntityId,
        b: EntityId,
    },
    /// Two circles/arcs sharing a centre.
    Concentric {
        a: EntityId,
        b: EntityId,
    },
    /// Two circles/arcs with the same centre and radius.
    Coradial {
        a: EntityId,
        b: EntityId,
    },
    /// Two lines on the same infinite line.
    Collinear {
        a: EntityId,
        b: EntityId,
    },
    /// A point lying on a line (extended infinitely) or on a circle/arc's circle.
    PointOnCurve {
        point: EntityId,
        curve: EntityId,
    },
    /// Two points mirrored about a line.
    Symmetric {
        a: EntityId,
        b: EntityId,
        axis: EntityId,
    },
    /// A smooth anchor (G1): the handles `a` and `b` on either side of `point` stay on one
    /// line through it, so the curves meeting there share a tangent.
    Smooth {
        point: EntityId,
        a: EntityId,
        b: EntityId,
    },
    /// A point kept at the midpoint of a line.
    Midpoint {
        point: EntityId,
        line: EntityId,
    },
    /// A point locked at a location.
    Fix {
        point: EntityId,
        at: DVec2,
    },
    /// A point's x locked (what Fix leaves of a point whose y something else holds).
    FixX {
        point: EntityId,
        x: f64,
    },
    /// A point's y locked (what Fix leaves of a point whose x something else holds).
    FixY {
        point: EntityId,
        y: f64,
    },
    /// A circle's or arc's radius locked (used when fixing circular geometry).
    FixRadius {
        curve: EntityId,
        radius: f64,
    },
    /// Driving dimension: distance between two points (a line's length uses its end points).
    /// Either may be a line, meaning its midpoint.
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
    /// Driving dimension: diameter of a circle or arc.
    Diameter {
        curve: EntityId,
        value: f64,
    },
    /// Driving dimension: horizontal (x) distance between two points.
    HorizontalDistance {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    /// Driving dimension: vertical (y) distance between two points.
    VerticalDistance {
        a: EntityId,
        b: EntityId,
        value: f64,
    },
    /// Driving dimension: perpendicular distance from a point to a line (extended infinitely).
    PointLineDistance {
        point: EntityId,
        line: EntityId,
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
            | Constraint::VerticalPoints { a, b } => vec![(a, Point), (b, Point)],
            // Distance dimensions may measure to a line's midpoint: a line id in a point slot.
            Constraint::Distance { a, b, .. }
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. } => {
                vec![(a, PointOrMidpoint), (b, PointOrMidpoint)]
            }
            Constraint::PointLineDistance { point, line, .. } => {
                vec![(point, PointOrMidpoint), (line, Line)]
            }
            Constraint::Midpoint { point, line } => vec![(point, Point), (line, Line)],
            Constraint::Smooth { point, a, b } => vec![(point, Point), (a, Point), (b, Point)],
            Constraint::Tangent { a, b } => vec![(a, LineOrCircular), (b, Curve)],
            Constraint::Concentric { a, b } | Constraint::Coradial { a, b } => {
                vec![(a, Circular), (b, Circular)]
            }
            Constraint::Collinear { a, b } => vec![(a, Line), (b, Line)],
            Constraint::PointOnCurve { point, curve } => vec![(point, Point), (curve, Curve)],
            Constraint::Symmetric { a, b, axis } => vec![(a, Point), (b, Point), (axis, Line)],
            Constraint::Horizontal { line } | Constraint::Vertical { line } => {
                vec![(line, Line)]
            }
            Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Angle { a, b, .. } => vec![(a, Line), (b, Line)],
            Constraint::Equal { a, b } => vec![(a, LineOrCircular), (b, LineOrCircular)],
            Constraint::Fix { point, .. }
            | Constraint::FixX { point, .. }
            | Constraint::FixY { point, .. } => vec![(point, Point)],
            Constraint::FixRadius { curve, .. }
            | Constraint::Radius { curve, .. }
            | Constraint::Diameter { curve, .. } => vec![(curve, Circular)],
        }
    }

    /// The entities this constraint refers to.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> {
        self.slots().into_iter().map(|(id, _)| id)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Constraint::Coincident { .. } => "Coincident",
            Constraint::Midpoint { .. } => "Midpoint",
            Constraint::Smooth { .. } => "Smooth",
            Constraint::Tangent { .. } => "Tangent",
            Constraint::Concentric { .. } => "Concentric",
            Constraint::Coradial { .. } => "Coradial",
            Constraint::Collinear { .. } => "Collinear",
            Constraint::PointOnCurve { .. } => "On curve",
            Constraint::Symmetric { .. } => "Symmetric",
            Constraint::Horizontal { .. } | Constraint::HorizontalPoints { .. } => "Horizontal",
            Constraint::Vertical { .. } | Constraint::VerticalPoints { .. } => "Vertical",
            Constraint::Parallel { .. } => "Parallel",
            Constraint::Perpendicular { .. } => "Perpendicular",
            Constraint::Equal { .. } => "Equal",
            Constraint::Fix { .. }
            | Constraint::FixX { .. }
            | Constraint::FixY { .. }
            | Constraint::FixRadius { .. } => "Fix",
            Constraint::Distance { .. } | Constraint::PointLineDistance { .. } => "Distance",
            Constraint::Radius { .. } => "Radius",
            Constraint::Diameter { .. } => "Diameter",
            Constraint::HorizontalDistance { .. } => "Horizontal distance",
            Constraint::VerticalDistance { .. } => "Vertical distance",
            Constraint::Angle { .. } => "Angle",
        }
    }

    /// Dimensions have a value and are drawn with a label; relations are not.
    pub fn is_dimension(&self) -> bool {
        self.value().is_some()
    }

    /// The driving value of a dimension (mm, or degrees for angles).
    pub fn value(&self) -> Option<f64> {
        match *self {
            Constraint::Distance { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. }
            | Constraint::HorizontalDistance { value, .. }
            | Constraint::VerticalDistance { value, .. }
            | Constraint::PointLineDistance { value, .. } => Some(value),
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
            Constraint::Distance { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. }
            | Constraint::HorizontalDistance { value, .. }
            | Constraint::VerticalDistance { value, .. }
            | Constraint::PointLineDistance { value, .. } => *value = new,
            Constraint::Angle { degrees, .. } => *degrees = new,
            _ => {}
        }
        self
    }

    /// Whether a dimension value is acceptable.
    pub fn value_is_valid(&self) -> bool {
        match *self {
            Constraint::Distance { value, .. }
            | Constraint::Radius { value, .. }
            | Constraint::Diameter { value, .. }
            | Constraint::HorizontalDistance { value, .. }
            | Constraint::VerticalDistance { value, .. }
            | Constraint::PointLineDistance { value, .. } => value.is_finite() && value > 0.0,
            Constraint::Angle { degrees, .. } => degrees.is_finite(),
            Constraint::Fix { at, .. } => at.is_finite(),
            Constraint::FixX { x: v, .. } | Constraint::FixY { y: v, .. } => v.is_finite(),
            Constraint::FixRadius { radius, .. } => radius.is_finite() && radius > 0.0,
            _ => true,
        }
    }

    /// Redirect references from point `from` to point `to`.
    /// The same constraint with every entity reference passed through `f` (for copies and
    /// replacements).
    pub fn map_entities(&self, f: impl Fn(EntityId) -> EntityId) -> Constraint {
        use Constraint::*;
        match *self {
            Coincident { a, b } => Coincident { a: f(a), b: f(b) },
            Horizontal { line } => Horizontal { line: f(line) },
            Vertical { line } => Vertical { line: f(line) },
            HorizontalPoints { a, b } => HorizontalPoints { a: f(a), b: f(b) },
            VerticalPoints { a, b } => VerticalPoints { a: f(a), b: f(b) },
            Parallel { a, b } => Parallel { a: f(a), b: f(b) },
            Perpendicular { a, b } => Perpendicular { a: f(a), b: f(b) },
            Equal { a, b } => Equal { a: f(a), b: f(b) },
            Tangent { a, b } => Tangent { a: f(a), b: f(b) },
            Concentric { a, b } => Concentric { a: f(a), b: f(b) },
            Coradial { a, b } => Coradial { a: f(a), b: f(b) },
            Collinear { a, b } => Collinear { a: f(a), b: f(b) },
            PointOnCurve { point, curve } => PointOnCurve {
                point: f(point),
                curve: f(curve),
            },
            Symmetric { a, b, axis } => Symmetric {
                a: f(a),
                b: f(b),
                axis: f(axis),
            },
            Midpoint { point, line } => Midpoint {
                point: f(point),
                line: f(line),
            },
            Smooth { point, a, b } => Smooth {
                point: f(point),
                a: f(a),
                b: f(b),
            },
            Fix { point, at } => Fix {
                point: f(point),
                at,
            },
            FixX { point, x } => FixX { point: f(point), x },
            FixY { point, y } => FixY { point: f(point), y },
            FixRadius { curve, radius } => FixRadius {
                curve: f(curve),
                radius,
            },
            Distance { a, b, value } => Distance {
                a: f(a),
                b: f(b),
                value,
            },
            Radius { curve, value } => Radius {
                curve: f(curve),
                value,
            },
            Diameter { curve, value } => Diameter {
                curve: f(curve),
                value,
            },
            HorizontalDistance { a, b, value } => HorizontalDistance {
                a: f(a),
                b: f(b),
                value,
            },
            VerticalDistance { a, b, value } => VerticalDistance {
                a: f(a),
                b: f(b),
                value,
            },
            PointLineDistance { point, line, value } => PointLineDistance {
                point: f(point),
                line: f(line),
                value,
            },
            Angle { a, b, degrees } => Angle {
                a: f(a),
                b: f(b),
                degrees,
            },
        }
    }

    /// True for relations that depend on the sketch's orientation (they don't survive a
    /// rotation or a mirror about a slanted line).
    pub fn is_orientation_dependent(&self) -> bool {
        matches!(
            self,
            Constraint::Horizontal { .. }
                | Constraint::Vertical { .. }
                | Constraint::HorizontalPoints { .. }
                | Constraint::VerticalPoints { .. }
                | Constraint::HorizontalDistance { .. }
                | Constraint::VerticalDistance { .. }
                | Constraint::Fix { .. }
                | Constraint::FixX { .. }
                | Constraint::FixY { .. }
        )
    }

    /// True for a pin of a point where it is: Fix, or its x or y alone.
    pub fn is_point_fix(&self) -> bool {
        matches!(
            self,
            Constraint::Fix { .. } | Constraint::FixX { .. } | Constraint::FixY { .. }
        )
    }

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
            | Constraint::Distance { a, b, .. }
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. } => {
                swap(a);
                swap(b);
            }
            Constraint::Fix { point, .. }
            | Constraint::FixX { point, .. }
            | Constraint::FixY { point, .. }
            | Constraint::PointLineDistance { point, .. }
            | Constraint::Midpoint { point, .. }
            | Constraint::PointOnCurve { point, .. } => swap(point),
            Constraint::Symmetric { a, b, .. } => {
                swap(a);
                swap(b);
            }
            Constraint::Smooth { point, a, b } => {
                swap(point);
                swap(a);
                swap(b);
            }
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
            | Constraint::HorizontalDistance { a, b, .. }
            | Constraint::VerticalDistance { a, b, .. }
            | Constraint::Tangent { a, b }
            | Constraint::Concentric { a, b }
            | Constraint::Coradial { a, b }
            | Constraint::Collinear { a, b }
            | Constraint::Symmetric { a, b, .. }
            | Constraint::Parallel { a, b }
            | Constraint::Perpendicular { a, b }
            | Constraint::Equal { a, b }
            | Constraint::Angle { a, b, .. } => a == b,
            Constraint::Smooth { point, a, b } => point == a || point == b || a == b,
            _ => false,
        }
    }
}
