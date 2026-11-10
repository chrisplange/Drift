//! The solver's acceptance bar as censuses (`docs/solver-audit.md`, section 5, items 2 and 3).
//! Each is ignored and prints one line per drag, for `scripts/solver-bar/analyse.py` to read
//! and to compare two builds by. Run them in release, one test per process:
//!
//! ```text
//! cargo test --release -p drift-edit --lib drag_props::census::similarity_census -- --ignored --exact --nocapture
//! ```
//!
//! - [`similarity_census`]: the audit's similarity census. Each random drag runs at the origin,
//!   then at 0.1×, 3.7× and 10×, and 1 m, 100 m and 1 km out; each result is compared with
//!   the origin's, mapped through the transform. `CENSUS_FROM` and `CENSUS_TO` pick the cases
//!   (default 0 to 12,000: the audit's census 2).
//! - [`drag_census`]: #377's random census, one place per run (`CENSUS_SHIFT`, mm added to x
//!   and y; `CENSUS_N` cases, default 20,000).
//! - [`drag_census_seeded`]: #377's seeded census, the shapes #278 shrank to at random offsets,
//!   frame counts, scales and turns (`CENSUS_N`, default 2,000; `CENSUS_SHIFT`, or
//!   `CENSUS_RATIO` to place the sketch's middle that many of its own sizes out).
//!
//! Every line also carries the solver's exit counts over the drag (`drift_solver::exits`):
//! `ex` for the drag frames' component solves (settled, conflict, cap, no step),
//! `it` their iterations and `solves` their number, `oex` and `oit` the same for every other
//! solve (the commit's). The random generator and its seed are the ones #365, #377 and the
//! audit used, so case numbers match theirs.

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

use super::*;

/// A number from the environment, or `default`.
fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The census's random drags, in order: a sketch's plan, what is dragged, by how far and in
/// how many frames.
fn cases() -> impl FnMut() -> (Plan, Pick, DVec2, u32) {
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        TestRng::from_seed(RngAlgorithm::ChaCha, &[7u8; 32]),
    );
    let strategy = (plan(true), pick(), offset(20.0), 1u32..=20);
    move || strategy.new_tree(&mut runner).unwrap().current()
}

/// The solver's exit counts since the last call, as the census lines print them.
fn exits() -> String {
    let n = drift_solver::exits::take();
    let csv = |c: &[u64; 4]| c.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
    format!(
        "ex {} it {} solves {} oex {} oit {}",
        csv(&n.drag),
        n.drag_iterations,
        n.drag.iter().sum::<u64>(),
        csv(&n.other),
        n.other_iterations
    )
}

/// The similarities the census compares, by tag: the origin first (the reference), then
/// three scales and three places, each place √2/2 of its distance out in x and in y.
const PLACES: [(&str, f64, f64); 7] = [
    ("id", 1.0, 0.0),
    ("s0.1", 0.1, 0.0),
    ("s3.7", 3.7, 0.0),
    ("s10", 10.0, 0.0),
    ("x1m", 1.0, 707.10678),
    ("x100m", 1.0, 70710.678),
    ("x1km", 1.0, 707106.78),
];

/// The similarity census: one line per (case, place). `BUILD` where the sketch can't be
/// committed there; `ERR` where the drag broke a property (a relation or a structural row
/// left unmet, or a refused frame that moved geometry); otherwise whether it refused a
/// different number of frames or committed differently from the origin's (`rdiff`), and how
/// far its end is from the origin's mapped through the transform (`dev`, in the origin's
/// millimetres, and `rel`, as a share of the sketch's size). A drag fails similarity if
/// `rdiff` is 1 or `rel` is over 1e-6, the bar of `drag_is_invariant_under_similarity`.
#[test]
#[ignore = "census: run in release with --ignored --exact --nocapture"]
fn similarity_census() {
    let from: usize = env_or("CENSUS_FROM", 0);
    let to: usize = env_or("CENSUS_TO", 12_000);
    let mut next = cases();
    for case in 0..to {
        let (plan, pick, off, steps) = next();
        if case < from {
            continue;
        }
        let Some(sketch) = plan.sketch() else {
            continue;
        };
        let mut reference: Option<(Run, f64)> = None;
        for (tag, scale, shift) in PLACES {
            let t = Similarity {
                scale,
                turn: 0.0,
                shift: DVec2::splat(shift),
            };
            let Some(mut built) = Built::new(&sketch, t) else {
                println!("A {case} {tag} BUILD");
                continue;
            };
            let ids = pick.ids(&built);
            let what = if built.curves.contains(&ids[0]) {
                "curves"
            } else {
                "point"
            };
            let _ = drift_solver::exits::take();
            let run = drag(&mut built, &ids, t.vector(off), steps);
            let counts = exits();
            DRAGS.with(|d| d.borrow_mut().clear());
            let Ok(run) = run else {
                println!("A {case} {tag} {what} steps {steps} ERR {counts}");
                continue;
            };
            if tag == "id" {
                let pts = &run.start;
                let mid = pts.iter().sum::<DVec2>() / pts.len().max(1) as f64;
                let size = 2.0 * pts.iter().map(|p| p.distance(mid)).fold(0.0, f64::max);
                reference = Some((run.clone(), size.max(1e-300)));
            }
            let (dev, rel) = match &reference {
                None => (0.0, 0.0),
                Some((origin, size)) => {
                    let dev = origin
                        .end
                        .iter()
                        .zip(&run.end)
                        .map(|(p, q)| t.point(*p).distance(*q) / t.scale)
                        .fold(0.0, f64::max);
                    (dev, dev / size)
                }
            };
            // (The origin's own line compares it with itself. A place whose origin drag
            // failed has no reference, and is compared with itself too.)
            let (refused, committed) = reference
                .as_ref()
                .map_or((run.refused, run.committed), |(o, _)| {
                    (o.refused, o.committed)
                });
            println!(
                "A {case} {tag} {what} steps {steps} ref {} com {} rdiff {} dev {dev:e} rel {rel:e} {counts}",
                run.refused,
                run.committed,
                u8::from(run.refused != refused || run.committed != committed),
            );
        }
    }
}

