//! Edits on a big drawing commit before their areas are found, and their fills follow them
//! once the areas are in (`Editor::settle_areas`). Property tests: whatever the edits, a
//! drawing edited that way ends as one edited with every area found at once — fills, their
//! shapes, the areas, the analysis and the undo steps alike — and undo takes both back the
//! same way.
//!
//! `cargo test -p drift-app areas_props` (the `*_heavy` versions: `--release -- --ignored`).

use drift_core::{DVec2, Document, EntityId};
use proptest::prelude::*;

use crate::editor::Editor;

/// A small version of the big drawing of the perf tests: rooms with doors, columns and
/// loose lines, every third one hatched, and grid lines across.
fn drawing(cells: usize, defer: bool) -> Editor {
    let mut ed = Editor::new(Document::new());
    ed.doc.begin();
    drift_io::dxf::add_to_document(&mut ed.doc, &crate::perf_big::big_drawing(cells));
    assert!(ed.commit_import("Import"));
    // (The import is where the edits start: undo stops there.)
    ed.history.clear();
    ed.areas.defer = Some(defer);
    let _ = ed.fill_shapes();
    ed
}

/// One edit, from the numbers in `n`.
#[derive(Clone, Debug)]
enum Step {
    /// Move curve `k` (of the curves, by index) by a few units.
    Nudge(usize, i8, i8),
    /// Delete curves `k` and the next `n`.
    Delete(usize, u8),
    /// A line from one point to another (of the grid of the drawing, by indices).
    Line(u16, u16, u16, u16),
    /// Fill the area around a point.
    Fill(u16, u16),
    Undo,
    Redo,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (any::<usize>(), -3i8..4, -3i8..4).prop_map(|(k, x, y)| Step::Nudge(k, x, y)),
        2 => (any::<usize>(), 0u8..3).prop_map(|(k, n)| Step::Delete(k, n)),
        2 => (0u16..90, 0u16..90, 0u16..90, 0u16..90).prop_map(|(a, b, c, d)| Step::Line(a, b, c, d)),
        2 => (0u16..90, 0u16..90).prop_map(|(x, y)| Step::Fill(x, y)),
        2 => Just(Step::Undo),
        1 => Just(Step::Redo),
    ]
}

fn curves(ed: &Editor) -> Vec<EntityId> {
    ed.doc
        .iter()
        .filter(|(_, e)| !e.kind.is_point())
        .map(|(id, _)| id)
        .collect()
}

/// A point of the drawing's area (its rooms are 3,000 apart), by grid indices.
fn at(x: u16, y: u16) -> DVec2 {
    DVec2::new(f64::from(x) * 100.0 - 500.0, f64::from(y) * 100.0 - 500.0)
}

fn apply(ed: &mut Editor, step: &Step) {
    match *step {
        Step::Nudge(k, x, y) => {
            let c = curves(ed);
            ed.selection = [c[k % c.len()]].into();
            let _ = ed.nudge(DVec2::new(f64::from(x) * 7.0, f64::from(y) * 5.0));
        }
        Step::Delete(k, n) => {
            let c = curves(ed);
            ed.selection = (0..=usize::from(n)).map(|i| c[(k + i) % c.len()]).collect();
            ed.delete_selection();
        }
        Step::Line(a, b, c, d) => {
            let (p, q) = (at(a, b), at(c, d));
            if p == q {
                return;
            }
            ed.doc.begin();
            let (p, q) = (ed.doc.add_point(p), ed.doc.add_point(q));
            ed.doc.add_line(p, q).unwrap();
            ed.commit("Line");
        }
        Step::Fill(x, y) => {
            let _ = ed.add_fill_at(at(x, y) + DVec2::new(13.0, 17.0));
        }
        Step::Undo => {
            ed.undo();
        }
        Step::Redo => {
            ed.redo();
        }
    }
    ed.selection.clear();
    ed.selected_fills.clear();
}

/// Everything an edit's fills following it can change, as text.
fn state(ed: &mut Editor) -> String {
    let fills: Vec<_> = ed.doc.fills().map(|(id, f)| (id, f.clone())).collect();
    let paths: Vec<_> = ed.doc.paths().map(|(id, p)| (id, p.clone())).collect();
    let shapes = ed.fill_shapes();
    let regions = ed.regions();
    let analysis = ed.analysis();
    format!(
        "fills {fills:?}\npaths {paths:?}\nshapes {shapes:?}\nregions {regions:?}\n\
         dof {:?} {:?} {:?}\nundo {} {} redo {}",
        analysis.dof,
        analysis.conflicts,
        analysis.definitions.len(),
        ed.history.undo_depth(),
        ed.history.recorded(),
        ed.history.can_redo(),
    )
}

