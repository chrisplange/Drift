//! Big made-up drawings the tests of more than one crate time and measure: the 10,000-curve
//! torture sketch and a big imported CAD drawing. Test-only: compiled for drift-edit's tests
//! and, through the `test-support` feature, for drift-app's (memory, frames, the whole-app
//! probes).

use drift_core::{Constraint, DVec2, Document, EntityId};

const TARGET_CURVES: usize = 10_000;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

/// About 10,000 curves in a grid of cells, each one of: a rectangle (shared corners, H/V
/// relations) with a circle; a rectangle with a corner arc; a path of four Bézier curves with
/// smooth points; an ellipse with a cross of lines.
pub fn torture_sketch() -> (Document, usize) {
    let mut doc = Document::new();
    doc.begin();
    let mut curves = 0;
    let cols = 45;
    let mut k = 0;
    while curves < TARGET_CURVES {
        let (i, j) = (k % cols, k / cols);
        let o = v(i as f64 * 30.0, j as f64 * 30.0);
        match k % 4 {
            0 | 1 => {
                let c = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 14.0), v(0.0, 14.0)]
                    .map(|p| doc.add_point(o + p));
                let l: Vec<EntityId> = (0..4)
                    .map(|n| doc.add_line(c[n], c[(n + 1) % 4]).unwrap())
                    .collect();
                for (n, line) in l.iter().enumerate() {
                    let rel = if n % 2 == 0 {
                        Constraint::Horizontal { line: *line }
                    } else {
                        Constraint::Vertical { line: *line }
                    };
                    doc.add_constraint(rel).unwrap();
                }
                curves += 4;
                if k % 4 == 0 {
                    let centre = doc.add_point(o + v(10.0, 7.0));
                    doc.add_circle(centre, 4.0).unwrap();
                } else {
                    let centre = doc.add_point(o + v(10.0, 7.0));
                    let s = doc.add_point(o + v(15.0, 7.0));
                    let e = doc.add_point(o + v(10.0, 12.0));
                    doc.add_arc(centre, s, e).unwrap();
                }
                curves += 1;
            }
            2 => {
                let pts: Vec<EntityId> = (0..13)
                    .map(|n| {
                        let x = n as f64 * 20.0 / 12.0;
                        let y = 7.0 + 5.0 * (n as f64 * 0.9).sin();
                        doc.add_point(o + v(x, y))
                    })
                    .collect();
                for seg in 0..4 {
                    let b = seg * 3;
                    doc.add_bezier(pts[b], pts[b + 1], pts[b + 2], pts[b + 3])
                        .unwrap();
                    curves += 1;
                    if seg > 0 {
                        doc.add_constraint(Constraint::Smooth {
                            point: pts[b],
                            a: pts[b - 1],
                            b: pts[b + 1],
                        })
                        .ok();
                    }
                }
            }
            _ => {
                let [c, m, n] =
                    [v(10.0, 7.0), v(19.0, 7.0), v(10.0, 12.0)].map(|p| doc.add_point(o + p));
                doc.add_ellipse(c, m, n).unwrap();
                let [a, b, p, q] = [v(0.0, 7.0), v(20.0, 7.0), v(10.0, 0.0), v(10.0, 14.0)]
                    .map(|p| doc.add_point(o + p));
                doc.add_line(a, b).unwrap();
                doc.add_line(p, q).unwrap();
                curves += 3;
            }
        }
        k += 1;
    }
    doc.commit("Torture");
    (doc, curves)
}

/// A made-up big CAD drawing: a grid of `cells` rooms, each a closed outline with a door
/// swing (an arc), a column (a circle) and loose lines across it; every third room hatched
/// solid; long grid lines across the sheet. About 10 curves a room.
pub fn big_drawing(cells: usize) -> drift_io::dxf::DxfImport {
    use drift_core::Shape;
    use drift_io::dxf::{DxfEntity, DxfHatch, DxfImport};
    let mut import = DxfImport::default();
    let curve = |layer: &str, shape: Shape| DxfEntity {
        layer: layer.into(),
        shape,
        line_type: None,
        color: None,
        weight: None,
    };
    let v = DVec2::new;
    let cols = (cells as f64).sqrt().ceil() as usize;
    let mut seed = 11u64;
    let mut rnd = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for k in 0..cells {
        let o = v((k % cols) as f64 * 3000.0, (k / cols) as f64 * 3000.0);
        let c = [
            v(0.0, 0.0),
            v(2400.0, 0.0),
            v(2400.0, 2000.0),
            v(0.0, 2000.0),
        ]
        .map(|p| o + p);
        for n in 0..4 {
            import.entities.push(curve(
                "walls",
                Shape::Line {
                    a: c[n],
                    b: c[(n + 1) % 4],
                },
            ));
        }
        import.entities.push(curve(
            "doors",
            Shape::Arc {
                center: o + v(300.0, 0.0),
                radius: 800.0,
                start: 0.0,
                sweep: std::f64::consts::FRAC_PI_2,
            },
        ));
        import.entities.push(curve(
            "columns",
            Shape::Circle {
                center: o + v(1800.0 + 200.0 * rnd(), 1400.0 + 200.0 * rnd()),
                radius: 150.0,
            },
        ));
        for _ in 0..4 {
            let a = o + v(200.0 + 2000.0 * rnd(), 200.0 + 1600.0 * rnd());
            let b = o + v(200.0 + 2000.0 * rnd(), 200.0 + 1600.0 * rnd());
            import
                .entities
                .push(curve("furniture", Shape::Line { a, b }));
        }
        if k % 3 == 0 {
            import.hatches.push(DxfHatch {
                layer: "fills".into(),
                loops: vec![
                    (0..4)
                        .map(|n| Shape::Line {
                            a: c[n],
                            b: c[(n + 1) % 4],
                        })
                        .collect(),
                ],
                solid: true,
                pattern: "SOLID".into(),
                scale: 1.0,
                angle: 0.0,
                definition: Vec::new(),
                origin: DVec2::ZERO,
                color: None,
                opacity: None,
                seeds: Vec::new(),
            });
        }
    }
    let size = cols as f64 * 3000.0;
    for n in 0..20 {
        let t = 2700.0 + n as f64 * size / 20.0;
        import.entities.push(curve(
            "grid",
            Shape::Line {
                a: v(t, -1000.0),
                b: v(t, size),
            },
        ));
        import.entities.push(curve(
            "grid",
            Shape::Line {
                a: v(-1000.0, t),
                b: v(size, t),
            },
        ));
    }
    import
}
