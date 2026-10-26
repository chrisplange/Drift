//! Walls from lines (W1.5): find the walls in a plan drawn as pairs of face lines, as imported
//! DWG and DXF plans and old script walls have them (`docs/walls-design.md` §3.3 rule 6, §10).
//!
//! [`find`] is a pure function of the lines. It works in five steps:
//!
//! 1. **Pair.** Two lines (or two concentric arcs) of one group pair when they are near-parallel,
//!    a wall-like distance apart, and overlap along their length (they needn't be the same
//!    length). Each line looks to both sides and keeps its nearest partners on one side; a
//!    pair counts when both lines keep each other. Lines with partners at the same distance on
//!    both sides are a pattern (a hatch, a grid), not wall faces. A pair whose spacing changes
//!    along it (a taper) is skipped.
//! 2. **Strips.** Pairs that share a line and agree on thickness and centreline make one strip,
//!    so a face broken where another wall tees in is bridged by its unbroken partner.
//! 3. **Gaps.** Collinear strips with a gap between them meet across it when the gap is where
//!    another wall crosses or tees in from both sides; otherwise the gap is an opening, and the
//!    strips stay apart, capped at its jambs
//!    (they don't become hosted openings yet).
//! 4. **Weld.** Each strip end looks for the wall it runs into: it moves to where the
//!    centrelines cross, and two ends meeting there share one point (an L, or a node of more);
//!    an end that lands part-way along a wall is a T. The wall generator then joins them the
//!    usual way (its joins come from where the ends are).
//! 5. **Runs.** Strips meeting two at a point, of one thickness and group, chain into one run.
//!
//! Thicknesses snap to the given presets when they are close, and are otherwise rounded to the
//! drawing's step. What it used, skipped and left as openings is reported, so the command can
//! say so.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::f64::consts::{PI, TAU};

use drift_core::{DVec2, EntityId};
use rstar::{AABB, RTree, RTreeObject};

use super::Seg;

/// How lines are matched.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The thinnest and thickest walls to look for (mm).
    pub min_thickness: f64,
    pub max_thickness: f64,
    /// How far from parallel two faces may be (radians); within that, how much their spacing
    /// may change along them decides.
    pub angle: f64,
    /// The drawing's gap tolerance: ends this close are the same place.
    pub snap: f64,
    /// Collinear walls further apart than this aren't one run with an opening between them.
    pub max_opening: f64,
    /// Thicknesses to snap to when one is close (the presets, both unit systems).
    pub thicknesses: Vec<f64>,
    /// Otherwise thicknesses are rounded to this (1 mm, or 1/16″); 0 keeps them as measured.
    pub round_to: f64,
}

impl Settings {
    pub fn new(snap: f64, thicknesses: Vec<f64>, round_to: f64) -> Self {
        Self {
            min_thickness: 50.0,
            max_thickness: 460.0,
            angle: 2f64.to_radians(),
            snap,
            max_opening: 5000.0,
            thicknesses,
            round_to,
        }
    }

    /// Ends and lines this close are on each other (at least a millimetre: imported plans
    /// are rarely more exact than that).
    fn tol(&self) -> f64 {
        self.snap.max(1.0)
    }

    /// How much a wall's spacing may vary along it, or two pieces of one wall may differ.
    fn spread(&self, t: f64) -> f64 {
        (2.0 * self.tol()).max(0.05 * t)
    }

    /// The thickness to build at: a preset when close, else rounded.
    pub fn snapped(&self, t: f64) -> f64 {
        let near = self
            .thicknesses
            .iter()
            .copied()
            .filter(|p| (p - t).abs() <= (0.03 * t).min(3.0))
            .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()));
        match near {
            Some(p) => p,
            None if self.round_to > 0.0 => {
                ((t / self.round_to).round() * self.round_to).max(self.round_to)
            }
            None => t,
        }
    }
}

/// A line or arc to look at.
#[derive(Clone, Copy, Debug)]
pub struct LineIn {
    pub id: EntityId,
    /// Only lines of one group pair (the app's groups are layers).
    pub group: u32,
    /// `Seg::Line`, or a `Seg::Arc` either way round.
    pub seg: Seg,
    /// A construction line: never a face, but used up when it lies on a wall's centreline
    /// (old script walls drew one).
    pub guide: bool,
}

/// A wall found: its centreline from point to point (indices into [`Found::points`]).
#[derive(Clone, Debug, PartialEq)]
pub struct FoundWall {
    /// Segments in the order and direction the run goes, each with its start and end point.
    pub segs: Vec<(Seg, usize, usize)>,
    pub closed: bool,
    pub thickness: f64,
    pub group: u32,
    /// The lines it was made from (faces, end caps, centrelines), to remove or keep.
    pub lines: Vec<EntityId>,
}

/// Why lines that looked like a wall weren't made into one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Why {
    /// The two faces aren't parallel enough: the spacing changes along them.
    SpacingVaries,
    /// Evenly spaced lines, partners on both sides: a hatch or a pattern.
    Pattern,
    /// A short piece standing on its own: furniture, a column, a fixture.
    Object,
}

impl Why {
    pub fn text(self) -> &'static str {
        match self {
            Why::SpacingVaries => "spacing varies",
            Why::Pattern => "evenly spaced lines (a hatch?)",
            Why::Object => "too short to stand alone (furniture or a column?)",
        }
    }
}

/// Lines skipped, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Skip {
    pub lines: Vec<EntityId>,
    pub why: Why,
    /// Somewhere to show it.
    pub at: DVec2,
}

/// What [`find`] found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The centrelines' points: shared where walls meet.
    pub points: Vec<DVec2>,
    pub walls: Vec<FoundWall>,
    /// Face lines the walls cover only in part (the rest is something else): made into
    /// walls, but kept.
    pub partly: Vec<EntityId>,
    pub skipped: Vec<Skip>,
    /// Gaps between collinear walls left open (doors and windows): their middle and width.
    pub openings: Vec<(DVec2, f64)>,
}

impl Found {
    /// Every line made into a wall.
    pub fn used(&self) -> BTreeSet<EntityId> {
        self.walls
            .iter()
            .flat_map(|w| w.lines.iter().copied())
            .collect()
    }
}

/// A line or circle a piece or strip lies on, with a parameter along it: distance for a line,
/// angle (counter-clockwise, radians) for a circle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Carrier {
    Line { p: DVec2, d: DVec2 },
    Circle { c: DVec2, r: f64 },
}