/// Arcs crushed to a point or closed on themselves (ends together): radius within 1e-9 of the
/// sketch's spread, or chord within 1e-9 of the radius.
fn collapsed_arcs(built: &Built) -> usize {
    let doc = &built.ed.doc;
    let pts = built.positions();
    let mid = pts.iter().sum::<DVec2>() / pts.len().max(1) as f64;
    let size = pts.iter().map(|p| p.distance(mid)).fold(0.0, f64::max);
    built
        .curves
        .iter()
        .filter(|&&id| match doc.get(id).map(|e| e.kind) {
            Some(drift_core::EntityKind::Arc { center, start, end }) => {
                let (c, s, e) = (
                    doc.point(center).unwrap(),
                    doc.point(start).unwrap(),
                    doc.point(end).unwrap(),
                );
                let r = c.distance(s).max(c.distance(e));
                r <= 1e-9 * size || s.distance(e) <= 1e-9 * r
            }
            _ => false,
        })
        .count()
}

/// Each arc's radius (centre to start), in the order of `built.curves`.
fn radii(built: &Built) -> Vec<f64> {
    let doc = &built.ed.doc;
    built
        .curves
        .iter()
        .filter_map(|&id| match doc.get(id).map(|e| e.kind) {
            Some(drift_core::EntityKind::Arc { center, start, .. }) => Some(
                doc.point(center)
                    .unwrap()
                    .distance(doc.point(start).unwrap()),
            ),
            _ => None,
        })
        .collect()
}

/// One drag of #377's censuses, as a line: refusals, whether it committed, arcs collapsed,
/// how far the farthest point went, the exit counts, each arc's radius before and after
/// (`radii`, for the binding drag rule: a drag that goes while a radius changes is one to
/// check), and every point's end position (exact, so two builds compare bit for bit).
fn census_line(head: &str, built: &mut Built, ids: &[EntityId], off: DVec2, steps: u32) {
    let before = radii(built);
    let _ = drift_solver::exits::take();
    let run = drag(built, ids, off, steps);
    let counts = exits();
    DRAGS.with(|d| d.borrow_mut().clear());
    let Ok(run) = run else {
        println!("{head} ERR {counts}");
        return;
    };
    let far = run
        .start
        .iter()
        .zip(&run.end)
        .map(|(p, q)| p.distance(*q))
        .fold(0.0, f64::max);
    let radii: String = before
        .iter()
        .zip(radii(built))
        .map(|(a, b)| format!("{a:?}>{b:?};"))
        .collect();
    let ends: String = run
        .end
        .iter()
        .map(|p| format!("{:?},{:?};", p.x, p.y))
        .collect();
    println!(
        "{head} refused {} committed {} collapsed {} far {far:.6e} drag {:.6e} {counts} radii {radii} | {ends}",
        run.refused,
        run.committed,
        collapsed_arcs(built),
        off.length(),
    );
}

/// #377's random census at one place (`CENSUS_SHIFT` mm out in x and y): one `C` line per
/// drag (see [`census_line`]).
#[test]
#[ignore = "census: run in release with --ignored --exact --nocapture"]
fn drag_census() {
    let n: usize = env_or("CENSUS_N", 20_000);
    let shift: f64 = env_or("CENSUS_SHIFT", 0.0);
    let mut next = cases();
    for case in 0..n {
        let (plan, pick, off, steps) = next();
        let Some(sketch) = plan.sketch() else {
            continue;
        };
        let t = Similarity {
            scale: 1.0,
            turn: 0.0,
            shift: DVec2::splat(shift),
        };
        let Some(mut built) = Built::new(&sketch, t) else {
            println!("C {case} BUILD");
            continue;
        };
        let ids = pick.ids(&built);
        let what = if built.curves.contains(&ids[0]) {
            "curves"
        } else {
            "point"
        };
        census_line(
            &format!("C {case} {what} steps {steps}"),
            &mut built,
            &ids,
            off,
            steps,
        );
    }
}

