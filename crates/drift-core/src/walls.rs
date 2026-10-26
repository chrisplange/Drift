//! Walls: a run of wall drawn along a centreline (a [`Path`](crate::Path) of construction
//! curves), with a thickness, a justification and a poché. Its faces, caps and the corners
//! between them are ordinary lines, arcs and points that the wall owns: they are generated
//! from the centreline and the walls it joins (`drift_geom::walls`), and recorded here by key
//! ([`Made`]), so regenerating them updates the same entities instead of making new ones.
//!
//! What each key names is described in `docs/walls-design.md` (§2.3). In short: a face piece
//! is named by its centreline segment, which face (or end edge) it is, and the
//! [`Station`] that starts it, so adding a T or a crossing part-way along a face changes only
//! the piece it lands in.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::document::EntityId;
use crate::drafting::{FillId, FillStyle};
use crate::layers::LayerId;
use crate::paths::PathId;

/// Stable identifier of a wall, "w3".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WallId(pub u32);

impl std::fmt::Display for WallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "w{}", self.0)
    }
}

/// Where a wall lies against its centreline, looking along it: centred on it, or with the
/// centreline as its left or right face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Justify {
    #[default]
    Centre,
    Left,
    Right,
}

impl Justify {
    /// In the order the tool bar shows them.
    pub const ALL: [Justify; 3] = [Justify::Left, Justify::Centre, Justify::Right];

    pub fn label(self) -> &'static str {
        match self {
            Justify::Centre => "Centre",
            Justify::Left => "Left face",
            Justify::Right => "Right face",
        }
    }

    /// The other face (Space while drawing); a centred wall stays centred.
    pub fn flipped(self) -> Self {
        match self {
            Justify::Left => Justify::Right,
            Justify::Right => Justify::Left,
            Justify::Centre => Justify::Centre,
        }
    }

    /// How far each face lies to the left of the centreline: (left face, right face).
    pub fn offsets(self, thickness: f64) -> (f64, f64) {
        match self {
            Justify::Centre => (thickness / 2.0, -thickness / 2.0),
            // The centreline is the left face: the wall extends to its right.
            Justify::Left => (0.0, -thickness),
            Justify::Right => (thickness, 0.0),
        }
    }
}

/// One end of an open wall: how it meets what is there.
#[derive(Clone, Debug, PartialEq)]
pub struct WallEnd {
    /// Clean up with walls that meet this end ("Don't join" in the Inspector turns it off).
    pub join: bool,
    /// This end sits on another wall's centreline (a T): which wall, and which of its
    /// segments. An L or n-way join needs no record: the walls share the centreline point.
    pub tee: Option<(WallId, u16)>,
    /// This end stops against a curve that isn't a wall (a column, a slab edge, a grid
    /// line): both faces end where they cross it.
    pub against: Option<EntityId>,
}

impl Default for WallEnd {
    fn default() -> Self {
        Self {
            join: true,
            tee: None,
            against: None,
        }
    }
}

/// A run of wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    /// The centreline: construction curves in order (lines and arcs), open or closed. A
    /// closed path is a ring of walls (a room drawn in one go).
    pub centre: PathId,
    /// Thickness in mm, and the expression it comes from, if any ("t_ext", "W / 20").
    pub thickness: f64,
    pub thickness_expr: Option<String>,
    pub justify: Justify,
    /// None: faces only, no fill.
    pub poche: Option<FillStyle>,
    pub layer: LayerId,
    /// Where the wall's cluster fill stacks on its layer (as `Fill::order`).
    pub order: Option<i32>,
    /// The start and end of an open centreline (unused when it is closed).
    pub ends: [WallEnd; 2],
    /// Clean up with walls of its cluster that it crosses mid-segment (X joins).
    pub cross: bool,
    /// What the wall made, by key.
    pub made: Made,
}

impl Wall {
    /// A wall along `centre`, `thickness` thick, centred, with a solid poché, joining at
    /// both ends and where it crosses walls.
    pub fn new(centre: PathId, thickness: f64, layer: LayerId) -> Self {
        Self {
            centre,
            thickness,
            thickness_expr: None,
            justify: Justify::Centre,
            poche: Some(FillStyle::solid()),
            layer,
            order: None,
            ends: [WallEnd::default(), WallEnd::default()],
            cross: true,
            made: Made::default(),
        }
    }

    /// How far each face lies to the left of the centreline: (left face, right face).
    pub fn offsets(&self) -> (f64, f64) {
        self.justify.offsets(self.thickness)
    }
}

/// What a wall generated, by key: face pieces and end edges, the points between them, and
/// (for the lead wall of a cluster) the cluster's outline paths and poché fills.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Made {
    pub curves: BTreeMap<FaceKey, EntityId>,
    pub points: BTreeMap<CornerKey, EntityId>,
    /// The outline loops of the clusters this wall leads.
    pub outlines: BTreeMap<LoopKey, PathId>,
    /// Their poché, one fill per outer loop.
    pub fills: BTreeMap<LoopKey, FillId>,
}

impl Made {
    /// Every entity it holds: curves, then points.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.curves.values().chain(self.points.values()).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
            && self.points.is_empty()
            && self.outlines.is_empty()
            && self.fills.is_empty()
    }
}

