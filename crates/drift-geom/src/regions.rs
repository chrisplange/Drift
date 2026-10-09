//! Closed regions formed by curves: the faces of the planar arrangement of lines, arcs and
//! circles, with exact areas, perimeters and centroids.
//!
//! Curves are split where they cross, the pieces become edges of a graph, dangling edges are
//! pruned, and faces are traced by always taking the next edge clockwise at each vertex (so
//! each bounded face is traced counter-clockwise). Edges leaving a vertex in the same
//! direction (a fillet arc tangent to its line) are ordered by curvature. A separate shape
//! inside a region becomes a hole of the smallest region containing it.

use std::f64::consts::TAU;

use drift_core::{Bounds, DVec2, Shape};

use crate::intersect::{intersect, tolerance};

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
                let n = ((sweep.abs() / max_angle).ceil() as usize).max(1);
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

    fn contains(&self, p: DVec2) -> bool {
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

/// Split curves (lines, arcs, circles) where they cross into edges, each with the index of
/// the curve it came from.
fn split(curves: &[Shape]) -> Vec<(Edge, usize)> {
    let n = curves.len();
    let mut cuts: Vec<Vec<(f64, DVec2)>> = vec![Vec::new(); n];
    let bounds: Vec<_> = curves.iter().map(Shape::bounds).collect();
    // Sweep along x: only curves whose x ranges overlap are compared.
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| bounds[a].min.x.total_cmp(&bounds[b].min.x));
    let pad_all = bounds
        .iter()
        .map(|b| tolerance(b.max.abs().max_element()))
        .fold(0.0, f64::max);
    for (k, &first) in order.iter().enumerate() {
        for &second in &order[k + 1..] {
            if bounds[second].min.x > bounds[first].max.x + pad_all {
                break;
            }
            // Same order as an all-pairs loop, so cut lists come out the same.
            let (i, j) = (first.min(second), first.max(second));
            let (a, b) = (bounds[i], bounds[j]);
            let pad = tolerance(a.max.abs().max_element().max(b.max.abs().max_element()));
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
    let mut edges = Vec::new();
    for (source, (shape, mut c)) in curves.iter().zip(cuts).enumerate() {
        c.sort_by(|x, y| x.0.total_cmp(&y.0));
        match *shape {
            Shape::Line { a, b } => {
                let mut pts = vec![a];
                pts.extend(c.iter().map(|(_, p)| *p));
                pts.push(b);
                pts.dedup_by(|p, q| p.distance(*q) <= tolerance(p.abs().max_element()));
                for w in pts.windows(2) {
                    edges.push((Edge::Line { a: w[0], b: w[1] }, source));
                }
            }
            Shape::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let mut ts = vec![0.0];
                ts.extend(c.iter().map(|(t, _)| *t));
                ts.push(sweep);
                ts.dedup_by(|x, y| {
                    (*x - *y).abs() * radius <= tolerance(center.abs().max_element() + radius)
                });
                for w in ts.windows(2) {
                    edges.push((
                        Edge::Arc {
                            center,
                            radius,
                            start: start + w[0],
                            sweep: w[1] - w[0],
                        },
                        source,
                    ));
                }
            }
            Shape::Circle { center, radius } => {
                let mut ts: Vec<f64> = c.iter().map(|(t, _)| *t).collect();
                ts.dedup_by(|x, y| {
                    (*x - *y).abs() * radius <= tolerance(center.abs().max_element() + radius)
                });
                if ts.is_empty() {
                    ts.push(0.0);
                }
                for k in 0..ts.len() {
                    let t0 = ts[k];
                    let t1 = if k + 1 < ts.len() {
                        ts[k + 1]
                    } else {
                        ts[0] + TAU
                    };
                    edges.push((
                        Edge::Arc {
                            center,
                            radius,
                            start: t0,
                            sweep: t1 - t0,
                        },
                        source,
                    ));
                }
            }
            Shape::Ellipse {
                center,
                major,
                minor,
                start,
                sweep,
            } => {
                // Ellipses take part as fine straight edges (they aren't cut by crossings).
                let pts =
                    drift_core::shape::flatten_ellipse(center, major, minor, start, sweep, 48);
                for w in pts.windows(2) {
                    edges.push((Edge::Line { a: w[0], b: w[1] }, source));
                }
            }
            Shape::Bezier { p } => {
                for w in drift_core::shape::flatten_bezier(&p, 24).windows(2) {
                    edges.push((Edge::Line { a: w[0], b: w[1] }, source));
                }
            }
            Shape::Point(_) => {}
        }
    }
    edges.retain(|(e, _)| e.length() > 0.0);
    edges
}

/// Find the closed regions formed by `curves`.
pub fn regions(curves: &[Shape]) -> Vec<Region> {
    let edges = split(curves);
    // Vertices: edge ends merged within tolerance.
    let mut verts: Vec<DVec2> = Vec::new();
    // A grid of cells much larger than the tolerance: a point's match is in its own cell or
    // a neighbouring one.
    let cell = edges
        .iter()
        .map(|(e, _)| tolerance(e.start().abs().max_element()))
        .fold(1e-9, f64::max)
        * 8.0;
    let key = |p: DVec2| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64);
    let mut grid: std::collections::HashMap<(i64, i64), Vec<usize>> =
        std::collections::HashMap::new();
    let mut vertex = |p: DVec2| -> usize {
        let tol = tolerance(p.abs().max_element());
        let (kx, ky) = key(p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = grid.get(&(kx + dx, ky + dy))
                    && let Some(&i) = list.iter().find(|&&i| verts[i].distance(p) <= tol)
                {
                    return i;
                }
            }
        }
        verts.push(p);
        grid.entry((kx, ky)).or_default().push(verts.len() - 1);
        verts.len() - 1
    };
    // Half-edges: 2k forward, 2k+1 backward.
    let mut half: Vec<(usize, usize, Edge)> = Vec::new(); // (from, to, geometry)
    for (e, _) in &edges {
        let (u, v) = (vertex(e.start()), vertex(e.end()));
        half.push((u, v, *e));
        half.push((v, u, e.reversed()));
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
        let mut h = start;
        let mut guard = 0;
        while !used[h] {
            used[h] = true;
            lp.push(half[h].2);
            sources.push(edges[h / 2].1);
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
            loops.push(Loop { edges: lp, sources });
        }
    }
    // Counter-clockwise loops bound regions; clockwise ones are outer boundaries of shapes
    // (holes when inside a region).
    let tol_area = 1e-12;
    let mut outers: Vec<Loop> = Vec::new();
    let mut boundaries: Vec<Loop> = Vec::new();
    for lp in loops {
        let a = lp.signed_area();
        if a > tol_area {
            outers.push(lp);
        } else if a < -tol_area {
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
            .filter(|(_, r)| r.area > own_area * (1.0 + 1e-9))
            // Only regions whose bounds hold this boundary's (cheap) are checked closely.
            .filter(|(_, r)| {
                let pad = tolerance(own.max.abs().max_element()) * 10.0;
                r.bounds.min.x <= own.min.x + pad
                    && r.bounds.min.y <= own.min.y + pad
                    && r.bounds.max.x >= own.max.x - pad
                    && r.bounds.max.y >= own.max.y - pad
            })
            .filter(|(_, r)| {
                outline
                    .iter()
                    .all(|p| r.outer.contains(*p) || near_outline(&r.outer, *p))
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

fn near_outline(lp: &Loop, p: DVec2) -> bool {
    lp.edges.iter().any(|e| {
        crate::primitives::distance(&e.shape(), p) <= tolerance(p.abs().max_element()) * 10.0
    })
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
}
