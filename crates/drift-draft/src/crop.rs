//! Cropping what a sheet's views show to their windows (#258): a view used to carry the whole
//! drawing, masked by its window, so releasing the clip in Illustrator showed everything
//! outside it. Each item seen through a window is now kept whole if it lies inside, left out
//! if it lies outside, and otherwise cut at the window's edge; the clip stays as well.
//!
//! Cuts are exact and keep each curve's kind: lines and polylines at the edge, arcs as arcs
//! (a circle crossing the edge as arcs), ellipses as elliptical arcs, Béziers split. A cut
//! piece of a dashed item starts its dashes where the whole item had them
//! ([`Pen::dash_offset`]). Fills are cut ring by ring and triangle by triangle (even–odd
//! fill of the cut rings is the fill's area inside the window, the window being convex). Text
//! is kept whole if its box reaches the window, else left out: cutting it would mean outlines.
//!
//! The window's edge counts as inside, within [`EDGE`]: a line drawn on a view's frame stays
//! whole, and a curve only touching the edge isn't split.

use std::f64::consts::TAU;

use drift_core::shape::{
    bezier_piece, bezier_point, bezier_segments, ellipse_angle, ellipse_point,
};
use drift_core::tolerance::FLATTEN_MM;
use drift_core::{Bounds, DVec2, Shape};

use crate::page::{Align, Drawing, Item, Prim, line_width_in};

/// How far outside its window something still counts as inside (page mm): the output's own
/// tolerance, so nothing that shows is lost. Cut pieces no longer than this are left out.
pub const EDGE: f64 = FLATTEN_MM;

impl Drawing {
    /// Every item seen through a window cropped to it (see the [module](self)); items with no
    /// window (a sheet's frame, title block, view titles) are left as they are.
    pub fn crop_to_windows(&mut self) {
        if !self.items.iter().any(|i| i.clip.is_some()) {
            return;
        }
        if self
            .items
            .iter()
            .any(|i| i.clip.is_some() && matches!(i.prim, Prim::Insert { .. }))
        {
            *self = self.exploded().into_owned();
        }
        let window = |clip: Bounds| Bounds {
            min: clip.min - DVec2::splat(EDGE),
            max: clip.max + DVec2::splat(EDGE),
        };
        let reaches: Vec<Reach> = self
            .items
            .iter()
            .map(|i| i.clip.map_or(Reach::Inside, |c| reach(&i.prim, &window(c))))
            .collect();
        // Most often every view shows its whole drawing, or only leaves things out: then the
        // items stay where they are.
        if !reaches.contains(&Reach::Crosses) {
            if reaches.contains(&Reach::Outside) {
                let mut keep = reaches.iter();
                self.items.retain(|_| keep.next() == Some(&Reach::Inside));
            }
            return;
        }
        let items = std::mem::take(&mut self.items);
        let mut out = Vec::with_capacity(items.len());
        for (item, reach) in items.into_iter().zip(reaches) {
            match (reach, item.clip) {
                (Reach::Inside, _) | (_, None) => out.push(item),
                (Reach::Outside, _) => {}
                (Reach::Crosses, Some(clip)) => {
                    let window = window(clip);
                    match &item.prim {
                        Prim::Hatch(h) => {
                            for part in item.hatch_parts(h) {
                                let prim = part.prim.clone();
                                cut(part, &prim, &window, &mut out);
                            }
                        }
                        prim => {
                            let prim = prim.clone();
                            cut(item, &prim, &window, &mut out);
                        }
                    }
                }
            }
        }
        self.items = out;
    }
}

/// Where an item lies against a window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    Inside,
    Outside,
    Crosses,
}

