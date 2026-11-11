//! The relation-adding census (the audit's PR 6, #350; #333's corpus): random sketches from
//! the drag properties' generator, each given one new relation or dimension between random
//! picks, with the last pick active or with none, as the app adds them. It prints one line per
//! case, for `scripts/solver-bar/relations.py` to compare two builds by:
//!
//! ```text
//! cargo test --release -p drift-edit --lib drag_props::relation_census::relation_census -- --ignored --exact --nocapture
//! ```
//!
//! `CENSUS_N` cases (default 4,000), drawn with their own seed, so the drag censuses' case
//! numbers stay as they are. `CENSUS_CASE` runs that case alone and prints the drawing before
//! and after; `CENSUS_POS` adds a `P <case>` line of every point's position, to compare two
//! builds point by point. An `S <case>` line lists the edit's second-stage endings where one
//! stopped with the shapes' projected gradient still above the bar, or at the cap, and
//! `S2 TOTAL` sums them (`drift_solver::exits::STAGE2`: the PR 3 review's items 1 and 2). Each line: `R <case> <kind> <active> <result>`, then (where the
//! relation was added) `mv` how far the farthest point moved, `pts` how many points moved, and
//! for the lines and the arcs, apart for the geometry the relation names (`n…`: a line counts
//! if it or one of its ends is named) and the rest
//! (`r…`): how many changed length or radius (`nl`, `rl`, `na`, `ra`) and by at most how much
//! (`ndl`, `rdl`, `nda`, `rda`), and how far the rest's lines turned at most (`rturn`, in
//! degrees), and how many curves it left collapsed (`col`: a line's length or an arc's radius
//! now under the bar). A change counts where it is over 1e-6 of the sketch's size. Positions are in mm
//! on a sketch some 10 to 100 mm across.

use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};

use drift_core::EntityKind;

use crate::editor::EditError;

use super::*;

/// What is added: a relation between picks, or a size typed for one.
#[derive(Clone, Copy, Debug)]
enum Add {
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    EqualLines,
    Collinear,
    Coincident,
    OnCurve,
    Midpoint,
    Tangent,
    EqualArcs,
    Concentric,
    /// A line's length dimensioned at `factor` times what it is.
    Length(f64),
    /// An arc's radius dimensioned at `factor` times what it is.
    Radius(f64),
    /// A dimension already on the sketch (its length dimension, or else a new length dimension
    /// added at its value first) typed `factor` times what it is (`Editor::set_dimension`).
    Retype(f64),
}

