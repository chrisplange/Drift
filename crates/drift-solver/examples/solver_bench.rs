//! Solver timing: `cargo run --release -p drift-solver --example solver_bench`

use std::time::Instant;

use drift_core::{Constraint, DVec2, Document, EntityId};
use drift_solver::{SolveOptions, analyze, solve};

/// A rectangle with width/height dimensions. With `substitutable`, its edges are horizontal /
/// vertical (merged away by substitution); otherwise it is held square by perpendicular
/// constraints, which stay as nonlinear equations. Returns its corners.
fn rectangle(doc: &mut Document, origin: DVec2, skew: f64, substitutable: bool) -> [EntityId; 4] {
    let corners = [
        DVec2::new(0.0, 0.0),
        DVec2::new(10.0 + skew, skew),
        DVec2::new(10.0, 6.0 - skew),
        DVec2::new(-skew, 6.0),
    ];
    let p = corners.map(|c| doc.add_point(origin + c));
    let l = [0, 1, 2, 3].map(|i| doc.add_line(p[i], p[(i + 1) % 4]).unwrap());
    if substitutable {
        for (i, line) in l.iter().enumerate() {
            let c = if i % 2 == 0 {
                Constraint::Horizontal { line: *line }
            } else {
                Constraint::Vertical { line: *line }
            };
            doc.add_constraint(c).unwrap();
        }
    } else {
        doc.add_constraint(Constraint::Horizontal { line: l[0] })
            .unwrap();
        for i in 0..3 {
            doc.add_constraint(Constraint::Perpendicular {
                a: l[i],
                b: l[i + 1],
            })
            .unwrap();
        }
    }
    doc.add_constraint(Constraint::Distance {
        a: p[0],
        b: p[1],
        value: 10.0,
    })
    .unwrap();
    doc.add_constraint(Constraint::Distance {
        a: p[1],
        b: p[2],
        value: 6.0,
    })
    .unwrap();
    p
}

fn time(label: &str, doc: &Document) {
    let mut work = doc.clone();
    let start = Instant::now();
    let report = solve(&mut work, &SolveOptions::default());
    let solve_time = start.elapsed();
    let start = Instant::now();
    let analysis = analyze(&work);
    let analyze_time = start.elapsed();
    println!(
        "{label}: {} entities, {} constraints | solve {solve_time:?} ({} iterations, converged {}) | DOF analysis {analyze_time:?} (dof {:?})",
        doc.len(),
        doc.constraint_count(),
        report.iterations,
        report.converged,
        analysis.dof,
    );
}

fn main() {
    // Many independent rectangles: typical of real sketches, solved component by component.
    let mut doc = Document::new();
    for i in 0..125 {
        let origin = DVec2::new(f64::from(i % 25) * 20.0, f64::from(i / 25) * 20.0);
        rectangle(&mut doc, origin, 0.3, true);
    }
    time("125 separate rectangles", &doc);
    time(
        "250 chained rectangles (shared bench sketch)",
        &drift_solver::bench::chained_rectangles(250),
    );

    // One connected chain of rectangles: a single large component (dense worst case).
    for (n, substitutable) in [
        (100, true),
        (400, true),
        (25, false),
        (50, false),
        (100, false),
        (200, false),
        (500, false),
    ] {
        let mut doc = Document::new();
        let mut prev: Option<[EntityId; 4]> = None;
        for i in 0..n {
            let p = rectangle(
                &mut doc,
                DVec2::new(f64::from(i) * 12.0, 0.0),
                0.3,
                substitutable,
            );
            if let Some(q) = prev {
                // Chain: this rectangle's left-bottom corner 2 mm right of the previous one's.
                doc.add_constraint(Constraint::HorizontalPoints { a: q[1], b: p[0] })
                    .unwrap();
                doc.add_constraint(Constraint::Distance {
                    a: q[1],
                    b: p[0],
                    value: 2.0,
                })
                .unwrap();
            }
            prev = Some(p);
        }
        let kind = if substitutable {
            "H/V"
        } else {
            "perpendicular"
        };
        time(&format!("{n} chained {kind} rectangles"), &doc);
    }
}
