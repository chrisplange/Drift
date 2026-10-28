//! Properties of the region arrangement: snapping makes it robust to tiny perturbations,
//! it moves with the drawing, it conserves area, and it doesn't depend on input order.

use std::collections::HashSet;

use drift_core::{DVec2, Shape};
use drift_geom::regions::{Region, RegionTolerance, regions, regions_reusing};
use proptest::prelude::*;

const N: i32 = 12;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// Axis-aligned rectangles on an integer grid, as four lines each.
fn rects() -> impl Strategy<Value = Vec<(i32, i32, i32, i32)>> {
    prop::collection::vec(
        (0..N, 0..N, 1..N, 1..N).prop_map(|(x, y, w, h)| (x, y, (x + w).min(N), (y + h).min(N))),
        1..6,
    )
    .prop_map(|r| r.into_iter().filter(|r| r.0 < r.2 && r.1 < r.3).collect())
}

fn outlines(rects: &[(i32, i32, i32, i32)]) -> Vec<Shape> {
    let mut out = Vec::new();
    for &(x0, y0, x1, y1) in rects {
        let c = [
            v(x0.into(), y0.into()),
            v(x1.into(), y0.into()),
            v(x1.into(), y1.into()),
            v(x0.into(), y1.into()),
        ];
        out.extend((0..4).map(|i| Shape::Line {
            a: c[i],
            b: c[(i + 1) % 4],
        }));
    }
    out
}

/// Rectangles, plus a circle whose radius keeps it off the grid lines (never tangent).
fn drawing() -> impl Strategy<Value = Vec<Shape>> {
    (rects(), prop::option::of((0..N, 0..N, 0..4))).prop_map(|(r, circle)| {
        let mut shapes = outlines(&r);
        if let Some((x, y, k)) = circle {
            shapes.push(Shape::Circle {
                center: v(x.into(), y.into()),
                radius: f64::from(k) + 0.31,
            });
        }
        shapes
    })
}

fn map_shape(s: &Shape, f: &impl Fn(DVec2) -> DVec2, scale: f64, turn: f64) -> Shape {
    match *s {
        Shape::Line { a, b } => Shape::Line { a: f(a), b: f(b) },
        Shape::Circle { center, radius } => Shape::Circle {
            center: f(center),
            radius: radius * scale,
        },
        Shape::Arc {
            center,
            radius,
            start,
            sweep,
        } => Shape::Arc {
            center: f(center),
            radius: radius * scale,
            start: start + turn,
            sweep,
        },
        _ => unreachable!("only lines, arcs and circles are generated"),
    }
}

/// Region areas, sorted.
fn areas(found: &[Region]) -> Vec<f64> {
    let mut a: Vec<f64> = found.iter().map(|r| r.area).collect();
    a.sort_by(f64::total_cmp);
    a
}

