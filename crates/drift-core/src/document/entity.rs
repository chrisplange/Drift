//! Entities: their ids, kinds and look, and the document's error type.

use super::*;

/// Stable identifier of an entity within a document. Never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u32);

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "e{}", self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EntityKind {
    Point(DVec2),
    Line {
        a: EntityId,
        b: EntityId,
    },
    Circle {
        center: EntityId,
        radius: f64,
    },
    /// Counter-clockwise arc from `start` to `end` around `center`. The radius is
    /// `|start - center|`; `end` is kept on that circle.
    Arc {
        center: EntityId,
        start: EntityId,
        end: EntityId,
    },
    /// An ellipse: its centre and the ends of two semi-axes. `minor` is kept on the
    /// perpendicular to `center → major` through the centre.
    Ellipse {
        center: EntityId,
        major: EntityId,
        minor: EntityId,
    },
    /// A cubic Bézier curve from `a` to `b`, shaped by the handle points `c1` (leaving `a`)
    /// and `c2` (arriving at `b`). Paths are chains of these sharing end points.
    Bezier {
        a: EntityId,
        c1: EntityId,
        c2: EntityId,
        b: EntityId,
    },
    /// The counter-clockwise part of an ellipse from `start` to `end` (both kept on it).
    EllipseArc {
        center: EntityId,
        major: EntityId,
        minor: EntityId,
        start: EntityId,
        end: EntityId,
    },
}

impl EntityKind {
    /// The point entities this entity is defined by.
    pub fn points(&self) -> impl Iterator<Item = EntityId> {
        let ids: [Option<EntityId>; 5] = match *self {
            EntityKind::Point(_) => [None; 5],
            EntityKind::Line { a, b } => [Some(a), Some(b), None, None, None],
            EntityKind::Circle { center, .. } => [Some(center), None, None, None, None],
            EntityKind::Arc { center, start, end } => {
                [Some(center), Some(start), Some(end), None, None]
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => [Some(center), Some(major), Some(minor), None, None],
            EntityKind::Bezier { a, c1, c2, b } => [Some(a), Some(c1), Some(c2), Some(b), None],
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => [
                Some(center),
                Some(major),
                Some(minor),
                Some(start),
                Some(end),
            ],
        };
        ids.into_iter().flatten()
    }

    pub fn is_point(&self) -> bool {
        matches!(self, EntityKind::Point(_))
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            EntityKind::Point(_) => "Point",
            EntityKind::Line { .. } => "Line",
            EntityKind::Circle { .. } => "Circle",
            EntityKind::Arc { .. } => "Arc",
            EntityKind::Ellipse { .. } => "Ellipse",
            EntityKind::EllipseArc { .. } => "Elliptical arc",
            EntityKind::Bezier { .. } => "Curve",
        }
    }

    /// A curve's end points (where it connects to others): a line's or arc's ends, a
    /// Bézier's anchors, an elliptical arc's ends.
    pub fn ends(&self) -> Option<[EntityId; 2]> {
        match *self {
            EntityKind::Line { a, b } | EntityKind::Bezier { a, b, .. } => Some([a, b]),
            EntityKind::Arc { start, end, .. } | EntityKind::EllipseArc { start, end, .. } => {
                Some([start, end])
            }
            _ => None,
        }
    }

    /// An end point this curve and `other` both have.
    pub fn shared_end(&self, other: &EntityKind) -> Option<EntityId> {
        let theirs = other.ends()?;
        self.ends()?.into_iter().find(|p| theirs.contains(p))
    }

    /// Use point `to` wherever the entity uses `from`.
    pub fn replace_point(&mut self, from: EntityId, to: EntityId) {
        let swap = |id: &mut EntityId| {
            if *id == from {
                *id = to;
            }
        };
        match self {
            EntityKind::Point(_) => {}
            EntityKind::Line { a, b } => {
                swap(a);
                swap(b);
            }
            EntityKind::Circle { center, .. } => swap(center),
            EntityKind::Arc { center, start, end } => {
                swap(center);
                swap(start);
                swap(end);
            }
            EntityKind::Ellipse {
                center,
                major,
                minor,
            } => {
                swap(center);
                swap(major);
                swap(minor);
            }
            EntityKind::Bezier { a, c1, c2, b } => {
                swap(a);
                swap(c1);
                swap(c2);
                swap(b);
            }
            EntityKind::EllipseArc {
                center,
                major,
                minor,
                start,
                end,
            } => {
                swap(center);
                swap(major);
                swap(minor);
                swap(start);
                swap(end);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entity {
    pub kind: EntityKind,
    /// Geometry, construction (reference) or centre line.
    pub style: LineKind,
    pub layer: LayerId,
    /// Colour / weight / line type overrides (by layer when unset).
    pub appearance: Appearance,
    /// Where a curve stacks among its layer's fills and curves: higher draws on top (as a
    /// fill's `order`); `None`, at the top of the layer, as curves always were. The curves
    /// outlining a fill draw with it instead.
    pub order: Option<i32>,
}

impl Entity {
    pub fn new(kind: EntityKind) -> Self {
        Self {
            kind,
            style: LineKind::Solid,
            layer: LayerId::DEFAULT,
            appearance: Appearance::default(),
            order: None,
        }
    }
}

/// Errors from document operations that reference entities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentError {
    Missing(EntityId),
    MissingConstraint(ConstraintId),
    NotAPoint(EntityId),
    /// A constraint refers to an entity of the wrong kind.
    WrongKind {
        id: EntityId,
        expected: &'static str,
    },
    Degenerate(&'static str),
    InvalidValue,
    MissingVariable(VariableId),
    /// A variable name that is invalid, reserved or already used.
    BadName(String),
    /// A variable still used by expressions (their owners are listed).
    InUse(String),
    MissingVariant(VariantId),
    MissingLayer(LayerId),
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocumentError::Missing(id) => write!(f, "entity {id} does not exist"),
            DocumentError::MissingConstraint(id) => write!(f, "constraint {id} does not exist"),
            DocumentError::NotAPoint(id) => write!(f, "entity {id} is not a point"),
            DocumentError::WrongKind { id, expected } => {
                write!(f, "entity {id} is not {expected}")
            }
            DocumentError::Degenerate(what) => write!(f, "degenerate {what}"),
            DocumentError::InvalidValue => write!(f, "invalid value"),
            DocumentError::MissingVariable(id) => write!(f, "variable {id} does not exist"),
            DocumentError::BadName(why) => f.write_str(why),
            DocumentError::InUse(by) => write!(f, "still used by {by}"),
            DocumentError::MissingVariant(id) => write!(f, "variant {id} does not exist"),
            DocumentError::MissingLayer(id) => write!(f, "layer {id} does not exist"),
        }
    }
}

impl std::error::Error for DocumentError {}