impl Carrier {
    fn at(&self, u: f64) -> DVec2 {
        match *self {
            Carrier::Line { p, d } => p + d * u,
            Carrier::Circle { c, r } => c + DVec2::from_angle(u) * r,
        }
    }

    /// The parameter of `x` (for a circle, the angle within half a turn of `near`).
    fn param(&self, x: DVec2, near: f64) -> f64 {
        match *self {
            Carrier::Line { p, d } => (x - p).dot(d),
            Carrier::Circle { c, .. } => {
                let a = (x - c).to_angle();
                near + (a - near + PI).rem_euclid(TAU) - PI
            }
        }
    }

    /// The direction of increasing parameter at `u`.
    fn tangent(&self, u: f64) -> DVec2 {
        match *self {
            Carrier::Line { d, .. } => d,
            Carrier::Circle { .. } => DVec2::from_angle(u).perp(),
        }
    }

    /// Millimetres per unit of parameter.
    fn scale(&self) -> f64 {
        match *self {
            Carrier::Line { .. } => 1.0,
            Carrier::Circle { r, .. } => r,
        }
    }

    /// How far `x` is to the left of it (towards the centre, for a circle).
    fn offset(&self, x: DVec2) -> f64 {
        match *self {
            Carrier::Line { p, d } => (x - p).dot(d.perp()),
            Carrier::Circle { c, r } => r - x.distance(c),
        }
    }

