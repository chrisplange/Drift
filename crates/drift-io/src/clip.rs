//! Clipping imported artwork to the clip paths (and masks, and the page) it was drawn
//! through, so that what comes in is what the file shows.
//!
//! Strokes are trimmed exactly: lines at the clip's edges, Béziers split there (they stay
//! Béziers). Fills are cut to a clip that is convex (the usual rectangle among them) as
//! outlines. A fill that crosses a clip of any other shape comes in whole, and is counted, so
//! the import can say so.
//!
//! An opaque fill hides what was painted before it; [`uncovered`] splits a stroke into what
//! still shows and what the fill covers.
// Reads files from anywhere: bad input is an error or is skipped, never a panic
// (tests/import_robustness.rs). Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use drift_core::DVec2;
use drift_core::polygon::area;
use drift_core::tolerance;

use crate::vector::{Seg, Subpath};

/// A clipping region: its outline as one or more rings of points (inside is where an odd
/// number of them surround a point).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    rings: Vec<Vec<DVec2>>,
    convex: bool,
    /// An upright rectangle, from corner to corner (what most clips are).
    rect: Option<(DVec2, DVec2)>,
}

/// How many paths clipping changed, over a whole drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClipCount {
    pub trimmed: usize,
    pub hidden: usize,
    pub whole: usize,
}

impl ClipCount {
    pub fn add(&mut self, c: Clipped) {
        self.trimmed += usize::from(c.trimmed && !c.hidden);
        self.hidden += usize::from(c.hidden);
        self.whole += usize::from(c.whole);
    }
}

/// What clipping did to a painted path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Clipped {
    /// It was cut at a clip's edge.
    pub trimmed: bool,
    /// Wholly outside a clip, so left out.
    pub hidden: bool,
    /// A fill crossing a clip too intricate to cut: it came in whole.
    pub whole: bool,
}

