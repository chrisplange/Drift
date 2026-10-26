//! Walls' geometry: the faces, end edges and corners of a network of walls, and the outline
//! loops of each cluster of joined walls (`docs/walls-design.md` §3).
//!
//! Every join is a **node**, a point where wall ends meet: walls sharing a centreline point
//! (L, Y, n-way), a wall's end on another's centreline (T: the host's segment is split there
//! into two virtual ends), and two walls crossing mid-segment (X: four virtual ends). At a
//! node the ends are sorted by the direction they leave it in, and the corner between
//! consecutive ends `i` and `j` (counter-clockwise from `i`) is where `i`'s
//! counter-clockwise face crosses `j`'s clockwise face. A face piece runs from the corner
//! that starts it to the next one that stops it, so the host of a T is split only in the
//! derived geometry, and each corner is computed once and shared by the faces it ends:
//! outlines are chained by corner identity, with no tolerance.
//!
//! [`generate`] is a pure function of the walls' centrelines and settings; [`regen`] writes
//! what it makes into a document, by key, so that regenerating updates the same entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::f64::consts::{PI, TAU};

use drift_core::{
    CornerKey, DVec2, Dir, EndKey, EntityId, FaceKey, LoopKey, Part, Shape, Station, Turn, WallId,
};

use crate::regions::{Edge, Loop, Region};

pub mod from_lines;
mod frozen;
pub mod lower;
pub mod openings;
pub mod regen;
pub use frozen::{Joins, infer_joins};
pub use lower::lower;
pub use openings::{Frame, OpeningIn, free_spans, opening_inputs, write_symbol};
pub use regen::{
    Report, regenerate, regenerate_moved, regenerate_moved_with, regenerate_with, wall_inputs,
};
#[cfg(test)]
mod openings_tests;
#[cfg(test)]
mod tests;

/// A corner further from its node than this many times the thicker wall is cut square
/// (a bevel), as SVG's default mitre limit.
pub const MITRE_LIMIT: f64 = 4.0;

/// Two ends leaving a node this close to opposite directions (radians) run straight on.
pub const STRAIGHT_ON: f64 = 1e-6;

/// A segment of a centreline, in the direction the wall runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// From `a` to `b` round `center`, counter-clockwise if `ccw`.
    Arc {
        center: DVec2,
        radius: f64,
        a: DVec2,
        b: DVec2,
        ccw: bool,
    },
}

impl Seg {
    pub fn start(&self) -> DVec2 {
        match *self {
            Seg::Line { a, .. } | Seg::Arc { a, .. } => a,
        }
    }

    pub fn end(&self) -> DVec2 {
        match *self {
            Seg::Line { b, .. } | Seg::Arc { b, .. } => b,
        }
    }

    /// The arc through three points, from `a` to `b` through `through` (None if they are in
    /// a line).
    pub fn arc_through(a: DVec2, through: DVec2, b: DVec2) -> Option<Seg> {
        let (center, radius) = circle_through(a, through, b)?;
        let ccw = (through - a).perp_dot(b - through) > 0.0;
        Some(Seg::Arc {
            center,
            radius,
            a,
            b,
            ccw,
        })
    }

    /// How far the segment turns from `a` to `b` (radians, positive): 0 for a line.
    fn sweep(&self) -> f64 {
        match *self {
            Seg::Line { .. } => 0.0,
            Seg::Arc {
                center, a, b, ccw, ..
            } => {
                let (sa, sb) = (angle(a - center), angle(b - center));
                let mut s = if ccw { sb - sa } else { sa - sb }.rem_euclid(TAU);
                if s <= 0.0 {
                    s = TAU;
                }
                s
            }
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Seg::Line { a, b } => a.distance(b),
            Seg::Arc { radius, .. } => radius * self.sweep(),
        }
    }

    /// The point `u` along it (beyond its ends too).
    pub fn at(&self, u: f64) -> DVec2 {
        match *self {
            Seg::Line { a, b } => a + (b - a).normalize_or_zero() * u,
            Seg::Arc {
                center,
                radius,
                a,
                ccw,
                ..
            } => {
                let t = angle(a - center) + if ccw { u / radius } else { -u / radius };
                center + radius * DVec2::from_angle(t)
            }
        }
    }

    /// The direction it runs in `u` along it.
    pub fn tangent(&self, u: f64) -> DVec2 {
        match *self {
            Seg::Line { a, b } => (b - a).normalize_or_zero(),
            Seg::Arc { center, ccw, .. } => {
                let r = (self.at(u) - center).normalize_or_zero().perp();
                if ccw { r } else { -r }
            }
        }
    }

    /// How far along it `p` is (projected onto it: beyond its ends, below 0 or above its
    /// length; round an arc, within half a turn of its middle).
    pub fn param(&self, p: DVec2) -> f64 {
        match *self {
            Seg::Line { a, b } => (p - a).dot((b - a).normalize_or_zero()),
            Seg::Arc {
                center,
                radius,
                a,
                ccw,
                ..
            } => {
                let sweep = self.sweep();
                let from = angle(a - center);
                let to = angle(p - center);
                let mut t = if ccw { to - from } else { from - to };
                // Within half a turn of the middle.
                t = (t - sweep / 2.0 + PI).rem_euclid(TAU) - PI + sweep / 2.0;
                t * radius
            }
        }
    }

    /// The face `o` to its left (negative: its right), as the whole line or circle it lies
    /// on; None for an arc too tight for it.
    fn face(&self, o: f64, tol: f64) -> Option<Curve> {
        match *self {
            Seg::Line { a, b } => {
                let d = (b - a).try_normalize()?;
                Some(Curve::Line {
                    p: a + d.perp() * o,
                    d,
                })
            }
            Seg::Arc {
                center,
                radius,
                ccw,
                ..
            } => {
                // Turning left (counter-clockwise), the left is towards the centre.
                let r = if ccw { radius - o } else { radius + o };
                (r > tol).then_some(Curve::Circle { c: center, r })
            }
        }
    }

    /// As a shape (for crossings and drawing).
    pub fn shape(&self) -> Shape {
        match *self {
            Seg::Line { a, b } => Shape::Line { a, b },
            Seg::Arc {
                center,
                radius,
                a,
                b,
                ccw,
            } => {
                let (s, e) = if ccw { (a, b) } else { (b, a) };
                let start = angle(s - center);
                let mut sweep = (angle(e - center) - start).rem_euclid(TAU);
                if sweep <= 0.0 {
                    sweep = TAU;
                }
                Shape::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                }
            }
        }
    }

    pub fn bounds(&self) -> drift_core::Bounds {
        self.shape().bounds()
    }
}

fn angle(v: DVec2) -> f64 {
    v.y.atan2(v.x)
}

/// The circle through three points: centre and radius.
pub fn circle_through(a: DVec2, b: DVec2, c: DVec2) -> Option<(DVec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 * (1.0 + a.length_squared() + b.length_squared() + c.length_squared()) {
        return None;
    }
    let (a2, b2, c2) = (a.length_squared(), b.length_squared(), c.length_squared());
    let center = DVec2::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    );
    Some((center, center.distance(a)))
}

/// A face as the whole line or circle it lies on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Curve {
    Line { p: DVec2, d: DVec2 },
    Circle { c: DVec2, r: f64 },
}

impl Curve {
    /// Where two faces cross (none for parallel lines or concentric circles).
    fn cross(&self, other: &Curve) -> Vec<DVec2> {
        match (*self, *other) {
            (Curve::Line { p, d }, Curve::Line { p: q, d: e }) => {
                let den = d.perp_dot(e);
                if den.abs() <= 1e-12 {
                    return Vec::new();
                }
                vec![p + d * ((q - p).perp_dot(e) / den)]
            }
            (Curve::Line { p, d }, Curve::Circle { c, r })
            | (Curve::Circle { c, r }, Curve::Line { p, d }) => line_circle(p, d, c, r),
            (Curve::Circle { c: c1, r: r1 }, Curve::Circle { c: c2, r: r2 }) => {
                let dist = c1.distance(c2);
                if dist <= 1e-12 * (1.0 + r1 + r2) || dist > r1 + r2 || dist < (r1 - r2).abs() {
                    return Vec::new();
                }
                let x = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let u = (c2 - c1) / dist;
                let m = c1 + u * x;
                vec![m + u.perp() * h, m - u.perp() * h]
            }
        }
    }