/// An item against `window`, from a box round it (made without allocating, but for a line
/// of text near the edge, which is measured): inside or outside when the box is, else
/// crossing (to be cut). Text that reaches the window counts as inside: it is kept whole.
fn reach(prim: &Prim, window: &Bounds) -> Reach {
    let b = match prim {
        Prim::Line(a, b) => {
            let mut r = Bounds::from_point(*a);
            r.include(*b);
            r
        }
        Prim::Polyline { points, .. } => points.iter().fold(Bounds::EMPTY, |mut r, p| {
            r.include(*p);
            r
        }),
        Prim::Circle { center, radius } => {
            Bounds::from_center_half_size(*center, DVec2::splat(radius.abs()))
        }
        Prim::Arc {
            center,
            radius,
            start,
            sweep,
        } => drift_core::shape::arc_bounds(*center, *radius, *start, *sweep),
        // The whole ellipse's box (a little loose for a part of one; the cut is exact).
        Prim::Ellipse {
            center,
            major,
            minor,
            ..
        } => {
            let v = major.perp().normalize_or_zero() * *minor;
            let half = DVec2::new(major.x.hypot(v.x), major.y.hypot(v.y));
            Bounds::from_center_half_size(*center, half)
        }
        // Inside its control points.
        Prim::Bezier { p } => p.iter().fold(Bounds::EMPTY, |mut r, q| {
            r.include(*q);
            r
        }),
        Prim::Fill { rings, .. } => rings.iter().flatten().fold(Bounds::EMPTY, |mut r, p| {
            r.include(*p);
            r
        }),
        Prim::Text {
            at,
            height,
            angle,
            align,
            text,
            style,
            font,
        } => {
            // First a box no letter is wider than (two cap heights each, and the tracking):
            // measuring the line takes its font's advances, so only text near the edge is
            // measured.
            let loose = text.chars().count() as f64 * (2.0 + 2.0 * style.tracking.abs()) * height;
            let b = text_box(*at, *height, *angle, *align, loose);
            if !b.intersects(window) {
                return Reach::Outside;
            }
            if within(&b, window) {
                return Reach::Inside;
            }
            let w = line_width_in(text, *height, *style, font.as_ref());
            return if text_box(*at, *height, *angle, *align, w).intersects(window) {
                Reach::Inside
            } else {
                Reach::Outside
            };
        }
        Prim::Hatch(h) => h.region.bounds,
        // (Exploded before cropping.)
        Prim::Insert { .. } => return Reach::Inside,
    };
    if b.is_empty() || !b.intersects(window) {
        Reach::Outside
    } else if within(&b, window) {
        Reach::Inside
    } else {
        Reach::Crosses
    }
}

fn within(b: &Bounds, window: &Bounds) -> bool {
    b.min.x >= window.min.x
        && b.min.y >= window.min.y
        && b.max.x <= window.max.x
        && b.max.y <= window.max.y
}

/// A point of a curve in the window (its edge within [`EDGE`], and rounding).
fn inside(p: DVec2, window: &Bounds) -> bool {
    let slack = drift_core::tolerance::SAME;
    p.x >= window.min.x - slack
        && p.y >= window.min.y - slack
        && p.x <= window.max.x + slack
        && p.y <= window.max.y + slack
}

/// A line of text's box, turned with it: from its baseline's start (per `align`) a little
/// below the baseline to above its capitals, `width` long.
fn text_box(at: DVec2, height: f64, angle: f64, align: Align, width: f64) -> Bounds {
    let shift = match align {
        Align::Left => 0.0,
        Align::Center => width / 2.0,
        Align::Right => width,
    };
    let (u, v) = (DVec2::from_angle(angle), DVec2::from_angle(angle).perp());
    let mut b = Bounds::EMPTY;
    for (x, y) in [
        (-shift, -0.3 * height),
        (width - shift, -0.3 * height),
        (width - shift, 1.2 * height),
        (-shift, 1.2 * height),
    ] {
        b.include(at + u * x + v * y);
    }
    b
}

