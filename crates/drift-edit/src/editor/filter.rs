//! What the select tool may pick, and the dimension tool's mode.

use super::*;

/// A kind of thing the select tool picks, turned on and off on its own (as AutoCAD's, Rhino's
/// and Vectorworks' selection filters are).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    /// End points, centres and standalone points.
    Points,
    /// Lines, arcs, circles, ellipses and Béziers.
    Edges,
    /// Solid fills and hatches.
    Fills,
    /// Empty closed areas (to measure or fill).
    Areas,
    /// Copies of blocks, and groups (a group is always picked whole; this lets one be
    /// picked with Edges off).
    Blocks,
    /// Sketch text.
    Text,
    /// Dimensions, notes, callouts and marks.
    Annotations,
}

impl PickKind {
    pub const ALL: [PickKind; 7] = [
        PickKind::Points,
        PickKind::Edges,
        PickKind::Fills,
        PickKind::Areas,
        PickKind::Blocks,
        PickKind::Text,
        PickKind::Annotations,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PickKind::Points => "Points",
            PickKind::Edges => "Edges",
            PickKind::Fills => "Fills",
            PickKind::Areas => "Areas",
            PickKind::Blocks => "Blocks & groups",
            PickKind::Text => "Text",
            PickKind::Annotations => "Annotations",
        }
    }

    /// What it picks, in a few words.
    pub fn picks(self) -> &'static str {
        match self {
            PickKind::Points => "end points, centres and standalone points",
            PickKind::Edges => "lines, arcs, circles, ellipses and curves",
            PickKind::Fills => "solid fills and hatches",
            PickKind::Areas => "empty closed areas (to measure or fill)",
            PickKind::Blocks => "copies of blocks, and groups (even with Edges off)",
            PickKind::Text => "sketch text",
            PickKind::Annotations => "dimensions, notes, callouts and marks",
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// What the select tool picks: any mix of [`PickKind`]s, all of them by default. It applies
/// to clicks, box selection and Select all. A view setting: it isn't saved in the drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PickFilter(u8);

impl Default for PickFilter {
    fn default() -> Self {
        Self::ALL
    }
}

impl PickFilter {
    /// Everything: no filter.
    pub const ALL: PickFilter = PickFilter(0x7f);

    /// Only `kind`.
    pub const fn only(kind: PickKind) -> Self {
        Self(kind.bit())
    }

    pub fn has(self, kind: PickKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// With `kind` turned on or off.
    pub fn with(self, kind: PickKind, on: bool) -> Self {
        if on {
            Self(self.0 | kind.bit())
        } else {
            Self(self.0 & !kind.bit())
        }
    }

    /// Every kind on: no filter.
    pub fn is_all(self) -> bool {
        self == Self::ALL
    }

    /// The kinds that are on.
    pub fn kinds(self) -> impl Iterator<Item = PickKind> {
        PickKind::ALL.into_iter().filter(move |k| self.has(*k))
    }

    /// Whether an entity may be picked: a point with Points, a curve with Edges (or, in a
    /// group, with Blocks & groups: the group is picked as a whole).
    pub fn allows(self, doc: &drift_core::Document, id: EntityId) -> bool {
        match doc.get(id) {
            Some(e) if e.kind.is_point() => self.has(PickKind::Points),
            Some(_) => {
                self.has(PickKind::Edges)
                    || (self.has(PickKind::Blocks) && doc.group_of(id).is_some())
            }
            None => false,
        }
    }

    /// Whether a line's midpoint may be picked (it stands for the line and its middle).
    pub fn midpoints(self) -> bool {
        self.has(PickKind::Points) && self.has(PickKind::Edges)
    }

    /// Whether geometry (and the relations on it) may be picked.
    pub fn geometry(self) -> bool {
        self.has(PickKind::Points) || self.has(PickKind::Edges)
    }

    pub fn fills(self) -> bool {
        self.has(PickKind::Fills)
    }

    pub fn areas(self) -> bool {
        self.has(PickKind::Areas)
    }

    /// Whether copies of blocks and whole groups may be picked.
    pub fn blocks(self) -> bool {
        self.has(PickKind::Blocks)
    }

    /// Whether dimensions and annotations may be picked.
    pub fn annotations(self) -> bool {
        self.has(PickKind::Annotations)
    }

    /// Whether sketch text may be picked.
    pub fn texts(self) -> bool {
        self.has(PickKind::Text)
    }
}

/// How the dimension tool goes on after placing a horizontal or vertical dimension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DimMode {
    /// One at a time (Shift+click still continues a row).
    #[default]
    Single,
    /// Each click adds the next dimension along the row, end to end.
    Chain,
    /// Each click adds a dimension from the same base, each a step further out.
    Baseline,
}

impl DimMode {
    pub const ALL: [DimMode; 3] = [DimMode::Single, DimMode::Chain, DimMode::Baseline];

    pub fn label(self) -> &'static str {
        match self {
            DimMode::Single => "Single",
            DimMode::Chain => "Chain",
            DimMode::Baseline => "Baseline",
        }
    }
}