impl Clip {
    /// A rectangle, from corner to corner.
    pub fn rect(a: DVec2, b: DVec2) -> Clip {
        let (lo, hi) = (a.min(b), a.max(b));
        Clip {
            rings: vec![vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)]],
            convex: true,
            rect: Some((lo, hi)),
        }
    }

    /// The region inside subpaths (each as a closed ring). `None` if they enclose nothing.
    pub fn from_subpaths(subpaths: &[Subpath]) -> Option<Clip> {
        let rings: Vec<Vec<DVec2>> = subpaths
            .iter()
            .map(|s| {
                let mut ring = vec![s.start];
                let mut from = s.start;
                for seg in &s.segs {
                    match *seg {
                        Seg::Line(p) => ring.push(p),
                        Seg::Cubic(c1, c2, p) => ring.extend(
                            drift_core::Shape::Bezier {
                                p: [from, c1, c2, p],
                            }
                            .flatten(drift_core::tolerance::FLATTEN_MM)
                            .into_iter()
                            .skip(1),
                        ),
                    }
                    from = seg.end();
                }
                if ring.len() > 1 && ring.first() == ring.last() {
                    ring.pop();
                }
                ring.dedup_by(|a, b| a.distance(*b) < tolerance::SAME);
                ring
            })
            .filter(|r| r.len() >= 3 && area(r).abs() > tolerance::NONE)
            .collect();
        if rings.is_empty() {
            return None;
        }
        let convex = rings.len() == 1 && is_convex(&rings[0]);
        // Four corners square to the axes: a rectangle.
        let rect = (rings.len() == 1 && rings[0].len() == 4)
            .then(|| {
                let r = &rings[0];
                let upright = (0..4).all(|i| {
                    let d = r[(i + 1) % 4] - r[i];
                    d.x.abs() < tolerance::SAME || d.y.abs() < tolerance::SAME
                });
                upright.then(|| {
                    let lo = r.iter().fold(DVec2::splat(f64::MAX), |m, p| m.min(*p));
                    let hi = r.iter().fold(DVec2::splat(f64::MIN), |m, p| m.max(*p));
                    (lo, hi)
                })
            })
            .flatten();
        Some(Clip {
            rings,
            convex,
            rect,
        })
    }

    /// Whether every point is inside, for a rectangle (curves stay within their handles).
    fn holds_all(&self, mut points: impl Iterator<Item = DVec2>) -> bool {
        self.rect.is_some_and(|(lo, hi)| {
            let eps = tolerance::same((hi - lo).max_element());
            points.all(|p| {
                p.x >= lo.x - eps && p.x <= hi.x + eps && p.y >= lo.y - eps && p.y <= hi.y + eps
            })
        })
    }

    /// Even-odd over all the rings together, so a point inside a hole is outside.
    pub fn contains(&self, p: DVec2) -> bool {
        let inside = self
            .rings
            .iter()
            .filter(|ring| drift_core::polygon::contains(ring, p))
            .count();
        inside % 2 == 1
    }

    /// How far a point is from the region's outline.
    fn distance_to_edge(&self, p: DVec2) -> f64 {
        self.edges()
            .map(|(a, b)| {
                let d = b - a;
                let t = ((p - a).dot(d)
                    / d.length_squared().max(tolerance::NONE * tolerance::NONE))
                .clamp(0.0, 1.0);
                p.distance(a + d * t)
            })
            .fold(f64::INFINITY, f64::min)
    }

    /// Whether a point is inside, further than `margin` from the outline.
    pub fn covers(&self, p: DVec2, margin: f64) -> bool {
        self.contains(p) && self.distance_to_edge(p) > margin
    }

    /// Its bounding box, corner to corner.
    pub fn bounds(&self) -> (DVec2, DVec2) {
        self.rings.iter().flatten().fold(
            (DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    fn edges(&self) -> impl Iterator<Item = (DVec2, DVec2)> + '_ {
        self.rings
            .iter()
            .flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()])))
    }

    /// Where along a segment it crosses the clip's edges (parameters strictly inside 0–1).
    fn crossings(&self, from: DVec2, seg: &Seg) -> Vec<f64> {
        let mut ts = Vec::new();
        for (e0, e1) in self.edges() {
            let d = e1 - e0;
            let len2 = d.length_squared();
            if len2 < tolerance::NONE * tolerance::NONE {
                continue;
            }
            // Which side of the edge's line a point is on, and whether it's along the edge.
            let side = |p: DVec2| (p - e0).perp_dot(d);
            let on_edge = |p: DVec2| {
                (-tolerance::SAME..=1.0 + tolerance::SAME).contains(&((p - e0).dot(d) / len2))
            };
            let at = |t: f64| point(from, seg, t);
            let mut push = |t: f64| {
                if t > tolerance::SAME && t < 1.0 - tolerance::SAME && on_edge(at(t)) {
                    ts.push(t);
                }
            };
            match *seg {
                Seg::Line(to) => {
                    let (s0, s1) = (side(from), side(to));
                    if (s0 < 0.0) != (s1 < 0.0) && (s0 - s1).abs() > 0.0 {
                        push(s0 / (s0 - s1));
                    }
                }
                Seg::Cubic(..) => {
                    // Sign changes along the curve, found by sampling, then narrowed down.
                    const N: usize = 32;
                    let mut t0 = 0.0;
                    let mut s0 = side(at(0.0));
                    for k in 1..=N {
                        let t1 = k as f64 / N as f64;
                        let s1 = side(at(t1));
                        if (s0 < 0.0) != (s1 < 0.0) {
                            let (mut lo, mut hi) = (t0, t1);
                            for _ in 0..48 {
                                let mid = (lo + hi) / 2.0;
                                if (side(at(mid)) < 0.0) == (s0 < 0.0) {
                                    lo = mid;
                                } else {
                                    hi = mid;
                                }
                            }
                            push((lo + hi) / 2.0);
                        }
                        t0 = t1;
                        s0 = s1;
                    }
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < tolerance::SAME);
        ts
    }
}

fn point(from: DVec2, seg: &Seg, t: f64) -> DVec2 {
    match *seg {
        Seg::Line(to) => from + (to - from) * t,
        Seg::Cubic(c1, c2, p) => drift_core::shape::bezier_point(&[from, c1, c2, p], t),
    }
}

/// The part of a segment from `t0` to `t1`.
fn piece(from: DVec2, seg: &Seg, t0: f64, t1: f64) -> (DVec2, Seg) {
    match *seg {
        Seg::Line(_) => (point(from, seg, t0), Seg::Line(point(from, seg, t1))),
        Seg::Cubic(c1, c2, p) => {
            let q = drift_core::shape::bezier_piece(&[from, c1, c2, p], t0, t1);
            (q[0], Seg::Cubic(q[1], q[2], q[3]))
        }
    }
}

fn is_convex(ring: &[DVec2]) -> bool {
    let n = ring.len();
    let mut sign = 0.0;
    for i in 0..n {
        let (a, b, c) = (ring[i], ring[(i + 1) % n], ring[(i + 2) % n]);
        let turn = (b - a).perp_dot(c - b);
        if turn.abs() < tolerance::NONE {
            continue;
        }
        if sign == 0.0 {
            sign = turn.signum();
        } else if turn.signum() != sign {
            return false;
        }
    }
    true
}

/// A stroked subpath's parts inside a clip: whole if it doesn't cross the clip's edges,
/// otherwise the runs between crossings that are inside.
fn stroke_inside(s: &Subpath, clip: &Clip) -> (Vec<Subpath>, bool) {
    let (inside, outside, cut) = split(s, clip, |p| clip.contains(p));
    (inside, cut || !outside.is_empty())
}

/// A stroked subpath's parts that still show once an opaque fill over `cover` is painted,
/// and those it covers. A piece within `margin` of the fill's outline (half the stroke's
/// width, say: it shows along the edge) still shows.
pub fn uncovered(s: &Subpath, cover: &Clip, margin: f64) -> (Vec<Subpath>, Vec<Subpath>) {
    let (covered, shown, _) = split(s, cover, |p| cover.covers(p, margin));
    (shown, covered)
}

/// A subpath split where it crosses a region's edges into the runs where `keep` holds and
/// the runs where it doesn't (each run tested at its middle), and whether it was split. Not
/// split, it is wholly in one or the other.
fn split(
    s: &Subpath,
    clip: &Clip,
    keep: impl Fn(DVec2) -> bool,
) -> (Vec<Subpath>, Vec<Subpath>, bool) {
    // Runs of each kind (kept, then not), and the one each is building.
    let mut runs: [Vec<Subpath>; 2] = [Vec::new(), Vec::new()];
    let mut open: [Option<Subpath>; 2] = [None, None];
    let mut cut = false;
    let mut from = s.start;
    let mut segs: Vec<Seg> = s.segs.clone();
    // A closed subpath's closing edge, as a segment of its own.
    let end = segs.last().map_or(s.start, Seg::end);
    if s.closed && end.distance(s.start) > tolerance::SAME {
        segs.push(Seg::Line(s.start));
    }
    for seg in &segs {
        let mut ts = vec![0.0];
        let crossings = clip.crossings(from, seg);
        cut |= !crossings.is_empty();
        ts.extend(crossings);
        ts.push(1.0);
        for w in ts.windows(2) {
            let (t0, t1) = (w[0], w[1]);
            if t1 - t0 < tolerance::NONE {
                continue;
            }
            let kind = usize::from(!keep(point(from, seg, (t0 + t1) / 2.0)));
            // The other kind's run ends here.
            if let Some(run) = open[1 - kind].take() {
                runs[1 - kind].push(run);
            }
            let (start, part) = piece(from, seg, t0, t1);
            match &mut open[kind] {
                Some(run) => run.segs.push(part),
                None => {
                    open[kind] = Some(Subpath {
                        start,
                        segs: vec![part],
                        closed: false,
                    })
                }
            }
        }
        from = seg.end();
    }
    for kind in 0..2 {
        if let Some(run) = open[kind].take() {
            runs[kind].push(run);
        }
    }
    let [kept, other] = runs;
    if other.is_empty() && !cut {
        return (vec![s.clone()], Vec::new(), false);
    }
    if kept.is_empty() && !cut {
        return (Vec::new(), vec![s.clone()], false);
    }
    // A closed subpath cut open: its last run carries on into its first.
    let rejoin = |mut runs: Vec<Subpath>| {
        if s.closed
            && runs.len() > 1
            && let (Some(last), Some(first)) = (runs.last(), runs.first())
            && last
                .segs
                .last()
                .map_or(last.start, Seg::end)
                .distance(first.start)
                < tolerance::SAME
        {
            #[allow(clippy::expect_used)] // There are more than one.
            let last = runs.pop().expect("runs");
            let first = runs.remove(0);
            let mut joined = last;
            joined.segs.extend(first.segs);
            runs.insert(0, joined);
        }
        runs
    };
    (rejoin(kept), rejoin(other), true)
}

/// A filled subpath cut to a convex clip, as an outline of points (Sutherland–Hodgman).
fn fill_inside(s: &Subpath, clip: &Clip) -> Option<Subpath> {
    let mut ring = Clip::from_subpaths(std::slice::from_ref(s))?
        .rings
        .remove(0);
    let edge_ring = &clip.rings[0];
    // Keep the clip's ring anticlockwise, so inside is to the left of each edge.
    let ccw = area(edge_ring) > 0.0;
    let n = edge_ring.len();
    for i in 0..n {
        let (a, b) = if ccw {
            (edge_ring[i], edge_ring[(i + 1) % n])
        } else {
            (edge_ring[(i + 1) % n], edge_ring[i])
        };
        let inside = |p: DVec2| (b - a).perp_dot(p - a) >= -tolerance::NONE;
        let cross = |p: DVec2, q: DVec2| {
            let (sp, sq) = ((b - a).perp_dot(p - a), (b - a).perp_dot(q - a));
            p + (q - p) * (sp / (sp - sq))
        };
        let input = std::mem::take(&mut ring);
        for k in 0..input.len() {
            let (p, q) = (input[k], input[(k + 1) % input.len()]);
            match (inside(p), inside(q)) {
                (true, true) => ring.push(q),
                (true, false) => ring.push(cross(p, q)),
                (false, true) => {
                    ring.push(cross(p, q));
                    ring.push(q);
                }
                (false, false) => {}
            }
        }
        if ring.len() < 3 {
            return None;
        }
    }
    ring.dedup_by(|a, b| a.distance(*b) < tolerance::SAME);
    (ring.len() >= 3 && area(&ring).abs() > tolerance::NONE).then(|| Subpath {
        start: ring[0],
        segs: ring[1..].iter().map(|p| Seg::Line(*p)).collect(),
        closed: true,
    })
}

/// A painted path's subpaths seen through clips: those to stroke, and those to fill.
pub fn clip_paint(
    subpaths: &[Subpath],
    clips: &[std::rc::Rc<Clip>],
) -> (Vec<Subpath>, Vec<Subpath>, Clipped) {
    let mut stroke: Vec<Subpath> = subpaths.to_vec();
    let mut fill: Vec<Subpath> = subpaths.to_vec();
    let mut outcome = Clipped::default();
    for clip in clips {
        // Wholly inside a rectangle (the page, most clips): nothing to do.
        let points = stroke.iter().chain(&fill).flat_map(|s| {
            std::iter::once(s.start).chain(s.segs.iter().flat_map(|g| match *g {
                Seg::Line(p) => vec![p],
                Seg::Cubic(a, b, p) => vec![a, b, p],
            }))
        });
        if clip.holds_all(points) {
            continue;
        }
        let mut next = Vec::new();
        for s in &stroke {
            let (parts, cut) = stroke_inside(s, clip);
            outcome.trimmed |= cut;
            next.extend(parts);
        }
        stroke = next;
        // A fill: whole if no edge of it crosses the clip and it's inside, gone if it's
        // outside and doesn't hold the clip, cut if the clip is convex.
        let crosses = fill
            .iter()
            .any(|s| stroke_inside(s, clip).1 || s.segs.is_empty());
        if !crosses {
            let inside = fill.iter().any(|s| clip.contains(s.start));
            let holds = clip.rings.first().and_then(|r| r.first()).is_some_and(|c| {
                Clip::from_subpaths(&fill).is_some_and(|region| region.contains(*c))
            });
            if !inside && !holds {
                fill.clear();
            } else if !inside && holds {
                // The clip lies within the fill: what shows is the clip's shape.
                if clip.convex {
                    fill = fill.iter().filter_map(|s| fill_inside(s, clip)).collect();
                    outcome.trimmed = true;
                } else {
                    outcome.whole = true;
                }
            }
        } else if clip.convex {
            fill = fill.iter().filter_map(|s| fill_inside(s, clip)).collect();
            outcome.trimmed = true;
        } else {
            outcome.whole = true;
        }
    }
    outcome.hidden = stroke.is_empty() && fill.is_empty() && !subpaths.is_empty();
    (stroke, fill, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn line(a: DVec2, b: DVec2) -> Subpath {
        Subpath {
            start: a,
            segs: vec![Seg::Line(b)],
            closed: false,
        }
    }

    fn square(lo: f64, hi: f64) -> Subpath {
        Subpath {
            start: v(lo, lo),
            segs: vec![
                Seg::Line(v(hi, lo)),
                Seg::Line(v(hi, hi)),
                Seg::Line(v(lo, hi)),
            ],
            closed: true,
        }
    }

    #[test]
    fn strokes_are_trimmed_and_hidden_ones_left_out() {
        let clip = Rc::new(Clip::rect(v(0.0, 0.0), v(10.0, 10.0)));
        // Across: trimmed to the edges.
        let (stroke, _, out) = clip_paint(
            &[line(v(-5.0, 5.0), v(15.0, 5.0))],
            std::slice::from_ref(&clip),
        );
        assert!(out.trimmed && !out.hidden);
        assert_eq!(stroke.len(), 1);
        assert!(stroke[0].start.distance(v(0.0, 5.0)) < 1e-9);
        assert!(stroke[0].segs[0].end().distance(v(10.0, 5.0)) < 1e-9);
        // Outside: hidden.
        let (stroke, fill, out) = clip_paint(
            &[line(v(20.0, 5.0), v(30.0, 5.0))],
            std::slice::from_ref(&clip),
        );
        assert!(stroke.is_empty() && fill.is_empty() && out.hidden);
        // Inside: untouched.
        let (stroke, _, out) = clip_paint(
            &[line(v(2.0, 5.0), v(8.0, 5.0))],
            std::slice::from_ref(&clip),
        );
        assert!(!out.trimmed && stroke[0] == line(v(2.0, 5.0), v(8.0, 5.0)));
        // A curve across the edge stays a curve, split where it leaves.
        let curve = Subpath {
            start: v(5.0, 5.0),
            segs: vec![Seg::Cubic(v(10.0, 5.0), v(15.0, 5.0), v(20.0, 5.0))],
            closed: false,
        };
        let (stroke, _, _) = clip_paint(&[curve], &[clip]);
        assert!(
            matches!(stroke[0].segs[0], Seg::Cubic(_, _, end) if end.distance(v(10.0, 5.0)) < 1e-6)
        );
    }

    #[test]
    fn fills_are_cut_to_convex_clips() {
        let clip = Rc::new(Clip::rect(v(0.0, 0.0), v(10.0, 10.0)));
        // A square half over the edge: cut to the half inside.
        let (_, fill, out) = clip_paint(&[square(5.0, 15.0)], std::slice::from_ref(&clip));
        assert!(out.trimmed);
        assert_eq!(fill.len(), 1);
        let pts: Vec<DVec2> = std::iter::once(fill[0].start)
            .chain(fill[0].segs.iter().map(Seg::end))
            .collect();
        assert!((area(&pts).abs() - 25.0).abs() < 1e-9, "{pts:?}");
        // A fill bigger than the clip all round: the clip's shape.
        let (_, fill, _) = clip_paint(&[square(-5.0, 15.0)], std::slice::from_ref(&clip));
        let pts: Vec<DVec2> = std::iter::once(fill[0].start)
            .chain(fill[0].segs.iter().map(Seg::end))
            .collect();
        assert!((area(&pts).abs() - 100.0).abs() < 1e-9);
        // An L-shaped clip can't cut a fill across it: it comes in whole, and says so.
        let l_shape = Subpath {
            start: v(0.0, 0.0),
            segs: vec![
                Seg::Line(v(10.0, 0.0)),
                Seg::Line(v(10.0, 4.0)),
                Seg::Line(v(4.0, 4.0)),
                Seg::Line(v(4.0, 10.0)),
                Seg::Line(v(0.0, 10.0)),
            ],
            closed: true,
        };
        let l = Rc::new(Clip::from_subpaths(&[l_shape]).unwrap());
        let (stroke, fill, out) = clip_paint(&[square(2.0, 8.0)], &[l]);
        assert!(out.whole && fill.len() == 1);
        // Its outline, though, is trimmed to the L: one run, from where it leaves the L's
        // inner corner round to where it comes back.
        assert!(out.trimmed);
        assert_eq!(stroke.len(), 1);
        assert!(
            stroke[0].start.distance(v(4.0, 8.0)) < 1e-9,
            "{:?}",
            stroke[0].start
        );
    }
}
