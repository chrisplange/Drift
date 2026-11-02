//! Offsets: the offset of a chain of curves at one distance, generated from the chain and
//! following it (#247). The pieces are ordinary lines, arcs, circles and pen curves the
//! offset owns, recorded by key so regenerating updates the same entities. Walls set the
//! protocol (`docs/editor-split-design.md` §6); the generator is `drift_geom::offsets`.

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
