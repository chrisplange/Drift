//! Benchmark sketches, shared by the `solver_bench` example and the in-app benchmark (which also
//! runs in the browser, where single-threaded WASM performance is what matters).

use drift_core::{Constraint, DVec2, Document, EntityId};

/// A slightly skewed rectangle held square by perpendicular constraints, with width and height
/// dimensions. Returns its corners.
fn rectangle(doc: &mut Document, origin: DVec2) -> [EntityId; 4] {
    let skew = 0.3;
    let corners = [
        DVec2::new(0.0, 0.0),
        DVec2::new(10.0 + skew, skew),
        DVec2::new(10.0, 6.0 - skew),
        DVec2::new(-skew, 6.0),
    ];
    let p = corners.map(|c| doc.add_point(origin + c));
    let l = [0, 1, 2, 3].map(|i| doc.add_line(p[i], p[(i + 1) % 4]).expect("valid edge"));
    let mut add = |c| {
        doc.add_constraint(c).expect("valid constraint");
    };
    add(Constraint::Horizontal { line: l[0] });
    for i in 0..3 {
        add(Constraint::Perpendicular {
            a: l[i],
            b: l[i + 1],
        });
    }
    add(Constraint::Distance {
        a: p[0],
        b: p[1],
        value: 10.0,
    });
    add(Constraint::Distance {
        a: p[1],
        b: p[2],
        value: 6.0,
    });
    p
}

/// `n` rectangles chained into one connected component (the hardest case: nothing can be
/// solved independently). About 8 entities and 8 constraints per rectangle.
pub fn chained_rectangles(n: usize) -> Document {
    let mut doc = Document::new();
    let mut prev: Option<[EntityId; 4]> = None;
    for i in 0..n {
        let p = rectangle(&mut doc, DVec2::new(i as f64 * 12.0, 0.0));
        if let Some(q) = prev {
            doc.add_constraint(Constraint::HorizontalPoints { a: q[1], b: p[0] })
                .expect("valid constraint");
            doc.add_constraint(Constraint::Distance {
                a: q[1],
                b: p[0],
                value: 2.0,
            })
            .expect("valid constraint");
        }
        prev = Some(p);
    }
    doc
}
