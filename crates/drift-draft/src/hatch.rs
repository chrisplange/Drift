//! Hatch lines clipped exactly to a region.
//!
//! Each pattern family is a set of parallel lines. For each line, crossings with the region's
//! boundary edges (lines and arcs, holes included) are found analytically in `f64`, sorted,
//! and paired into inside intervals (even–odd), which the family's dash pattern then cuts into
//! segments. Arcs are split where they turn back across the scan direction, so every piece
//! crosses a scan line at most once.

use std::f64::consts::{PI, TAU};

use drift_core::{Bounds, DVec2};
use drift_geom::regions::{Edge, Region};

use crate::pattern::Pattern;

/// Where and how big a pattern is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub scale: f64,
    /// Degrees counter-clockwise.
    pub angle: f64,
    pub origin: DVec2,
}

impl Default for Placement {
    fn default() -> Self {
        Self {
            scale: 1.0,
            angle: 0.0,
            origin: DVec2::ZERO,
        }
    }
}

/// Hatch segments in world coordinates. A segment with equal ends is a dot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hatching {
    pub segments: Vec<[DVec2; 2]>,
    /// The segment limit was reached (the pattern is too dense for the area asked for).
    pub truncated: bool,
}

/// Hatch `region` with `pattern`. Only the part inside `window` (if given) is generated, and
/// at most `limit` segments.
pub fn hatch(
    region: &Region,
    pattern: &Pattern,
    placement: &Placement,
    window: Option<Bounds>,
    limit: usize,
) -> Hatching {
    let mut out = Hatching::default();
    let scale = placement.scale.abs().max(1e-9);
    let rotation = DVec2::from_angle(placement.angle.to_radians());
    let mut bounds = Bounds::EMPTY;
    for p in region.outer.polygon(0.05) {
        bounds.include(p);
    }
    if bounds.is_empty() {
        return out;
    }
    let pad = bounds.size().max_element() * 0.01;
    bounds.min -= DVec2::splat(pad);
    bounds.max += DVec2::splat(pad);
    let area = match window {
        Some(w) if !w.intersects(&bounds) => return out,
        Some(w) => Bounds {
            min: bounds.min.max(w.min),
            max: bounds.max.min(w.max),
        },
        None => bounds,
    };
    let edges: Vec<&Edge> = std::iter::once(&region.outer)
        .chain(region.holes.iter())
        .flat_map(|l| l.edges.iter())
        .collect();

    for family in &pattern.lines {
        let d = DVec2::from_angle((family.angle + placement.angle).to_radians());
        let n = d.perp();
        let o = placement.origin + rotation.rotate(family.origin * scale);
        let dx = family.delta.x * scale;
        let dy = family.delta.y * scale;
        let dashes: Vec<f64> = family.dashes.iter().map(|v| v * scale).collect();
        // Which lines cross the area: v = k·dy across the family.
        let corners = [
            area.min,
            DVec2::new(area.max.x, area.min.y),
            area.max,
            DVec2::new(area.min.x, area.max.y),
        ];
        let vs = corners.map(|c| (c - o).dot(n));
        let (vmin, vmax) = (
            vs.iter().copied().fold(f64::INFINITY, f64::min),
            vs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        );
        let (k0, k1) = if dy.abs() < 1e-12 {
            if vmin <= 0.0 && vmax >= 0.0 {
                (0, 0)
            } else {
                continue;
            }
        } else {
            let (a, b) = (vmin / dy, vmax / dy);
            (a.min(b).ceil() as i64, a.max(b).floor() as i64)
        };
        if k1 < k0 {
            continue;
        }
        if (k1 - k0) as usize > limit {
            out.truncated = true;
            return out;
        }
        // Scan slightly off each line, so vertices exactly on it don't count twice.
        let nudge = 1e-9 * dy.abs().max(1e-6);
        let mut crossings: Vec<f64> = Vec::new();
        for k in k0..=k1 {
            let c = k as f64 * dy;
            crossings.clear();
            for e in &edges {
                edge_crossings(e, o, d, n, c, c + nudge, &mut crossings);
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by(f64::total_cmp);
            let base = o + n * c;
            let (wu0, wu1) = match clip_to(area, base, d) {
                Some(r) => r,
                None => continue,
            };
            let phase = k as f64 * dx;
            for [a, b] in crossings.as_chunks::<2>().0 {
                let (ua, ub) = (a.max(wu0), b.min(wu1));
                if ub <= ua {
                    continue;
                }
                if !emit(&mut out, base, d, ua, ub, phase, &dashes, limit) {
                    out.truncated = true;
                    return out;
                }
            }
        }
    }
    out
}

/// Emit the dashes of one inside interval. Returns false at the segment limit.
#[allow(clippy::too_many_arguments)]
fn emit(
    out: &mut Hatching,
    base: DVec2,
    d: DVec2,
    ua: f64,
    ub: f64,
    phase: f64,
    dashes: &[f64],
    limit: usize,
) -> bool {
    let at = |u: f64| base + d * u;
    let period: f64 = dashes.iter().map(|v| v.abs()).sum();
    if dashes.is_empty() || period <= 1e-12 {
        out.segments.push([at(ua), at(ub)]);
        return out.segments.len() < limit;
    }
    let mut s = phase + ((ua - phase) / period).floor() * period;
    while s < ub {
        for &e in dashes {
            let len = e.abs();
            if e > 0.0 {
                let (a, b) = (s.max(ua), (s + len).min(ub));
                if b > a {
                    out.segments.push([at(a), at(b)]);
                }
            } else if e == 0.0 && s >= ua && s <= ub {
                out.segments.push([at(s), at(s)]);
            }
            if out.segments.len() >= limit {
                return false;
            }
            s += len;
            if s >= ub {
                break;
            }
        }
    }
    true
}

/// The `u` range of the line `base + u·d` inside `b`.
fn clip_to(b: Bounds, base: DVec2, d: DVec2) -> Option<(f64, f64)> {
    b.clip_line(base, d).filter(|(lo, hi)| hi > lo)
}

/// Crossings of the scan line `v = c` (in the frame of `o`, `d`, `n`) with an edge, as `u`
/// values along the line. Whether the edge crosses is decided at `side` (just off the line),
/// so a vertex on the line counts once; where it crosses is found on the line itself.
fn edge_crossings(e: &Edge, o: DVec2, d: DVec2, n: DVec2, c: f64, side: f64, out: &mut Vec<f64>) {
    match *e {
        Edge::Line { a, b } => {
            let (va, vb) = ((a - o).dot(n), (b - o).dot(n));
            if (va > side) != (vb > side) {
                let t = ((c - va) / (vb - va)).clamp(0.0, 1.0);
                out.push((a + (b - a) * t - o).dot(d));
            }
        }
        Edge::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let vc = (center - o).dot(n);
            let beta = n.y.atan2(n.x);
            // Split where v turns (φ = β or β + π) so each piece is monotone.
            let mut ts = vec![0.0];
            for psi in [beta, beta + PI] {
                let t = if sweep >= 0.0 {
                    (psi - start).rem_euclid(TAU)
                } else {
                    -(start - psi).rem_euclid(TAU)
                };
                if t.abs() > 1e-12 && t.abs() < sweep.abs() - 1e-12 {
                    ts.push(t);
                }
            }
            ts.push(sweep);
            ts.sort_by(|a, b| {
                if sweep >= 0.0 {
                    a.total_cmp(b)
                } else {
                    b.total_cmp(a)
                }
            });
            let v = |phi: f64| vc + radius * (phi - beta).cos();
            for w in ts.windows(2) {
                let (p1, p2) = (start + w[0], start + w[1]);
                if (v(p1) > side) != (v(p2) > side) {
                    let x = ((c - vc) / radius).clamp(-1.0, 1.0);
                    let acos = x.acos();
                    let (lo, hi) = if p1 <= p2 { (p1, p2) } else { (p2, p1) };
                    let pick = [beta + acos, beta - acos]
                        .into_iter()
                        .flat_map(|phi| (-2..=2).map(move |m| phi + m as f64 * TAU))
                        .filter(|phi| *phi >= lo - 1e-9 && *phi <= hi + 1e-9)
                        .min_by(|a, b| {
                            let mid = (lo + hi) / 2.0;
                            (a - mid).abs().total_cmp(&(b - mid).abs())
                        });
                    if let Some(phi) = pick {
                        let p = center + DVec2::from_angle(phi) * radius;
                        out.push((p - o).dot(d));
                    }
                }
            }
        }
    }
}

