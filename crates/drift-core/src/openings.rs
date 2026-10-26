//! Openings: doors, windows and cased openings hosted in a wall (`docs/walls-design.md` §6).
//!
//! An opening sits on one segment of its wall's centreline: where along it ([`OpeningAt`]),
//! how wide its cut is, and what is drawn in it ([`Symbol`]). The wall's faces stop at its two
//! jambs (the cut is made by the walls' generator, keyed `Station::Jamb`), so the poché goes
//! round it; the symbol (leaf and swing, sashes, sill) is generated at the wall's thickness as
//! the opening's own entities, recorded by key in [`OpeningMade`] so regenerating updates
//! them in place.
//!
//! A file stores the opening's actual sizes, never a reference to a size table's row: size
//! sets (US imperial, generic metric, …) only offer sizes when placing.

use std::collections::BTreeMap;

use crate::document::EntityId;
use crate::layers::LayerId;
use crate::walls::WallId;

/// Stable identifier of an opening, "o2".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpeningId(pub u32);

impl std::fmt::Display for OpeningId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "o{}", self.0)
    }
}

/// Which end of its segment an opening's offset is measured from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SegEnd {
    #[default]
    Start,
    End,
}

/// Where an opening sits along its segment.
#[derive(Clone, Debug, PartialEq)]
pub enum OpeningAt {
    /// In the middle of the segment.
    Centred,
    /// Its near jamb `offset` along the centreline from that end of the segment (the
    /// expression it comes from, if any).
    From {
        end: SegEnd,
        offset: f64,
        expr: Option<String>,
    },
}

impl OpeningAt {
    /// From the segment's start.
    pub fn from_start(offset: f64) -> Self {
        OpeningAt::From {
            end: SegEnd::Start,
            offset,
            expr: None,
        }
    }

    /// Where its first jamb is along a segment `len` long, for an opening `width` wide.
    pub fn start_on(&self, len: f64, width: f64) -> f64 {
        match self {
            OpeningAt::Centred => (len - width) / 2.0,
            OpeningAt::From {
                end: SegEnd::Start,
                offset,
                ..
            } => *offset,
            OpeningAt::From {
                end: SegEnd::End,
                offset,
                ..
            } => len - offset - width,
        }
    }
}

/// The hinge side, standing on the face it swings to and looking at the wall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Hand {
    #[default]
    Left,
    Right,
}

impl Hand {
    pub fn flipped(self) -> Self {
        match self {
            Hand::Left => Hand::Right,
            Hand::Right => Hand::Left,
        }
    }
}

/// A face of a wall, looking along its centreline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WallFace {
    #[default]
    Left,
    Right,
}

impl WallFace {
    pub fn flipped(self) -> Self {
        match self {
            WallFace::Left => WallFace::Right,
            WallFace::Right => WallFace::Left,
        }
    }
}

/// What is drawn in an opening.
#[derive(Clone, Debug, PartialEq)]
pub enum Symbol {
    /// A library family's symbol, generated at the wall's thickness: the family's id
    /// (`door.single`, `window.casement`, …) and the type it was placed as
    /// (`3′0″ × 6′8″`), kept for schedules and the Inspector.
    Library { family: String, type_name: String },
    /// Nothing: the wall's jambs alone (a plain cut, as for a pass-through).
    Void,
}

/// What an opening's symbol made, by key: its curves in the order the family draws them,
/// and their points (`curve × 3 + n`: a line's two ends, an arc's centre, start and end).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpeningMade {
    pub curves: BTreeMap<u32, EntityId>,
    pub points: BTreeMap<u32, EntityId>,
}

impl OpeningMade {
    /// Every entity it holds: curves, then points.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.curves.values().chain(self.points.values()).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty() && self.points.is_empty()
    }
}