/// `item` (whose prim is `prim`) cut to `window`: its pieces inside, pushed to `out`.
fn cut(item: Item, prim: &Prim, window: &Bounds, out: &mut Vec<Item>) {
    let piece = |prim: Prim, along: f64| {
        let mut it = Item {
            prim,
            ..item.clone()
        };
        it.pen.dash_offset += along;
        it
    };
    match prim {
        Prim::Line(a, b) => {
            if let Some((t0, t1)) = window.clip_segment(*a, *b) {
                let d = *b - *a;
                if d.length() * (t1 - t0) > EDGE {
                    out.push(piece(Prim::Line(*a + d * t0, *a + d * t1), d.length() * t0));
                }
            }
        }
        Prim::Polyline { points, closed } => {
            for (along, points) in polyline_runs(points, *closed, window) {
                out.push(piece(
                    Prim::Polyline {
                        points,
                        closed: false,
                    },
                    along,
                ));
            }
        }
        Prim::Circle { center, radius } => {
            let at = |t: f64| *center + DVec2::from_angle(t) * *radius;
            let cuts = circle_cuts(*center, *radius, 0.0, window);
            for (t0, t1) in spans(cuts, TAU, true, |t| inside(at(t), window)) {
                if (t1 - t0) * radius.abs() > EDGE {
                    out.push(piece(
                        Prim::Arc {
                            center: *center,
                            radius: *radius,
                            start: t0,
                            sweep: t1 - t0,
                        },
                        t0 * radius.abs(),
                    ));
                }
            }
        }
        Prim::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let at = |t: f64| *center + DVec2::from_angle(start + t) * *radius;
            let cuts = circle_cuts(*center, *radius, *start, window);
            for (t0, t1) in spans(cuts, *sweep, false, |t| inside(at(t), window)) {
                if (t1 - t0) * radius.abs() > EDGE {
                    out.push(piece(
                        Prim::Arc {
                            center: *center,
                            radius: *radius,
                            start: start + t0,
                            sweep: t1 - t0,
                        },
                        t0 * radius.abs(),
                    ));
                }
            }
        }
        Prim::Ellipse {
            center,
            major,
            minor,
            start,
            sweep,
        } => {
            let at = |t: f64| ellipse_point(*center, *major, *minor, start + t);
            let whole = *sweep >= TAU - 1e-12;
            let mut cuts = Vec::new();
            for (a, b) in edges(window) {
                for q in drift_geom::ellipse_segment_intersections(
                    *center, *major, *minor, 0.0, TAU, a, b,
                ) {
                    cuts.push((ellipse_angle(*center, *major, *minor, q) - start).rem_euclid(TAU));
                }
            }
            let part = |t0: f64, t1: f64| Shape::Ellipse {
                center: *center,
                major: *major,
                minor: *minor,
                start: start + t0,
                sweep: t1 - t0,
            };
            for (t0, t1) in spans(cuts, sweep.min(TAU), whole, |t| inside(at(t), window)) {
                if part(t0, t1).length() > EDGE {
                    let along = if t0 > 0.0 {
                        part(0.0, t0).length()
                    } else {
                        0.0
                    };
                    out.push(piece(
                        Prim::Ellipse {
                            center: *center,
                            major: *major,
                            minor: *minor,
                            start: start + t0,
                            sweep: t1 - t0,
                        },
                        along,
                    ));
                }
            }
        }
        Prim::Bezier { p } => {
            let cuts = bezier_cuts(p, window);
            for (t0, t1) in spans(cuts, 1.0, false, |t| inside(bezier_point(p, t), window)) {
                let q = bezier_piece(p, t0, t1);
                if (Shape::Bezier { p: q }).length() > EDGE {
                    let along = if t0 > 0.0 {
                        (Shape::Bezier {
                            p: bezier_piece(p, 0.0, t0),
                        })
                        .length()
                    } else {
                        0.0
                    };
                    out.push(piece(Prim::Bezier { p: q }, along));
                }
            }
        }
        Prim::Fill {
            rings,
            triangles,
            mask,
        } => {
            let rings: Vec<Vec<DVec2>> = rings
                .iter()
                .map(|r| clip_ring(r, window))
                .filter(|r| r.len() >= 3 && drift_core::polygon::area(r).abs() > EDGE * EDGE)
                .collect();
            if rings.is_empty() {
                return;
            }
            let mut cut_triangles = Vec::with_capacity(triangles.len());
            for t in triangles {
                let poly = clip_ring(t, window);
                for k in 1..poly.len().saturating_sub(1) {
                    let tri = [poly[0], poly[k], poly[k + 1]];
                    if drift_core::polygon::area(&tri).abs() > drift_core::tolerance::AREA_NOISE {
                        cut_triangles.push(tri);
                    }
                }
            }
            out.push(piece(
                Prim::Fill {
                    rings,
                    triangles: cut_triangles,
                    mask: *mask,
                },
                0.0,
            ));
        }
        // Text reaches the window or not (see `reach`); hatches are cut as their parts.
        Prim::Text { .. } | Prim::Hatch(_) | Prim::Insert { .. } => {
            out.push(piece(prim.clone(), 0.0))
        }
    }
}

/// The window's four edges, anticlockwise.
fn edges(w: &Bounds) -> [(DVec2, DVec2); 4] {
    let c = [
        w.min,
        DVec2::new(w.max.x, w.min.y),
        w.max,
        DVec2::new(w.min.x, w.max.y),
    ];
    [(c[0], c[1]), (c[1], c[2]), (c[2], c[3]), (c[3], c[0])]
}

/// Where a circle crosses the window's edges, as angles from `start` (0 to 2π).
fn circle_cuts(center: DVec2, radius: f64, start: f64, window: &Bounds) -> Vec<f64> {
    let mut cuts = Vec::new();
    for (a, b) in edges(window) {
        for q in drift_geom::circle_segment_intersections(center, radius, a, b) {
            let r = q - center;
            cuts.push((r.y.atan2(r.x) - start).rem_euclid(TAU));
        }
    }
    cuts
}

