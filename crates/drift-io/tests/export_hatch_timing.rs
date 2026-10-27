//! How long writing one big hatch takes (issue #193): a hatch of about 100 000 lines over a
//! rectangle, to R12 DXF (which draws it as its lines). Ignored: run it by hand with
//! `cargo test --release -p drift-io --test export_hatch_timing -- --ignored --nocapture`.

use std::time::Instant;

use drift_core::DVec2;
use drift_draft::page::{Drawing, HatchPrim, Pen, Prim};
use drift_draft::{Placement, find};
use drift_io::dxf::{DxfUnits, write};

/// One ANSI31 hatch (3.175 mm apart at scale 1) at scale 0.01 over a 2.25 m square (`sides`
/// 4) or a 2.25 m wide polygon of `sides` sides: about 100 000 lines.
fn big_hatch(sides: usize) -> Drawing {
    let mut d = Drawing::new(DVec2::splat(2250.0));
    let region = if sides == 4 {
        drift_geom::regions::Region::rectangle(DVec2::ZERO, DVec2::splat(2250.0))
    } else {
        let at = |k: usize| {
            let a = std::f64::consts::TAU * k as f64 / sides as f64;
            DVec2::splat(1125.0) + DVec2::from_angle(a) * 1125.0
        };
        let sides: Vec<drift_core::Shape> = (0..sides)
            .map(|k| drift_core::Shape::Line {
                a: at(k),
                b: at(k + 1),
            })
            .collect();
        drift_geom::regions::regions(&sides).remove(0)
    };
    d.push(
        "Fills",
        Pen::new(0.13),
        Prim::Hatch(Box::new(HatchPrim {
            region,
            pattern: Some((
                find("ANSI31").expect("ANSI31").clone(),
                Placement {
                    scale: 0.01,
                    angle: 0.0,
                    origin: DVec2::ZERO,
                },
            )),
            opacity: 1.0,
        })),
    );
    d
}

#[test]
#[ignore = "a timing, run by hand"]
fn writing_a_big_hatch_to_r12() {
    for sides in [4, 2000] {
        let d = big_hatch(sides);
        let mut best = f64::INFINITY;
        let mut lines = 0;
        for _ in 0..5 {
            let t = Instant::now();
            let dxf = write(&d, DxfUnits::MM);
            best = best.min(t.elapsed().as_secs_f64());
            lines = dxf.matches("\nLINE\n").count();
        }
        println!("R12 DXF of one hatch over {sides} sides, {lines} lines: best of 5 {best:.3} s");
        assert!(lines > 70_000, "{lines} lines");
    }
}