/// Tessellate a region (holes excluded) into world-space triangles. Arcs are flattened in
/// steps of at most `max_angle` radians.
pub fn tessellate(region: &Region, max_angle: f64) -> Option<(Vec<DVec2>, Vec<u32>)> {
    use lyon_tessellation::math::point;
    use lyon_tessellation::path::Path;
    use lyon_tessellation::{
        BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex, VertexBuffers,
    };

    // Tessellate relative to the region's centroid: lyon works in f32.
    let origin = region.centroid;
    let mut builder = Path::builder();
    for lp in std::iter::once(&region.outer).chain(&region.holes) {
        let poly = lp.polygon(max_angle);
        let Some(first) = poly.first() else {
            continue;
        };
        let to = |p: DVec2| {
            let d = p - origin;
            point(d.x as f32, d.y as f32)
        };
        builder.begin(to(*first));
        for p in &poly[1..] {
            builder.line_to(to(*p));
        }
        builder.close();
    }
    let path = builder.build();
    let mut buffers: VertexBuffers<lyon_tessellation::math::Point, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::default()
                .with_fill_rule(FillRule::EvenOdd)
                .with_tolerance(1e-4),
            &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| v.position()),
        )
        .ok()?;
    let vertices = buffers
        .vertices
        .iter()
        .map(|v| origin + DVec2::new(f64::from(v.x), f64::from(v.y)))
        .collect();
    Some((vertices, buffers.indices))
}