/// Where a Bézier crosses the lines of the window's edges, as parameters: each change of
/// side between samples finer than its flattening, found by halving. (A crossing on an edge's
/// line but off the window only makes one more piece to sort, inside or out.)
fn bezier_cuts(p: &[DVec2; 4], window: &Bounds) -> Vec<f64> {
    let n = 4 * bezier_segments(p, FLATTEN_MM).max(16);
    let mut cuts = Vec::new();
    for (axis, value) in [
        (0, window.min.x),
        (0, window.max.x),
        (1, window.min.y),
        (1, window.max.y),
    ] {
        let f = |t: f64| bezier_point(p, t)[axis] - value;
        let mut prev = f(0.0);
        for k in 1..=n {
            let t1 = k as f64 / n as f64;
            let next = f(t1);
            if prev * next < 0.0 {
                let (mut lo, mut hi) = (t1 - 1.0 / n as f64, t1);
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if (f(mid) < 0.0) == (f(lo) < 0.0) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                cuts.push(0.5 * (lo + hi));
            }
            prev = next;
        }
    }
    cuts
}

/// The spans of a curve's parameter (0 to `end`) inside the window, between its `cuts`:
/// each span between two cuts is in or out by its middle, and spans in a row that are in
/// are one (so a curve touching the edge stays whole). For a closed curve (`closed`), a span
/// running to the end and one from the start are one across the seam: it starts past `end`'s
/// wrap, so its end is over `end`. A closed curve inside throughout gives `(0, end)`.
fn spans(
    mut cuts: Vec<f64>,
    end: f64,
    closed: bool,
    inside: impl Fn(f64) -> bool,
) -> Vec<(f64, f64)> {
    cuts.retain(|t| *t > 0.0 && *t < end);
    cuts.push(0.0);
    cuts.push(end);
    cuts.sort_by(f64::total_cmp);
    cuts.dedup();
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut joined = false;
    for w in cuts.windows(2) {
        let (t0, t1) = (w[0], w[1]);
        if !inside(0.5 * (t0 + t1)) {
            joined = false;
            continue;
        }
        match out.last_mut() {
            Some(last) if joined => last.1 = t1,
            _ => out.push((t0, t1)),
        }
        joined = true;
    }
    if closed && out.len() >= 2 && out[0].0 == 0.0 && out.last().is_some_and(|l| l.1 == end) {
        let first = out.remove(0);
        if let Some(last) = out.last_mut() {
            last.1 = end + first.1;
        }
    }
    out
}

/// A polyline's runs inside the window, each with how far along the polyline it starts.
/// A closed one cut open runs across its first point where it can (its run starts partway
/// round); one inside throughout comes back as one open run round it.
fn polyline_runs(points: &[DVec2], closed: bool, window: &Bounds) -> Vec<(f64, Vec<DVec2>)> {
    let n = points.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    let mut runs: Vec<(f64, Vec<DVec2>)> = Vec::new();
    let mut open: Option<(f64, Vec<DVec2>)> = None;
    let mut along = 0.0;
    // Whether the first run starts at the first point, and the last ends back at it.
    let (mut from_first, mut to_first) = (false, false);
    for i in 0..segments {
        let (a, b) = (points[i], points[(i + 1) % n]);
        let d = b - a;
        let length = d.length();
        match window.clip_segment(a, b) {
            None => runs.extend(open.take()),
            Some((t0, t1)) => {
                if t0 > 0.0 {
                    runs.extend(open.take());
                }
                if i == 0 && t0 == 0.0 {
                    from_first = true;
                }
                let run = open.get_or_insert_with(|| (along + length * t0, vec![a + d * t0]));
                run.1.push(a + d * t1);
                if t1 < 1.0 {
                    runs.extend(open.take());
                } else if i + 1 == segments && closed {
                    to_first = true;
                }
            }
        }
        along += length;
    }
    runs.extend(open.take());
    if closed && from_first && to_first && runs.len() >= 2 {
        let first = runs.remove(0);
        if let Some(last) = runs.last_mut() {
            last.1.extend(first.1.into_iter().skip(1));
        }
    }
    runs.retain(|(_, pts)| pts.windows(2).map(|w| w[0].distance(w[1])).sum::<f64>() > EDGE);
    runs
}