/// The two shapes #278 shrank to (its pin, and heavy seed 16), each with what is dragged and
/// the drag's base offset.
fn shrunk_shapes() -> [(&'static str, Plan, Pick, DVec2); 2] {
    let mut pin_extras = vec![false; 48];
    pin_extras[6] = true;
    let mut s16_extras = vec![false; 48];
    s16_extras[0] = true;
    [
        (
            "pin",
            Plan {
                ops: vec![
                    Op::Step {
                        from: 0,
                        angle: 0,
                        len: 0,
                    },
                    Op::Arc {
                        from: 48,
                        angle: 6,
                        radius: 0,
                        sweep: 0,
                        ccw: false,
                    },
                    Op::Tangent {
                        arc: 0,
                        end: false,
                        forward: false,
                        len: 0,
                    },
                    Op::Tangent {
                        arc: 0,
                        end: true,
                        forward: false,
                        len: 1,
                    },
                ],
                extras: pin_extras,
                fix: Some(0),
                length: None,
            },
            Pick::Curves(15068428138436115285),
            DVec2::new(-0.03276375320139522, 0.04953594298231851),
        ),
        (
            "seed16",
            Plan {
                ops: vec![
                    Op::Arc {
                        from: 0,
                        angle: 2,
                        radius: 0,
                        sweep: 0,
                        ccw: false,
                    },
                    Op::Step {
                        from: 19,
                        angle: 14,
                        len: 0,
                    },
                    Op::Tangent {
                        arc: 0,
                        end: true,
                        forward: false,
                        len: 2,
                    },
                    Op::Arc {
                        from: 8,
                        angle: 0,
                        radius: 1,
                        sweep: 0,
                        ccw: false,
                    },
                    Op::Arc {
                        from: 2,
                        angle: 4,
                        radius: 0,
                        sweep: 0,
                        ccw: false,
                    },
                ],
                extras: s16_extras,
                fix: Some(9),
                length: None,
            },
            Pick::Curves(6638522335503980748),
            DVec2::new(0.0, 0.07784862269881661),
        ),
    ]
}

/// #377's seeded census: [`shrunk_shapes`] in turn, each dragged a random distance (0.01× to
/// about 30× its base offset) in a random direction over 1 to 20 frames, at 0.1×, 1× or 10×,
/// turned at random, at one place: `CENSUS_SHIFT` mm out in x and y, or with `CENSUS_RATIO`
/// set, its middle that many of its own sizes out along the diagonal. One `C` line per drag
/// (see [`census_line`]).
#[test]
#[ignore = "census: run in release with --ignored --exact --nocapture"]
fn drag_census_seeded() {
    let n: usize = env_or("CENSUS_N", 2_000);
    let shift: f64 = env_or("CENSUS_SHIFT", 0.0);
    let ratio: Option<f64> = std::env::var("CENSUS_RATIO")
        .ok()
        .and_then(|v| v.parse().ok());
    // (SplitMix64: a fixed, dependency-free sequence.)
    let mut state: u64 = 0x2780_2780_2780_2780;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let shapes = shrunk_shapes();
    for case in 0..n {
        let (name, plan, pick, base) = &shapes[case % shapes.len()];
        let mag = base.length() * 10f64.powf(-2.0 + 3.5 * next());
        let angle = next() * std::f64::consts::TAU;
        let steps: u32 = 1 + (next() * 20.0) as u32 % 20;
        let scale = [0.1, 1.0, 10.0][(next() * 3.0) as usize % 3];
        let turn: f64 = next() * 360.0;
        let Some(sketch) = plan.sketch() else {
            continue;
        };
        let mut t = Similarity {
            scale,
            turn,
            shift: DVec2::splat(shift),
        };
        if let Some(ratio) = ratio {
            let Some(at_origin) = Built::new(
                &sketch,
                Similarity {
                    shift: DVec2::ZERO,
                    ..t
                },
            ) else {
                println!("C {case} {name} BUILD");
                continue;
            };
            let pts = at_origin.positions();
            let mid = pts.iter().sum::<DVec2>() / pts.len().max(1) as f64;
            let size = 2.0 * pts.iter().map(|p| p.distance(mid)).fold(0.0, f64::max);
            t.shift = DVec2::splat(ratio * size) - mid;
        }
        let Some(mut built) = Built::new(&sketch, t) else {
            println!("C {case} {name} BUILD");
            continue;
        };
        let ids = pick.ids(&built);
        let off = DVec2::new(angle.cos(), angle.sin()) * mag * scale;
        census_line(
            &format!("C {case} {name} steps {steps} scale {scale} turn {turn:.1}"),
            &mut built,
            &ids,
            off,
            steps,
        );
    }
}