#[cfg(test)]
mod tests {
    use drift_core::Shape;
    use drift_geom::regions::regions;

    use super::*;
    use crate::pattern::find;

    fn v(x: f64, y: f64) -> DVec2 {
        DVec2::new(x, y)
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Shape> {
        let c = [v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)];
        (0..4)
            .map(|i| Shape::Line {
                a: c[i],
                b: c[(i + 1) % 4],
            })
            .collect()
    }

    fn total_length(h: &Hatching) -> f64 {
        h.segments.iter().map(|[a, b]| a.distance(*b)).sum()
    }

    #[test]
    fn ansi31_fills_a_square_exactly() {
        let r = &regions(&rect(0.0, 0.0, 10.0, 10.0))[0];
        let h = hatch(
            r,
            find("ANSI31").unwrap(),
            &Placement::default(),
            None,
            10_000,
        );
        assert!(!h.truncated);
        // Parallel lines cover area / spacing in length.
        let expected = 100.0 / 3.175;
        assert!(
            (total_length(&h) - expected).abs() < 3.175 * 1.5,
            "{}",
            total_length(&h)
        );
        for [a, b] in &h.segments {
            for p in [a, b] {
                let on_edge = p.x.abs() < 1e-9
                    || (p.x - 10.0).abs() < 1e-9
                    || p.y.abs() < 1e-9
                    || (p.y - 10.0).abs() < 1e-9;
                assert!(on_edge, "{p:?} should be on the boundary");
            }
            assert!(r.contains((*a + *b) * 0.5));
        }
    }

