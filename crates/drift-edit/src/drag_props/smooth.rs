//! Smooth drags (#362): what a user sees when the pointer moves slowly and steadily. A drag
//! goes along a straight line in many small frames, as the select tool's grab makes them
//! (each `Editor::drag` call from the last accepted offset), and every frame is judged
//! against the one before it:
//!
//! - **a jump**: an accepted frame moves a point more than [`JUMP`] times as far as the
//!   pointer went since the last accepted frame (the bar of `small_drag_is_continuous`, held
//!   frame by frame, with the same excuse for a forced swing on the first frame);
//! - **back and forth**: a point goes more than [`BACK`] times the pointer's move one way and,
//!   on the next frame, more than that back again;
//! - **a kink**: a point's move changes from one frame to the next by more than [`KINK`]
//!   times the pointer's move (2.5 mm of a jerk in a frame of 0.25 mm), whichever way. A jump
//!   has to be 25 mm to count; this is what a snap between two shapes looks like when it is
//!   smaller than that;
//! - **a stutter**: the drag stops and then goes on again while the pointer keeps its line
//!   (a stop that stays stopped is the binding drag rule, not a stutter).
//!
//! [`slow_drag_is_smooth`] is the property (no jump, nothing back and forth, no kink); the
//! ignored
//! [`smooth_census`] prints one line per drag with all three counts, to compare two builds by:
//!
//! ```text
//! cargo test --release -p drift-edit --lib drag_props::smooth::smooth_census -- --ignored --exact --nocapture
//! ```

use super::*;

/// How far the pointer goes a frame, mm: a slow drag (15 mm a second at 60 frames a second).
const FRAME: f64 = 0.25;

/// How many times the pointer's move a frame may move anything: the hundredfold
/// `small_drag_is_continuous` allows a small drag.
const JUMP: f64 = 100.0;

/// A point that goes more than this many times the pointer's move one way, and on the next
/// frame more than this many times it back, went back and forth. A point on a smooth path
/// turns round only where it has slowed to nothing.
const BACK: f64 = 2.0;

/// The most a point's move may change from one frame to the next, as a multiple of the
/// pointer's move, in a smooth drag (the first frame's forced swing left out, as for a jump).
/// Along a smooth path the change shrinks with the frame; the slow drags that are smooth by
/// eye stay under 1, and a snap between two shapes is tens or hundreds.
const KINK: f64 = 20.0;

/// What a slow drag did, frame against frame.
#[derive(Clone, Debug, Default)]
struct Smoothness {
    frames: usize,
    refused: usize,
    /// Accepted frames that jumped, the first of them (1-based), and the worst, as a multiple
    /// of the pointer's move.
    jumps: usize,
    first_jump: usize,
    worst_jump: f64,
    /// Frames on which a point went back on its last move, and the worst (the smaller of the
    /// two moves, as a multiple of the pointer's).
    backs: usize,
    worst_back: f64,
    /// The largest change of any point's move from one frame to the next, as a multiple of
    /// the pointer's move: a smooth drag's is small, and smaller the smaller the frames.
    kink: f64,
    /// Times the drag went on again after a refused frame.
    resumes: usize,
    /// Solves over the whole drag.
    solves: u32,
    /// The drag came back along its line (nothing was refused on the way out), frames refused
    /// on the way back, and the farthest a point was then from where the same pointer position
    /// had it on the way out, as a share of the sketch's size.
    came_back: bool,
    back_refused: usize,
    creep: f64,
    /// What made the result not count: a relation left unmet, or a refused frame that moved
    /// geometry.
    broken: Option<String>,
}

impl Smoothness {
    fn smooth(&self) -> bool {
        self.broken.is_none() && self.jumps == 0 && self.backs == 0 && self.kink <= KINK
    }
}