/// A ring (or triangle) cut to the window (Sutherland–Hodgman): the part inside, as a ring.
/// Where the ring leaves and comes back the cut runs along the window's edge, which fills
/// nothing.
fn clip_ring(ring: &[DVec2], window: &Bounds) -> Vec<DVec2> {
    let mut poly: Vec<DVec2> = ring.to_vec();
    // Each edge as (axis, value, keep the side above it).
    for (axis, value, above) in [
        (0, window.min.x, true),
        (0, window.max.x, false),
        (1, window.min.y, true),
        (1, window.max.y, false),
    ] {
        if poly.is_empty() {
            break;
        }
        let keep = |p: DVec2| {
            if above {
                p[axis] >= value
            } else {
                p[axis] <= value
            }
        };
        let mut next = Vec::with_capacity(poly.len() + 4);
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (ka, kb) = (keep(a), keep(b));
            if ka {
                next.push(a);
            }
            if ka != kb {
                let t = (value - a[axis]) / (b[axis] - a[axis]);
                let mut q = a + (b - a) * t;
                q[axis] = value;
                next.push(q);
            }
        }
        poly = next;
    }
    poly.dedup_by(|a, b| a.distance(*b) <= drift_core::tolerance::SAME);
    if poly.len() > 1 && poly.first() == poly.last() {
        poly.pop();
    }
    poly
}