    /// Where two carriers cross.
    fn cross(&self, other: &Carrier) -> Vec<DVec2> {
        match (*self, *other) {
            (Carrier::Line { p, d }, Carrier::Line { p: q, d: e }) => {
                let den = d.perp_dot(e);
                if den.abs() <= 1e-9 {
                    return Vec::new();
                }
                vec![p + d * ((q - p).perp_dot(e) / den)]
            }
            (Carrier::Line { p, d }, Carrier::Circle { c, r })
            | (Carrier::Circle { c, r }, Carrier::Line { p, d }) => {
                let f = p - c;
                let b = f.dot(d);
                let disc = b * b - (f.dot(f) - r * r);
                if disc < 0.0 {
                    return Vec::new();
                }
                let s = disc.sqrt();
                vec![p + d * (-b - s), p + d * (-b + s)]
            }
            (Carrier::Circle { c: c1, r: r1 }, Carrier::Circle { c: c2, r: r2 }) => {
                let dist = c1.distance(c2);
                if dist <= 1e-9 || dist > r1 + r2 || dist < (r1 - r2).abs() {
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
}

/// An input line or arc, on its carrier from `lo` to `hi` (an arc counter-clockwise).
#[derive(Clone, Copy, Debug)]
struct Piece {
    carrier: Carrier,
    lo: f64,
    hi: f64,
}

impl Piece {
    fn of(seg: &Seg) -> Option<Piece> {
        match *seg {
            Seg::Line { a, b } => {
                let d = (b - a).try_normalize()?;
                Some(Piece {
                    carrier: Carrier::Line { p: a, d },
                    lo: 0.0,
                    hi: a.distance(b),
                })
            }
            Seg::Arc {
                center,
                radius,
                a,
                b,
                ccw,
            } => {
                let (s, e) = if ccw { (a, b) } else { (b, a) };
                let lo = (s - center).to_angle();
                let mut sweep = ((e - center).to_angle() - lo).rem_euclid(TAU);
                if sweep <= 0.0 {
                    sweep = TAU;
                }
                (radius > 0.0).then_some(Piece {
                    carrier: Carrier::Circle {
                        c: center,
                        r: radius,
                    },
                    lo,
                    hi: lo + sweep,
                })
            }
        }
    }

    fn length(&self) -> f64 {
        (self.hi - self.lo) * self.carrier.scale()
    }

    fn ends(&self) -> [DVec2; 2] {
        [self.carrier.at(self.lo), self.carrier.at(self.hi)]
    }

    fn bounds(&self) -> AABB<[f64; 2]> {
        let mut pts: Vec<DVec2> = self.ends().to_vec();
        if let Carrier::Circle { c, r } = self.carrier {
            // The quadrant points it passes.
            for k in 0..8 {
                let a = k as f64 * PI / 2.0;
                let a = self.lo + (a - self.lo).rem_euclid(TAU);
                if a <= self.hi {
                    pts.push(c + DVec2::from_angle(a) * r);
                }
            }
        }
        let min = pts
            .iter()
            .fold(DVec2::splat(f64::INFINITY), |m, p| m.min(*p));
        let max = pts
            .iter()
            .fold(DVec2::splat(f64::NEG_INFINITY), |m, p| m.max(*p));
        AABB::from_corners(min.to_array(), max.to_array())
    }
}

/// An index entry: a box and what it holds.
struct Boxed {
    env: AABB<[f64; 2]>,
    n: usize,
}

impl RTreeObject for Boxed {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> Self::Envelope {
        self.env
    }
}

fn grow(b: &AABB<[f64; 2]>, by: f64) -> AABB<[f64; 2]> {
    let (lo, hi) = (b.lower(), b.upper());
    AABB::from_corners([lo[0] - by, lo[1] - by], [hi[0] + by, hi[1] + by])
}

fn around(p: DVec2, by: f64) -> AABB<[f64; 2]> {
    AABB::from_corners([p.x - by, p.y - by], [p.x + by, p.y + by])
}

/// The ids in a tree whose boxes meet `b`, in order.
fn query(tree: &RTree<Boxed>, b: &AABB<[f64; 2]>) -> Vec<usize> {
    let mut v: Vec<usize> = tree
        .locate_in_envelope_intersecting(*b)
        .map(|e| e.n)
        .collect();
    v.sort_unstable();
    v
}

/// Two faces that pair: `i` and `j` (pieces), which side of each the other is on (+1: left),
/// the thickness, and the centre piece between them over their overlap.
#[derive(Clone, Copy, Debug)]
struct Pair {
    i: usize,
    j: usize,
    side_i: i8,
    side_j: i8,
    t: f64,
    varies: bool,
    /// The overlap on each line's own carrier.
    cover_i: (f64, f64),
    cover_j: (f64, f64),
    centre: Piece,
}

impl Pair {
    fn length(&self) -> f64 {
        self.centre.length()
    }
}

/// Union-find over indices.
struct Sets(Vec<usize>);

impl Sets {
    fn new(n: usize) -> Self {
        Sets((0..n).collect())
    }

    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            self.0[hi] = lo;
        }
    }
}

/// A wall found before it is built: its centre carrier, from `lo` to `hi`.
#[derive(Clone, Debug)]
struct Strip {
    carrier: Carrier,
    lo: f64,
    hi: f64,
    t: f64,
    group: u32,
    pairs: Vec<usize>,
    /// Faces: the pieces of its pairs.
    faces: BTreeSet<usize>,
    /// Ends at an opening's jamb (lo, hi): capped, never welded.
    jamb: [bool; 2],
    /// One of a run of collinear strips with openings between them.
    in_run: bool,
}

impl Strip {
    fn end(&self, k: usize) -> DVec2 {
        self.carrier.at(if k == 0 { self.lo } else { self.hi })
    }

    /// The direction it leaves end `k` in.
    fn out(&self, k: usize) -> DVec2 {
        if k == 0 {
            -self.carrier.tangent(self.lo)
        } else {
            self.carrier.tangent(self.hi)
        }
    }

    fn length(&self) -> f64 {
        (self.hi - self.lo) * self.carrier.scale()
    }

    fn mid(&self) -> f64 {
        (self.lo + self.hi) / 2.0
    }

    fn bounds(&self) -> AABB<[f64; 2]> {
        Piece {
            carrier: self.carrier,
            lo: self.lo,
            hi: self.hi,
        }
        .bounds()
    }
}

/// Find the walls among `lines`.
pub fn find(lines: &[LineIn], s: &Settings) -> Found {
    let pieces: Vec<Option<Piece>> = lines.iter().map(|l| Piece::of(&l.seg)).collect();
    let faces: Vec<usize> = (0..lines.len())
        .filter(|&n| !lines[n].guide && pieces[n].is_some_and(|p| p.length() > s.tol()))
        .collect();
    let mut found = Found::default();

    // 1. Pair.
    let tree = RTree::bulk_load(
        faces
            .iter()
            .map(|&n| Boxed {
                env: pieces[n].expect("a piece").bounds(),
                n,
            })
            .collect(),
    );
    let mut pairs: Vec<Pair> = Vec::new();
    for &i in &faces {
        let pi = pieces[i].expect("a piece");
        for j in query(&tree, &grow(&pi.bounds(), s.max_thickness + s.tol())) {
            if j <= i || lines[j].group != lines[i].group {
                continue;
            }
            if let Some(p) = pair(i, &pi, j, &pieces[j].expect("a piece"), s) {
                pairs.push(p);
            }
        }
    }
    // A line midway between two faces is a centreline (drawn solid, as some plans and older
    // scripts have it), not a face: the faces pair across it.
    let middle = middle_lines(&pairs, s);
    pairs.retain(|p| !middle.contains(&p.i) && !middle.contains(&p.j));
    let guide: Vec<bool> = (0..lines.len())
        .map(|n| lines[n].guide || middle.contains(&n))
        .collect();
    // Each line's partners on each side, nearest first, while they add cover.
    let mut by_line: HashMap<usize, [Vec<usize>; 2]> = HashMap::new();
    for (k, p) in pairs.iter().enumerate() {
        by_line.entry(p.i).or_default()[side(p.side_i)].push(k);
        by_line.entry(p.j).or_default()[side(p.side_j)].push(k);
    }
    let mut kept: HashMap<usize, [BTreeSet<usize>; 2]> = HashMap::new();
    let mut chosen: HashMap<usize, Option<usize>> = HashMap::new();
    let mut lines_sorted: Vec<usize> = by_line.keys().copied().collect();
    lines_sorted.sort_unstable();
    for &n in &lines_sorted {
        let sides = &by_line[&n];
        let mut keep: [BTreeSet<usize>; 2] = Default::default();
        // The nearest steady partner on each side, else the nearest varying one.
        let mut nearest = [f64::INFINITY; 2];
        let mut nearest_varying = [f64::INFINITY; 2];
        for sd in 0..2 {
            let mut list = sides[sd].clone();
            // Steady partners first: a line slanting across the wall (a fill's triangle edge)
            // doesn't hide the face beyond it.
            list.sort_by(|a, b| {
                let (pa, pb) = (&pairs[*a], &pairs[*b]);
                (pa.varies, pa.t)
                    .partial_cmp(&(pb.varies, pb.t))
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.cmp(b))
            });
            let mut covered: Vec<(f64, f64)> = Vec::new();
            for k in list {
                let c = pairs[k].cover_of(n);
                let len = c.1 - c.0;
                if len <= 0.0 {
                    continue;
                }
                if uncovered(&covered, c) >= 0.5 * len {
                    covered.push(c);
                    let near = if pairs[k].varies {
                        &mut nearest_varying[sd]
                    } else {
                        &mut nearest[sd]
                    };
                    *near = near.min(pairs[k].t);
                    keep[sd].insert(k);
                }
            }
        }
        if !nearest[0].is_finite() && !nearest[1].is_finite() {
            nearest = nearest_varying;
        }
        let pick = match (nearest[0].is_finite(), nearest[1].is_finite()) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            (true, true) if nearest[0] < 0.9 * nearest[1] => Some(0),
            (true, true) if nearest[1] < 0.9 * nearest[0] => Some(1),
            _ => None,
        };
        chosen.insert(n, pick);
        kept.insert(n, keep);
    }
    let keeps = |n: usize, k: usize, sd: i8| kept[&n][side(sd)].contains(&k);
    let mut good: Vec<usize> = Vec::new();
    let mut pattern = Sets::new(lines.len());
    let mut in_pattern: BTreeSet<usize> = BTreeSet::new();
    let mut varying: Vec<usize> = Vec::new();
    for (k, p) in pairs.iter().enumerate() {
        if !(keeps(p.i, k, p.side_i) && keeps(p.j, k, p.side_j)) {
            continue;
        }
        let (ci, cj) = (chosen[&p.i], chosen[&p.j]);
        if ci.is_none() || cj.is_none() {
            pattern.join(p.i, p.j);
            in_pattern.extend([p.i, p.j]);
        } else if ci == Some(side(p.side_i)) && cj == Some(side(p.side_j)) {
            if p.varies {
                varying.push(k);
            } else {
                good.push(k);
            }
        }
    }
    let mut patterns: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &n in &in_pattern {
        patterns.entry(pattern.find(n)).or_default().push(n);
    }
    for (_, ns) in patterns {
        let at = pieces[ns[0]].expect("a piece").ends()[0];
        found.skipped.push(Skip {
            lines: ns.iter().map(|n| lines[*n].id).collect(),
            why: Why::Pattern,
            at,
        });
    }
    for k in varying {
        let p = &pairs[k];
        found.skipped.push(Skip {
            lines: vec![lines[p.i].id, lines[p.j].id],
            why: Why::SpacingVaries,
            at: p.centre.carrier.at((p.centre.lo + p.centre.hi) / 2.0),
        });
    }