/// Drag `ids` to `offset` along a straight line in frames of `frame` mm, and judge each
/// frame against the last.
fn slow_drag(built: &mut Built, ids: &[EntityId], offset: DVec2, frame: f64) -> Smoothness {
    let mut out = Smoothness::default();
    let dragged: Vec<Param> = built
        .ed
        .doc
        .defining_points(ids.iter().copied())
        .into_iter()
        .flat_map(Param::point)
        .collect();
    let whole = offset.length();
    if whole <= 0.0 {
        return out;
    }
    let frames = (whole / frame).ceil().max(1.0) as usize;
    let start = built.positions();
    built.ed.doc.begin();
    let mut applied = DVec2::ZERO;
    // The last accepted frame's moves, each over the pointer's (none across a refused frame).
    let mut last: Option<Vec<DVec2>> = None;
    let mut stopped = false;
    // Where each accepted frame left the points (only read where none was refused).
    let mut way_out: Vec<Vec<DVec2>> = Vec::new();
    let trace = std::env::var_os("SMOOTH_TRACE").is_some();
    for k in 1..=frames {
        let target = if k == frames {
            offset
        } else {
            offset * (k as f64 * frame / whole)
        };
        let before = built.positions();
        let went = built.ed.drag(ids, target - applied, &dragged);
        out.frames += 1;
        out.solves += built.ed.drag_solves;
        let after = built.positions();
        if !went {
            if trace {
                println!("  frame {k} solves {} refused", built.ed.drag_solves);
            }
            out.refused += 1;
            stopped = true;
            last = None;
            if after != before {
                out.broken = Some(format!("frame {k}: a refused drag moved geometry"));
                break;
            }
            continue;
        }
        if let Some(what) = broken(&built.ed.doc, None) {
            out.broken = Some(format!("frame {k}: {what}"));
            break;
        }
        if std::mem::take(&mut stopped) {
            out.resumes += 1;
        }
        way_out.push(after.clone());
        let travel = (target - applied).length();
        applied = target;
        // (A forced swing is the geometry's own answer on the first frame only: after it the
        // dragged point is off the point it began on.)
        let swing = (k == 1)
            .then(|| {
                let run = Run {
                    start: before.clone(),
                    end: after.clone(),
                    applied: target,
                    refused: 0,
                    committed: true,
                };
                forced_swing(built, ids, &run, target)
            })
            .flatten();
        let mut worst = 0.0f64;
        for i in 0..before.len() {
            let moved = match swing {
                Some(reach) => (before[i].distance(after[i]) - SWING_MIRROR * reach).max(0.0),
                None => before[i].distance(after[i]),
            };
            let floor = LANDS * start[i].length().max(1.0);
            if moved > JUMP * travel + floor {
                worst = worst.max(moved / travel);
            }
        }
        if worst > 0.0 {
            out.jumps += 1;
            if out.first_jump == 0 {
                out.first_jump = k;
            }
            out.worst_jump = out.worst_jump.max(worst);
        }
        let moves: Vec<DVec2> = before
            .iter()
            .zip(&after)
            .map(|(a, b)| (*b - *a) / travel)
            .collect();
        if trace {
            let (i, m) = moves
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.length().total_cmp(&b.1.length()))
                .unwrap();
            println!(
                "  frame {k} solves {} most: point {i} by {:.3} ({:.3}, {:.3}) x the pointer",
                built.ed.drag_solves,
                m.length(),
                m.x,
                m.y
            );
        }
        if let Some(was) = &last
            && swing.is_none()
        {
            let mut back = 0.0f64;
            for (a, b) in was.iter().zip(&moves) {
                out.kink = out.kink.max(a.distance(*b));
                if a.dot(*b) < 0.0 {
                    back = back.max(a.length().min(b.length()));
                }
            }
            if back > BACK {
                out.backs += 1;
                out.worst_back = out.worst_back.max(back);
            }
        }
        last = swing.is_none().then_some(moves);
    }
    // Back along the same line, where nothing was refused on the way out: how far each frame
    // leaves a point from where the same pointer position had it on the way out (creep), as a
    // share of the sketch's size. Not the last frame, back at the start: that one is exact.
    if out.broken.is_none() && out.refused == 0 && frames > 1 {
        let mid = start.iter().sum::<DVec2>() / start.len().max(1) as f64;
        let size = 2.0 * start.iter().map(|p| p.distance(mid)).fold(0.0, f64::max);
        for k in (1..frames).rev() {
            let target = offset * (k as f64 * frame / whole);
            if !built.ed.drag(ids, target - applied, &dragged) {
                out.back_refused += 1;
                continue;
            }
            applied = target;
            let creep = built
                .positions()
                .iter()
                .zip(&way_out[k - 1])
                .map(|(a, b)| a.distance(*b))
                .fold(0.0, f64::max);
            out.creep = out.creep.max(creep / size.max(1e-300));
        }
        out.came_back = true;
    }
    built.ed.commit("Move");
    out
}