fn add() -> impl Strategy<Value = Add> {
    let factor = || prop_oneof![0.5..0.9, 1.1..1.8];
    prop_oneof![
        Just(Add::Horizontal),
        Just(Add::Vertical),
        Just(Add::Parallel),
        Just(Add::Perpendicular),
        Just(Add::EqualLines),
        Just(Add::Collinear),
        Just(Add::Coincident),
        Just(Add::OnCurve),
        Just(Add::Midpoint),
        Just(Add::Tangent),
        Just(Add::EqualArcs),
        Just(Add::Concentric),
        factor().prop_map(Add::Length),
        factor().prop_map(Add::Radius),
        factor().prop_map(Add::Retype),
    ]
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// The two-element pick `seed` makes from `n` things, distinct, as indices.
fn two(seed: u64, n: usize) -> Option<(usize, usize)> {
    if n < 2 {
        return None;
    }
    let a = (seed % n as u64) as usize;
    let b = ((seed / 977) % (n as u64 - 1)) as usize;
    Some((a, if b >= a { b + 1 } else { b }))
}

/// The constraint `add` makes from `built`'s geometry with picks `seed`, and the picks in the
/// order they were made (the last is the active one).
fn constraint(built: &Built, add: Add, seed: u64) -> Option<(Constraint, Vec<EntityId>)> {
    let lines: Vec<EntityId> = built
        .sketch
        .lines()
        .into_iter()
        .map(|i| built.curves[i])
        .collect();
    let arcs: Vec<EntityId> = built
        .sketch
        .arcs()
        .into_iter()
        .map(|i| built.curves[i])
        .collect();
    let doc = &built.ed.doc;
    let pick = |v: &[EntityId], k: u64| v.get((k % v.len().max(1) as u64) as usize).copied();
    let ends = |line: EntityId| match doc.get(line).map(|e| e.kind) {
        Some(EntityKind::Line { a, b }) => Some((a, b)),
        _ => None,
    };
    // Points that are ends or centres of curves, as a user would pick them.
    let mut points: Vec<EntityId> = built
        .curves
        .iter()
        .flat_map(|c| {
            doc.get(*c)
                .map(|e| e.kind.points().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    points.sort_unstable();
    points.dedup();
    let two_of = |v: &[EntityId]| two(seed, v.len()).map(|(a, b)| (v[a], v[b]));
    Some(match add {
        Add::Horizontal => {
            let l = pick(&lines, seed)?;
            (Constraint::Horizontal { line: l }, vec![l])
        }
        Add::Vertical => {
            let l = pick(&lines, seed)?;
            (Constraint::Vertical { line: l }, vec![l])
        }
        Add::Parallel => {
            let (a, b) = two_of(&lines)?;
            (Constraint::Parallel { a, b }, vec![a, b])
        }
        Add::Perpendicular => {
            let (a, b) = two_of(&lines)?;
            (Constraint::Perpendicular { a, b }, vec![a, b])
        }
        Add::EqualLines => {
            let (a, b) = two_of(&lines)?;
            (Constraint::Equal { a, b }, vec![a, b])
        }
        Add::Collinear => {
            let (a, b) = two_of(&lines)?;
            (Constraint::Collinear { a, b }, vec![a, b])
        }
        Add::Coincident => {
            let (a, b) = two_of(&points)?;
            (Constraint::Coincident { a, b }, vec![a, b])
        }
        Add::OnCurve => {
            let curves: Vec<EntityId> = lines.iter().chain(&arcs).copied().collect();
            let curve = pick(&curves, seed)?;
            let own: Vec<EntityId> = doc.get(curve)?.kind.points().collect();
            let others: Vec<EntityId> = points
                .iter()
                .copied()
                .filter(|p| !own.contains(p))
                .collect();
            let point = pick(&others, seed / 31)?;
            (
                Constraint::PointOnCurve { point, curve },
                vec![point, curve],
            )
        }
        Add::Midpoint => {
            let line = pick(&lines, seed)?;
            let (a, b) = ends(line)?;
            let others: Vec<EntityId> = points
                .iter()
                .copied()
                .filter(|p| *p != a && *p != b)
                .collect();
            let point = pick(&others, seed / 31)?;
            (Constraint::Midpoint { point, line }, vec![point, line])
        }
        Add::Tangent => {
            let line = pick(&lines, seed)?;
            let arc = pick(&arcs, seed / 31)?;
            let order = if seed & 1 == 0 {
                vec![line, arc]
            } else {
                vec![arc, line]
            };
            (Constraint::Tangent { a: line, b: arc }, order)
        }
        Add::EqualArcs => {
            let (a, b) = two_of(&arcs)?;
            (Constraint::Equal { a, b }, vec![a, b])
        }
        Add::Concentric => {
            let (a, b) = two_of(&arcs)?;
            (Constraint::Concentric { a, b }, vec![a, b])
        }
        Add::Length(f) => {
            let line = pick(&lines, seed)?;
            let (a, b) = ends(line)?;
            let value = doc.point(a).ok()?.distance(doc.point(b).ok()?) * f;
            (Constraint::Distance { a, b, value }, vec![line])
        }
        Add::Retype(_) => {
            let line = pick(&lines, seed)?;
            let (a, b) = ends(line)?;
            let value = doc.point(a).ok()?.distance(doc.point(b).ok()?);
            (Constraint::Distance { a, b, value }, vec![line])
        }
        Add::Radius(f) => {
            let arc = pick(&arcs, seed)?;
            let value = drift_solver::radius_of(doc, arc)? * f;
            (Constraint::Radius { curve: arc, value }, vec![arc])
        }
    })
}

/// A line by id, and its ends.
type LineEnds = (EntityId, DVec2, DVec2);

/// Each line's ends and each arc's radius, by id.
fn shapes(ed: &Editor) -> (Vec<LineEnds>, Vec<(EntityId, f64)>) {
    let mut lines = Vec::new();
    let mut arcs = Vec::new();
    for (id, e) in ed.doc.iter() {
        match e.kind {
            EntityKind::Line { a, b } => {
                lines.push((id, ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap()));
            }
            EntityKind::Arc { .. } => {
                arcs.push((id, drift_solver::radius_of(&ed.doc, id).unwrap_or(0.0)));
            }
            _ => {}
        }
    }
    (lines, arcs)
}

#[test]
#[ignore = "census: run in release with --ignored --exact --nocapture"]
fn relation_census() {
    let n: usize = env_or("CENSUS_N", 4_000);
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        TestRng::from_seed(RngAlgorithm::ChaCha, &[33u8; 32]),
    );
    let strategy = (plan(true), add(), any::<u64>(), any::<bool>());
    let mut total = drift_solver::exits::Stage2Counts::default();
    for case in 0..n {
        let (plan, add, seed, active) = strategy.new_tree(&mut runner).unwrap().current();
        let only: Option<usize> = std::env::var("CENSUS_CASE")
            .ok()
            .and_then(|v| v.parse().ok());
        if only.is_some_and(|o| o != case) {
            continue;
        }
        let kind = format!("{add:?}");
        let kind = kind.split('(').next().unwrap_or("?");
        let act = if active { "act" } else { "none" };
        let Some(sketch) = plan.sketch() else {
            println!("R {case} {kind} {act} BUILD");
            continue;
        };
        let Some(mut built) = Built::new(&sketch, Similarity::IDENTITY) else {
            println!("R {case} {kind} {act} BUILD");
            continue;
        };
        let Some((c, picks)) = constraint(&built, add, seed) else {
            println!("R {case} {kind} {act} NOPICK");
            continue;
        };
        let start_points: Vec<(EntityId, DVec2)> = built
            .ed
            .doc
            .iter()
            .filter(|(_, e)| e.kind.is_point())
            .map(|(id, _)| (id, built.ed.doc.point(id).unwrap()))
            .collect();
        let mid =
            start_points.iter().map(|(_, p)| *p).sum::<DVec2>() / start_points.len().max(1) as f64;
        let size = 2.0
            * start_points
                .iter()
                .map(|(_, p)| p.distance(mid))
                .fold(0.0, f64::max);
        let tol = 1e-6 * size.max(1e-9);
        let (lines0, arcs0) = shapes(&built.ed);
        let named: HashSet<EntityId> = c.entities().collect();
        let ed = &mut built.ed;
        ed.selection = picks.iter().copied().collect();
        ed.set_active(if active { picks.last().copied() } else { None });
        let show = |ed: &Editor, when: &str| {
            if only.is_some() {
                eprintln!("{when}:");
                for (id, e) in ed.doc.iter() {
                    eprintln!("  {id:?} {:?}", e.kind);
                }
                for (id, c) in ed.doc.constraints() {
                    eprintln!("  {id:?} {c:?}");
                }
            }
        };
        show(ed, &format!("before {c:?}, picks {picks:?}"));
        let _ = drift_solver::exits::take_stage2();
        let result = if let Add::Retype(f) = add {
            // The sketch's own length dimension, or one added at the line's length (moving
            // nothing), then typed.
            let existing = ed
                .doc
                .constraints()
                .find(|(_, c)| matches!(c, Constraint::Distance { .. }))
                .map(|(id, c)| (id, *c));
            let found = match existing {
                Some(found) => Some(found),
                None => ed
                    .add_constraints("Length", &[c])
                    .ok()
                    .map(|ids| (ids[0], c)),
            };
            match found {
                None => Err(EditError::Redundant),
                Some((id, c)) => {
                    let value = c.value().unwrap_or(0.0) * f;
                    ed.set_dimension(id, value)
                }
            }
            .map(|()| Vec::new())
        } else {
            ed.add_picked_constraints(c.type_name(), &[c])
        };
        show(ed, "after");
        // How the edit's solves' second stages ended: a stall (stopped with the shapes'
        // projected gradient above the bar) or a cap is printed, case by case.
        let s2 = drift_solver::exits::take_stage2();
        for k in 0..7 {
            total.other[k] += s2.other[k];
            total.other_stalled[k] += s2.other_stalled[k];
        }
        let stalled: u64 = s2.other_stalled.iter().sum();
        if stalled > 0 || s2.other[6] > 0 {
            println!(
                "S {case} {kind} {act} stalled {stalled} cap {} ends {:?} stalls {:?}",
                s2.other[6], s2.other, s2.other_stalled
            );
        }
        let result = match result {
            Ok(_) => "ok".to_owned(),
            Err(e) => format!("{e:?}")
                .split(['(', ' '])
                .next()
                .unwrap_or("err")
                .to_owned(),
        };
        if result != "ok" {
            println!("R {case} {kind} {act} {result}");
            continue;
        }
        let mut mv: f64 = 0.0;
        let mut pts = 0;
        for (id, p) in &start_points {
            let d = ed.doc.point(*id).map_or(0.0, |q| q.distance(*p));
            mv = mv.max(d);
            if d > tol {
                pts += 1;
            }
        }
        let (lines1, arcs1) = shapes(ed);
        if std::env::var_os("CENSUS_POS").is_some() {
            let pos: Vec<String> = start_points
                .iter()
                .map(|(id, _)| {
                    let p = ed.doc.point(*id).unwrap();
                    format!("{:.9},{:.9}", p.x, p.y)
                })
                .collect();
            println!("P {case} {}", pos.join(";"));
        }
        let (mut nl, mut rl, mut ndl, mut rdl, mut rturn) = (0, 0, 0.0f64, 0.0f64, 0.0f64);
        for ((id, a0, b0), (_, a1, b1)) in lines0.iter().zip(&lines1) {
            let dl = (a1.distance(*b1) - a0.distance(*b0)).abs();
            // (A line counts with what the relation names if it or one of its ends is named.)
            let ends_named = match ed.doc.get(*id).map(|e| e.kind) {
                Some(EntityKind::Line { a, b }) => named.contains(&a) || named.contains(&b),
                _ => false,
            };
            if named.contains(id) || ends_named {
                ndl = ndl.max(dl);
                nl += usize::from(dl > tol);
            } else {
                rdl = rdl.max(dl);
                rl += usize::from(dl > tol);
                let (u, w) = (*b0 - *a0, *b1 - *a1);
                if u.length() > tol && w.length() > tol {
                    rturn = rturn.max(u.angle_to(w).abs().to_degrees());
                }
            }
        }
        // Curves the relation left collapsed (a line or radius under the change bar that wasn't).
        let mut col = 0;
        for ((_, a0, b0), (_, a1, b1)) in lines0.iter().zip(&lines1) {
            col += usize::from(a1.distance(*b1) <= tol && a0.distance(*b0) > tol);
        }
        for ((_, r0), (_, r1)) in arcs0.iter().zip(&arcs1) {
            col += usize::from(*r1 <= tol && *r0 > tol);
        }
        let (mut na, mut ra, mut nda, mut rda) = (0, 0, 0.0f64, 0.0f64);
        for ((id, r0), (_, r1)) in arcs0.iter().zip(&arcs1) {
            let dr = (r1 - r0).abs();
            if named.contains(id) {
                nda = nda.max(dr);
                na += usize::from(dr > tol);
            } else {
                rda = rda.max(dr);
                ra += usize::from(dr > tol);
            }
        }
        println!(
            "R {case} {kind} {act} ok size {size:.3} mv {mv:.6} pts {pts} nl {nl} ndl {ndl:.6} \
             rl {rl} rdl {rdl:.6} na {na} nda {nda:.6} ra {ra} rda {rda:.6} rturn {rturn:.4}              col {col}"
        );
    }
    println!(
        "S2 TOTAL ends {:?} stalled {:?} ({:?})",
        total.other,
        total.other_stalled,
        drift_solver::exits::STAGE2
    );
}