/// A door, window or cased opening in a wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    /// The wall it is in, and which segment of its centreline.
    pub wall: WallId,
    pub seg: u16,
    pub at: OpeningAt,
    /// The structural opening: the cut in the wall (a door's rough opening), and the
    /// expression it comes from, if any.
    pub width: f64,
    pub width_expr: Option<String>,
    /// What the symbol draws: a door's leaf, a window's nominal width (at most `width`).
    pub leaf: f64,
    /// The opening's height (0 when not known): not drawn in plan, kept for schedules.
    pub height: f64,
    pub symbol: Symbol,
    pub hand: Hand,
    /// The face it swings to (a door), or the room side (a window's stool).
    pub facing: WallFace,
    /// None: the wall's layer.
    pub layer: Option<LayerId>,
    pub made: OpeningMade,
}

impl Opening {
    /// A void opening `width` wide, centred on segment `seg` of `wall`.
    pub fn new(wall: WallId, seg: u16, width: f64) -> Self {
        Self {
            wall,
            seg,
            at: OpeningAt::Centred,
            width,
            width_expr: None,
            leaf: width,
            height: 0.0,
            symbol: Symbol::Void,
            hand: Hand::Left,
            facing: WallFace::Left,
            layer: None,
            made: OpeningMade::default(),
        }
    }

    /// Where its two jambs are along a segment `len` long.
    pub fn span_on(&self, len: f64) -> (f64, f64) {
        let u0 = self.at.start_on(len, self.width);
        (u0, u0 + self.width)
    }

    /// The four ways it can open, in the order Space steps through them: (hand, facing).
    pub const SWINGS: [(Hand, WallFace); 4] = [
        (Hand::Right, WallFace::Right),
        (Hand::Left, WallFace::Right),
        (Hand::Left, WallFace::Left),
        (Hand::Right, WallFace::Left),
    ];

    /// The next way round (Space while placing).
    pub fn next_swing(hand: Hand, facing: WallFace) -> (Hand, WallFace) {
        let i = Self::SWINGS
            .iter()
            .position(|s| *s == (hand, facing))
            .unwrap_or(0);
        Self::SWINGS[(i + 1) % Self::SWINGS.len()]
    }
}

/// The id of a size set (`us`, `metric`, `din-18101`): a short ASCII name, kept inline so
/// the drawing's rules stay `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SizeSetId {
    len: u8,
    bytes: [u8; SizeSetId::MAX],
}

impl SizeSetId {
    /// The longest id there can be.
    pub const MAX: usize = 23;

    /// None if `id` is empty, too long or not plain ASCII.
    pub fn new(id: &str) -> Option<Self> {
        if id.is_empty() || id.len() > Self::MAX || !id.bytes().all(|b| b.is_ascii_graphic()) {
            return None;
        }
        let mut bytes = [0; Self::MAX];
        bytes[..id.len()].copy_from_slice(id.as_bytes());
        Some(Self {
            len: id.len() as u8,
            bytes,
        })
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

impl std::fmt::Debug for SizeSetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SizeSetId({:?})", self.as_str())
    }
}

impl std::fmt::Display for SizeSetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_an_opening_sits() {
        let mut o = Opening::new(WallId(1), 0, 900.0);
        assert_eq!(o.span_on(3000.0), (1050.0, 1950.0));
        o.at = OpeningAt::from_start(100.0);
        assert_eq!(o.span_on(3000.0), (100.0, 1000.0));
        o.at = OpeningAt::From {
            end: SegEnd::End,
            offset: 100.0,
            expr: None,
        };
        assert_eq!(o.span_on(3000.0), (2000.0, 2900.0));
    }

    #[test]
    fn swings_cycle_through_all_four() {
        let mut s = Opening::SWINGS[0];
        let mut seen = vec![s];
        for _ in 0..3 {
            s = Opening::next_swing(s.0, s.1);
            seen.push(s);
        }
        assert_eq!(seen, Opening::SWINGS);
        assert_eq!(Opening::next_swing(s.0, s.1), Opening::SWINGS[0]);
    }

    #[test]
    fn size_set_ids_are_short_names() {
        assert_eq!(SizeSetId::new("din-18101").unwrap().as_str(), "din-18101");
        assert!(SizeSetId::new("").is_none());
        assert!(SizeSetId::new("a very long name for a set").is_none());
        assert!(SizeSetId::new("us ").is_none());
    }
}