/// Which way an end leaves its node along its segment: forward (a segment's start, or the
/// piece after a station) or back (its end, or the piece before a station).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dir {
    Fwd,
    Back,
}

/// What starts a face piece (or where an end is) along a segment, so pieces keep their keys
/// when others are added before them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Station {
    /// The segment's start.
    SegStart,
    /// The segment's end.
    SegEnd,
    /// Where that wall's end (0: its start, 1: its end) meets this segment in a T.
    Tee(WallId, u8),
    /// Where this segment crosses that wall's segment (the crossing's number among theirs,
    /// in case an arc crosses a segment twice).
    Cross(WallId, u16, u8),
}

/// An end at a node: a wall's segment at a station, leaving it one way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EndKey {
    pub wall: WallId,
    pub seg: u16,
    pub at: Station,
    pub dir: Dir,
}

/// Which side of an end a seam (a line between materials at a node) is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Turn {
    Ccw,
    Cw,
}

/// What a generated curve is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// A piece of the left face (looking along the centreline).
    Left,
    Right,
    /// The edge across a free end.
    Cap(Dir),
    /// The edge where an end stops on something that draws the line itself (a thicker wall
    /// running through, or the curve it is against): never stroked.
    Butt(Dir),
    /// The short edge between two walls running straight on at different thicknesses.
    Step(Dir),
    /// The edge across a corner cut at the mitre limit.
    Bevel(Dir),
    /// The line between two materials meeting at a node.
    Seam(Dir, Turn),
}

/// A generated curve: on which centreline segment, what it is, and the station that starts
/// it (a face piece's start, or the station of the end an end edge belongs to).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceKey {
    pub seg: u16,
    pub part: Part,
    pub from: Station,
}

/// A generated point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CornerKey {
    /// Where end `a`'s counter-clockwise face meets end `b`'s clockwise face at a node (`b`
    /// being the next end counter-clockwise from `a`). Owned by the node's lowest wall.
    Pair(EndKey, EndKey),
    /// The second point of a step or a bevel between them (on `b`'s face).
    PairB(EndKey, EndKey),
    /// Where an end's counter-clockwise (`true`) or clockwise face stops on its own: at a
    /// cap, a butt, or the curve it is against.
    Own(EndKey, bool),
    /// The node itself, where seams between several materials meet (named by its lowest
    /// end).
    Node(EndKey),
    /// The centre of an arc piece of a wall's.
    Centre(WallId, FaceKey),
}

/// An outline loop of a cluster: its outer loop's number, and 0 for the outer loop itself
/// or the number of one of its holes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoopKey {
    pub outer: u16,
    pub hole: u16,
}

/// What owns an entity that belongs to a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Owner {
    /// Generated by the wall (a face, end edge or corner).
    Wall(WallId),
    /// A curve or point of its centreline.
    Centre(WallId),
}

impl Owner {
    pub fn wall(self) -> WallId {
        match self {
            Owner::Wall(w) | Owner::Centre(w) => w,
        }
    }
}

/// Who owns which entity, for the walls of a document (see
/// [`Document::wall_owners`](crate::Document::wall_owners)).
#[derive(Clone, Debug, Default)]
pub struct Owners {
    pub of: HashMap<EntityId, Owner>,
    /// The paths and fills walls made (their outlines and poché).
    pub paths: HashMap<PathId, WallId>,
    pub fills: HashMap<FillId, WallId>,
    /// Each wall's centreline path.
    pub centres: HashMap<PathId, WallId>,
}

impl Owners {
    pub fn get(&self, id: EntityId) -> Option<Owner> {
        self.of.get(&id).copied()
    }

    /// The wall an entity belongs to, generated or centreline.
    pub fn wall(&self, id: EntityId) -> Option<WallId> {
        self.get(id).map(Owner::wall)
    }

    /// Whether the entity was generated by a wall (never a solver unknown).
    pub fn generated(&self, id: EntityId) -> bool {
        matches!(self.get(id), Some(Owner::Wall(_)))
    }

    /// The generated entities.
    pub fn generated_set(&self) -> HashSet<EntityId> {
        self.of
            .iter()
            .filter(|(_, o)| matches!(o, Owner::Wall(_)))
            .map(|(id, _)| *id)
            .collect()
    }
}

/// Stable identifier of a property dimension, "q2".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PropertyDimId(pub u32);

impl std::fmt::Display for PropertyDimId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "q{}", self.0)
    }
}

/// The property a property dimension shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PropertyRef {
    /// A wall's thickness.
    WallThickness(WallId),
}

impl PropertyRef {
    pub fn wall(self) -> WallId {
        match self {
            PropertyRef::WallThickness(w) => w,
        }
    }
}

/// A dimension that shows a property of a wall (its thickness) and writes it when typed
/// into. The solver never sees it: it edits the property, so it never conflicts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropertyDim {
    pub target: PropertyRef,
    /// The centreline segment it is drawn across.
    pub seg: u16,
    /// Where along the segment, as a fraction of its length.
    pub along: f64,
    /// How far its label sits from the centreline, to the left (negative: the right).
    pub out: f64,
}
