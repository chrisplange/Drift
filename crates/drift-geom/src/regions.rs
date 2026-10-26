//! Closed regions formed by curves: the faces of the planar arrangement of lines, arcs and
//! circles, with exact areas, perimeters and centroids.
//!
//! Curves are split where they cross, and every end and crossing is snapped to one vertex
//! set with one tolerance ([`RegionTolerance`]); each curve then passes through every vertex
//! within that tolerance of it. The pieces become edges of a graph, dangling edges are
//! pruned, and faces are traced by always taking the next edge clockwise at each vertex (so
//! each bounded face is traced counter-clockwise). Edges leaving a vertex in the same
//! direction (a fillet arc tangent to its line) are ordered by curvature. A separate shape
//! inside a region becomes a hole of the smallest region containing it.

use std::cmp::Ordering;
use std::f64::consts::TAU;

use drift_core::{Bounds, DVec2, Shape};

use crate::intersect::intersect;

/// One piece of a curve between two vertices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Line {
        a: DVec2,
        b: DVec2,
    },
    /// From `start` sweeping `sweep` radians (negative = clockwise).
    Arc {
        center: DVec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

impl Edge {
    fn reversed(&self) -> Edge {
        match *self {
            Edge::Line { a, b } => Edge::Line { a: b, b: a },
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => Edge::Arc {
                center,
                radius,
                start: start + sweep,
                sweep: -sweep,
            },
        }
    }

    pub fn start(&self) -> DVec2 {
        match *self {
            Edge::Line { a, .. } => a,
            Edge::Arc {
                center,
                radius,
                start,
                ..
            } => center + radius * DVec2::from_angle(start),
        }
    }

    pub fn end(&self) -> DVec2 {
        match *self {
            Edge::Line { b, .. } => b,
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => center + radius * DVec2::from_angle(start + sweep),
        }
    }

    /// Unit direction leaving the start, and signed curvature (left turns positive).
    fn departure(&self) -> (DVec2, f64) {
        match *self {
            Edge::Line { a, b } => ((b - a).normalize(), 0.0),
            Edge::Arc {
                radius,
                start,
                sweep,
                ..
            } => {
                let t = DVec2::from_angle(start).perp() * sweep.signum();
                (t, sweep.signum() / radius)
            }
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            Edge::Line { a, b } => a.distance(b),
            Edge::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }

    /// Twice the signed area this edge contributes to a loop (shoelace plus the circular
    /// segment between chord and arc).
    fn area2(&self) -> f64 {
        let (p, q) = (self.start(), self.end());
        let chord = p.perp_dot(q);
        match *self {
            Edge::Line { .. } => chord,
            Edge::Arc { radius, sweep, .. } => chord + radius * radius * (sweep - sweep.sin()),
        }
    }

    /// Points along the edge (start included, end excluded), at most `max_angle` apart on arcs.
    pub fn flatten(&self, max_angle: f64, out: &mut Vec<DVec2>) {
        match *self {
            Edge::Line { a, .. } => out.push(a),
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let n = (sweep.abs() / max_angle).ceil().clamp(1.0, 4096.0) as usize;
                for k in 0..n {
                    let a = start + sweep * k as f64 / n as f64;
                    out.push(center + radius * DVec2::from_angle(a));
                }
            }
        }
    }

    /// The shape this edge covers, for drawing.
    pub fn shape(&self) -> Shape {
        match *self {
            Edge::Line { a, b } => Shape::Line { a, b },
            Edge::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let (s, w) = if sweep >= 0.0 {
                    (start, sweep)
                } else {
                    (start + sweep, -sweep)
                };
                Shape::Arc {
                    center,
                    radius,
                    start: s,
                    sweep: w,
                }
            }
        }
    }
}

/// A closed loop of edges, in traversal order.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub edges: Vec<Edge>,
    /// For each edge, the index of the input curve it is a piece of.
    pub sources: Vec<usize>,
}

impl Loop {
    /// The input curves this loop runs along (indices into the `curves` given to
    /// [`regions`]), sorted and deduplicated.
    pub fn curves(&self) -> Vec<usize> {
        let mut c = self.sources.clone();
        c.sort_unstable();
        c.dedup();
        c
    }

    /// Signed area: positive counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        self.edges.iter().map(Edge::area2).sum::<f64>() / 2.0
    }

    pub fn length(&self) -> f64 {
        self.edges.iter().map(Edge::length).sum()
    }

    /// A polygon through the loop (arcs flattened to `max_angle` steps).
    pub fn polygon(&self, max_angle: f64) -> Vec<DVec2> {
        let mut out = Vec::new();
        for e in &self.edges {
            e.flatten(max_angle, &mut out);
        }
        out
    }

    /// Whether the loop winds round `p`.
    pub fn contains(&self, p: DVec2) -> bool {
        winding(&self.polygon(0.02), p) != 0
    }
}

/// Winding number of a polygon around `p`.
fn winding(poly: &[DVec2], p: DVec2) -> i32 {
    let mut w = 0;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if a.y <= p.y {
            if b.y > p.y && (b - a).perp_dot(p - a) > 0.0 {
                w += 1;
            }
        } else if b.y <= p.y && (b - a).perp_dot(p - a) < 0.0 {
            w -= 1;
        }
    }
    w
}