    #[test]
    fn holes_and_arcs_are_respected() {
        let mut curves = rect(0.0, 0.0, 20.0, 20.0);
        curves.push(Shape::Circle {
            center: v(10.0, 10.0),
            radius: 5.0,
        });
        let all = regions(&curves);
        let plate = all.iter().find(|r| !r.holes.is_empty()).unwrap();
        let h = hatch(
            plate,
            find("ANSI37").unwrap(),
            &Placement::default(),
            None,
            100_000,
        );
        assert!(!h.segments.is_empty());
        for [a, b] in &h.segments {
            let mid = (*a + *b) * 0.5;
            assert!(mid.distance(v(10.0, 10.0)) >= 5.0 - 1e-9, "inside the hole");
            for p in [a, b] {
                let r = p.distance(v(10.0, 10.0));
                let on_circle = (r - 5.0).abs() < 1e-9;
                let on_square = p.x.abs() < 1e-9
                    || (p.x - 20.0).abs() < 1e-9
                    || p.y.abs() < 1e-9
                    || (p.y - 20.0).abs() < 1e-9;
                assert!(on_circle || on_square, "{p:?} r={r}");
            }
        }
        // The disc itself hatches inside the circle only.
        let disc = all.iter().find(|r| r.holes.is_empty()).unwrap();
        let h = hatch(
            disc,
            find("ANSI31").unwrap(),
            &Placement::default(),
            None,
            10_000,
        );
        let expected = std::f64::consts::PI * 25.0 / 3.175;
        assert!((total_length(&h) - expected).abs() < 3.175 * 2.0);
    }

    #[test]
    fn dashes_scale_and_window() {
        let r = &regions(&rect(0.0, 0.0, 50.0, 50.0))[0];
        let solid = hatch(
            r,
            find("ANSI31").unwrap(),
            &Placement::default(),
            None,
            100_000,
        );
        let dashed = hatch(
            r,
            find("ANSI36").unwrap(),
            &Placement::default(),
            None,
            100_000,
        );
        // ANSI36: dash 7.9375 of a 11.1125 period (plus dots).
        let ratio = total_length(&dashed) / total_length(&solid);
        assert!((ratio - 7.9375 / 11.1125).abs() < 0.05, "{ratio}");
        let coarse = hatch(
            r,
            find("ANSI31").unwrap(),
            &Placement {
                scale: 2.0,
                ..Placement::default()
            },
            None,
            100_000,
        );
        assert!((total_length(&coarse) * 2.0 - total_length(&solid)).abs() < 10.0);
        let window = Bounds {
            min: v(0.0, 0.0),
            max: v(10.0, 10.0),
        };
        let part = hatch(
            r,
            find("ANSI31").unwrap(),
            &Placement::default(),
            Some(window),
            100_000,
        );
        assert!(total_length(&part) < total_length(&solid) / 10.0);
        assert!(
            part.segments
                .iter()
                .flatten()
                .all(|p| p.x <= 10.0 + 1e-9 && p.y <= 10.0 + 1e-9)
        );
        let capped = hatch(r, find("ANSI31").unwrap(), &Placement::default(), None, 5);
        assert!(capped.truncated);
    }

    #[test]
    fn rotation_and_origin_move_the_lines() {
        let r = &regions(&rect(0.0, 0.0, 10.0, 10.0))[0];
        let p = crate::pattern::parse_pat("*H\n0, 0,0, 0,2\n").unwrap();
        let flat = hatch(r, &p[0], &Placement::default(), None, 1000);
        assert!(flat.segments.iter().all(|[a, b]| (a.y - b.y).abs() < 1e-12));
        assert!(flat.segments.iter().any(|[a, _]| (a.y - 2.0).abs() < 1e-12));
        let shifted = hatch(
            r,
            &p[0],
            &Placement {
                origin: v(0.0, 0.5),
                ..Placement::default()
            },
            None,
            1000,
        );
        assert!(
            shifted
                .segments
                .iter()
                .any(|[a, _]| (a.y - 2.5).abs() < 1e-12)
        );
        let turned = hatch(
            r,
            &p[0],
            &Placement {
                angle: 90.0,
                ..Placement::default()
            },
            None,
            1000,
        );
        assert!(
            turned
                .segments
                .iter()
                .all(|[a, b]| (a.x - b.x).abs() < 1e-9)
        );
    }
}