/// Face count, holes per face and areas (sorted) match within `tol` relative.
fn same_topology(a: &[Region], b: &[Region], scale2: f64, tol: f64) -> Result<(), TestCaseError> {
    prop_assert_eq!(a.len(), b.len());
    let mut ha: Vec<usize> = a.iter().map(|r| r.holes.len()).collect();
    let mut hb: Vec<usize> = b.iter().map(|r| r.holes.len()).collect();
    ha.sort_unstable();
    hb.sort_unstable();
    prop_assert_eq!(ha, hb);
    for (x, y) in areas(a).iter().zip(areas(b)) {
        prop_assert!(
            (x * scale2 - y).abs() <= tol * y.abs().max(1.0),
            "{x} vs {y}"
        );
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Moving every point by less than half the snap distance changes nothing that matters.
    #[test]
    fn perturbation_below_snap_keeps_topology(
        shapes in drawing(),
        jitter in prop::collection::vec((-1.0f64..1.0, -1.0f64..1.0), 64),
    ) {
        let snap = RegionTolerance::for_curves(&shapes).snap;
        let mut k = 0;
        let mut nudge = |p: DVec2| {
            let (dx, dy) = jitter[k % jitter.len()];
            k += 1;
            p + v(dx, dy) * snap * 0.2
        };
        let jittered: Vec<Shape> = shapes
            .iter()
            .map(|s| match *s {
                Shape::Line { a, b } => Shape::Line { a: nudge(a), b: nudge(b) },
                Shape::Circle { center, radius } => Shape::Circle { center: nudge(center), radius },
                other => other,
            })
            .collect();
        same_topology(&regions(&shapes), &regions(&jittered), 1.0, 1e-6)?;
    }

    /// Translating, rotating and scaling the drawing gives the same faces, moved.
    #[test]
    fn faces_move_with_the_drawing(
        shapes in drawing(),
        dx in -1e4f64..1e4,
        dy in -1e4f64..1e4,
        turn in 0.0f64..std::f64::consts::TAU,
        scale in 0.1f64..10.0,
    ) {
        let rot = DVec2::from_angle(turn);
        let f = |p: DVec2| rot.rotate(p * scale) + v(dx, dy);
        let moved: Vec<Shape> = shapes.iter().map(|s| map_shape(s, &f, scale, turn)).collect();
        let (before, after) = (regions(&shapes), regions(&moved));
        same_topology(&before, &after, scale * scale, 1e-6)?;
        // Each face's centroid lands on a face's centroid.
        for r in &before {
            let c = f(r.centroid);
            prop_assert!(
                after.iter().any(|q| q.centroid.distance(c) < 1e-5 * (1.0 + scale)),
                "no face at {c:?}"
            );
        }
    }

    /// Faces don't overlap and together cover exactly what the outlines enclose (holes
    /// left out), checked against a flood fill of the grid.
    #[test]
    fn faces_conserve_area(r in rects()) {
        let found = regions(&outlines(&r));
        // Walls on the unit-cell sides: (x, y, vertical) for the side from (x, y).
        let mut walls = HashSet::new();
        for &(x0, y0, x1, y1) in &r {
            for x in x0..x1 {
                walls.insert((x, y0, false));
                walls.insert((x, y1, false));
            }
            for y in y0..y1 {
                walls.insert((x0, y, true));
                walls.insert((x1, y, true));
            }
        }
        // Flood the cells from outside the grid.
        let mut outside = HashSet::new();
        let mut stack = vec![(-1, -1)];
        while let Some((x, y)) = stack.pop() {
            if !(-1..=N).contains(&x) || !(-1..=N).contains(&y) || !outside.insert((x, y)) {
                continue;
            }
            if !walls.contains(&(x + 1, y, true)) { stack.push((x + 1, y)); }
            if !walls.contains(&(x, y, true)) { stack.push((x - 1, y)); }
            if !walls.contains(&(x, y + 1, false)) { stack.push((x, y + 1)); }
            if !walls.contains(&(x, y, false)) { stack.push((x, y - 1)); }
        }
        let mut enclosed = 0;
        for x in 0..N {
            for y in 0..N {
                let centre = v(f64::from(x) + 0.5, f64::from(y) + 0.5);
                let hits = found.iter().filter(|q| q.contains(centre)).count();
                let inside = !outside.contains(&(x, y));
                prop_assert_eq!(hits, usize::from(inside), "cell {} {}", x, y);
                enclosed += usize::from(inside);
            }
        }
        let total: f64 = found.iter().map(|q| q.area).sum();
        prop_assert!((total - enclosed as f64).abs() < 1e-9, "{total} vs {enclosed}");
    }

    /// Shuffling the curves gives the same regions, in the same order, edge for edge.
    #[test]
    fn output_does_not_depend_on_input_order(
        (shapes, shuffled) in drawing().prop_flat_map(|s| {
            let idx: Vec<usize> = (0..s.len()).collect();
            (Just(s), Just(idx).prop_shuffle())
        }),
    ) {
        let other: Vec<Shape> = shuffled.iter().map(|&i| shapes[i]).collect();
        let (a, b) = (regions(&shapes), regions(&other));
        prop_assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            prop_assert_eq!(&x.outer.edges, &y.outer.edges);
            prop_assert_eq!(x.holes.len(), y.holes.len());
            for (h, k) in x.holes.iter().zip(&y.holes) {
                prop_assert_eq!(&h.edges, &k.edges);
            }
            prop_assert_eq!(x.area, y.area);
            prop_assert_eq!(x.centroid, y.centroid);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Centroids taken from an earlier result for the regions an edit left alone: the same
    /// regions, to the bit, as found afresh.
    #[test]
    fn reusing_an_earlier_result_changes_nothing(
        before in drawing(),
        edits in prop::collection::vec((0usize..64, 0..N, 0..N, any::<bool>()), 1..5),
    ) {
        let prior = regions(&before);
        let mut after = before.clone();
        for (k, x, y, delete) in edits {
            if after.is_empty() {
                break;
            }
            let k = k % after.len();
            if delete {
                after.remove(k);
            } else {
                let d = v(x.into(), y.into()) * 0.5;
                after[k] = map_shape(&after[k], &|p| p + d, 1.0, 0.0);
            }
        }
        let fresh = format!("{:?}", regions(&after));
        prop_assert_eq!(format!("{:?}", regions_reusing(&after, &prior)), fresh.clone());
        // And reusing a result for the very same curves.
        prop_assert_eq!(
            format!("{:?}", regions_reusing(&after, &regions(&after))),
            fresh
        );
    }
}