    // 2. Strips: pairs sharing a line that agree.
    let mut sets = Sets::new(pairs.len());
    let mut of_line: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &k in &good {
        of_line.entry(pairs[k].i).or_default().push(k);
        of_line.entry(pairs[k].j).or_default().push(k);
    }
    for ks in of_line.values() {
        for (x, &a) in ks.iter().enumerate() {
            for &b in &ks[x + 1..] {
                if agree(&pairs[a], &pairs[b], s) {
                    sets.join(a, b);
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &k in &good {
        groups.entry(sets.find(k)).or_default().push(k);
    }
    let mut strips: Vec<Strip> = groups
        .into_values()
        .filter_map(|ks| strip(&pairs, ks, lines))
        .collect();

    // 3. Gaps between collinear strips: crossings close, openings stay.
    strips = close_gaps(strips, &pairs, lines, s, &mut found);

    // Thickness: a preset, or rounded.
    for st in &mut strips {
        st.t = s.snapped(st.t);
    }

    // 4. Weld ends.
    let (nodes, at) = weld(&mut strips, s);

    // Short pieces on their own are furniture or columns.
    let mut keep = vec![true; strips.len()];
    let mut welded = vec![[false; 2]; strips.len()];
    for (&(n, k), &node) in &at {
        let shared = at.values().filter(|&&v| v == node).count() > 1;
        welded[n][k] = shared || nodes[node].1;
    }
    for (n, st) in strips.iter().enumerate() {
        if !st.in_run && !welded[n][0] && !welded[n][1] && st.length() < 4.0 * st.t {
            keep[n] = false;
            found.skipped.push(Skip {
                lines: st.faces.iter().map(|f| lines[*f].id).collect(),
                why: Why::Object,
                at: st.carrier.at(st.mid()),
            });
        }
    }

    // 5. Runs.
    runs(
        &strips, &keep, &nodes, &at, lines, &pieces, &faces, &guide, s, &mut found,
    );
    found
        .skipped
        .sort_by(|a, b| (a.why, &a.lines).cmp(&(b.why, &b.lines)));
    found
}

fn side(sd: i8) -> usize {
    usize::from(sd < 0)
}

/// Lines with a partner on each side at the same distance, where those two partners pair
/// with each other at twice it and have nothing at that distance beyond them: centrelines.
/// (In a hatch the partners have more lines beyond, so it stays a pattern.)
fn middle_lines(pairs: &[Pair], s: &Settings) -> HashSet<usize> {
    // Each line's partners on each side: (partner, distance).
    let mut sides: HashMap<usize, [Vec<(usize, f64)>; 2]> = HashMap::new();
    let mut paired: HashMap<(usize, usize), f64> = HashMap::new();
    for p in pairs.iter().filter(|p| !p.varies) {
        sides.entry(p.i).or_default()[side(p.side_i)].push((p.j, p.t));
        sides.entry(p.j).or_default()[side(p.side_j)].push((p.i, p.t));
        paired.insert((p.i.min(p.j), p.i.max(p.j)), p.t);
    }
    let nearest = |n: usize, sd: usize| {
        sides.get(&n).map_or(f64::INFINITY, |l| {
            l[sd].iter().map(|x| x.1).fold(f64::INFINITY, f64::min)
        })
    };
    let mut out = HashSet::new();
    for (&m, l) in &sides {
        let (d0, d1) = (nearest(m, 0), nearest(m, 1));
        if !(d0.is_finite() && d1.is_finite()) || (d0 - d1).abs() > s.spread(d0.max(d1)) {
            continue;
        }
        let d = (d0 + d1) / 2.0;
        let near = |x: f64| (x - d).abs() <= s.spread(d);
        // A partner with nothing at `d` on its far side (away from m).
        let clean = |p: usize| {
            let towards = sides[&p]
                .iter()
                .position(|l| l.iter().any(|x| x.0 == m))
                .unwrap_or(0);
            !near(nearest(p, 1 - towards))
        };
        let found = l[0].iter().filter(|x| near(x.1)).any(|&(p, _)| {
            l[1].iter().filter(|x| near(x.1)).any(|&(q, _)| {
                paired
                    .get(&(p.min(q), p.max(q)))
                    .is_some_and(|t| (t - 2.0 * d).abs() <= s.spread(2.0 * d))
                    && clean(p)
                    && clean(q)
            })
        });
        if found {
            out.insert(m);
        }
    }
    out
}

impl Pair {
    fn cover_of(&self, n: usize) -> (f64, f64) {
        if n == self.i {
            self.cover_i
        } else {
            self.cover_j
        }
    }
}

/// How much of `c` the intervals in `covered` leave uncovered.
fn uncovered(covered: &[(f64, f64)], c: (f64, f64)) -> f64 {
    let mut parts: Vec<(f64, f64)> = covered
        .iter()
        .map(|&(a, b)| (a.max(c.0), b.min(c.1)))
        .filter(|(a, b)| b > a)
        .collect();
    parts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut sum, mut end) = (0.0, c.0);
    for (a, b) in parts {
        if b > end {
            sum += b - a.max(end);
            end = b;
        }
    }
    (c.1 - c.0) - sum
}

/// Whether two pieces pair as the faces of a wall.
fn pair(i: usize, pi: &Piece, j: usize, pj: &Piece, s: &Settings) -> Option<Pair> {
    match (pi.carrier, pj.carrier) {
        (Carrier::Line { p: a, d: di }, Carrier::Line { p: b, d: dj }) => {
            let flip = di.dot(dj) < 0.0;
            let dj2 = if flip { -dj } else { dj };
            if di.perp_dot(dj2).abs() > s.angle.sin() {
                return None;
            }
            let dm = (di + dj2).normalize();
            let nm = dm.perp();
            // Each line as (along, across) in the mean frame, as a function of along.
            let frame = |p: DVec2, d: DVec2, len: f64| {
                let (x, y) = (p, p + d * len);
                let (sx, sy) = (x.dot(dm), y.dot(dm));
                let (ox, oy) = (x.dot(nm), y.dot(nm));
                let slope = (oy - ox) / (sy - sx);
                let (lo, hi) = (sx.min(sy), sx.max(sy));
                (lo, hi, move |u: f64| ox + (u - sx) * slope)
            };
            let (ilo, ihi, oi) = frame(a, di, pi.hi);
            let (jlo, jhi, oj) = frame(b, dj, pj.hi);
            let (lo, hi) = (ilo.max(jlo), ihi.min(jhi));
            if hi <= lo {
                return None;
            }
            let (wlo, whi) = (oj(lo) - oi(lo), oj(hi) - oi(hi));
            if wlo.signum() != whi.signum() {
                return None;
            }
            let t = (wlo.abs() + whi.abs()) / 2.0;
            if t < s.min_thickness || t > s.max_thickness || hi - lo < 0.5 * t {
                return None;
            }
            let varies = (wlo - whi).abs() > s.spread(t);
            let pt = |u: f64, o: f64| dm * u + nm * o;
            let (clo, chi) = (
                pt(lo, (oi(lo) + oj(lo)) / 2.0),
                pt(hi, (oi(hi) + oj(hi)) / 2.0),
            );
            let own = |p: DVec2, d: DVec2, o: &dyn Fn(f64) -> f64| {
                let (x, y) = ((pt(lo, o(lo)) - p).dot(d), (pt(hi, o(hi)) - p).dot(d));
                (x.min(y), x.max(y))
            };
            let w = wlo + whi;
            Some(Pair {
                i,
                j,
                side_i: if w * nm.dot(di.perp()) > 0.0 { 1 } else { -1 },
                side_j: if -w * nm.dot(dj.perp()) > 0.0 { 1 } else { -1 },
                t,
                varies,
                cover_i: own(a, di, &oi),
                cover_j: own(b, dj, &oj),
                centre: Piece {
                    carrier: Carrier::Line { p: clo, d: dm },
                    lo: 0.0,
                    hi: clo.distance(chi),
                },
            })
        }
        (Carrier::Circle { c: ci, r: ri }, Carrier::Circle { c: cj, r: rj }) => {
            let t = (ri - rj).abs();
            if t < s.min_thickness || t > s.max_thickness {
                return None;
            }
            let off = ci.distance(cj);
            if off > t / 2.0 {
                return None;
            }
            let mid = (pi.lo + pi.hi) / 2.0;
            let shift = ((pj.lo + pj.hi) / 2.0 - mid + PI).rem_euclid(TAU) - PI + mid
                - (pj.lo + pj.hi) / 2.0;
            let (jlo, jhi) = (pj.lo + shift, pj.hi + shift);
            let (lo, hi) = (pi.lo.max(jlo), pi.hi.min(jhi));
            let r = (ri + rj) / 2.0;
            if hi <= lo || (hi - lo) * r < 0.5 * t {
                return None;
            }
            // Turning counter-clockwise, the left is the inside.
            let inside = |me: f64, other: f64| if other < me { 1 } else { -1 };
            Some(Pair {
                i,
                j,
                side_i: inside(ri, rj),
                side_j: inside(rj, ri),
                t,
                varies: off > s.spread(t),
                cover_i: (lo, hi),
                cover_j: (lo - shift, hi - shift),
                centre: Piece {
                    carrier: Carrier::Circle {
                        c: (ci + cj) / 2.0,
                        r,
                    },
                    lo,
                    hi,
                },
            })
        }
        _ => None,
    }
}

/// Two pairs are one wall: the same thickness on one centreline.
fn agree(a: &Pair, b: &Pair, s: &Settings) -> bool {
    let tol = s.spread(a.t.max(b.t));
    if (a.t - b.t).abs() > tol {
        return false;
    }
    on_carrier(&a.centre.carrier, &b.centre, tol, s.angle)
}

/// Whether piece `p` lies along carrier `c` (within `tol` across, `angle` in direction).
fn on_carrier(c: &Carrier, p: &Piece, tol: f64, angle: f64) -> bool {
    match (*c, p.carrier) {
        (Carrier::Line { d, .. }, Carrier::Line { d: e, .. }) => {
            d.perp_dot(e).abs() <= angle.sin() && p.ends().iter().all(|x| c.offset(*x).abs() <= tol)
        }
        (Carrier::Circle { c: c1, r: r1 }, Carrier::Circle { c: c2, r: r2 }) => {
            c1.distance(c2) <= tol && (r1 - r2).abs() <= tol
        }
        _ => false,
    }
}

/// A strip from pairs: their centres averaged, weighted by length.
fn strip(pairs: &[Pair], ks: Vec<usize>, lines: &[LineIn]) -> Option<Strip> {
    let first = pairs[*ks.first()?];
    let weight: f64 = ks.iter().map(|k| pairs[*k].length()).sum::<f64>().max(1e-9);
    let t = ks
        .iter()
        .map(|k| pairs[*k].t * pairs[*k].length())
        .sum::<f64>()
        / weight;
    let carrier = match first.centre.carrier {
        Carrier::Line { d: d0, .. } => {
            let mut d = DVec2::ZERO;
            let mut m = DVec2::ZERO;
            for k in &ks {
                let c = &pairs[*k].centre;
                let Carrier::Line { d: e, .. } = c.carrier else {
                    continue;
                };
                let w = c.length();
                d += if e.dot(d0) < 0.0 { -e } else { e } * w;
                m += c.carrier.at((c.lo + c.hi) / 2.0) * w;
            }
            Carrier::Line {
                p: m / weight,
                d: d.try_normalize()?,
            }
        }
        Carrier::Circle { .. } => {
            let (mut c0, mut r0) = (DVec2::ZERO, 0.0);
            for k in &ks {
                if let Carrier::Circle { c, r } = pairs[*k].centre.carrier {
                    let w = pairs[*k].length();
                    c0 += c * w;
                    r0 += r * w;
                }
            }
            Carrier::Circle {
                c: c0 / weight,
                r: r0 / weight,
            }
        }
    };
    let near = (first.centre.lo + first.centre.hi) / 2.0;
    let near = carrier.param(first.centre.carrier.at(near), 0.0);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for k in &ks {
        for x in pairs[*k].centre.ends() {
            let u = carrier.param(x, near);
            lo = lo.min(u);
            hi = hi.max(u);
        }
    }
    if matches!(carrier, Carrier::Circle { .. }) && hi - lo >= TAU - 1e-6 {
        // A whole ring: not handled (round rooms come as circles, not arcs).
        return None;
    }
    let faces = ks.iter().flat_map(|k| [pairs[*k].i, pairs[*k].j]).collect();
    Some(Strip {
        carrier,
        lo,
        hi,
        t,
        group: lines[first.i].group,
        pairs: ks,
        faces,
        jamb: [false; 2],
        in_run: false,
    })
}

/// Join collinear strips across gaps that other walls cross or tee into, and note the other
/// gaps (up to an opening's width) as openings.
fn close_gaps(
    strips: Vec<Strip>,
    pairs: &[Pair],
    lines: &[LineIn],
    s: &Settings,
    found: &mut Found,
) -> Vec<Strip> {
    let tree = RTree::bulk_load(
        strips
            .iter()
            .enumerate()
            .map(|(n, st)| Boxed {
                env: st.bounds(),
                n,
            })
            .collect(),
    );
    let mut sets = Sets::new(strips.len());
    // (a, b, gap, where along a): b after a along a's carrier.
    let mut gaps: Vec<(usize, usize, f64, f64)> = Vec::new();
    for (a, sa) in strips.iter().enumerate() {
        for b in query(&tree, &grow(&sa.bounds(), s.max_opening)) {
            let sb = &strips[b];
            if b == a || sb.group != sa.group {
                continue;
            }
            let tol = s.spread(sa.t.max(sb.t));
            if (sa.t - sb.t).abs() > tol {
                continue;
            }
            let pb = Piece {
                carrier: sb.carrier,
                lo: sb.lo,
                hi: sb.hi,
            };
            if !on_carrier(&sa.carrier, &pb, tol, s.angle) {
                continue;
            }
            let (u0, u1) = {
                let [x, y] = pb.ends();
                let (u, v) = (sa.carrier.param(x, sa.mid()), sa.carrier.param(y, sa.mid()));
                (u.min(v), u.max(v))
            };
            let scale = sa.carrier.scale();
            if u0 <= sa.hi + s.tol() / scale && u1 >= sa.lo - s.tol() / scale {
                // Overlapping or touching: one wall.
                sets.join(a, b);
            } else if u0 > sa.hi {
                let gap = (u0 - sa.hi) * scale;
                if gap <= s.max_opening {
                    gaps.push((a, b, gap, sa.hi));
                }
            }
        }
    }
    gaps.sort_by(|x, y| x.2.total_cmp(&y.2).then((x.0, x.1).cmp(&(y.0, y.1))));
    let mut openings: Vec<(usize, usize, f64, f64)> = Vec::new();
    for &(a, b, gap, from) in &gaps {
        let sa = &strips[a];
        let to = from + gap / sa.carrier.scale();
        // Another wall ending in the gap, at this wall: a crossing or tees from both sides.
        let mid = sa.carrier.at((from + to) / 2.0);
        let reach = gap / 2.0 + s.max_thickness + s.tol();
        let crossed = query(&tree, &around(mid, reach)).into_iter().any(|c| {
            let sc = &strips[c];
            if c == a || c == b {
                return false;
            }
            (0..2).any(|k| {
                let e = sc.end(k);
                let u = sa.carrier.param(e, sa.mid());
                let across = sa.carrier.offset(e).abs();
                let tangent = sa.carrier.tangent(u);
                tangent.perp_dot(sc.out(k)).abs() > 0.2
                    && u >= from - s.tol() / sa.carrier.scale()
                    && u <= to + s.tol() / sa.carrier.scale()
                    && across <= sa.t / 2.0 + sc.t + s.tol()
            })
        });
        if crossed && gap <= s.max_thickness / 0.2 + s.tol() {
            sets.join(a, b);
        } else {
            openings.push((a, b, gap, from));
        }
    }
    // Merge.
    let mut merged: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for n in 0..strips.len() {
        merged.entry(sets.find(n)).or_default().push(n);
    }
    let mut index: HashMap<usize, usize> = HashMap::new();
    let mut out: Vec<Strip> = Vec::new();
    for (_, ns) in merged {
        let ks: Vec<usize> = ns
            .iter()
            .flat_map(|n| strips[*n].pairs.iter().copied())
            .collect();
        let Some(st) = (if ns.len() == 1 {
            Some(strips[ns[0]].clone())
        } else {
            strip(pairs, ks, lines)
        }) else {
            continue;
        };
        for n in ns {
            index.insert(n, out.len());
        }
        out.push(st);
    }
    let mut seen = HashSet::new();
    for (a, b, gap, from) in openings {
        let (na, nb) = (index[&a], index[&b]);
        if na == nb || !seen.insert((na.min(nb), na.max(nb), from.to_bits())) {
            continue;
        }
        let sa = &strips[a];
        let mid = sa.carrier.at(from + gap / sa.carrier.scale() / 2.0);
        // The ends at the jambs: a's at `from`, b's facing it.
        for (n, x) in [
            (na, sa.carrier.at(from)),
            (nb, sa.carrier.at(from + gap / sa.carrier.scale())),
        ] {
            let st = &mut out[n];
            let k = usize::from(st.end(1).distance(x) < st.end(0).distance(x));
            if st.end(k).distance(x) <= s.spread(st.t) + s.tol() {
                st.jamb[k] = true;
            }
            st.in_run = true;
        }
        found.openings.push((mid, gap));
    }
    out
}

/// Weld strip ends where walls meet: each end moves to where its centreline crosses the wall it
/// runs into. Returns the nodes (position, and whether it is a T on another wall) and each
/// end's node.
#[allow(clippy::type_complexity)]
fn weld(
    strips: &mut [Strip],
    s: &Settings,
) -> (Vec<(DVec2, bool)>, BTreeMap<(usize, usize), usize>) {
    let tree = RTree::bulk_load(
        strips
            .iter()
            .enumerate()
            .map(|(n, st)| Boxed {
                env: st.bounds(),
                n,
            })
            .collect(),
    );
    let min_sin = 10f64.to_radians().sin();
    // For each end: the strip it runs into, where, and whether that is near the other's end.
    enum Hit {
        L(usize, usize, DVec2),
        T(DVec2),
    }
    let mut hits: BTreeMap<(usize, usize), (f64, Hit)> = BTreeMap::new();
    for a in 0..strips.len() {
        for k in 0..2 {
            let sa = &strips[a];
            if sa.jamb[k] {
                continue;
            }
            let e = sa.end(k);
            let reach = (s.max_thickness + sa.t) / min_sin + s.tol();
            for c in query(&tree, &around(e, reach)) {
                let sc = &strips[c];
                if c == a || sc.group != sa.group {
                    continue;
                }
                let Some(x) = sa
                    .carrier
                    .cross(&sc.carrier)
                    .into_iter()
                    .filter(|p| p.is_finite())
                    .min_by(|p, q| p.distance(e).total_cmp(&q.distance(e)))
                else {
                    continue;
                };
                let ua = sa.carrier.param(x, if k == 0 { sa.lo } else { sa.hi });
                let uc = sc.carrier.param(x, sc.mid());
                let (ta, tc) = (sa.carrier.tangent(ua), sc.carrier.tangent(uc));
                let sin = ta.perp_dot(tc).abs();
                if sin < min_sin {
                    continue;
                }
                let cot = ta.dot(tc).abs() / sin;
                // How far the end may be from the crossing: the other wall's half thickness
                // across, plus this wall's slant.
                let limit_a = sc.t / 2.0 / sin + sa.t / 2.0 * cot + s.tol() + s.spread(sa.t);
                let limit_c = sa.t / 2.0 / sin + sc.t / 2.0 * cot + s.tol() + s.spread(sc.t);
                let along = (x - e).dot(sa.out(k));
                let gap = x.distance(e);
                if gap > limit_a || along < -limit_a {
                    continue;
                }
                let scale = sc.carrier.scale();
                if uc < sc.lo - limit_c / scale || uc > sc.hi + limit_c / scale {
                    continue;
                }
                let (d0, d1) = ((uc - sc.lo) * scale, (sc.hi - uc) * scale);
                let hit = if d0.min(d1) <= limit_c {
                    let kc = usize::from(d1 < d0);
                    if sc.jamb[kc] {
                        continue;
                    }
                    Hit::L(c, kc, x)
                } else {
                    Hit::T(x)
                };
                if hits.get(&(a, k)).is_none_or(|(g, _)| gap < *g) {
                    hits.insert((a, k), (gap, hit));
                }
            }
        }
    }
    // Ends meeting at a corner share a node.
    let ends: Vec<(usize, usize)> = (0..strips.len()).flat_map(|n| [(n, 0), (n, 1)]).collect();
    let slot = |e: (usize, usize)| e.0 * 2 + e.1;
    let mut sets = Sets::new(ends.len());
    for (&e, (_, hit)) in &hits {
        if let Hit::L(c, kc, _) = hit {
            sets.join(slot(e), slot((*c, *kc)));
        }
    }
    // Ends already on each other (a curved wall carrying straight on, a step in thickness)
    // share a node too.
    let end_tree = RTree::bulk_load(
        ends.iter()
            .enumerate()
            .map(|(n, &(s0, k))| Boxed {
                env: around(strips[s0].end(k), 0.0),
                n,
            })
            .collect(),
    );
    for (n, &(a, k)) in ends.iter().enumerate() {
        let sa = &strips[a];
        if sa.jamb[k] || matches!(hits.get(&(a, k)), Some((_, Hit::T(_)))) {
            continue;
        }
        let reach = s.tol() + s.spread(sa.t);
        for m in query(&end_tree, &around(sa.end(k), reach)) {
            let (c, kc) = ends[m];
            let sc = &strips[c];
            if c != a
                && sc.group == sa.group
                && !sc.jamb[kc]
                && sc.end(kc).distance(sa.end(k)) <= reach
                && !matches!(hits.get(&(c, kc)), Some((_, Hit::T(_))))
            {
                sets.join(n, m);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    for &e in &ends {
        groups.entry(sets.find(slot(e))).or_default().push(e);
    }
    let mut nodes: Vec<(DVec2, bool)> = Vec::new();
    let mut at: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (_, es) in groups {
        let (pos, tee) = if es.len() > 1 {
            let xs: Vec<DVec2> = es
                .iter()
                .filter_map(|e| match hits.get(e) {
                    Some((_, Hit::L(_, _, x))) => Some(*x),
                    _ => None,
                })
                .collect();
            // Where the corners say, or else between the ends.
            let xs = if xs.is_empty() {
                es.iter().map(|e| strips[e.0].end(e.1)).collect()
            } else {
                xs
            };
            let x = xs.iter().fold(DVec2::ZERO, |m, p| m + *p) / xs.len() as f64;
            (x, false)
        } else {
            match hits.get(&es[0]) {
                Some((_, Hit::T(x))) => (*x, true),
                _ => (strips[es[0].0].end(es[0].1), false),
            }
        };
        for e in es {
            at.insert(e, nodes.len());
        }
        nodes.push((pos, tee));
    }
    // Move the ends to their nodes.
    for (&(n, k), &node) in &at {
        let st = &mut strips[n];
        let u = st
            .carrier
            .param(nodes[node].0, if k == 0 { st.lo } else { st.hi });
        if k == 0 {
            st.lo = u;
        } else {
            st.hi = u;
        }
    }
    (nodes, at)
}

/// Chain the strips into runs and work out which lines each used.
#[allow(clippy::too_many_arguments)]
fn runs(
    strips: &[Strip],
    keep: &[bool],
    nodes: &[(DVec2, bool)],
    at: &BTreeMap<(usize, usize), usize>,
    lines: &[LineIn],
    pieces: &[Option<Piece>],
    faces: &[usize],
    guide: &[bool],
    s: &Settings,
    found: &mut Found,
) {
    // Which kept strip ends are at each node.
    let mut at_node: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    for (&(n, k), &node) in at {
        if keep[n] && strips[n].hi > strips[n].lo {
            at_node.entry(node).or_default().push((n, k));
        }
    }
    let chains = |e: (usize, usize)| -> Option<(usize, usize)> {
        let list = &at_node[&at[&e]];
        if list.len() != 2 || nodes[at[&e]].1 {
            return None;
        }
        let other = if list[0] == e { list[1] } else { list[0] };
        let (a, b) = (&strips[e.0], &strips[other.0]);
        (other.0 != e.0 && a.group == b.group && a.t == b.t).then_some(other)
    };
    // Points: one per node in use.
    let mut point_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut point = |node: usize, found: &mut Found| {
        *point_of.entry(node).or_insert_with(|| {
            found.points.push(nodes[node].0);
            found.points.len() - 1
        })
    };
    let mut done = vec![false; strips.len()];
    let usable: Vec<usize> = (0..strips.len())
        .filter(|n| keep[*n] && strips[*n].hi > strips[*n].lo)
        .collect();
    // Open runs first (from an end that doesn't chain), then rings.
    let mut starts: Vec<(usize, usize)> = Vec::new();
    for &n in &usable {
        for k in 0..2 {
            if chains((n, k)).is_none() {
                starts.push((n, k));
            }
        }
    }
    starts.extend(usable.iter().map(|n| (*n, 0)));
    let tree = RTree::bulk_load(
        (0..lines.len())
            .filter_map(|n| pieces[n].map(|p| Boxed { env: p.bounds(), n }))
            .collect(),
    );
    let face_set: HashSet<usize> = faces.iter().copied().collect();
    let mut claimed: HashSet<usize> = HashSet::new();
    let skipped: HashSet<EntityId> = found
        .skipped
        .iter()
        .flat_map(|sk| sk.lines.iter().copied())
        .collect();
    for (n0, k0) in starts {
        if done[n0] {
            continue;
        }
        // Walk from end k0 of n0 through the strip and on.
        let (mut n, mut from) = (n0, k0);
        let mut segs: Vec<(Seg, usize, usize)> = Vec::new();
        let mut members: Vec<usize> = Vec::new();
        let mut closed = false;
        loop {
            done[n] = true;
            members.push(n);
            let st = &strips[n];
            let (pa, pb) = (
                point(at[&(n, from)], found),
                point(at[&(n, 1 - from)], found),
            );
            let (xa, xb) = (found.points[pa], found.points[pb]);
            let seg = match st.carrier {
                Carrier::Line { .. } => Seg::Line { a: xa, b: xb },
                Carrier::Circle { c, r } => Seg::Arc {
                    center: c,
                    radius: r,
                    a: xa,
                    b: xb,
                    ccw: from == 0,
                },
            };
            segs.push((seg, pa, pb));
            let Some(next) = chains((n, 1 - from)) else {
                break;
            };
            if next.0 == n0 {
                closed = true;
                break;
            }
            if done[next.0] {
                break;
            }
            (n, from) = next;
        }
        // The lines it used.
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for &m in &members {
            let st = &strips[m];
            let ext = |k: usize| {
                let node = at[&(m, k)];
                let others = at_node
                    .get(&node)
                    .map_or(0.0, |l| l.iter().map(|e| strips[e.0].t).fold(0.0, f64::max));
                if others > 0.0 && (at_node[&node].len() > 1 || nodes[node].1) {
                    2.0 * others.max(st.t)
                } else {
                    st.t
                }
            };
            let (lo, hi) = (
                st.lo - (ext(0) + s.tol()) / st.carrier.scale(),
                st.hi + (ext(1) + s.tol()) / st.carrier.scale(),
            );
            let tol = s.spread(st.t) + s.tol();
            let within = |x: DVec2| {
                let u = st.carrier.param(x, st.mid());
                u >= lo && u <= hi
            };
            for &f in &st.faces {
                let p = pieces[f].expect("a piece");
                if p.ends().iter().all(|x| within(*x)) {
                    used.insert(f);
                } else {
                    found.partly.push(lines[f].id);
                }
            }
            // End caps, stray pieces of face and old centrelines.
            let mut env = st.bounds();
            env = grow(&env, st.t + tol);
            for c in query(&tree, &env) {
                if used.contains(&c) || st.faces.contains(&c) || claimed.contains(&c) {
                    continue;
                }
                if skipped.contains(&lines[c].id) {
                    continue;
                }
                let p = pieces[c].expect("a piece");
                let ends = p.ends();
                let offs: Vec<f64> = ends.iter().map(|x| st.carrier.offset(*x)).collect();
                let on = |o: f64, want: f64| (o - want).abs() <= tol;
                let half = st.t / 2.0;
                let take = if guide[c] {
                    offs.iter().all(|o| on(*o, 0.0)) && ends.iter().all(|x| within(*x))
                } else if !face_set.contains(&c) && !matches!(p.carrier, Carrier::Line { .. }) {
                    false
                } else {
                    // A piece of a face: on it and of some length (not a speck of something
                    // else drawn there, as PDFs have).
                    let along_face = p.length() >= 0.25 * st.t
                        && (offs.iter().all(|o| on(*o, half))
                            || offs.iter().all(|o| on(*o, -half)))
                        && ends.iter().all(|x| within(*x));
                    // A cap: one end on each face, near one of the wall's free ends.
                    let cap = ((on(offs[0], half) && on(offs[1], -half))
                        || (on(offs[0], -half) && on(offs[1], half)))
                        && (0..2).any(|k| {
                            let free = at_node.get(&at[&(m, k)]).is_none_or(|l| l.len() == 1)
                                && !nodes[at[&(m, k)]].1;
                            let end = if k == 0 { st.lo } else { st.hi };
                            free && ends.iter().all(|x| {
                                (st.carrier.param(*x, end) - end).abs() * st.carrier.scale()
                                    <= st.t + tol
                            })
                        });
                    along_face || cap
                };
                if take {
                    used.insert(c);
                }
            }
        }
        claimed.extend(used.iter().copied());
        let t = strips[n0].t;
        found.walls.push(FoundWall {
            segs,
            closed,
            thickness: t,
            group: strips[n0].group,
            lines: used.iter().map(|c| lines[*c].id).collect(),
        });
    }
    found.partly.sort_unstable();
    found.partly.dedup();
    let used = found.used();
    found.partly.retain(|id| !used.contains(id));
}

#[cfg(test)]
mod tests;