fn same(a: &mut Editor, b: &mut Editor, when: &str) -> Result<(), TestCaseError> {
    let (sa, sb) = (state(a), state(b));
    if sa != sb {
        let line = sa
            .lines()
            .zip(sb.lines())
            .position(|(x, y)| x != y)
            .unwrap_or(0);
        let short = |s: &str| {
            s.lines()
                .nth(line)
                .unwrap_or("")
                .chars()
                .take(400)
                .collect::<String>()
        };
        return Err(TestCaseError::fail(format!(
            "{when}: deferred and at once differ at line {line}:\n  at once:  {}\n  deferred: {}",
            short(&sa),
            short(&sb)
        )));
    }
    Ok(())
}

/// The deferred editor is settled before each edit (as the app does on any input), at
/// random points between.
fn run(cells: usize, steps: &[(Step, bool)]) -> Result<(), TestCaseError> {
    let mut now = drawing(cells, false);
    let mut later = drawing(cells, true);
    let mut deferred = 0;
    for (k, (step, settle)) in steps.iter().enumerate() {
        later.settle_areas();
        apply(&mut now, step);
        apply(&mut later, step);
        deferred += usize::from(later.areas.pending.is_some());
        if *settle {
            later.settle_areas();
            same(&mut now, &mut later, &format!("after step {k} {step:?}"))?;
        }
    }
    later.settle_areas();
    same(&mut now, &mut later, "at the end")?;
    let _ = deferred;
    // Undo takes both back the same way.
    while now.history.can_undo() {
        now.undo();
        later.undo();
        same(&mut now, &mut later, "undoing")?;
    }
    prop_assert!(!later.history.can_undo());
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn deferred_fills_end_as_fills_followed_at_once(
        steps in prop::collection::vec((step(), any::<bool>()), 1..8),
    ) {
        run(4, &steps)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    #[ignore = "heavy: run with --release -- --ignored"]
    fn deferred_fills_end_as_fills_followed_at_once_heavy(
        steps in prop::collection::vec((step(), any::<bool>()), 1..20),
        cells in 2usize..10,
    ) {
        run(cells, &steps)?;
    }
}

/// An edit begun while the last one's fills still wait (as the assistant or a script
/// might): they follow first, in the last edit's step, and the new edit is its own step.
#[test]
fn an_edit_begun_before_the_last_one_settled_keeps_both_steps() {
    let mut now = drawing(4, false);
    let mut later = drawing(4, true);
    let steps = [
        Step::Nudge(5, 2, 1),
        Step::Line(5, 5, 60, 40),
        Step::Delete(17, 1),
        Step::Nudge(40, -2, 3),
    ];
    for step in &steps {
        apply(&mut now, step);
        apply(&mut later, step);
    }
    assert!(later.areas.pending.is_some(), "the last edit's fills wait");
    later.settle_areas();
    assert_eq!(summary(&mut now), summary(&mut later));
    for _ in 0..steps.len() {
        now.undo();
        later.undo();
        assert_eq!(summary(&mut now), summary(&mut later));
    }
}

/// Fill shapes (areas, whether lost), the undo depth and the number of fills: what two
/// editors whose ids may differ (an edit's own made before or after its fills followed)
/// can be compared by.
fn summary(ed: &mut Editor) -> (Vec<String>, usize, usize) {
    let shapes = ed.fill_shapes();
    let mut areas: Vec<String> = shapes
        .values()
        .map(|s| format!("{:.6} {}", s.region.area, s.lost))
        .collect();
    areas.sort();
    (areas, ed.history.undo_depth(), ed.doc.fills().count())
}

/// A step pushed straight onto the history on top of an edit whose fills wait (as some
/// commands do): it stays a step of its own, made again over the followed fills.
#[test]
fn a_step_pushed_over_a_waiting_edit_stays_its_own() {
    let mut now = drawing(4, false);
    let mut later = drawing(4, true);
    for ed in [&mut now, &mut later] {
        apply(ed, &Step::Delete(17, 1));
        let point = ed.doc.iter().find(|(_, e)| e.kind.is_point()).unwrap().0;
        ed.doc.begin();
        ed.doc.set_point(point, DVec2::new(-900.0, -900.0)).unwrap();
        let tx = ed.doc.commit("Pushed");
        ed.history.push(tx);
        ed.sync();
    }
    assert!(later.areas.pending.is_some(), "the edit's fills wait");
    later.settle_areas();
    assert_eq!(summary(&mut now), summary(&mut later));
    assert_eq!(later.history.undo_label(), Some("Pushed"));
    for _ in 0..2 {
        now.undo();
        later.undo();
        assert_eq!(summary(&mut now), summary(&mut later));
    }
}

/// Edits squashed into one step while the last one's fills wait: they follow in that step.
#[test]
fn edits_squashed_over_a_waiting_edit_stay_one_step() {
    let mut now = drawing(4, false);
    let mut later = drawing(4, true);
    for ed in [&mut now, &mut later] {
        apply(ed, &Step::Nudge(5, 2, 1));
        apply(ed, &Step::Delete(17, 1));
        ed.history.squash_last(2, "Both");
    }
    later.settle_areas();
    assert_eq!(summary(&mut now), summary(&mut later));
    assert_eq!(later.history.undo_depth(), 1);
    now.undo();
    later.undo();
    assert_eq!(summary(&mut now), summary(&mut later));
}
