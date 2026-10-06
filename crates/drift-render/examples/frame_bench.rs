//! Times CPU frame building for a large sketch: `cargo run --release -p drift-render --example frame_bench`

use std::time::Instant;

use drift_core::{DVec2, LineKind, Shape, SketchItem};
use drift_render::{Camera, Palette, Viewport, build_frame};

fn main() {
    // 50k entities on a 250 × 200 grid: alternating lines, circles, arcs and dashed lines.
    let mut items = Vec::new();
    for i in 0..250 {
        for j in 0..200 {
            let p = DVec2::new(f64::from(i) * 10.0, f64::from(j) * 10.0);
            let shape = match (i + j) % 3 {
                0 => Shape::Line {
                    a: p,
                    b: p + DVec2::new(8.0, 3.0),
                },
                1 => Shape::Circle {
                    center: p,
                    radius: 3.0,
                },
                _ => Shape::Arc {
                    center: p,
                    radius: 4.0,
                    start: 0.3,
                    sweep: 2.0,
                },
            };
            let kind = if (i * j) % 7 == 0 {
                LineKind::Construction
            } else {
                LineKind::Solid
            };
            items.push(SketchItem::new(shape).with_kind(kind));
        }
    }

    let viewport = Viewport {
        size_px: DVec2::new(2560.0, 1440.0),
        pixels_per_point: 2.0,
        srgb_target: false,
    };
    let bounds = items.iter().map(|i| i.shape.bounds()).collect();
    let mut camera = Camera::default();
    camera.fit(bounds, viewport.size_px / viewport.pixels_per_point, 20.0);

    let palette = Palette::dark();
    let runs = 50;
    let start = Instant::now();
    let mut instances = 0;
    for k in 0..runs {
        camera.pan(DVec2::new(f64::from(k % 3), 0.0));
        let frame = build_frame(&items, &camera, viewport, &palette);
        instances = frame.segments.len() + frame.arcs.len();
    }
    let per_frame = start.elapsed() / runs;
    println!(
        "{} entities, all visible -> {instances} instances, {per_frame:?} per frame",
        items.len()
    );
}