/// A cut fill ring's edges off its window's edge (`clip`, the item's window): where it was
/// cut it runs along the window's edge, in places doubling back with nothing between, so
/// an output that strokes a fill's edges strokes only these. `None` when no side of it lies
/// on the window's edge (stroke it all round, closed).
pub fn off_window_edge(ring: &[DVec2], clip: Bounds) -> Option<Vec<Vec<DVec2>>> {
    let w = Bounds {
        min: clip.min - DVec2::splat(EDGE),
        max: clip.max + DVec2::splat(EDGE),
    };
    let near = |a: f64, b: f64| (a - b).abs() <= drift_core::tolerance::SAME;
    let on_edge = |a: DVec2, b: DVec2| {
        (near(a.x, w.min.x) && near(b.x, w.min.x))
            || (near(a.x, w.max.x) && near(b.x, w.max.x))
            || (near(a.y, w.min.y) && near(b.y, w.min.y))
            || (near(a.y, w.max.y) && near(b.y, w.max.y))
    };
    let n = ring.len();
    let first_on = (0..n).find(|&i| on_edge(ring[i], ring[(i + 1) % n]))?;
    // From the end of a side on the edge, round the ring once.
    let mut runs = Vec::new();
    let mut run: Vec<DVec2> = Vec::new();
    for k in 1..=n {
        let i = (first_on + k) % n;
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if on_edge(a, b) {
            if run.len() >= 2 {
                runs.push(std::mem::take(&mut run));
            }
            run.clear();
        } else {
            if run.is_empty() {
                run.push(a);
            }
            run.push(b);
        }
    }
    if run.len() >= 2 {
        runs.push(run);
    }
    Some(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::{Pen, TextStyle};
    use drift_core::LineType;

    const W: Bounds = Bounds {
        min: DVec2::new(0.0, 0.0),
        max: DVec2::new(100.0, 50.0),
    };

    fn crop(prims: Vec<Prim>) -> Vec<Item> {
        crop_with(Pen::new(0.25), prims)
    }

    fn crop_with(pen: Pen, prims: Vec<Prim>) -> Vec<Item> {
        let mut d = Drawing::new(DVec2::new(200.0, 100.0));
        for prim in prims {
            d.items.push(Item {
                layer: "0".into(),
                pen,
                prim,
                clip: Some(W),
            });
        }
        d.crop_to_windows();
        d.items
    }

    /// Every point the item is drawn through lies in the window (within EDGE).
    fn all_inside(items: &[Item]) -> bool {
        let w = Bounds {
            min: W.min - DVec2::splat(EDGE + 1e-9),
            max: W.max + DVec2::splat(EDGE + 1e-9),
        };
        items.iter().all(|it| match &it.prim {
            Prim::Line(a, b) => inside(*a, &w) && inside(*b, &w),
            Prim::Polyline { points, .. } => points.iter().all(|p| inside(*p, &w)),
            Prim::Fill {
                rings, triangles, ..
            } => {
                rings.iter().flatten().all(|p| inside(*p, &w))
                    && triangles.iter().flatten().all(|p| inside(*p, &w))
            }
            prim @ (Prim::Arc { .. } | Prim::Ellipse { .. } | Prim::Bezier { .. }) => {
                crate::page::prim_points_within(prim, 1e-4)
                    .iter()
                    .all(|p| inside(*p, &w))
            }
            _ => true,
        })
    }

    #[test]
    fn keeps_whats_inside_and_leaves_out_whats_outside() {
        let inside = Prim::Line(DVec2::new(10.0, 10.0), DVec2::new(90.0, 40.0));
        let outside = Prim::Line(DVec2::new(110.0, 10.0), DVec2::new(190.0, 40.0));
        let items = crop(vec![inside.clone(), outside]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].prim, inside);
        assert_eq!(items[0].pen.dash_offset, 0.0);
    }

    #[test]
    fn leaves_items_without_a_window_alone() {
        let mut d = Drawing::new(DVec2::new(200.0, 100.0));
        d.line(
            "0",
            Pen::new(0.25),
            DVec2::new(-50.0, 0.0),
            DVec2::new(500.0, 0.0),
        );
        let before = d.items.clone();
        d.crop_to_windows();
        assert_eq!(d.items, before);
    }

    #[test]
    fn cuts_a_line_at_the_edge() {
        let items = crop(vec![Prim::Line(
            DVec2::new(-50.0, 25.0),
            DVec2::new(150.0, 25.0),
        )]);
        assert_eq!(items.len(), 1);
        let Prim::Line(a, b) = items[0].prim else {
            panic!("a line")
        };
        assert!(a.distance(DVec2::new(-EDGE, 25.0)) < 1e-9, "{a}");
        assert!(b.distance(DVec2::new(100.0 + EDGE, 25.0)) < 1e-9, "{b}");
        assert!((items[0].pen.dash_offset - (50.0 - EDGE)).abs() < 1e-9);
    }

    #[test]
    fn a_line_on_the_edge_stays_whole() {
        let on = Prim::Line(DVec2::new(0.0, 0.0), DVec2::new(100.0, 0.0));
        let border = Prim::Polyline {
            points: vec![
                W.min,
                DVec2::new(W.max.x, W.min.y),
                W.max,
                DVec2::new(W.min.x, W.max.y),
            ],
            closed: true,
        };
        let items = crop(vec![on.clone(), border.clone()]);
        assert_eq!(
            items.iter().map(|i| &i.prim).collect::<Vec<_>>(),
            [&on, &border]
        );
    }

    #[test]
    fn a_tangent_arc_stays_whole() {
        // A circle touching the top edge from inside, and one from outside.
        let touching = Prim::Circle {
            center: DVec2::new(50.0, 40.0),
            radius: 10.0,
        };
        let items = crop(vec![touching.clone()]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].prim, touching);
        let arc = Prim::Arc {
            center: DVec2::new(50.0, 30.0),
            radius: 20.0,
            start: 0.3,
            sweep: 2.5,
        };
        let items = crop(vec![arc.clone()]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].prim, arc);
        // Crossing the left edge and touching the top one just where the edge's tolerance
        // ends: one arc, not split where it touches.
        let items = crop(vec![Prim::Circle {
            center: DVec2::new(5.0, 40.0),
            radius: 10.0 + EDGE,
        }]);
        assert_eq!(items.len(), 1, "{items:?}");
        assert!(all_inside(&items));
        // Touching from outside, just past the edge's tolerance: left out.
        let outside = Prim::Circle {
            center: DVec2::new(50.0, 60.0 + 2.0 * EDGE),
            radius: 10.0,
        };
        assert!(crop(vec![outside]).is_empty());
    }

    #[test]
    fn a_circle_across_an_edge_becomes_an_arc() {
        let items = crop(vec![Prim::Circle {
            center: DVec2::new(100.0, 25.0),
            radius: 10.0,
        }]);
        assert_eq!(items.len(), 1);
        let Prim::Arc { start, sweep, .. } = items[0].prim else {
            panic!("an arc: {:?}", items[0].prim)
        };
        // The left half, from the top round to the bottom (just past the edge's tolerance).
        assert!((sweep - std::f64::consts::PI).abs() < 2e-3, "{sweep}");
        assert!(
            (start - std::f64::consts::FRAC_PI_2).abs() < 2e-3,
            "{start}"
        );
        assert!((items[0].pen.dash_offset - start * 10.0).abs() < 1e-9);
        assert!(all_inside(&items));
    }

    #[test]
    fn a_circle_cut_across_its_seam_is_one_arc() {
        // Crossing the left edge: what's inside runs through angle 0.
        let items = crop(vec![Prim::Circle {
            center: DVec2::new(5.0, 25.0),
            radius: 10.0,
        }]);
        assert_eq!(items.len(), 1);
        let Prim::Arc { start, sweep, .. } = items[0].prim else {
            panic!("an arc")
        };
        assert!(
            start > std::f64::consts::PI && start + sweep > TAU,
            "{start} {sweep}"
        );
        assert!(all_inside(&items));
    }

    #[test]
    fn cuts_arcs_ellipses_and_beziers_keeping_their_kind() {
        let items = crop(vec![
            Prim::Arc {
                center: DVec2::new(0.0, 0.0),
                radius: 30.0,
                start: -1.0,
                sweep: 3.0,
            },
            Prim::Ellipse {
                center: DVec2::new(95.0, 25.0),
                major: DVec2::new(20.0, 5.0),
                minor: 8.0,
                start: 0.0,
                sweep: TAU,
            },
            Prim::Bezier {
                p: [
                    DVec2::new(-20.0, 10.0),
                    DVec2::new(40.0, 80.0),
                    DVec2::new(60.0, -30.0),
                    DVec2::new(120.0, 30.0),
                ],
            },
        ]);
        assert!(items.iter().any(|i| matches!(i.prim, Prim::Arc { .. })));
        assert!(items.iter().any(|i| matches!(i.prim, Prim::Ellipse { .. })));
        assert!(items.iter().any(|i| matches!(i.prim, Prim::Bezier { .. })));
        assert!(items.iter().all(|i| matches!(
            i.prim,
            Prim::Arc { .. } | Prim::Ellipse { .. } | Prim::Bezier { .. }
        )));
        assert!(all_inside(&items));
        // The pieces of the Bézier join up where it stays inside, and end on the edges.
        for it in &items {
            if let Prim::Bezier { p } = it.prim {
                for q in [p[0], p[3]] {
                    let on_edge = (q.x.abs() - EDGE).abs() < 1e-6
                        || (q.x - 100.0 - EDGE).abs() < 1e-6
                        || (q.y + EDGE).abs() < 1e-6
                        || (q.y - 50.0 - EDGE).abs() < 1e-6;
                    assert!(on_edge, "{q}");
                }
            }
        }
    }

    #[test]
    fn a_closed_polyline_cut_open_runs_across_its_first_point() {
        // A square around the window's left edge, starting inside it.
        let items = crop(vec![Prim::Polyline {
            points: vec![
                DVec2::new(10.0, 20.0),
                DVec2::new(10.0, 30.0),
                DVec2::new(-10.0, 30.0),
                DVec2::new(-10.0, 20.0),
            ],
            closed: true,
        }]);
        assert_eq!(items.len(), 1);
        let Prim::Polyline { points, closed } = &items[0].prim else {
            panic!("a polyline")
        };
        assert!(!closed);
        let e = -EDGE;
        let want = [
            DVec2::new(e, 20.0),
            DVec2::new(10.0, 20.0),
            DVec2::new(10.0, 30.0),
            DVec2::new(e, 30.0),
        ];
        assert_eq!(points.len(), want.len(), "{points:?}");
        assert!(
            points.iter().zip(want).all(|(p, q)| p.distance(q) < 1e-9),
            "{points:?}"
        );
        // It starts on the last side, partway round: 10 + 20 + 10 + (10 − EDGE) along.
        assert!(
            (items[0].pen.dash_offset - (50.0 - EDGE)).abs() < 1e-9,
            "{}",
            items[0].pen.dash_offset
        );
    }

    #[test]
    fn short_pieces_are_left_out() {
        let items = crop(vec![Prim::Line(
            DVec2::new(-10.0, 25.0),
            DVec2::new(-EDGE / 2.0, 25.0),
        )]);
        assert!(items.is_empty());
    }

    /// Even–odd area of rings: the sum of their signed areas' magnitudes taken by nesting
    /// works only for simple cases; sampled instead on a fine grid.
    fn even_odd_area(rings: &[Vec<DVec2>], b: Bounds, n: usize) -> f64 {
        let cell = (b.max - b.min) / n as f64;
        let mut count = 0;
        for i in 0..n {
            for j in 0..n {
                let p = b.min + DVec2::new((i as f64 + 0.5) * cell.x, (j as f64 + 0.5) * cell.y);
                let crossings = rings
                    .iter()
                    .filter(|r| drift_core::polygon::contains(r, p))
                    .count();
                count += crossings % 2;
            }
        }
        count as f64 * cell.x * cell.y
    }

    #[test]
    fn a_u_shaped_fill_with_a_hole_is_cut_to_its_part_inside() {
        // A U whose two arms reach up out of the window's top, with a hole across the
        // window's right edge.
        let u = vec![
            DVec2::new(10.0, 10.0),
            DVec2::new(130.0, 10.0),
            DVec2::new(130.0, 80.0),
            DVec2::new(110.0, 80.0),
            DVec2::new(110.0, 30.0),
            DVec2::new(30.0, 30.0),
            DVec2::new(30.0, 80.0),
            DVec2::new(10.0, 80.0),
        ];
        let hole = vec![
            DVec2::new(90.0, 15.0),
            DVec2::new(90.0, 25.0),
            DVec2::new(120.0, 25.0),
            DVec2::new(120.0, 15.0),
        ];
        let items = crop(vec![Prim::Fill {
            rings: vec![u.clone(), hole.clone()],
            triangles: Vec::new(),
            mask: false,
        }]);
        assert_eq!(items.len(), 1);
        let Prim::Fill { rings, .. } = &items[0].prim else {
            panic!("a fill")
        };
        assert!(all_inside(&items));
        // Exactly: the U inside the window (20×(50−10) arm + 80×20 base, the right arm
        // gone past x = 100 but the base running on to the edge) less the hole's part.
        let shoelace: f64 = rings.iter().map(|r| drift_core::polygon::area(r)).sum();
        let window = Bounds {
            min: W.min - DVec2::splat(EDGE),
            max: W.max + DVec2::splat(EDGE),
        };
        let expected = |w: Bounds| {
            let x1 = w.max.x;
            let y1 = w.max.y;
            // base 10..x1 × 10..30, left arm 10..30 × 30..y1, hole 90..x1 × 15..25.
            (x1 - 10.0) * 20.0 + 20.0 * (y1 - 30.0) - (x1 - 90.0) * 10.0
        };
        // Outer ring anticlockwise, hole clockwise: their signed areas add to the area.
        let signed =
            drift_core::polygon::area(&rings[0]).abs() - drift_core::polygon::area(&rings[1]).abs();
        let want = expected(window);
        assert!(
            (signed - want).abs() / want < 1e-6,
            "{signed} vs {want} ({shoelace})"
        );
        // And by even–odd sampling, agreeing to the grid.
        let sampled = even_odd_area(rings, window, 400);
        assert!((sampled - want).abs() / want < 1e-2, "{sampled} vs {want}");
        // What an output may stroke of its edges runs nowhere along the window's edge.
        let runs = off_window_edge(&rings[0], W).expect("cut at the edge");
        assert!(!runs.is_empty());
        let along_edge = |a: DVec2, b: DVec2| {
            let same =
                |u: f64, v: f64, edge: f64| (u - edge).abs() < 1e-9 && (v - edge).abs() < 1e-9;
            same(a.x, b.x, window.min.x)
                || same(a.x, b.x, window.max.x)
                || same(a.y, b.y, window.min.y)
                || same(a.y, b.y, window.max.y)
        };
        for run in &runs {
            assert!(run.windows(2).all(|w| !along_edge(w[0], w[1])), "{run:?}");
        }
        // A ring nowhere on the edge is stroked whole.
        let square = [
            DVec2::new(40.0, 40.0),
            DVec2::new(45.0, 40.0),
            DVec2::new(45.0, 45.0),
        ];
        assert_eq!(off_window_edge(&square, W), None);
    }

    #[test]
    fn fill_triangles_are_cut_too() {
        let tri = [
            DVec2::new(50.0, 25.0),
            DVec2::new(150.0, 25.0),
            DVec2::new(50.0, 75.0),
        ];
        let items = crop(vec![Prim::Fill {
            rings: vec![tri.to_vec()],
            triangles: vec![tri],
            mask: false,
        }]);
        assert!(all_inside(&items));
        let Prim::Fill {
            rings, triangles, ..
        } = &items[0].prim
        else {
            panic!("a fill")
        };
        let a: f64 = triangles
            .iter()
            .map(|t| drift_core::polygon::area(t).abs())
            .sum();
        assert!((a - drift_core::polygon::area(&rings[0]).abs()).abs() < 1e-9);
    }

    #[test]
    fn text_is_kept_whole_or_left_out() {
        let text = |x: f64| Prim::Text {
            at: DVec2::new(x, 25.0),
            height: 5.0,
            angle: 0.0,
            align: Align::Left,
            text: "Straddling".into(),
            style: TextStyle::REGULAR,
            font: None,
        };
        let straddling = text(95.0);
        let items = crop(vec![straddling.clone(), text(120.0)]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].prim, straddling);
        // Turned a quarter, it reaches up and out of the window, not along.
        let turned = Prim::Text {
            at: DVec2::new(102.0, 10.0),
            height: 5.0,
            angle: std::f64::consts::FRAC_PI_2,
            align: Align::Left,
            text: "Straddling".into(),
            style: TextStyle::REGULAR,
            font: None,
        };
        assert_eq!(crop(vec![turned]).len(), 1);
    }

    #[test]
    fn dashes_carry_on_from_where_the_whole_item_had_them() {
        let pen = Pen::new(0.25).with_type(LineType::Dashed);
        let items = crop_with(
            pen,
            vec![Prim::Arc {
                center: DVec2::new(50.0, -10.0),
                radius: 30.0,
                start: 0.0,
                sweep: std::f64::consts::PI,
            }],
        );
        assert_eq!(items.len(), 1);
        let Prim::Arc { start, .. } = items[0].prim else {
            panic!("an arc")
        };
        assert!((items[0].pen.dash_offset - 30.0 * start).abs() < 1e-9);
    }
}
