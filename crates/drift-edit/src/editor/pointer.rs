//! Where the pointer is and what it means: snaps, guides, and the anchor a click makes.

use super::*;

/// Why the pointer was moved off its raw position other than by a snap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideKind {
    /// Aligned horizontally and/or vertically with recently hovered points (inference lines).
    Aligned,
    /// Made exactly horizontal from the previous point; the new line gets a Horizontal relation.
    Horizontal,
    /// Made exactly vertical from the previous point; the new line gets a Vertical relation.
    Vertical,
    /// Square to this line (a guide from its end), or onto it (a perpendicular snap); onto a
    /// circle or arc, along its radius. The new line gets a Perpendicular relation (through
    /// the centre for a circle or arc).
    Perpendicular(EntityId),
    /// Tangent to this circle or arc (a guide from its end, or a tangent snap); the new line
    /// gets a Tangent relation.
    Tangent(EntityId),
}

/// An inference: the adjusted position and the points the dotted guide lines run from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub at: DVec2,
    pub kind: GuideKind,
    /// Up to two reference points (for drawing guide lines).
    pub from: [Option<DVec2>; 2],
}

/// Where the pointer is, in world coordinates, with its snap target (if any).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// Unsnapped position.
    pub raw: DVec2,
    pub snap: Option<Snap>,
    /// Picking tolerance in world units.
    pub tolerance: f64,
    /// Inference adjustment, used unless the pointer snapped to an existing point.
    pub guide: Option<Guide>,
    /// Whether snapping onto a midpoint or a curve should add a Midpoint / On curve relation
    /// (automatic relations).
    pub relations: bool,
    /// How to show lengths in readouts.
    pub units: Units,
    /// Alt is held (places freely, without snapping; the pen breaks a handle with it).
    pub alt: bool,
    /// Shift is held.
    pub shift: bool,
    /// Ctrl (Cmd on macOS) is held.
    pub command: bool,
    /// Scaling with dimensions (Alt) steps through round sizes.
    pub round_scale: bool,
    /// Lines being drawn turn in steps of this many degrees, if angle snapping is on.
    pub angle_step: Option<f64>,
    /// Perpendicular and tangent guides from the end a line is drawn on from.
    pub perp_tangent: bool,
    /// A snap target an axis lock moved the point off: the point goes in line with it on the
    /// axis instead. Kept so the canvas can mark both it and where the click lands.
    pub overridden: Option<Snap>,
}

impl Pointer {
    pub fn new(raw: DVec2, tolerance: f64) -> Self {
        Self {
            raw,
            snap: None,
            tolerance,
            guide: None,
            relations: true,
            units: Units::default(),
            alt: false,
            shift: false,
            command: false,
            round_scale: true,
            angle_step: None,
            perp_tangent: false,
            overridden: None,
        }
    }

    /// The line whose midpoint the pointer snapped to, if it should get a Midpoint relation.
    pub fn midpoint_of(&self) -> Option<EntityId> {
        self.snap
            .filter(|s| s.kind == drift_geom::SnapKind::Midpoint && self.relations)
            .and_then(|s| s.entity)
    }

    /// The curve the pointer snapped onto (anywhere along it, or at a quadrant), if a new point
    /// there should get an On curve relation.
    pub fn on_curve(&self) -> Option<EntityId> {
        use drift_geom::SnapKind::{OnCurve, Perpendicular, Quadrant, Tangent};
        self.exact_snap()
            .filter(|s| {
                matches!(s.kind, OnCurve | Quadrant | Perpendicular | Tangent) && self.relations
            })
            .and_then(|s| s.entity)
    }

    /// A snap to a specific spot on the geometry (a point, midpoint, quadrant or a point on a
    /// curve), which wins over inference guides. Grid snaps combine with guides instead.
    pub fn exact_snap(&self) -> Option<Snap> {
        self.snap.filter(|s| s.kind != drift_geom::SnapKind::Grid)
    }

    /// The effective position: an exact snap, else an inference guide (itself on the grid when
    /// grid snapping), else a grid snap, else the raw position. What the snap marker shows is
    /// where geometry goes.
    pub fn pos(&self) -> DVec2 {
        match (self.exact_snap(), self.guide, self.snap) {
            (Some(s), ..) => s.at,
            (None, Some(g), _) => g.at,
            (None, None, Some(grid)) => grid.at,
            (None, None, None) => self.raw,
        }
    }

    /// The automatic relation implied by a perpendicular or tangent snap, else by the guide.
    /// With another exact snap deciding the position, a tool sets a guide only when the
    /// segment comes out level, plumb, square or tangent on the snapped point (#204).
    pub fn auto_relation(&self) -> Option<GuideKind> {
        if let Some(snap) = self.exact_snap() {
            match (snap.kind, snap.entity) {
                (drift_geom::SnapKind::Perpendicular, Some(curve)) => {
                    return Some(GuideKind::Perpendicular(curve));
                }
                (drift_geom::SnapKind::Tangent, Some(curve)) => {
                    return Some(GuideKind::Tangent(curve));
                }
                _ => {}
            }
        }
        self.guide
            .filter(|g| self.exact_snap().is_none_or(|s| s.at == g.at))
            .map(|g| g.kind)
            .filter(|k| *k != GuideKind::Aligned)
    }

    /// The existing point entity under the pointer, if it snapped to one.
    pub fn snapped_point(&self) -> Option<EntityId> {
        self.snap
            .filter(|s| s.kind.is_point())
            .and_then(|s| s.entity)
    }

    pub fn anchor(&self) -> Anchor {
        Anchor {
            pos: self.pos(),
            existing: self.snapped_point(),
            midpoint_of: self.midpoint_of(),
            on_curve: self.on_curve(),
        }
    }
}

/// A position a tool has captured: either an existing point to reuse or a new location.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub pos: DVec2,
    pub existing: Option<EntityId>,
    /// A new point here should be kept at this line's midpoint.
    pub midpoint_of: Option<EntityId>,
    /// A new point here should be kept on this curve.
    pub on_curve: Option<EntityId>,
}

impl Anchor {
    /// A new point at an exact position.
    pub fn at(pos: DVec2) -> Self {
        Self {
            pos,
            existing: None,
            midpoint_of: None,
            on_curve: None,
        }
    }

    /// An existing point, at `pos`.
    pub fn existing(pos: DVec2, point: EntityId) -> Self {
        Self {
            existing: Some(point),
            ..Self::at(pos)
        }
    }

    /// A point of the shape's own at this anchor, whatever it was snapped to: a closed
    /// shape or a pen path drawn onto another shape's point or curve is placed there, not
    /// joined to it (join shapes with relations, or by trimming). Call inside a transaction.
    pub fn own(&self, doc: &mut Document) -> EntityId {
        doc.add_point(self.pos)
    }

    /// The point entity for this anchor, creating it if needed (with its Midpoint or On curve
    /// relation, if it was snapped to a midpoint or curve): lines and arcs drawn from a
    /// point continue from it. Call inside a transaction.
    pub fn resolve(&self, doc: &mut Document) -> EntityId {
        match self.existing {
            Some(id) if doc.contains(id) => id,
            _ => {
                let point = doc.add_point(self.pos);
                if let Some(line) = self.midpoint_of {
                    let _ = doc.add_constraint(Constraint::Midpoint { point, line });
                } else if let Some(curve) = self.on_curve {
                    let _ = doc.add_constraint(Constraint::PointOnCurve { point, curve });
                }
                point
            }
        }
    }
}