/// The property: a slow, straight drag is smooth. No accepted frame jumps, nothing goes back
/// and forth, and no point's move kinks. (A frame may be refused: the drag stops at the last
/// valid shape.)
fn smooth(plan: &Plan, pick: &Pick, offset: DVec2) -> Result<(), TestCaseError> {
    let Some(sketch) = plan.sketch() else {
        return Ok(());
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = pick.ids(&built);
    let what = if built.curves.contains(&ids[0]) {
        "curves"
    } else {
        "point"
    };
    let names: Vec<usize> = ids
        .iter()
        .map(|id| match built.curves.iter().position(|c| c == id) {
            Some(i) => i,
            None => built.points.iter().position(|p| p == id).unwrap(),
        })
        .collect();
    let start = built.positions();
    let s = slow_drag(&mut built, &ids, offset, FRAME);
    prop_assert!(
        s.smooth(),
        "a slow drag wasn't smooth: {s:?}
  points {start:?}
  curves {:?}
  relations {:?}
  dragged {what} {names:?} to {offset:?} in frames of {FRAME}",
        built.sketch.curves,
        built.sketch.rels,
    );
    Ok(())
}

/// Census case 2237, the drag #362's jitter was traced on: a line (hanging, tangent at its
/// end to an arc) and the arc are moved, slowly. In one solve from the drag's start a frame,
/// with steps only where that failed or wandered, the hanging line's far end ran away at 6 to
/// 53 times the pointer's pace over a few frames and snapped back 180 times its move on the
/// next: 4 jumps, 6 frames going back and forth, a kink of 329. In steps from the start every
/// frame, it is smooth. This is the test that fails if the steps go.
#[test]
fn a_line_hanging_from_a_moved_arc_follows_a_slow_drag_smoothly() {
    let p = DVec2::new;
    let sketch = Sketch {
        points: vec![
            p(0.0, 0.0),
            p(0.0, -30.0),
            p(0.0, 15.0),
            p(-28.97777478867205, 7.235428646924376),
            p(3.105828541230249, -18.40889008453118),
            p(14.696938456699069, -21.51471862576143),
            p(10.814652780161259, -36.00360602009745),
        ],
        curves: vec![
            Curve::Line(0, 1),
            Curve::Line(2, 3),
            Curve::Arc(4, 1, 5),
            Curve::Line(5, 6),
        ],
        rels: vec![Rel::OnCurve(2, 0), Rel::Tangent(3, 2), Rel::Vertical(0)],
    };
    let mut built = Built::new(&sketch, Similarity::IDENTITY).unwrap();
    let ids = [built.curves[1], built.curves[2]];
    let offset = p(-13.345513509701808, -17.126803960491497);
    let s = slow_drag(&mut built, &ids, offset, FRAME);
    assert_eq!(s.refused, 0, "{s:?}");
    assert!(s.smooth(), "{s:?}");
}

property!(
    #[ignore = "known failure: #362"]
    slow_drag_is_smooth, slow_drag_is_smooth_heavy,
    (plan in plan(true), pick in pick(), offset in offset(20.0))
        => smooth(&plan, &pick, offset));

/// The smoothness census: the similarity census's random drags (same generator and seed, so
/// case numbers match), each as a slow drag. One line per drag, and totals at the end.
/// `CENSUS_N` cases (default 4,000), `CENSUS_CASE` for one, `SMOOTH_FRAME` for the frame's
/// length in mm (default [`FRAME`]).
#[test]
#[ignore = "census: run in release with --ignored --exact --nocapture"]
fn smooth_census() {
    let n: usize = census::env_or("CENSUS_N", 4_000);
    let only: Option<usize> = std::env::var("CENSUS_CASE")
        .ok()
        .and_then(|v| v.parse().ok());
    let frame: f64 = census::env_or("SMOOTH_FRAME", FRAME);
    let mut next = census::cases();
    let mut drags = 0usize;
    let mut total = Smoothness::default();
    let (mut jumped, mut backed, mut stuttered, mut broke, mut first_frame) = (0, 0, 0, 0, 0);
    let mut rough = 0;
    let mut kinks: Vec<f64> = Vec::new();
    let mut creeps: Vec<f64> = Vec::new();
    let mut back_refused = 0usize;
    for case in 0..n {
        let (plan, pick, off, _) = next();
        if only.is_some_and(|c| c != case) {
            continue;
        }
        let Some(sketch) = plan.sketch() else {
            continue;
        };
        let Some(mut built) = Built::new(&sketch, Similarity::IDENTITY) else {
            continue;
        };
        let ids = pick.ids(&built);
        let what = if built.curves.contains(&ids[0]) {
            "curves"
        } else {
            "point"
        };
        let s = slow_drag(&mut built, &ids, off, frame);
        drags += 1;
        total.frames += s.frames;
        total.refused += s.refused;
        total.jumps += s.jumps;
        total.backs += s.backs;
        total.resumes += s.resumes;
        total.solves += s.solves;
        jumped += usize::from(s.jumps > 0);
        first_frame += usize::from(s.first_jump == 1);
        backed += usize::from(s.backs > 0);
        stuttered += usize::from(s.resumes > 0);
        rough += usize::from(!s.smooth());
        broke += usize::from(s.broken.is_some());
        kinks.push(s.kink);
        if s.came_back {
            creeps.push(s.creep);
            back_refused += usize::from(s.back_refused > 0);
        }
        println!(
            "S {case} {what} frames {} ref {} res {} jumps {} first {} worst {:.3e} backs {} wback {:.3e} kink {:.3e} solves {} creep {:.3e}{}",
            s.frames,
            s.refused,
            s.resumes,
            s.jumps,
            s.first_jump,
            s.worst_jump,
            s.backs,
            s.worst_back,
            s.kink,
            s.solves,
            s.creep,
            s.broken.map(|b| format!(" ERR {b}")).unwrap_or_default(),
        );
        if only.is_some() {
            println!(
                "  points {:?}
  curves {:?}
  relations {:?}
  dragged {what} to {off:?}",
                sketch.points, sketch.curves, sketch.rels
            );
        }
    }
    kinks.sort_by(f64::total_cmp);
    creeps.sort_by(f64::total_cmp);
    let creep_pct = |p: f64| {
        creeps
            .get(((creeps.len() as f64 - 1.0) * p).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "B out and back {} drags | refused on the way back {back_refused} | creep over 1e-6 of the size: {} over 1e-3: {} over 1e-2: {} | p50 {:.2e} p90 {:.2e} p99 {:.2e} max {:.2e}",
        creeps.len(),
        creeps.iter().filter(|&&c| c > 1e-6).count(),
        creeps.iter().filter(|&&c| c > 1e-3).count(),
        creeps.iter().filter(|&&c| c > 1e-2).count(),
        creep_pct(0.5),
        creep_pct(0.9),
        creep_pct(0.99),
        creep_pct(1.0),
    );
    let pct = |p: f64| {
        kinks
            .get(((kinks.len() as f64 - 1.0) * p).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    println!(
        "T drags {drags} frames {} refused {} | not smooth by the property {rough} | drags that jump {jumped} (on frame 1: {first_frame}) jump frames {} | drags back and forth {backed} frames {} | drags that stutter {stuttered} resumes {} | broken {broke} | kink p50 {:.2e} p90 {:.2e} p99 {:.2e} max {:.2e} | kink>10: {} >20: {} >30: {} >100: {} | solves {} ({:.2} a frame)",
        total.frames,
        total.refused,
        total.jumps,
        total.backs,
        total.resumes,
        pct(0.5),
        pct(0.9),
        pct(0.99),
        pct(1.0),
        kinks.iter().filter(|&&k| k > 10.0).count(),
        kinks.iter().filter(|&&k| k > KINK).count(),
        kinks.iter().filter(|&&k| k > 30.0).count(),
        kinks.iter().filter(|&&k| k > 100.0).count(),
        total.solves,
        f64::from(total.solves) / total.frames.max(1) as f64,
    );
}