/// A closed region: an outer boundary (counter-clockwise) and holes (clockwise).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
    pub area: f64,
    /// Total boundary length, holes included.
    pub perimeter: f64,
    pub centroid: DVec2,
    /// Bounds of the outer boundary (for quick rejection).
    pub bounds: Bounds,
}

impl Region {
    /// An axis-aligned rectangle from `min` to `max`, built directly (for a swatch, say,
    /// without finding regions among curves).
    pub fn rectangle(min: DVec2, max: DVec2) -> Region {
        let corners = [min, DVec2::new(max.x, min.y), max, DVec2::new(min.x, max.y)];
        let edges = (0..4)
            .map(|i| Edge::Line {
                a: corners[i],
                b: corners[(i + 1) % 4],
            })
            .collect();
        let size = max - min;
        Region {
            outer: Loop {
                edges,
                sources: vec![0, 1, 2, 3],
            },
            holes: Vec::new(),
            area: size.x * size.y,
            perimeter: 2.0 * (size.x + size.y),
            centroid: (min + max) / 2.0,
            bounds: Bounds { min, max },
        }
    }

    /// A region from loops known exactly (a wall cluster's outline and the loops inside it),
    /// without finding regions among curves: `outer` counter-clockwise, `holes` clockwise.
    pub fn from_loops(outer: Loop, holes: Vec<Loop>) -> Region {
        let area = outer.signed_area() + holes.iter().map(Loop::signed_area).sum::<f64>();
        let perimeter = outer.length() + holes.iter().map(Loop::length).sum::<f64>();
        let mut r = Region {
            bounds: loop_bounds(&outer),
            outer,
            holes,
            area,
            perimeter,
            centroid: DVec2::ZERO,
        };
        r.centroid = centroid(&r);
        r
    }

    pub fn contains(&self, p: DVec2) -> bool {
        let b = &self.bounds;
        if p.x < b.min.x || p.x > b.max.x || p.y < b.min.y || p.y > b.max.y {
            return false;
        }
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
}

/// Bounds of a loop's edges.
fn loop_bounds(lp: &Loop) -> Bounds {
    lp.edges.iter().map(|e| e.shape().bounds()).collect()
}

/// The one set of tolerances a region build works with, derived from the drawing's extent
/// (its largest coordinate, which is what bounds floating-point error). Every stage uses
/// these and nothing else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionTolerance {
    /// Points this close are one vertex; a curve end this close to another curve lies on it.
    pub snap: f64,
    /// Pieces of curve this short collapse to a point.
    pub min_edge: f64,
    /// Loops enclosing less than this are numerical noise, not faces.
    pub min_area: f64,
}

impl RegionTolerance {
    /// Tolerances for coordinates up to `extent` in size.
    pub fn for_extent(extent: f64) -> Self {
        let scale = if extent.is_finite() {
            extent.abs().max(1.0)
        } else {
            1.0
        };
        let snap = 1e-9 * scale;
        RegionTolerance {
            snap,
            min_edge: snap,
            min_area: 1e-12 * scale * scale,
        }
    }

    /// Tolerances for a set of curves.
    pub fn for_curves(curves: &[Shape]) -> Self {
        let extent = curves
            .iter()
            .map(|c| {
                let b = c.bounds();
                b.min.abs().max(b.max.abs()).max_element()
            })
            .filter(|e| e.is_finite())
            .fold(0.0, f64::max);
        Self::for_extent(extent)
    }
}

/// The snap stage: points within `snap` of each other (transitively) become one vertex,
/// placed at the lowest of its points (by x, then y), and vertices are numbered in that
/// order. Neither depends on the order the points come in. Returns the vertices and each
/// point's vertex.
fn snap_points(points: &[DVec2], snap: f64) -> (Vec<DVec2>, Vec<usize>) {
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let n = points.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| by_position(points[a], points[b]));
    let mut parent: Vec<usize> = (0..n).collect();
    // Cells as large as the snap distance: a point's neighbours are in its own cell or the
    // eight around it.
    let cell = snap.max(f64::MIN_POSITIVE);
    let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    for &i in &order {
        let p = points[i];
        let (kx, ky) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &j in grid.get(&(kx + dx, ky + dy)).into_iter().flatten() {
                    if points[j].distance(p) <= snap {
                        let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                        parent[ri.max(rj)] = ri.min(rj);
                    }
                }
            }
        }
        grid.entry((kx, ky)).or_default().push(i);
    }
    let mut id_of_root = vec![usize::MAX; n];
    let mut verts = Vec::new();
    let mut vertex = vec![0; n];
    for &i in &order {
        let r = root(&mut parent, i);
        if id_of_root[r] == usize::MAX {
            id_of_root[r] = verts.len();
            verts.push(points[i]);
        }
        vertex[i] = id_of_root[r];
    }
    (verts, vertex)
}

fn by_position(p: DVec2, q: DVec2) -> Ordering {
    p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y))
}

/// A key ordering curves by where they are, so each pair is intersected the same way round
/// whatever order the curves were given in.
fn curve_key(s: &Shape) -> [f64; 6] {
    let b = s.bounds();
    let p = match *s {
        Shape::Point(p) => p,
        Shape::Line { a, .. } => a,
        Shape::Circle { center, .. }
        | Shape::Arc { center, .. }
        | Shape::Ellipse { center, .. } => center,
        Shape::Bezier { p } => p[0],
    };
    [b.min.x, b.min.y, b.max.x, b.max.y, p.x, p.y]
}