    /// The whole line or circle as a shape (a line as a long segment through `near`).
    fn shape(&self, near: DVec2, reach: f64) -> Shape {
        match *self {
            Curve::Line { p, d } => {
                let m = p + d * (near - p).dot(d);
                Shape::Line {
                    a: m - d * reach,
                    b: m + d * reach,
                }
            }
            Curve::Circle { c, r } => Shape::Circle {
                center: c,
                radius: r,
            },
        }
    }
}

/// Where the line through `p` along unit `d` crosses the circle.
fn line_circle(p: DVec2, d: DVec2, c: DVec2, r: f64) -> Vec<DVec2> {
    let f = p - c;
    let b = f.dot(d);
    let cc = f.dot(f) - r * r;
    let disc = b * b - cc;
    if disc < 0.0 {
        return Vec::new();
    }
    let s = disc.sqrt();
    vec![p + d * (-b - s), p + d * (-b + s)]
}

fn nearest(points: Vec<DVec2>, to: DVec2) -> Option<DVec2> {
    points
        .into_iter()
        .filter(|p| p.is_finite())
        .min_by(|a, b| a.distance(to).total_cmp(&b.distance(to)))
}

/// One end of an open wall, as the generator sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct EndIn {
    pub join: bool,
    pub tee: Option<(WallId, u16)>,
    /// The curve it stops against, if any.
    pub against: Option<Shape>,
}

impl Default for EndIn {
    fn default() -> Self {
        Self {
            join: true,
            tee: None,
            against: None,
        }
    }
}

/// A wall, as the generator sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct WallIn {
    pub id: WallId,
    pub segs: Vec<Seg>,
    /// The centreline's points: `segs[k]` runs from `points[k]` to `points[k + 1]` (round
    /// to `points[0]` for a closed run). Walls sharing a point join there.
    pub points: Vec<EntityId>,
    pub closed: bool,
    /// How far its left and right faces lie to the left of the centreline.
    pub offsets: (f64, f64),
    /// Walls of the same cluster (poché and layer) merge into one area.
    pub cluster: u32,
    pub cross: bool,
    pub ends: [EndIn; 2],
}

impl WallIn {
    fn thickness(&self) -> f64 {
        (self.offsets.0 - self.offsets.1).abs()
    }

    fn point(&self, i: usize) -> EntityId {
        self.points[i % self.points.len()]
    }
}

/// A generated curve: from one corner to another, in the direction that keeps the wall on
/// its left (so outer loops run counter-clockwise and holes clockwise).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub from: CornerKey,
    pub to: CornerKey,
    /// An arc: its centre's key, and whether it runs counter-clockwise from `from` to `to`.
    pub arc: Option<(CornerKey, bool)>,
    /// Drawn (false for an edge another line already draws).
    pub stroke: bool,
    /// Its wall's cluster.
    pub cluster: u32,
}

/// An outline loop of a cluster, in order, the wall on its left.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopOut {
    pub cluster: u32,
    /// The lowest wall of its region (outer loop and holes), which keeps its paths and fill.
    pub lead: WallId,
    pub key: LoopKey,
    pub edges: Vec<(WallId, FaceKey)>,
    /// A point inside the region's material (for an outer loop: its fill's seed).
    pub seed: DVec2,
}

/// What the walls make.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Generated {
    /// Every corner, with the wall that owns it.
    pub points: BTreeMap<CornerKey, (WallId, DVec2)>,
    pub curves: BTreeMap<(WallId, FaceKey), Piece>,
    pub loops: Vec<LoopOut>,
    /// Each free end of an open run (one that joins what it lands on): the wall and segment
    /// it tees into, if it does.
    pub tees: BTreeMap<(WallId, u8), Option<(WallId, u16)>>,
    /// Each end of an open run that joins something: the walls it meets there (its own,
    /// where a run closes on itself).
    pub joined: BTreeMap<(WallId, u8), BTreeSet<WallId>>,
    /// End points joined to others by where they are, with no shared point (an L found by
    /// position): each to the point that names its join.
    pub aliased: BTreeMap<EntityId, EntityId>,
    /// Why something isn't as asked (a wall too tight to make, an outline that doesn't
    /// close, an end that had to be capped).
    pub notices: Vec<String>,
    /// Openings that don't fit where they are, so aren't cut.
    pub unfit: BTreeSet<drift_core::OpeningId>,
}

impl Generated {
    pub fn point(&self, key: CornerKey) -> Option<DVec2> {
        self.points.get(&key).map(|(_, p)| *p)
    }

    /// A curve as a shape.
    pub fn shape(&self, piece: &Piece) -> Option<Shape> {
        let (a, b) = (self.point(piece.from)?, self.point(piece.to)?);
        Some(match piece.arc {
            None => Shape::Line { a, b },
            Some((c, ccw)) => {
                let c = self.point(c)?;
                drift_core::arc_shape(c, if ccw { a } else { b }, if ccw { b } else { a })
            }
        })
    }

    /// A curve as a region edge, in its direction.
    fn edge(&self, piece: &Piece) -> Option<Edge> {
        let (a, b) = (self.point(piece.from)?, self.point(piece.to)?);
        Some(match piece.arc {
            None => Edge::Line { a, b },
            Some((c, ccw)) => {
                let c = self.point(c)?;
                let start = angle(a - c);
                let mut sweep = (angle(b - c) - start).rem_euclid(TAU);
                if sweep <= 0.0 {
                    sweep = TAU;
                }
                Edge::Arc {
                    center: c,
                    radius: a.distance(c),
                    start,
                    sweep: if ccw { sweep } else { sweep - TAU },
                }
            }
        })
    }

    /// The loop of edges an outline is (for its region).
    pub fn loop_of(&self, out: &LoopOut) -> Option<Loop> {
        let edges: Vec<Edge> = out
            .edges
            .iter()
            .map(|k| self.edge(self.curves.get(k)?))
            .collect::<Option<_>>()?;
        Some(Loop {
            sources: (0..edges.len()).collect(),
            edges,
        })
    }

    /// The region of each outer loop, with its holes (keyed by lead and loop).
    pub fn regions(&self) -> Vec<(WallId, LoopKey, Region)> {
        let mut out = Vec::new();
        for o in self.loops.iter().filter(|l| l.key.hole == 0) {
            let Some(outer) = self.loop_of(o) else {
                continue;
            };
            let holes: Vec<Loop> = self
                .loops
                .iter()
                .filter(|h| h.lead == o.lead && h.key.outer == o.key.outer && h.key.hole > 0)
                .filter_map(|h| self.loop_of(h))
                .collect();
            out.push((o.lead, o.key, Region::from_loops(outer, holes)));
        }
        out
    }

    /// Everything as shapes, for a preview.
    pub fn shapes(&self) -> Vec<Shape> {
        self.curves
            .values()
            .filter(|p| p.stroke)
            .filter_map(|p| self.shape(p))
            .collect()
    }
}

/// Which face of a wall.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Side {
    Left,
    Right,
}

/// A face of a wall's segment: (wall index, segment, side).
type FaceRef = (usize, usize, Side);

/// An end at a node.
#[derive(Clone, Debug)]
struct End {
    key: EndKey,
    w: usize,
    seg: usize,
    /// Where along the segment the station is.
    u: f64,
    /// The direction it leaves the node in.
    d: DVec2,
    /// Its counter-clockwise and clockwise faces, with how far each lies to the left of `d`.
    ccw: (Side, f64),
    cw: (Side, f64),
    cluster: u32,
    thick: f64,
    /// For the two virtual ends of a station (a T or an X), its number: a face passes
    /// straight through between them.
    pass: Option<usize>,
    against: Option<Shape>,
}

impl End {
    fn wall(&self) -> WallId {
        self.key.wall
    }
}

