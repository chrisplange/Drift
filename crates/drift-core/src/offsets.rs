//! Offsets: the offset of a chain of curves at one distance, generated from the chain and
//! following it (#247). The pieces are ordinary lines, arcs, circles and pen curves the
//! offset owns, recorded by key ([`OffsetMade`]) so regenerating updates the same entities
//! instead of making new ones. Walls set the protocol (`docs/editor-split-design.md` §6);
//! the generator is `drift_geom::offsets`.

use std::collections::BTreeMap;

use crate::document::EntityId;

/// Stable identifier of an offset, "x3".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OffsetId(pub u32);

impl std::fmt::Display for OffsetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "x{}", self.0)
    }
}

/// Which side of its source an offset lies on: the left of travel along the chain (from
/// its first curve towards its second) or the right. For a whole circle or ellipse, which
/// has no travel to speak of, `Left` is outward (as the Offset tool has always signed it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    #[default]
    Left,
    Right,
}

impl Side {
    /// The sign of the offset distance the generator takes: positive to the left.
    pub fn sign(self) -> f64 {
        match self {
            Side::Left => 1.0,
            Side::Right => -1.0,
        }
    }

    /// The other side (a mirror image of a chain swaps them).
    pub fn flipped(self) -> Self {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// An offset of a chain of curves.
#[derive(Clone, Debug, PartialEq)]
pub struct Offset {
    /// The chain's curves in travel order (lines, arcs, circles, ellipses, pen curves,
    /// joined end to end). Travel runs from the first towards the second; a lone open curve
    /// runs as stored, a lone circle or ellipse counter-clockwise.
    pub source: Vec<EntityId>,
    pub side: Side,
    /// Distance in mm (more than 0), and the expression it comes from, if any.
    pub distance: f64,
    pub distance_expr: Option<String>,
    /// What the offset made, by key.
    pub made: OffsetMade,
}

impl Offset {
    /// An offset of `source` on `side` at `distance`, with nothing made yet.
    pub fn new(source: Vec<EntityId>, side: Side, distance: f64) -> Self {
        Self {
            source,
            side,
            distance,
            distance_expr: None,
            made: OffsetMade::default(),
        }
    }

    /// The distance signed as the generator takes it: positive to the left.
    pub fn signed(&self) -> f64 {
        self.side.sign() * self.distance
    }
}

/// What an offset generated, by key: its curves (pieces) and the points between them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OffsetMade {
    pub curves: BTreeMap<PieceKey, EntityId>,
    pub points: BTreeMap<PointKey, EntityId>,
}

impl OffsetMade {
    /// Every entity it holds: curves, then points.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.curves.values().chain(self.points.values()).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty() && self.points.is_empty()
    }
}

/// A generated curve of an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PieceKey {
    /// The `n`-th curve of the offset of source curve `of`: a line, arc or circle has one
    /// (n = 0); an ellipse or a pen curve is fitted with a run of pen curves (n = 0, 1, 2…).
    Of { of: EntityId, n: u16 },
    /// The straight bridge across the corner that opened between the offset of source curve
    /// `after` and the next curve's (fitted curves can't be extended to meet).
    Bridge { after: EntityId },
}

/// A generated point of an offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointKey {
    /// Where the piece starts (the piece before it ends there too, when they meet).
    Start(PieceKey),
    /// Where the piece ends when no piece starts there (the last piece of an open chain, or
    /// a gap the corners left).
    End(PieceKey),
    /// A fitted pen curve's handle: 0 leaving its start, 1 arriving at its end.
    Handle(PieceKey, u8),
}