fn key_cmp(a: &[f64; 6], b: &[f64; 6]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.total_cmp(y))
        .find(|o| o.is_ne())
        .unwrap_or(Ordering::Equal)
}

/// One piece of a curve between two snapped vertices.
struct Piece {
    edge: Edge,
    source: usize,
    from: usize,
    to: usize,
}

/// Split curves where they cross, snap every end and crossing to one vertex set, and return
/// the vertices and the pieces between them, each with the index of the curve it came from.
/// Ellipses and Béziers take part as fine chords, with a vertex at each crossing.
fn split(curves: &[Shape], tol: &RegionTolerance) -> (Vec<DVec2>, Vec<Piece>) {
    let n = curves.len();
    let pad = tol.snap;
    let mut cuts: Vec<Vec<(f64, DVec2)>> = vec![Vec::new(); n];
    let bounds: Vec<_> = curves.iter().map(Shape::bounds).collect();
    let keys: Vec<_> = curves.iter().map(curve_key).collect();
    // Sweep along x: only curves whose x ranges overlap are compared.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| bounds[a].min.x.total_cmp(&bounds[b].min.x));
    for (k, &first) in order.iter().enumerate() {
        for &second in &order[k + 1..] {
            if bounds[second].min.x > bounds[first].max.x + pad {
                break;
            }
            let (i, j) = if key_cmp(&keys[first], &keys[second])
                .then(first.cmp(&second))
                .is_lt()
            {
                (first, second)
            } else {
                (second, first)
            };
            let (a, b) = (bounds[i], bounds[j]);
            if a.max.x + pad < b.min.x
                || b.max.x + pad < a.min.x
                || a.max.y + pad < b.min.y
                || b.max.y + pad < a.min.y
            {
                continue;
            }
            for h in intersect(&curves[i], &curves[j]) {
                cuts[i].push((h.t[0], h.at));
                cuts[j].push((h.t[1], h.at));
            }
        }
    }
    // Each curve's stops in order along it (parameter and point): its ends, its cuts, and
    // the chord points of ellipses and Béziers. Circles wrap round to their first stop.
    let mut stops: Vec<Vec<(f64, DVec2)>> = Vec::with_capacity(n);
    for (shape, mut c) in curves.iter().zip(cuts) {
        c.sort_by(|x, y| x.0.total_cmp(&y.0));
        stops.push(match *shape {
            Shape::Line { a, b } => {
                let mut s = vec![(0.0, a)];
                s.extend(c);
                s.push((1.0, b));
                s
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let mut s = vec![(0.0, center + radius * DVec2::from_angle(start))];
                s.extend(c);
                s.push((sweep, center + radius * DVec2::from_angle(start + sweep)));
                s
            }
            Shape::Circle { center, radius } => {
                if c.is_empty() {
                    c.push((0.0, center + radius * DVec2::X));
                }
                c
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => chords_through(&c, sweep.min(TAU), 48, |t| {
                drift_core::shape::ellipse_point(center, major, minor, start + t)
            }),
            Shape::Bezier { p } => {
                chords_through(&c, 1.0, 24, |t| drift_core::shape::bezier_point(&p, t))
            }
            Shape::Point(_) => Vec::new(),
        });
    }
    let points: Vec<DVec2> = stops.iter().flatten().map(|(_, p)| *p).collect();
    let (verts, vertex_of) = snap_points(&points, tol.snap);
    let mut next = 0;
    let mut pieces = Vec::new();
    for (source, (shape, s)) in curves.iter().zip(stops).enumerate() {
        let mut raw: Vec<(f64, usize)> = s
            .iter()
            .map(|(t, _)| *t)
            .zip(&vertex_of[next..])
            .map(|(t, &v)| (t, v))
            .collect();
        next += s.len();
        // Each curve also passes through every vertex within `snap` of it: where another
        // curve ends on it, or crosses a curve lying along it. So curves lying along each
        // other share the same vertices, and the stretch they share becomes one edge.
        raw.extend(vertices_on(shape, &verts, tol.snap));
        raw.sort_by(|x, y| x.0.total_cmp(&y.0));
        // Stops at one vertex merge. The first is kept, except that the curve's far end
        // replaces a cut snapped onto it.
        let mut at: Vec<(f64, usize)> = Vec::with_capacity(raw.len());
        let count = raw.len();
        for (k, (t, v)) in raw.into_iter().enumerate() {
            let merged = at.len();
            match at.last_mut() {
                Some(last) if last.1 == v => {
                    if k + 1 == count && merged > 1 {
                        last.0 = t;
                    }
                }
                _ => at.push((t, v)),
            }
        }
        let mut push = |edge: Edge, from: usize, to: usize| {
            if from != to || edge.length() > tol.min_edge {
                pieces.push(Piece {
                    edge,
                    source,
                    from,
                    to,
                });
            }
        };
        match *shape {
            Shape::Circle { center, radius } => {
                if at.len() > 1 && at[0].1 == at[at.len() - 1].1 {
                    at.pop();
                }
                for k in 0..at.len() {
                    let (t0, u) = at[k];
                    let (t1, v) = match at.get(k + 1) {
                        Some(&next) => next,
                        None => (at[0].0 + TAU, at[0].1),
                    };
                    let edge = Edge::Arc {
                        center,
                        radius,
                        start: t0,
                        sweep: t1 - t0,
                    };
                    push(edge, u, v);
                }
            }
            Shape::Arc {
                center,
                radius,
                start,
                ..
            } => {
                for w in at.windows(2) {
                    let edge = Edge::Arc {
                        center,
                        radius,
                        start: start + w[0].0,
                        sweep: w[1].0 - w[0].0,
                    };
                    push(edge, w[0].1, w[1].1);
                }
            }
            _ => {
                for w in at.windows(2) {
                    let edge = Edge::Line {
                        a: verts[w[0].1],
                        b: verts[w[1].1],
                    };
                    push(edge, w[0].1, w[1].1);
                }
            }
        }
    }
    (verts, pieces)
}

/// The vertices within `snap` of the interior of `curve` (a line, arc or circle), with
/// their parameters along it.
fn vertices_on(curve: &Shape, verts: &[DVec2], snap: f64) -> Vec<(f64, usize)> {
    let b = curve.bounds();
    let mut found = Vec::new();
    if !matches!(
        curve,
        Shape::Line { .. } | Shape::Arc { .. } | Shape::Circle { .. }
    ) {
        return found;
    }
    // Vertices are sorted by x: only those in the curve's x range are looked at.
    let first = verts.partition_point(|p| p.x < b.min.x - snap);
    for (i, &p) in verts.iter().enumerate().skip(first) {
        if p.x > b.max.x + snap {
            break;
        }
        if p.y < b.min.y - snap || p.y > b.max.y + snap {
            continue;
        }
        match *curve {
            Shape::Line { a, b } => {
                let d = b - a;
                let len = d.length();
                if len == 0.0 {
                    continue;
                }
                let t = (p - a).dot(d) / (len * len);
                if t > 0.0 && t < 1.0 && d.perp_dot(p - a).abs() / len <= snap {
                    found.push((t, i));
                }
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let r = p - center;
                let t = (r.y.atan2(r.x) - start).rem_euclid(TAU);
                if (r.length() - radius).abs() <= snap && t > 0.0 && t < sweep {
                    found.push((t, i));
                }
            }
            Shape::Circle { center, radius } => {
                let r = p - center;
                if (r.length() - radius).abs() <= snap {
                    found.push((r.y.atan2(r.x).rem_euclid(TAU), i));
                }
            }
            _ => {}
        }
    }
    found
}

/// A curve over parameters `0..=end` as `n` chords, plus a vertex exactly at each cut (so
/// edges of the curves crossing it meet it at a shared point).
fn chords_through(
    cuts: &[(f64, DVec2)],
    end: f64,
    n: usize,
    at: impl Fn(f64) -> DVec2,
) -> Vec<(f64, DVec2)> {
    let inner: Vec<(f64, DVec2)> = cuts
        .iter()
        .copied()
        .filter(|(t, _)| *t > 0.0 && *t < end)
        .collect();
    // A sample right beside a cut gives way to the cut's exact point (the ends stay).
    let near = end / (n as f64 * 50.0);
    let mut stops: Vec<(f64, DVec2)> = (0..=n)
        .map(|k| end * k as f64 / n as f64)
        .filter(|t| *t == 0.0 || *t == end || !inner.iter().any(|(c, _)| (c - t).abs() < near))
        .map(|t| (t, at(t)))
        .collect();
    stops.extend(inner);
    stops.sort_by(|x, y| x.0.total_cmp(&y.0));
    stops
}

/// Find the closed regions formed by `curves`, with tolerances from their extent.
///
/// The output is canonical: vertices are numbered by position, edges by their vertices,
/// and loops start at, and regions and holes are ordered by, their lowest-numbered edge.
/// So the same picture gives the same regions in the same order, whatever order the curves
/// come in (only the curve indices in [`Loop::sources`] follow the input).
pub fn regions(curves: &[Shape]) -> Vec<Region> {
    regions_with(curves, &RegionTolerance::for_curves(curves))
}

/// [`regions`] with explicit tolerances.
pub fn regions_with(curves: &[Shape], tol: &RegionTolerance) -> Vec<Region> {
    let (verts, pieces) = split(curves, tol);
    // How many pieces each curve was cut into.
    let mut count = vec![0usize; curves.len()];
    for p in &pieces {
        count[p.source] += 1;
    }
    // Pieces in canonical order: each runs from its lower vertex, and they are sorted by
    // their vertices, then their middles.
    let middle = |e: &Edge| match *e {
        Edge::Line { a, b } => (a + b) * 0.5,
        Edge::Arc {
            center,
            radius,
            start,
            sweep,
        } => center + radius * DVec2::from_angle(start + sweep * 0.5),
    };
    let mut canon: Vec<(usize, usize, DVec2, Edge, usize)> = pieces
        .into_iter()
        .map(|p| {
            let (u, v, e) = if p.from <= p.to {
                (p.from, p.to, p.edge)
            } else {
                (p.to, p.from, p.edge.reversed())
            };
            (u, v, middle(&e), e, p.source)
        })
        .collect();
    canon.sort_by(|x, y| {
        (x.0, x.1)
            .cmp(&(y.0, y.1))
            .then(by_position(x.2, y.2))
            .then(x.4.cmp(&y.4))
    });
    // Half-edges: 2k forward, 2k+1 backward. The same edge twice (curves drawn on top of
    // each other) counts once: an edge is known by its ends and its middle. It is credited
    // to the curve cut into the fewest pieces, so a curve that spans just this piece (a
    // small shape's side along a longer line) owns it, not the longer line; each face it
    // bounds may still take another of the curves along it (see below).
    let mut half: Vec<(usize, usize, Edge)> = Vec::new(); // (from, to, geometry)
    let mut source_of: Vec<usize> = Vec::new();
    // Every curve along each edge, the one credited first.
    let mut along: Vec<Vec<usize>> = Vec::new();
    let mut middles: Vec<DVec2> = Vec::new();
    // Kept edges from `group` on share the current piece's ends.
    let (mut group, mut ends) = (0, None);
    for (u, v, mid, e, source) in canon {
        if ends != Some((u, v)) {
            (group, ends) = (source_of.len(), Some((u, v)));
        }
        if let Some(k) = (group..source_of.len()).find(|&k| middles[k].distance(mid) <= tol.snap) {
            if count[source] < count[source_of[k]] {
                source_of[k] = source;
            }
            if !along[k].contains(&source) {
                along[k].push(source);
            }
            continue;
        }
        half.push((u, v, e));
        half.push((v, u, e.reversed()));
        source_of.push(source);
        along.push(vec![source]);
        middles.push(mid);
    }
    // Prune dangling edges (vertices of degree one) repeatedly.
    let mut alive = vec![true; half.len()];
    {
        let mut degree = vec![0usize; verts.len()];
        let mut at: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
        for k in (0..half.len()).step_by(2) {
            let (u, v, _) = half[k];
            degree[u] += 1;
            degree[v] += 1;
            at[u].push(k);
            at[v].push(k);
        }
        let mut queue: Vec<usize> = (0..verts.len()).filter(|&i| degree[i] == 1).collect();
        while let Some(i) = queue.pop() {
            if degree[i] != 1 {
                continue;
            }
            let Some(&k) = at[i].iter().find(|&&k| alive[k]) else {
                continue;
            };
            alive[k] = false;
            alive[k + 1] = false;
            for end in [half[k].0, half[k].1] {
                degree[end] -= 1;
                if degree[end] == 1 {
                    queue.push(end);
                }
            }
        }
    }
    // Outgoing half-edges at each vertex, sorted counter-clockwise by direction, then
    // curvature.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); verts.len()];
    for (k, (u, ..)) in half.iter().enumerate() {
        if alive[k] {
            out[*u].push(k);
        }
    }
    let key = |k: usize| {
        let (d, curvature) = half[k].2.departure();
        (d.y.atan2(d.x), curvature)
    };
    for list in &mut out {
        list.sort_by(|&a, &b| {
            let (aa, ac) = key(a);
            let (ba, bc) = key(b);
            // Same direction up to rounding (an angle, not a distance): order by curvature.
            if (aa - ba).abs() <= 1e-9 {
                ac.total_cmp(&bc)
            } else {
                aa.total_cmp(&ba)
            }
        });
    }
    // Trace faces: after arriving at v along h, leave by the edge just clockwise of h's twin.
    let mut used = vec![false; half.len()];
    let mut loops: Vec<Loop> = Vec::new();
    for start in 0..half.len() {
        if !alive[start] || used[start] {
            continue;
        }
        let mut lp = Vec::new();
        let mut sources = Vec::new();
        let mut edges = Vec::new();
        let mut h = start;
        let mut guard = 0;
        while !used[h] {
            used[h] = true;
            lp.push(half[h].2);
            sources.push(source_of[h / 2]);
            edges.push(h / 2);
            let v = half[h].1;
            let twin = h ^ 1;
            let list = &out[v];
            let Some(pos) = list.iter().position(|&x| x == twin) else {
                break;
            };
            h = list[(pos + list.len() - 1) % list.len()];
            guard += 1;
            if guard > half.len() {
                break;
            }
        }
        if h == start && !lp.is_empty() {
            let sources = own_curves(&sources, &edges, &along, &count);
            loops.push(Loop { edges: lp, sources });
        }
    }
    // Counter-clockwise loops bound regions; clockwise ones are outer boundaries of shapes
    // (holes when inside a region).
    let mut outers: Vec<Loop> = Vec::new();
    let mut boundaries: Vec<Loop> = Vec::new();
    for lp in loops {
        let a = lp.signed_area();
        if a > tol.min_area {
            outers.push(lp);
        } else if a < -tol.min_area {
            boundaries.push(lp);
        }
    }
    let mut result: Vec<Region> = outers
        .into_iter()
        .map(|outer| Region {
            area: outer.signed_area(),
            perimeter: outer.length(),
            centroid: DVec2::ZERO,
            bounds: loop_bounds(&outer),
            outer,
            holes: Vec::new(),
        })
        .collect();
    // Each clockwise boundary is a hole in the smallest region strictly containing it (not
    // the face it bounds itself, which has the same outline).
    for b in boundaries {
        let own_area = -b.signed_area();
        let own = loop_bounds(&b);
        let outline = b.polygon(0.05);
        let host = result
            .iter()
            .enumerate()
            .filter(|(_, r)| r.area > own_area + tol.min_area)
            // Only regions whose bounds hold this boundary's (cheap) are checked closely.
            .filter(|(_, r)| {
                let pad = tol.snap;
                r.bounds.min.x <= own.min.x + pad
                    && r.bounds.min.y <= own.min.y + pad
                    && r.bounds.max.x >= own.max.x - pad
                    && r.bounds.max.y >= own.max.y - pad
            })
            .filter(|(_, r)| {
                outline
                    .iter()
                    .all(|p| r.outer.contains(*p) || near_outline(&r.outer, *p, tol))
            })
            .min_by(|a, b| a.1.area.total_cmp(&b.1.area))
            .map(|(i, _)| i);
        if let Some(i) = host {
            let r = &mut result[i];
            r.area -= own_area;
            r.perimeter += b.length();
            r.holes.push(b);
        }
    }
    for r in &mut result {
        r.centroid = centroid(r);
    }
    result
}

/// A loop's curves, where some of its edges lie along several (curves drawn over each other):
/// each such edge goes to the curve that bounds more of the rest of the loop, so two shapes
/// sharing part of a side each keep their own line along it (#133). Where none does, the edge
/// keeps the curve it was credited to (`sources`). `edges` are the loop's edges' indices,
/// `along` the curves along each edge, `count` the pieces each curve was cut into.
fn own_curves(
    sources: &[usize],
    edges: &[usize],
    along: &[Vec<usize>],
    count: &[usize],
) -> Vec<usize> {
    let mut own = sources.to_vec();
    for (i, &edge) in edges.iter().enumerate() {
        let curves = &along[edge];
        if curves.len() < 2 {
            continue;
        }
        let elsewhere = |c: usize| {
            sources
                .iter()
                .enumerate()
                .filter(|&(j, &s)| j != i && s == c)
                .count()
        };
        let credited = elsewhere(sources[i]);
        if let Some(best) = curves
            .iter()
            .copied()
            .filter(|&c| elsewhere(c) > credited)
            .max_by_key(|&c| {
                (
                    elsewhere(c),
                    std::cmp::Reverse(count[c]),
                    std::cmp::Reverse(c),
                )
            })
        {
            own[i] = best;
        }
    }
    own
}

fn near_outline(lp: &Loop, p: DVec2, tol: &RegionTolerance) -> bool {
    lp.edges
        .iter()
        .any(|e| crate::primitives::distance(&e.shape(), p) <= tol.snap)
}

/// Centroid from finely flattened loops (holes subtract).
fn centroid(r: &Region) -> DVec2 {
    let mut acc = DVec2::ZERO;
    let mut area2 = 0.0;
    for (lp, sign) in std::iter::once((&r.outer, 1.0)).chain(r.holes.iter().map(|h| (h, 1.0))) {
        let poly = lp.polygon(0.005);
        let origin = poly[0];
        for i in 0..poly.len() {
            let (a, b) = (poly[i] - origin, poly[(i + 1) % poly.len()] - origin);
            let cross = a.perp_dot(b) * sign;
            area2 += cross;
            acc += (a + b + origin * 3.0) * cross;
        }
    }
    if area2.abs() < 1e-300 {
        return r.outer.edges[0].start();
    }
    acc / (3.0 * area2)
}

/// The region containing `p` (the smallest one, if they nest).
pub fn region_at(regions: &[Region], p: DVec2) -> Option<&Region> {
    regions
        .iter()
        .filter(|r| r.contains(p))
        .min_by(|a, b| a.area.total_cmp(&b.area))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn line(a: DVec2, b: DVec2) -> Shape {
        Shape::Line { a, b }
    }

    #[test]
    fn a_rectangle_built_directly_is_the_region_its_sides_bound() {
        let found = regions(&rect(0.0, 0.0, 40.0, 16.0));
        let built = Region::rectangle(v(0.0, 0.0), v(40.0, 16.0));
        assert_eq!(found.len(), 1);
        let r = &found[0];
        assert!((r.area - built.area).abs() < 1e-9);
        assert!((r.perimeter - built.perimeter).abs() < 1e-9);
        assert!(r.centroid.distance(built.centroid) < 1e-9);
        assert_eq!(r.bounds, built.bounds);
        let corners = |lp: &Loop| {
            let mut c: Vec<(i64, i64)> = lp
                .polygon(0.1)
                .iter()
                .map(|p| (p.x.round() as i64, p.y.round() as i64))
                .collect();
            c.sort_unstable();
            c.dedup();
            c
        };
        assert_eq!(corners(&r.outer), corners(&built.outer));
        assert!(built.contains(v(20.0, 8.0)) && !built.contains(v(41.0, 8.0)));
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Shape> {
        let c = [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)];
        (0..4).map(|i| line(c[i], c[(i + 1) % 4])).collect()
    }

    #[test]
    fn a_rectangle_is_one_region() {
        let r = regions(&rect(0.0, 0.0, 20.0, 10.0));
        assert_eq!(r.len(), 1);
        assert!((r[0].area - 200.0).abs() < 1e-9);
        assert!((r[0].perimeter - 60.0).abs() < 1e-9);
        assert!(r[0].centroid.distance(v(10.0, 5.0)) < 1e-9);
        assert!(r[0].contains(v(5.0, 5.0)) && !r[0].contains(v(25.0, 5.0)));
    }

    #[test]
    fn crossing_lines_split_into_regions_and_tails_are_ignored() {
        // A rectangle divided by a vertical line, plus a dangling tail.
        let mut curves = rect(0.0, 0.0, 20.0, 10.0);
        curves.push(line(v(8.0, -5.0), v(8.0, 15.0)));
        curves.push(line(v(20.0, 5.0), v(30.0, 5.0)));
        let r = regions(&curves);
        assert_eq!(r.len(), 2);
        let mut areas: Vec<f64> = r.iter().map(|x| x.area).collect();
        areas.sort_by(f64::total_cmp);
        assert!((areas[0] - 80.0).abs() < 1e-9 && (areas[1] - 120.0).abs() < 1e-9);
        assert!((region_at(&r, v(4.0, 5.0)).unwrap().area - 80.0).abs() < 1e-9);
    }

    #[test]
    fn circles_and_arcs_are_exact() {
        let c = regions(&[Shape::Circle {
            center: v(3.0, 4.0),
            radius: 2.0,
        }]);
        assert_eq!(c.len(), 1);
        assert!((c[0].area - PI * 4.0).abs() < 1e-12);
        assert!((c[0].perimeter - 4.0 * PI).abs() < 1e-12);
        assert!(c[0].centroid.distance(v(3.0, 4.0)) < 1e-6);
        // A D shape: a diameter closed by a half circle.
        let d = regions(&[
            line(v(-5.0, 0.0), v(5.0, 0.0)),
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: 5.0,
                start: 0.0,
                sweep: PI,
            },
        ]);
        assert_eq!(d.len(), 1);
        assert!((d[0].area - PI * 12.5).abs() < 1e-12);
        // Centroid of a half disc: 4r/3π above the diameter.
        assert!((d[0].centroid.y - 20.0 / (3.0 * PI)).abs() < 1e-5);
    }

    #[test]
    fn holes_subtract() {
        let mut curves = rect(0.0, 0.0, 20.0, 10.0);
        curves.push(Shape::Circle {
            center: v(10.0, 5.0),
            radius: 2.0,
        });
        let r = regions(&curves);
        assert_eq!(r.len(), 2);
        let plate = r.iter().find(|x| x.area > 100.0).unwrap();
        assert_eq!(plate.holes.len(), 1);
        assert!((plate.area - (200.0 - 4.0 * PI)).abs() < 1e-9);
        assert!(!plate.contains(v(10.0, 5.0)));
        // Clicking in the hole picks the disc.
        assert!((region_at(&r, v(10.0, 5.0)).unwrap().area - 4.0 * PI).abs() < 1e-12);
    }

    #[test]
    fn fillets_tangent_to_lines_trace_correctly() {
        // A 20 × 10 rectangle with one corner rounded (radius 3).
        let r = 3.0;
        let curves = vec![
            line(v(0.0, 0.0), v(20.0 - r, 0.0)),
            Shape::Arc {
                center: v(20.0 - r, r),
                radius: r,
                start: -PI / 2.0,
                sweep: PI / 2.0,
            },
            line(v(20.0, r), v(20.0, 10.0)),
            line(v(20.0, 10.0), v(0.0, 10.0)),
            line(v(0.0, 10.0), v(0.0, 0.0)),
        ];
        let regs = regions(&curves);
        assert_eq!(regs.len(), 1);
        let expected = 200.0 - (r * r - PI * r * r / 4.0);
        assert!((regs[0].area - expected).abs() < 1e-9, "{}", regs[0].area);
    }

    #[test]
    fn loops_name_their_source_curves() {
        let mut curves = rect(0.0, 0.0, 20.0, 10.0);
        curves.push(Shape::Circle {
            center: v(10.0, 5.0),
            radius: 2.0,
        });
        let regs = regions(&curves);
        let plate = regs.iter().find(|r| !r.holes.is_empty()).expect("plate");
        assert_eq!(plate.outer.curves(), vec![0, 1, 2, 3]);
        assert_eq!(plate.holes[0].curves(), vec![4]);
        assert_eq!(plate.outer.sources.len(), plate.outer.edges.len());
    }

    #[test]
    fn abutting_rectangles_with_overlapping_edges_stay_two_regions() {
        // Side by side, each with its own copy of the shared wall.
        let mut curves = rect(0.0, 0.0, 10.0, 10.0);
        curves.extend(rect(10.0, 0.0, 20.0, 10.0));
        let found = regions(&curves);
        let mut areas: Vec<f64> = found.iter().map(|r| r.area).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert!(areas.iter().all(|a| (a - 100.0).abs() < 1e-9), "{areas:?}");
        // A taller neighbour: its wall overlaps only part of the way.
        let mut curves = rect(0.0, 0.0, 10.0, 10.0);
        curves.extend(rect(10.0, -5.0, 20.0, 20.0));
        let found = regions(&curves);
        let mut areas: Vec<f64> = found.iter().map(|r| r.area).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert!(
            (areas[0] - 100.0).abs() < 1e-9 && (areas[1] - 250.0).abs() < 1e-9,
            "{areas:?}"
        );
        // The same rectangle drawn twice is one region, not two stacked.
        let mut curves = rect(0.0, 0.0, 10.0, 10.0);
        curves.extend(rect(0.0, 0.0, 10.0, 10.0));
        let found = regions(&curves);
        assert_eq!(found.len(), 1);
        assert!((found[0].area - 100.0).abs() < 1e-9);
    }

    #[test]
    fn an_overlapped_piece_belongs_to_the_curve_that_spans_it() {
        // A small square in a big square's corner, its right and top sides along the big
        // square's (#120): the corner region is bounded by the small square's own sides.
        let mut curves = rect(0.0, 0.0, 100.0, 100.0);
        curves.extend(rect(50.0, 50.0, 100.0, 100.0));
        let found = regions(&curves);
        assert_eq!(found.len(), 2);
        let corner = region_at(&found, v(75.0, 75.0)).expect("corner");
        assert_eq!(corner.outer.curves(), vec![4, 5, 6, 7]);
        let l_shape = region_at(&found, v(25.0, 25.0)).expect("L");
        assert_eq!(l_shape.outer.curves(), vec![0, 1, 2, 3, 4, 7]);
        // Drawn the other way round, the same.
        let mut curves = rect(50.0, 50.0, 100.0, 100.0);
        curves.extend(rect(0.0, 0.0, 100.0, 100.0));
        let found = regions(&curves);
        let corner = region_at(&found, v(75.0, 75.0)).expect("corner");
        assert_eq!(corner.outer.curves(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn rectangles_sharing_part_of_a_side_each_keep_their_own_line_along_it() {
        // A below, B above and shifted right: A's top and B's bottom overlap from 50 to 100,
        // each cut once by the other's corner, a tie for "fewest pieces" (#133).
        for b_first in [false, true] {
            let (a, b) = (
                rect(0.0, 0.0, 100.0, 100.0),
                rect(50.0, 100.0, 150.0, 200.0),
            );
            let curves: Vec<Shape> = if b_first {
                b.iter().chain(&a).copied().collect()
            } else {
                a.iter().chain(&b).copied().collect()
            };
            let (of_a, of_b) = if b_first {
                (vec![4, 5, 6, 7], vec![0, 1, 2, 3])
            } else {
                (vec![0, 1, 2, 3], vec![4, 5, 6, 7])
            };
            let found = regions(&curves);
            let lower = region_at(&found, v(25.0, 25.0)).expect("A");
            let upper = region_at(&found, v(125.0, 175.0)).expect("B");
            assert_eq!(lower.outer.curves(), of_a, "B first: {b_first}");
            assert_eq!(upper.outer.curves(), of_b, "B first: {b_first}");
        }
    }

    /// The lower half of a circle, its diameter, and a curve ending on the arc's interior
    /// (#100).
    fn half_disc_with_lock(lock: Shape) -> Vec<Shape> {
        vec![
            Shape::Arc {
                center: v(0.0, 0.0),
                radius: 100.0,
                start: PI,
                sweep: PI,
            },
            line(v(-100.0, 0.0), v(100.0, 0.0)),
            lock,
        ]
    }

    #[test]
    fn a_bezier_ending_on_an_arc_is_a_junction() {
        // (60, -80) lies exactly on the circle, so the Bézier and the arc enclose a lock
        // shape, and the half disc beside it stays whole (#100).
        let regs = regions(&half_disc_with_lock(Shape::Bezier {
            p: [
                v(100.0, 0.0),
                v(160.0, -20.0),
                v(150.0, -70.0),
                v(60.0, -80.0),
            ],
        }));
        let lock = region_at(&regs, v(120.0, -40.0)).expect("lock");
        assert!((lock.area - 2715.9).abs() < 1.0, "{}", lock.area);
        let disc = region_at(&regs, v(0.0, -50.0)).expect("half disc");
        assert!((disc.area - PI * 5000.0).abs() < 1e-6, "{}", disc.area);
        assert_eq!(regs.len(), 2);
        // Control: a polyline through the control points was already found. Its area is
        // the 5000 polygon less the circular segment the arc cuts off (636.5).
        let mut curves = half_disc_with_lock(line(v(100.0, 0.0), v(160.0, -20.0)));
        curves.push(line(v(160.0, -20.0), v(150.0, -70.0)));
        curves.push(line(v(150.0, -70.0), v(60.0, -80.0)));
        let regs = regions(&curves);
        let lock = region_at(&regs, v(120.0, -40.0)).expect("polyline lock");
        assert!((lock.area - 4363.52).abs() < 0.01, "{}", lock.area);
        assert!(region_at(&regs, v(0.0, -50.0)).is_some());
        // Control: a Bézier ending on a line's interior.
        let mut curves = rect(0.0, 0.0, 200.0, 100.0);
        curves.push(Shape::Bezier {
            p: [v(0.0, 0.0), v(60.0, 80.0), v(120.0, 80.0), v(150.0, 0.0)],
        });
        let regs = regions(&curves);
        assert_eq!(
            regs.len(),
            2,
            "{:?}",
            regs.iter().map(|r| r.area).collect::<Vec<_>>()
        );
    }
}