/// Where a node is and the ends that meet there.
#[derive(Clone, Debug, Default)]
struct Node {
    at: DVec2,
    ends: Vec<End>,
}

/// A stop or start of a face piece.
#[derive(Clone, Copy, Debug)]
struct Event {
    /// The station's place along the segment, and 0 for a stop, 1 for a start (so a stop
    /// at a station comes before the start after it).
    order: (f64, u8),
    station: Station,
    start: bool,
    corner: CornerKey,
}

/// A generated curve before chaining.
#[derive(Clone, Copy, Debug)]
struct EdgeOut {
    owner: WallId,
    key: FaceKey,
    piece: Piece,
}

/// A stop or start on a station of a segment: where it is, what it is and its node.
#[derive(Clone, Copy, Debug)]
struct StationAt {
    u: f64,
    station: Station,
    node: usize,
    pass: usize,
}

/// A station found on a segment: where along it, what it is, the end teed there (wall
/// and which end), and for a crossing its node.
type StationIn = (f64, Station, Option<(usize, usize)>, Option<NodeId>);

/// Where a wall end sits on a host's segment (a T).
struct TeeAt {
    host: usize,
    seg: usize,
    u: f64,
}

/// The node an end belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum NodeId {
    Point(EntityId),
    Station(usize, usize, usize),
    Cross(usize, usize, usize, usize, u8),
    Alone(EndKey),
}

struct Gen<'a> {
    walls: &'a [WallIn],
    /// Each wall's faces, per segment: (left, right). None for a wall that can't be made.
    faces: Vec<Option<Vec<(Curve, Curve)>>>,
    /// Same-place tolerance.
    eps: f64,
    /// Snap tolerance (tees and crossings).
    snap: f64,
    out: Generated,
    events: BTreeMap<FaceRef, Vec<Event>>,
    edges: Vec<EdgeOut>,
    /// The faces (or end edges) each corner ends.
    corner_faces: HashMap<CornerKey, Vec<FaceRef>>,
    /// Corners merged into others (a short segment's, an empty edge's).
    alias: HashMap<CornerKey, CornerKey>,
    /// Where nodes are wanted, when not everywhere: segments clear of it (by more than the
    /// thickest wall and the snap) are left out of [`Gen::nodes`]. Every node inside it is
    /// whole; those outside may not be.
    focus: Option<drift_core::Bounds>,
    /// The openings cut into the walls.
    openings: Vec<OpeningIn>,
    /// Joins kept from before (a drag's), for the ends that aren't live (see [`Joins`]).
    frozen: Option<&'a Joins>,
}

/// Make the faces, end edges and corners of `walls`, and the outline loops of their
/// clusters. `snap` is how close a wall end must be to another wall to join it in a T, and
/// how far inside a segment a crossing must be to make an X.
pub fn generate(walls: &[WallIn], snap: f64) -> Generated {
    generate_all(walls, &[], snap, None)
}

/// [`generate`], keeping `joins` (a drag's) for the ends that aren't live.
pub fn generate_with(walls: &[WallIn], snap: f64, joins: Option<&Joins>) -> Generated {
    generate_all(walls, &[], snap, joins)
}

/// [`generate`], with `openings` cut into the walls (see [`openings`]).
pub fn generate_cut(walls: &[WallIn], openings: &[OpeningIn], snap: f64) -> Generated {
    generate_all(walls, openings, snap, None)
}

/// [`generate`], with `openings` cut into the walls and keeping `joins` (a drag's) for the
/// ends that aren't live.
pub fn generate_all(
    walls: &[WallIn],
    openings: &[OpeningIn],
    snap: f64,
    joins: Option<&Joins>,
) -> Generated {
    let mut g = Gen::new(walls, snap);
    g.openings = openings.to_vec();
    g.frozen = joins;
    g.make_faces();
    let nodes = g.nodes();
    for node in &nodes {
        g.solve(node);
    }
    g.check_openings();
    g.pieces();
    g.drop_empty_edges();
    g.chain();
    g.out
}

impl<'a> Gen<'a> {
    fn new(walls: &'a [WallIn], snap: f64) -> Self {
        let extent = walls
            .iter()
            .flat_map(|w| w.segs.iter())
            .flat_map(|s| [s.start(), s.end()])
            .map(|p| p.abs().max_element())
            .fold(1.0, f64::max);
        let eps = 1e-9 * extent;
        Gen {
            walls,
            faces: Vec::new(),
            eps,
            snap: snap.max(eps),
            out: Generated::default(),
            events: BTreeMap::new(),
            edges: Vec::new(),
            corner_faces: HashMap::new(),
            alias: HashMap::new(),
            focus: None,
            openings: Vec::new(),
            frozen: None,
        }
    }

    /// Look for nodes only round these segments (see `focus`).
    fn focus_on(&mut self, segs: impl IntoIterator<Item = (usize, usize)>) {
        let margin = self.walls.iter().map(WallIn::thickness).fold(0.0, f64::max) + self.snap;
        let mut b: Option<drift_core::Bounds> = None;
        for (w, k) in segs {
            let s = self.walls[w].segs[k].bounds();
            b = Some(b.map_or(s, |b| b.union(s)));
        }
        self.focus = b.map(|b| drift_core::Bounds {
            min: b.min - DVec2::splat(margin),
            max: b.max + DVec2::splat(margin),
        });
    }

    /// Whether segment `k` of wall `w` is near the focus (or there is none).
    fn near(&self, w: usize, k: usize) -> bool {
        let Some(f) = self.focus else {
            return true;
        };
        let b = self.walls[w].segs[k].bounds();
        b.min.x <= f.max.x && b.max.x >= f.min.x && b.min.y <= f.max.y && b.max.y >= f.min.y
    }
}

impl Gen<'_> {
    /// Each wall's faces; a wall with an arc too tight for its thickness, or with no
    /// segments, makes nothing (and says so).
    fn make_faces(&mut self) {
        for w in self.walls {
            let ok = !w.segs.is_empty()
                && w.points.len()
                    == if w.closed {
                        w.segs.len()
                    } else {
                        w.segs.len() + 1
                    }
                && w.thickness() > 0.0;
            let faces: Option<Vec<(Curve, Curve)>> = ok
                .then(|| {
                    w.segs
                        .iter()
                        .map(|s| {
                            Some((
                                s.face(w.offsets.0, self.eps)?,
                                s.face(w.offsets.1, self.eps)?,
                            ))
                        })
                        .collect()
                })
                .flatten();
            if faces.is_none() {
                self.out.notices.push(format!(
                    "Wall {} couldn't be made: a curve is too tight for its thickness",
                    w.id
                ));
            }
            self.faces.push(faces);
        }
    }

    fn face(&self, f: FaceRef) -> Curve {
        let faces = self.faces[f.0].as_ref().expect("made");
        match f.2 {
            Side::Left => faces[f.1].0,
            Side::Right => faces[f.1].1,
        }
    }

    /// An end leaving station `u` of a segment in `dir`.
    fn end(&self, w: usize, seg: usize, u: f64, at: Station, dir: Dir) -> End {
        let wall = &self.walls[w];
        let s = &wall.segs[seg];
        let t = s.tangent(u);
        let (ol, or) = wall.offsets;
        let (d, ccw, cw) = match dir {
            Dir::Fwd => (t, (Side::Left, ol), (Side::Right, or)),
            Dir::Back => (-t, (Side::Right, -or), (Side::Left, -ol)),
        };
        End {
            key: EndKey {
                wall: wall.id,
                seg: seg as u16,
                at,
                dir,
            },
            w,
            seg,
            u,
            d,
            ccw,
            cw,
            cluster: wall.cluster,
            thick: wall.thickness(),
            pass: None,
            against: None,
        }
    }

    /// The station a run end's point is on: its own place on its segment.
    fn end_point(&self, w: usize, which: usize) -> (EntityId, DVec2) {
        let wall = &self.walls[w];
        if which == 0 {
            (wall.point(0), wall.segs[0].start())
        } else {
            (
                wall.point(wall.segs.len()),
                wall.segs[wall.segs.len() - 1].end(),
            )
        }
    }

    /// Where crossing walls of a cluster cross mid-segment: the two segments, where along
    /// each, and the station each gets (named by the other).
    #[allow(clippy::type_complexity)]
    fn crossings(
        &self,
        stations: &BTreeMap<(usize, usize), Vec<StationIn>>,
    ) -> Vec<(
        (usize, usize),
        (usize, usize),
        f64,
        f64,
        Station,
        Station,
        u8,
    )> {
        let mut segs: Vec<(usize, usize, drift_core::Bounds)> = Vec::new();
        for (w, wall) in self.walls.iter().enumerate() {
            if self.faces[w].is_none() || !wall.cross {
                continue;
            }
            for (k, s) in wall.segs.iter().enumerate() {
                if self.near(w, k) {
                    segs.push((w, k, s.bounds()));
                }
            }
        }
        segs.sort_by(|a, b| {
            a.2.min
                .x
                .total_cmp(&b.2.min.x)
                .then((a.0, a.1).cmp(&(b.0, b.1)))
        });
        let near_station = |at: (usize, usize), u: f64| {
            stations
                .get(&at)
                .is_some_and(|l| l.iter().any(|(v, ..)| (v - u).abs() <= self.snap))
        };
        let mut out = Vec::new();
        for i in 0..segs.len() {
            let (wa, ka, ba) = segs[i];
            for &(wb, kb, bb) in &segs[i + 1..] {
                if bb.min.x > ba.max.x + self.snap {
                    break;
                }
                if wa == wb
                    || self.walls[wa].cluster != self.walls[wb].cluster
                    || bb.min.y > ba.max.y + self.snap
                    || bb.max.y < ba.min.y - self.snap
                {
                    continue;
                }
                // (In wall order, so the crossing's number counts along the lower wall.)
                let ((wa, ka), (wb, kb)) = if self.walls[wa].id < self.walls[wb].id {
                    ((wa, ka), (wb, kb))
                } else {
                    ((wb, kb), (wa, ka))
                };
                let (sa, sb) = (self.walls[wa].segs[ka], self.walls[wb].segs[kb]);
                let mut hits: Vec<(f64, f64)> =
                    crate::intersect::intersect(&sa.shape(), &sb.shape())
                        .into_iter()
                        .filter(|h| !h.tangent)
                        .map(|h| (sa.param(h.at), sb.param(h.at)))
                        .filter(|&(ua, ub)| {
                            ua > self.snap
                                && ua < sa.length() - self.snap
                                && ub > self.snap
                                && ub < sb.length() - self.snap
                                && !near_station((wa, ka), ua)
                                && !near_station((wb, kb), ub)
                        })
                        .collect();
                hits.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (n, (ua, ub)) in hits.into_iter().enumerate() {
                    let n = u8::try_from(n).unwrap_or(u8::MAX);
                    let (ida, idb) = (self.walls[wa].id, self.walls[wb].id);
                    out.push((
                        (wa, ka),
                        (wb, kb),
                        ua,
                        ub,
                        Station::Cross(ida, ka as u16, n),
                        Station::Cross(idb, kb as u16, n),
                        n,
                    ));
                }
            }
        }
        out
    }

    /// Where each free end of an open run lands, worked out from the geometry: an end inside
    /// another wall (or its own run, past the next segment) joins it, at that wall's end point
    /// when it lands near it (an L or n-way join; the run's own start closes it), else
    /// part-way along (a T); an end next to another end, outside both walls, joins it too.
    /// "Near" an end point is within half the joining wall's thickness of it, or for a free
    /// end of the host, within the host's thickness more (a stub shorter than the wall is
    /// thick is taken back to the corner). Ends that share their point with other segments,
    /// that are set not to join, or that stop against a curve keep what they have.
    ///
    /// Returns the tees, each end point joined to another point (by the point that names
    /// the join), where every centreline point is, and the free ends.
    #[allow(clippy::type_complexity)]
    fn joins(
        &self,
        made: &[bool],
    ) -> (
        BTreeMap<(usize, usize), TeeAt>,
        HashMap<EntityId, EntityId>,
        HashMap<EntityId, DVec2>,
        Vec<(usize, usize)>,
    ) {
        // Every centreline point: where it is and how many segment ends meet there.
        let mut at_point: HashMap<EntityId, DVec2> = HashMap::new();
        let mut uses: HashMap<EntityId, usize> = HashMap::new();
        let mut segs: Vec<(usize, usize, drift_core::Bounds)> = Vec::new();
        for (w, wall) in self.walls.iter().enumerate() {
            if !made[w] {
                continue;
            }
            for (k, s) in wall.segs.iter().enumerate() {
                if !self.near(w, k) {
                    continue;
                }
                for (i, p) in [(k, s.start()), (k + 1, s.end())] {
                    let id = wall.point(i);
                    at_point.entry(id).or_insert(p);
                    *uses.entry(id).or_default() += 1;
                }
                segs.push((w, k, s.bounds()));
            }
        }
        // The free ends.
        let mut ends: Vec<(usize, usize, EntityId, DVec2)> = Vec::new();
        for (w, wall) in self.walls.iter().enumerate() {
            if !made[w] || wall.closed {
                continue;
            }
            for (e, end) in wall.ends.iter().enumerate() {
                let k = if e == 0 { 0 } else { wall.segs.len() - 1 };
                let (id, p) = self.end_point(w, e);
                if end.join && end.against.is_none() && self.near(w, k) && uses.get(&id) == Some(&1)
                {
                    ends.push((w, e, id, p));
                }
            }
        }
        let mut tees = BTreeMap::new();
        let mut parent: HashMap<EntityId, EntityId> = HashMap::new();
        let free: Vec<(usize, usize)> = ends.iter().map(|e| (e.0, e.1)).collect();
        if ends.is_empty() {
            return (tees, parent, at_point, free);
        }
        // The ends by grid cell, so each segment looks only at those near it.
        let thickest = self.walls.iter().map(WallIn::thickness).fold(0.0, f64::max);
        let cell = (4.0 * (thickest + self.snap)).max(1e-6);
        let cell_of = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (n, e) in ends.iter().enumerate() {
            grid.entry(cell_of(e.3)).or_default().push(n);
        }
        // For each end, the nearest segment it is inside (or touches the end of).
        let mut best: Vec<Option<(f64, usize, usize, f64)>> = vec![None; ends.len()];
        for &(h, k, b) in &segs {
            let host = &self.walls[h];
            let s = host.segs[k];
            let reach = host.thickness() / 2.0 + self.snap;
            let (lo, hi) = (
                cell_of(b.min - DVec2::splat(reach)),
                cell_of(b.max + DVec2::splat(reach)),
            );
            let cells = (hi.0 - lo.0 + 1).saturating_mul(hi.1 - lo.1 + 1);
            let mut near_ends: Vec<usize> = Vec::new();
            if usize::try_from(cells).map_or(true, |c| c > ends.len()) {
                near_ends.extend(0..ends.len());
            } else {
                for x in lo.0..=hi.0 {
                    for y in lo.1..=hi.1 {
                        if let Some(l) = grid.get(&(x, y)) {
                            near_ends.extend(l);
                        }
                    }
                }
            }
            for n in near_ends {
                let (w, e, id, p) = ends[n];
                // Not its own segment, nor the next one along its run.
                if h == w {
                    let last = host.segs.len() - 1;
                    let own = if e == 0 { k <= 1 } else { k + 1 >= last };
                    if own {
                        continue;
                    }
                }
                if host.point(k) == id || host.point(k + 1) == id {
                    continue;
                }
                let u = s.param(p).clamp(0.0, s.length());
                let d = s.at(u).distance(p);
                if d > reach {
                    continue;
                }
                let better = best[n].is_none_or(|(bd, bh, bk, _)| {
                    d < bd - self.eps || (d <= bd + self.eps && (h, k) < (bh, bk))
                });
                if better {
                    best[n] = Some((d, h, k, u));
                }
            }
        }
        let find = |parent: &HashMap<EntityId, EntityId>, mut p: EntityId| {
            while let Some(&q) = parent.get(&p) {
                p = q;
            }
            p
        };
        let free_end: BTreeSet<EntityId> = ends.iter().map(|e| e.2).collect();
        for (n, &(w, e, id, p)) in ends.iter().enumerate() {
            let te = self.walls[w].thickness();
            let target = match best[n] {
                Some((_, h, k, u)) => {
                    let host = &self.walls[h];
                    let len = host.segs[k].length();
                    // How near each end point of the segment counts as on it.
                    let limit = |i: usize| {
                        let stub = free_end.contains(&host.point(i));
                        te / 2.0 + self.snap + if stub { host.thickness() } else { 0.0 }
                    };
                    // A point of its own run next to this end can't be joined.
                    let mine = |i: usize| {
                        h == w && {
                            let last = host.segs.len();
                            if e == 0 { i <= 2 } else { i + 2 >= last }
                        }
                    };
                    let (from_start, from_end) = (u, len - u);
                    let pick = match (from_start <= limit(k), from_end <= limit(k + 1)) {
                        (true, true) => Some(if from_start <= from_end { k } else { k + 1 }),
                        (true, false) => Some(k),
                        (false, true) => Some(k + 1),
                        (false, false) => None,
                    };
                    match pick {
                        Some(i) if mine(i) => None,
                        Some(i) => Some(Ok(host.point(i))),
                        None if u > self.snap && u < len - self.snap => {
                            Some(Err(TeeAt { host: h, seg: k, u }))
                        }
                        None => None,
                    }
                }
                None => {
                    // Next to another free end, outside both walls.
                    let mut near: Option<(f64, EntityId)> = None;
                    let c = cell_of(p);
                    for x in c.0 - 1..=c.0 + 1 {
                        for y in c.1 - 1..=c.1 + 1 {
                            for &m in grid.get(&(x, y)).into_iter().flatten() {
                                let (w2, _, id2, p2) = ends[m];
                                if m == n || (w2 == w && self.walls[w].segs.len() < 3) {
                                    continue;
                                }
                                let reach = te.max(self.walls[w2].thickness()) / 2.0 + self.snap;
                                let d = p.distance(p2);
                                if d <= reach && near.is_none_or(|(bd, bid)| (d, id2) < (bd, bid)) {
                                    near = Some((d, id2));
                                }
                            }
                        }
                    }
                    near.map(|(_, id2)| Ok(id2))
                }
            };
            match target {
                Some(Ok(v)) => {
                    let (a, b) = (find(&parent, id), find(&parent, v));
                    if a != b {
                        // (The lower point names the join, whichever way round it was found.)
                        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
                        parent.insert(hi, lo);
                    }
                }
                Some(Err(t)) => {
                    tees.insert((w, e), t);
                }
                None => {}
            }
        }
        let alias: HashMap<EntityId, EntityId> =
            parent.keys().map(|&p| (p, find(&parent, p))).collect();
        (tees, alias, at_point, free)
    }

    /// The nodes: ends gathered where they meet.
    fn nodes(&mut self) -> Vec<Node> {
        let made: Vec<bool> = self.faces.iter().map(Option::is_some).collect();
        // Where free ends land: on another wall's end (an L) or part-way along it (a T).
        let (tees, alias, at_point, free) = self.joins_kept(&made);
        for (w, e) in free {
            let tee = tees
                .get(&(w, e))
                .map(|t| (self.walls[t.host].id, t.seg as u16));
            self.out.tees.insert((self.walls[w].id, e as u8), tee);
        }
        // Stations inside segments: tees, then crossings.
        let mut stations: BTreeMap<(usize, usize), Vec<StationIn>> = BTreeMap::new();
        for (&(i, e), t) in &tees {
            let len = self.walls[t.host].segs[t.seg].length();
            if t.u <= self.snap || t.u >= len - self.snap {
                continue;
            }
            stations.entry((t.host, t.seg)).or_default().push((
                t.u,
                Station::Tee(self.walls[i].id, e as u8),
                Some((i, e)),
                None,
            ));
        }
        for (a, b, ua, ub, ka, kb, n) in self.crossings(&stations) {
            // (Both segments' stations are one node.)
            let id = NodeId::Cross(a.0, a.1, b.0, b.1, n);
            stations
                .entry(a)
                .or_default()
                .push((ua, kb, None, Some(id)));
            stations
                .entry(b)
                .or_default()
                .push((ub, ka, None, Some(id)));
        }
        let mut nodes: Vec<Node> = Vec::new();
        let mut node_of: HashMap<NodeId, usize> = HashMap::new();
        let mut node = |id: NodeId, at: DVec2, nodes: &mut Vec<Node>| -> usize {
            *node_of.entry(id).or_insert_with(|| {
                nodes.push(Node {
                    at,
                    ends: Vec::new(),
                });
                nodes.len() - 1
            })
        };
        // Stations within the snap tolerance of each other (two branches teeing at one
        // place) are one node, named by the first.
        let mut seg_stations: BTreeMap<(usize, usize), Vec<StationAt>> = BTreeMap::new();
        let mut tee_node: HashMap<(usize, usize), usize> = HashMap::new();
        let mut pass = 0;
        for (&(w, seg), list) in &mut stations {
            list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let s = self.walls[w].segs[seg];
            let mut merged: Vec<StationAt> = Vec::new();
            for &(u, station, tee, cross) in list.iter() {
                let n = match merged.last() {
                    Some(last) if u - last.u <= self.snap && cross.is_none() => last.node,
                    _ => {
                        let id = cross.unwrap_or(NodeId::Station(w, seg, merged.len()));
                        let n = node(id, s.at(u), &mut nodes);
                        merged.push(StationAt {
                            u,
                            station,
                            node: n,
                            pass,
                        });
                        pass += 1;
                        n
                    }
                };
                if let Some(t) = tee {
                    tee_node.insert(t, n);
                }
            }
            seg_stations.insert((w, seg), merged);
        }
        // A teed end's own point belongs to its tee's node (so other ends there join it
        // too); one teed onto its host's segment end joins the host's node there.
        let mut point_node: HashMap<EntityId, usize> = HashMap::new();
        for (&(i, e), t) in &tees {
            let (own, _) = self.end_point(i, e);
            let n = match tee_node.get(&(i, e)) {
                Some(&n) => n,
                None => {
                    let host = &self.walls[t.host];
                    let (k, at) = if t.u <= self.snap {
                        (t.seg, host.segs[t.seg].start())
                    } else {
                        (t.seg + 1, host.segs[t.seg].end())
                    };
                    node(NodeId::Point(host.point(k)), at, &mut nodes)
                }
            };
            point_node.insert(own, n);
        }
        // Every segment's ends, and its stations' two virtual ends.
        for (w, wall) in self.walls.iter().enumerate() {
            if !made[w] {
                continue;
            }
            let n = wall.segs.len();
            for (k, s) in wall.segs.iter().enumerate() {
                if !self.near(w, k) {
                    continue;
                }
                for (dir, pi) in [(Dir::Fwd, k), (Dir::Back, k + 1)] {
                    let (u, at, place) = match dir {
                        Dir::Fwd => (0.0, Station::SegStart, s.start()),
                        Dir::Back => (s.length(), Station::SegEnd, s.end()),
                    };
                    let mut end = self.end(w, k, u, at, dir);
                    let run_end = !wall.closed
                        && ((dir == Dir::Fwd && k == 0) || (dir == Dir::Back && k == n - 1));
                    let which = usize::from(dir == Dir::Back);
                    let info = &wall.ends[which];
                    let p = wall.point(pi);
                    let ni = if run_end && !info.join {
                        node(NodeId::Alone(end.key), place, &mut nodes)
                    } else if run_end && info.against.is_some() {
                        end.against = info.against;
                        node(NodeId::Alone(end.key), place, &mut nodes)
                    } else {
                        let root = alias.get(&p).copied().unwrap_or(p);
                        match point_node.get(&root) {
                            Some(&tn) => tn,
                            None => {
                                let place = at_point.get(&root).copied().unwrap_or(place);
                                node(NodeId::Point(root), place, &mut nodes)
                            }
                        }
                    };
                    nodes[ni].ends.push(end);
                }
                for st in seg_stations.get(&(w, k)).into_iter().flatten() {
                    for dir in [Dir::Back, Dir::Fwd] {
                        let mut end = self.end(w, k, st.u, st.station, dir);
                        end.pass = Some(st.pass);
                        nodes[st.node].ends.push(end);
                    }
                }
            }
        }
        // Openings' jambs: a node each.
        self.opening_ends(&mut nodes, &made);
        // Ends leaving a node the same way overlap: they don't join (the later one is
        // capped on its own).
        let mut out: Vec<Node> = Vec::new();
        for mut nd in nodes {
            nd.ends
                .sort_by(|a, b| angle(a.d).total_cmp(&angle(b.d)).then(a.key.cmp(&b.key)));
            let mut kept: Vec<End> = Vec::new();
            for e in nd.ends {
                let clash = kept.iter().any(|k| {
                    let gap = (angle(e.d) - angle(k.d)).rem_euclid(TAU);
                    gap.min(TAU - gap) < STRAIGHT_ON
                });
                if clash {
                    self.out.notices.push(format!(
                        "Wall {} runs along another wall where they meet: they don't join \
                         (make them one run)",
                        e.key.wall
                    ));
                    out.push(Node {
                        at: nd.at,
                        ends: vec![e],
                    });
                } else {
                    kept.push(e);
                }
            }
            if !kept.is_empty() {
                out.push(Node {
                    at: nd.at,
                    ends: kept,
                });
            }
        }
        for nd in out.iter().filter(|n| n.ends.len() >= 2) {
            for (i, e) in nd.ends.iter().enumerate() {
                let wall = &self.walls[e.w];
                let which = match (e.key.at, e.key.dir) {
                    (Station::SegStart, Dir::Fwd) if e.seg == 0 => 0,
                    (Station::SegEnd, Dir::Back) if e.seg + 1 == wall.segs.len() => 1,
                    _ => continue,
                };
                if wall.closed {
                    continue;
                }
                let met = nd
                    .ends
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, o)| o.key.wall);
                self.out
                    .joined
                    .entry((wall.id, which))
                    .or_default()
                    .extend(met);
            }
        }
        out
    }

    // ----- Nodes ------------------------------------------------------------------------

    /// The point on an end's face (counter-clockwise or not) at its own station.
    fn own_point(&self, e: &End, ccw: bool) -> DVec2 {
        let s = &self.walls[e.w].segs[e.seg];
        let lambda = if ccw { e.ccw.1 } else { e.cw.1 };
        s.at(e.u) + e.d.perp() * lambda
    }

    fn face_of(&self, e: &End, ccw: bool) -> Curve {
        self.face((e.w, e.seg, if ccw { e.ccw.0 } else { e.cw.0 }))
    }

    fn corner(&mut self, key: CornerKey, owner: WallId, at: DVec2) {
        self.out.points.insert(key, (owner, at));
    }

    /// End `e`'s face (counter-clockwise or not) stops or starts at `corner`.
    fn event(&mut self, e: &End, ccw: bool, corner: CornerKey) {
        let side = if ccw { e.ccw.0 } else { e.cw.0 };
        let face = (e.w, e.seg, side);
        let start = e.key.dir == Dir::Fwd;
        self.events.entry(face).or_default().push(Event {
            order: (e.u, u8::from(start)),
            station: e.key.at,
            start,
            corner,
        });
        self.corner_faces.entry(corner).or_default().push(face);
    }

    /// An edge owned by end `e`.
    fn edge(&mut self, e: &End, part: Part, from: CornerKey, to: CornerKey, stroke: bool) {
        self.edges.push(EdgeOut {
            owner: e.wall(),
            key: FaceKey {
                seg: e.key.seg,
                part,
                from: e.key.at,
            },
            piece: Piece {
                from,
                to,
                arc: None,
                stroke,
                cluster: e.cluster,
            },
        });
    }

    fn solve(&mut self, node: &Node) {
        let ends = &node.ends;
        let Some(owner) = ends.iter().map(End::wall).min() else {
            return;
        };
        if ends.len() == 1 {
            self.single(&ends[0]);
            return;
        }
        let mut clusters: Vec<u32> = ends.iter().map(|e| e.cluster).collect();
        clusters.sort_unstable();
        clusters.dedup();
        if clusters.len() == 1 {
            self.ring(node, ends, owner);
            return;
        }
        // Different materials: the thickest wall runs through (ties: the wall drawn first).
        let through = ends
            .iter()
            .max_by(|a, b| a.thick.total_cmp(&b.thick).then(b.wall().cmp(&a.wall())))
            .map(|e| e.cluster)
            .expect("ends");
        let t: Vec<End> = ends
            .iter()
            .filter(|e| e.cluster == through)
            .cloned()
            .collect();
        if t.len() >= 2 {
            self.ring(node, &t, owner);
            for e in ends.iter().filter(|e| e.cluster != through) {
                self.butt(node, e, &t);
            }
        } else {
            let leave = self.ring(node, ends, owner);
            self.seams(node, ends, owner, &leave);
        }
    }

    /// A free end: capped, or stopped against its curve.
    fn single(&mut self, e: &End) {
        let (kc, kw) = (CornerKey::Own(e.key, true), CornerKey::Own(e.key, false));
        if let Some(shape) = e.against {
            let hit = |ccw: bool| {
                let face = self.face_of(e, ccw);
                let own = self.own_point(e, ccw);
                let reach = 1e3 * (1.0 + e.thick + shape.bounds().max.abs().max_element());
                let hits = crate::intersect::intersect_extended(&face.shape(own, reach), &shape);
                nearest(hits.into_iter().map(|h| h.at).collect(), own)
            };
            if let (Some(xc), Some(xw)) = (hit(true), hit(false)) {
                self.corner(kc, e.wall(), xc);
                self.corner(kw, e.wall(), xw);
                self.event(e, true, kc);
                self.event(e, false, kw);
                self.edge(e, Part::Butt(e.key.dir), kc, kw, false);
                return;
            }
            self.out.notices.push(format!(
                "Wall {}'s end misses the curve it stops against: it is capped",
                e.key.wall
            ));
        }
        let (qc, qw) = (self.own_point(e, true), self.own_point(e, false));
        self.corner(kc, e.wall(), qc);
        self.corner(kw, e.wall(), qw);
        self.event(e, true, kc);
        self.event(e, false, kw);
        // (An opening's jamb is its own part.)
        let part = match e.key.at {
            Station::Jamb(..) => Part::Jamb,
            _ => Part::Cap(e.key.dir),
        };
        self.edge(e, part, kc, kw, true);
    }

    /// The corners between consecutive ends (counter-clockwise), for each pair the corner
    /// the next end's clockwise face leaves from (None where a face passes straight
    /// through).
    fn ring(&mut self, node: &Node, ends: &[End], owner: WallId) -> Vec<Option<CornerKey>> {
        let n = ends.len();
        let mut leave = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (&ends[i], &ends[(i + 1) % n]);
            if a.pass.is_some() && a.pass == b.pass && a.w == b.w {
                leave.push(None);
                continue;
            }
            let gap = (angle(b.d) - angle(a.d)).rem_euclid(TAU);
            let (ka, kb) = (
                CornerKey::Pair(a.key, b.key),
                CornerKey::PairB(a.key, b.key),
            );
            let (qa, qb) = (self.own_point(a, true), self.own_point(b, false));
            let one = |g: &mut Self, at: DVec2| {
                g.corner(ka, owner, at);
                g.event(a, true, ka);
                g.event(b, false, ka);
                Some(ka)
            };
            let two = |g: &mut Self, pa: DVec2, pb: DVec2, part: Part| {
                g.corner(ka, owner, pa);
                g.corner(kb, owner, pb);
                g.event(a, true, ka);
                g.event(b, false, kb);
                g.edge(a, part, ka, kb, true);
                Some(kb)
            };
            if (gap - PI).abs() < STRAIGHT_ON {
                // Straight on: the faces meet on the normal through the node, or a step
                // joins them where the thicknesses differ.
                let k = if qa.distance(qb) <= self.eps {
                    one(self, qa)
                } else {
                    two(self, qa, qb, Part::Step(a.key.dir))
                };
                leave.push(k);
                continue;
            }
            let (f, g) = (self.face_of(a, true), self.face_of(b, false));
            let k = match nearest(f.cross(&g), node.at) {
                None => {
                    self.out.notices.push(format!(
                        "Walls {} and {} don't meet where they join: the corner is closed \
                         straight across",
                        a.key.wall, b.key.wall
                    ));
                    two(self, qa, qb, Part::Bevel(a.key.dir))
                }
                Some(p) if p.distance(node.at) > MITRE_LIMIT * a.thick.max(b.thick) => {
                    // Past the mitre limit: cut square to the bisector.
                    let dir = (p - node.at).normalize_or_zero();
                    let at = node.at + dir * (MITRE_LIMIT * a.thick.max(b.thick));
                    let cut = Curve::Line {
                        p: at,
                        d: dir.perp(),
                    };
                    match (nearest(f.cross(&cut), at), nearest(g.cross(&cut), at)) {
                        (Some(pa), Some(pb)) if pa.distance(pb) > self.eps => {
                            two(self, pa, pb, Part::Bevel(a.key.dir))
                        }
                        _ => one(self, p),
                    }
                }
                Some(p) => one(self, p),
            };
            leave.push(k);
        }
        leave
    }

    /// An end of another material at a node a thicker wall runs through: its faces stop on
    /// the through wall's near face, and the edge between them is never stroked (the
    /// through wall's face draws that line).
    fn butt(&mut self, node: &Node, e: &End, through: &[End]) {
        let n = through.len();
        let pair = (0..n).find(|&i| {
            let (a, b) = (&through[i], &through[(i + 1) % n]);
            let gap = (angle(b.d) - angle(a.d)).rem_euclid(TAU);
            let at = (angle(e.d) - angle(a.d)).rem_euclid(TAU);
            at > 0.0 && (at < gap || gap == 0.0)
        });
        let Some(i) = pair else {
            self.single(e);
            return;
        };
        let (fa, fb) = (
            self.face_of(&through[i], true),
            self.face_of(&through[(i + 1) % n], false),
        );
        let stop = |g: &Self, ccw: bool| {
            let face = g.face_of(e, ccw);
            let mut hits = fa.cross(&face);
            hits.extend(fb.cross(&face));
            hits.into_iter()
                .filter(|x| x.is_finite())
                .map(|x| ((x - node.at).dot(e.d), x))
                .filter(|(t, _)| *t >= -g.eps)
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, x)| x)
        };
        match (stop(self, true), stop(self, false)) {
            (Some(xc), Some(xw)) => {
                let (kc, kw) = (CornerKey::Own(e.key, true), CornerKey::Own(e.key, false));
                self.corner(kc, e.wall(), xc);
                self.corner(kw, e.wall(), xw);
                self.event(e, true, kc);
                self.event(e, false, kw);
                self.edge(e, Part::Butt(e.key.dir), kc, kw, false);
            }
            _ => self.single(e),
        }
    }

    /// Lines between materials at a node where none runs through: with two materials, one
    /// seam across the node for each (drawn once); with more, seams from each boundary
    /// corner to the node.
    fn seams(&mut self, node: &Node, ends: &[End], owner: WallId, leave: &[Option<CornerKey>]) {
        let n = ends.len();
        let bounds: Vec<(usize, CornerKey)> = (0..n)
            .filter(|&i| ends[i].cluster != ends[(i + 1) % n].cluster)
            .filter_map(|i| Some((i, leave[i]?)))
            .collect();
        if bounds.len() == 2 {
            let ((i0, b0), (i1, b1)) = (bounds[0], bounds[1]);
            let (e0, e1) = (&ends[i0], &ends[i1]);
            let first = e0.wall() <= e1.wall();
            self.edge(e0, Part::Seam(e0.key.dir, Turn::Ccw), b0, b1, first);
            self.edge(e1, Part::Seam(e1.key.dir, Turn::Ccw), b1, b0, !first);
            return;
        }
        let Some(lowest) = ends.iter().map(|e| e.key).min() else {
            return;
        };
        let hub = CornerKey::Node(lowest);
        self.corner(hub, owner, node.at);
        for (i, b) in bounds {
            let (x, y) = (&ends[i], &ends[(i + 1) % n]);
            let first = x.wall() <= y.wall();
            self.edge(x, Part::Seam(x.key.dir, Turn::Ccw), b, hub, first);
            self.edge(y, Part::Seam(y.key.dir, Turn::Cw), hub, b, !first);
        }
    }

    // ----- Faces and outlines -----------------------------------------------------------

    /// The face pieces, between the corner that starts each and the next that stops it.
    fn pieces(&mut self) {
        let events = std::mem::take(&mut self.events);
        let mut inverted: Vec<(FaceRef, CornerKey, CornerKey)> = Vec::new();
        for (face, mut list) in events {
            list.sort_by(|a, b| {
                a.order
                    .0
                    .total_cmp(&b.order.0)
                    .then(a.order.1.cmp(&b.order.1))
            });
            let (w, seg, side) = face;
            let wall = &self.walls[w];
            let s = wall.segs[seg];
            let mut open: Option<Event> = None;
            for ev in list {
                if ev.start {
                    open = Some(ev);
                    continue;
                }
                let Some(st) = open.take() else {
                    continue;
                };
                let (Some(ps), Some(pe)) = (self.out.point(st.corner), self.out.point(ev.corner))
                else {
                    continue;
                };
                if s.param(pe) <= s.param(ps) + self.eps {
                    inverted.push((face, st.corner, ev.corner));
                    continue;
                }
                let key = FaceKey {
                    seg: seg as u16,
                    part: match side {
                        Side::Left => Part::Left,
                        Side::Right => Part::Right,
                    },
                    from: st.station,
                };
                // The wall on its left: a left face runs back, a right face forward.
                let (from, to) = match side {
                    Side::Left => (ev.corner, st.corner),
                    Side::Right => (st.corner, ev.corner),
                };
                let arc = match s {
                    Seg::Line { .. } => None,
                    Seg::Arc { center, ccw, .. } => {
                        let c = CornerKey::Centre(wall.id, key);
                        self.out.points.insert(c, (wall.id, center));
                        Some((c, if side == Side::Right { ccw } else { !ccw }))
                    }
                };
                self.edges.push(EdgeOut {
                    owner: wall.id,
                    key,
                    piece: Piece {
                        from,
                        to,
                        arc,
                        stroke: true,
                        cluster: wall.cluster,
                    },
                });
            }
        }
        // A segment shorter than its thickness: its inside piece would run backwards. It is
        // left out, and the faces either side join at their own crossing.
        for (face, a, b) in inverted {
            let other = |k: CornerKey| {
                self.corner_faces
                    .get(&k)
                    .and_then(|fs| fs.iter().find(|f| **f != face).copied())
            };
            let fixed = match (other(a), other(b), self.out.point(a), self.out.point(b)) {
                (Some(fa), Some(fb), Some(pa), Some(pb)) if fa != fb => {
                    nearest(self.face(fa).cross(&self.face(fb)), (pa + pb) / 2.0)
                        .map(|x| (x, fa, fb))
                }
                _ => None,
            };
            match fixed {
                Some((x, ..)) => {
                    if let Some(p) = self.out.points.get_mut(&a) {
                        p.1 = x;
                    }
                    self.alias.insert(b, a);
                }
                None => self.out.notices.push(format!(
                    "Wall {}: a segment is too short for its thickness",
                    self.walls[face.0].id
                )),
            }
        }
    }

    /// A corner after the ones merged into others.
    fn resolve(&self, mut k: CornerKey) -> CornerKey {
        let mut steps = 0;
        while let Some(&to) = self.alias.get(&k) {
            k = to;
            steps += 1;
            if steps > 64 {
                break;
            }
        }
        k
    }

    /// Edges of no length (a cap of a wall teed exactly onto a corner, say) go, their ends
    /// becoming one corner.
    fn drop_empty_edges(&mut self) {
        loop {
            let mut edges = std::mem::take(&mut self.edges);
            for e in &mut edges {
                e.piece.from = self.resolve(e.piece.from);
                e.piece.to = self.resolve(e.piece.to);
            }
            self.edges = edges;
            let empty = self.edges.iter().position(|e| {
                e.piece.from == e.piece.to
                    || match (self.out.point(e.piece.from), self.out.point(e.piece.to)) {
                        (Some(a), Some(b)) => a.distance(b) <= self.eps,
                        _ => true,
                    }
            });
            let Some(i) = empty else {
                break;
            };
            let e = self.edges.remove(i);
            if e.piece.from != e.piece.to {
                self.alias.insert(e.piece.to, e.piece.from);
            }
        }
        for (k, _) in self.alias.clone() {
            self.out.points.remove(&k);
        }
    }

    /// Chain each cluster's edges into loops by their corners, and key the loops.
    fn chain(&mut self) {
        let edges = std::mem::take(&mut self.edges);
        for e in &edges {
            if self.out.curves.insert((e.owner, e.key), e.piece).is_some() {
                self.out
                    .notices
                    .push(format!("Wall {}: two pieces with one key", e.owner));
            }
        }
        let mut by_cluster: BTreeMap<u32, Vec<(WallId, FaceKey)>> = BTreeMap::new();
        for (k, p) in &self.out.curves {
            by_cluster.entry(p.cluster).or_default().push(*k);
        }
        let mut open_walls: BTreeSet<WallId> = BTreeSet::new();
        for (cluster, keys) in by_cluster {
            let mut out_of: HashMap<CornerKey, Vec<(WallId, FaceKey)>> = HashMap::new();
            for k in &keys {
                out_of.entry(self.out.curves[k].from).or_default().push(*k);
            }
            let mut used: BTreeSet<(WallId, FaceKey)> = BTreeSet::new();
            let mut loops: Vec<Vec<(WallId, FaceKey)>> = Vec::new();
            for &start in &keys {
                if used.contains(&start) {
                    continue;
                }
                used.insert(start);
                let first = self.out.curves[&start].from;
                let mut at = self.out.curves[&start].to;
                let mut chain = vec![start];
                let closed = loop {
                    if at == first {
                        break true;
                    }
                    let next = out_of
                        .get(&at)
                        .and_then(|l| l.iter().find(|k| !used.contains(k)).copied());
                    let Some(next) = next else {
                        break false;
                    };
                    used.insert(next);
                    chain.push(next);
                    at = self.out.curves[&next].to;
                };
                if closed {
                    loops.push(chain);
                } else {
                    open_walls.extend(chain.iter().map(|(w, _)| *w));
                }
            }
            self.key_loops(cluster, loops);
        }
        if !open_walls.is_empty() {
            let list: Vec<String> = open_walls.iter().map(WallId::to_string).collect();
            self.out.notices.push(format!(
                "The outline of walls {} doesn't close: they have no poché",
                list.join(", ")
            ));
        }
    }

    /// Sort a cluster's loops into outer loops and their holes, and key them: by their lead
    /// wall (the lowest of the region), then in the order of their lowest curves.
    fn key_loops(&mut self, cluster: u32, loops: Vec<Vec<(WallId, FaceKey)>>) {
        struct L {
            edges: Vec<(WallId, FaceKey)>,
            lp: Loop,
            area: f64,
            bounds: drift_core::Bounds,
            min: (WallId, FaceKey),
        }
        let mut all: Vec<L> = Vec::new();
        for edges in loops {
            let probe = LoopOut {
                cluster,
                lead: WallId(0),
                key: LoopKey::default(),
                edges: edges.clone(),
                seed: DVec2::ZERO,
            };
            let Some(lp) = self.out.loop_of(&probe) else {
                continue;
            };
            let area = lp.signed_area();
            let bounds: drift_core::Bounds = lp.edges.iter().map(|e| e.shape().bounds()).collect();
            let min = *edges.iter().min().expect("a loop has edges");
            all.push(L {
                edges,
                lp,
                area,
                bounds,
                min,
            });
        }
        let outers: Vec<usize> = (0..all.len()).filter(|&i| all[i].area > 0.0).collect();
        let mut holes_of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for h in (0..all.len()).filter(|&i| all[i].area <= 0.0) {
            let hb = all[h].bounds;
            let probe = all[h].lp.edges[0].start();
            let pad = 1e-6 * (1.0 + hb.max.abs().max_element());
            let host = outers
                .iter()
                .copied()
                .filter(|&o| {
                    let ob = all[o].bounds;
                    ob.min.x <= hb.min.x + pad
                        && ob.min.y <= hb.min.y + pad
                        && ob.max.x >= hb.max.x - pad
                        && ob.max.y >= hb.max.y - pad
                })
                .filter(|&o| {
                    // (A point a little inside the hole's material side: just off its start.)
                    let e = &all[h].lp.edges[0];
                    let along = (e.end() - e.start()).normalize_or_zero();
                    all[o].lp.contains(probe + along.perp() * pad * 10.0)
                        || all[o].lp.contains(probe)
                })
                .min_by(|&a, &b| all[a].area.total_cmp(&all[b].area));
            match host {
                Some(o) => holes_of.entry(o).or_default().push(h),
                None => self
                    .out
                    .notices
                    .push("A wall outline loop lies outside its walls".to_owned()),
            }
        }
        // Regions by lead wall, in the order of their lowest curves.
        let mut by_lead: BTreeMap<WallId, Vec<(usize, Vec<usize>)>> = BTreeMap::new();
        for &o in &outers {
            let mut holes = holes_of.remove(&o).unwrap_or_default();
            holes.sort_by_key(|&h| all[h].min);
            let lead = std::iter::once(o)
                .chain(holes.iter().copied())
                .flat_map(|i| all[i].edges.iter().map(|(w, _)| *w))
                .min()
                .expect("edges");
            by_lead.entry(lead).or_default().push((o, holes));
        }
        for (lead, mut regions) in by_lead {
            regions.sort_by_key(|(o, holes)| {
                std::iter::once(*o)
                    .chain(holes.iter().copied())
                    .map(|i| all[i].min)
                    .min()
            });
            for (n, (o, holes)) in regions.into_iter().enumerate() {
                let outer = u16::try_from(n).unwrap_or(u16::MAX);
                let seed = self.seed(&all[o].edges);
                self.out.loops.push(LoopOut {
                    cluster,
                    lead,
                    key: LoopKey { outer, hole: 0 },
                    edges: all[o].edges.clone(),
                    seed,
                });
                for (m, h) in holes.into_iter().enumerate() {
                    self.out.loops.push(LoopOut {
                        cluster,
                        lead,
                        key: LoopKey {
                            outer,
                            hole: u16::try_from(m + 1).unwrap_or(u16::MAX),
                        },
                        edges: all[h].edges.clone(),
                        seed,
                    });
                }
            }
        }
    }

    /// A point in the middle of a wall of the loop: halfway between its faces, halfway along
    /// its lowest face piece.
    fn seed(&self, edges: &[(WallId, FaceKey)]) -> DVec2 {
        let face = edges
            .iter()
            .filter(|(_, k)| matches!(k.part, Part::Left | Part::Right))
            .min();
        let Some(&(wid, key)) = face else {
            return self
                .out
                .curves
                .get(&edges[0])
                .and_then(|p| self.out.point(p.from))
                .unwrap_or_default();
        };
        let Some(wall) = self.walls.iter().find(|w| w.id == wid) else {
            return DVec2::ZERO;
        };
        let s = wall.segs[usize::from(key.seg)];
        let piece = self.out.curves[&(wid, key)];
        let (Some(a), Some(b)) = (self.out.point(piece.from), self.out.point(piece.to)) else {
            return s.at(s.length() / 2.0);
        };
        let u = (s.param(a) + s.param(b)) / 2.0;
        s.at(u) + s.tangent(u).perp() * ((wall.offsets.0 + wall.offsets.1) / 2.0)
    }
}
