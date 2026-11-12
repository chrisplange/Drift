//! The Mirror tool: about a picked line (kept symmetric), and about a temporary axis through
//! two clicked points (previewed live, leaving nothing behind, one undo step).

use drift_core::{Document, EntityKind};
use drift_geom::SnapOptions;

use super::*;

const TOL: f64 = 0.5;

fn v(x: f64, y: f64) -> DVec2 {
    DVec2::new(x, y)
}

fn ptr(ed: &Editor, tool: &dyn Tool, at: DVec2) -> Pointer {
    let mut p = Pointer::new(at, TOL);
    if tool.wants_snap() {
        p.snap = ed.index.snap(
            at,
            SnapOptions {
                tolerance: TOL,
                grid: None,
                from: tool.snap_from(),
            },
            |id| tool.snap_excludes(ed, id),
        );
    }
    tool.refine_pointer(&mut p);
    p
}

fn hover(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
    let p = ptr(ed, tool, at);
    tool.handle(Input::Hover(p), ed);
}

fn click(ed: &mut Editor, tool: &mut dyn Tool, at: DVec2) {
    let p = ptr(ed, tool, at);
    tool.handle(Input::Hover(p), ed);
    tool.handle(Input::Click(p, Modifiers::default()), ed);
}

fn count(doc: &Document, pred: impl Fn(&EntityKind) -> bool) -> usize {
    doc.iter().filter(|(_, e)| pred(&e.kind)).count()
}

fn lines(doc: &Document) -> usize {
    count(doc, |k| matches!(k, EntityKind::Line { .. }))
}

fn has_point(ed: &Editor, at: DVec2) -> bool {
    ed.doc.iter().any(|(_, e)| match e.kind {
        EntityKind::Point(p) => (p - at).length() < 1e-9,
        _ => false,
    })
}

fn symmetric(ed: &Editor) -> usize {
    ed.doc
        .constraints()
        .filter(|(_, c)| matches!(c, Constraint::Symmetric { .. }))
        .count()
}

/// A line from (2, 1) to (5, 4), picked, and two loose points on x = 0 at (0, −3) and (0, 7)
/// to snap a temporary axis to.
fn fixture() -> (Editor, EntityId) {
    let mut ed = Editor::default();
    ed.doc.begin();
    let a = ed.doc.add_point(v(2.0, 1.0));
    let b = ed.doc.add_point(v(5.0, 4.0));
    let line = ed.doc.add_line(a, b).unwrap();
    ed.doc.add_point(v(0.0, -3.0));
    ed.doc.add_point(v(0.0, 7.0));
    ed.commit("Setup");
    ed.selection = [line].into();
    (ed, line)
}

#[test]
fn two_clicked_points_mirror_the_selection_with_no_line_drawn() {
    let (mut ed, _) = fixture();
    ed.prefs.auto_relations = true;
    let entities = ed.doc.iter().count();
    let mut tool = MirrorTool::default();
    // Both clicks land a little off the loose points: the snaps put the axis on x = 0.
    click(&mut ed, &mut tool, v(0.2, -2.8));
    assert!(tool.is_busy(), "the first point starts a temporary axis");
    assert_eq!(ed.doc.iter().count(), entities, "nothing made yet");
    click(&mut ed, &mut tool, v(-0.2, 7.3));
    assert!(!tool.is_busy());
    assert_eq!(ed.out.notice, None);
    assert!(has_point(&ed, v(-2.0, 1.0)), "mirrored about x = 0");
    assert!(has_point(&ed, v(-5.0, 4.0)), "mirrored about x = 0");
    // The copy and its two ends; no axis line or points left behind.
    assert_eq!(lines(&ed.doc), 2);
    assert_eq!(ed.doc.iter().count(), entities + 3);
    assert_eq!(symmetric(&ed), 0, "no axis to be symmetric about");
    // The copy is picked.
    assert!(
        ed.selection.len() == 1
            && ed.selection.iter().all(|id| matches!(
                ed.doc.get(*id).map(|e| e.kind),
                Some(EntityKind::Line { .. })
            ))
    );
    // One undo step takes it all away.
    assert_eq!(ed.history.undo_label(), Some("Mirror"));
    ed.undo();
    assert_eq!(ed.doc.iter().count(), entities);
    assert_eq!(ed.history.undo_label(), Some("Setup"));
}

#[test]
fn the_mirror_previews_live_while_the_second_point_is_picked() {
    let (mut ed, _) = fixture();
    let mut tool = MirrorTool::default();
    click(&mut ed, &mut tool, v(0.0, -3.0));
    // Pointing straight up from the first point: the axis is x = 0.
    hover(&mut ed, &mut tool, v(0.0, 20.0));
    let previewed: Vec<Shape> = tool.preview(None).into_iter().map(|i| i.shape).collect();
    assert!(
        previewed.iter().any(|s| match *s {
            Shape::Line { a, b } =>
                (a - v(-2.0, 1.0)).length() < 1e-9 && (b - v(-5.0, 4.0)).length() < 1e-9,
            _ => false,
        }),
        "{previewed:?}"
    );
    // Pointing along y = −3 instead: the preview follows.
    hover(&mut ed, &mut tool, v(20.0, -3.0));
    let previewed: Vec<Shape> = tool.preview(None).into_iter().map(|i| i.shape).collect();
    assert!(
        previewed.iter().any(|s| match *s {
            Shape::Line { a, b } =>
                (a - v(2.0, -7.0)).length() < 1e-9 && (b - v(5.0, -10.0)).length() < 1e-9,
            _ => false,
        }),
        "{previewed:?}"
    );
    assert_eq!(lines(&ed.doc), 1, "a preview makes nothing");
}

#[test]
fn esc_during_the_second_point_cancels_cleanly() {
    let (mut ed, _) = fixture();
    let entities = ed.doc.iter().count();
    let mut tool = MirrorTool::default();
    click(&mut ed, &mut tool, v(0.0, -3.0));
    hover(&mut ed, &mut tool, v(0.0, 7.0));
    assert!(tool.cancel(&mut ed));
    assert!(!tool.is_busy());
    assert!(tool.preview(None).is_empty());
    assert_eq!(ed.doc.iter().count(), entities);
    assert_eq!(ed.history.undo_label(), Some("Setup"), "no undo step");
    assert!(!tool.cancel(&mut ed), "nothing left to cancel");
    // The next click starts a new axis.
    click(&mut ed, &mut tool, v(0.0, 7.0));
    assert!(tool.is_busy());
}

#[test]
fn clicking_a_line_still_mirrors_about_it_kept_symmetric() {
    let (mut ed, _) = fixture();
    ed.prefs.auto_relations = true;
    ed.doc.begin();
    let a = ed.doc.add_point(v(-1.0, -10.0));
    let b = ed.doc.add_point(v(-1.0, 10.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Axis");
    let entities = ed.doc.iter().count();
    let mut tool = MirrorTool::default();
    // On the line's body (away from its ends and midpoint): mirrored at once about x = −1.
    click(&mut ed, &mut tool, v(-1.0, 5.0));
    assert!(!tool.is_busy());
    assert!(has_point(&ed, v(-4.0, 1.0)));
    assert!(has_point(&ed, v(-7.0, 4.0)));
    assert_eq!(
        ed.doc.iter().count(),
        entities + 3,
        "the copy; the line stays"
    );
    assert_eq!(symmetric(&ed), 2, "each end kept symmetric about the line");
    assert_eq!(ed.history.undo_label(), Some("Mirror"));
    ed.undo();
    assert_eq!(ed.doc.iter().count(), entities);
    assert_eq!(ed.history.undo_label(), Some("Axis"));
}

#[test]
fn a_snap_to_a_point_on_a_line_starts_a_temporary_axis() {
    let (mut ed, _) = fixture();
    ed.doc.begin();
    let a = ed.doc.add_point(v(-1.0, -10.0));
    let b = ed.doc.add_point(v(-1.0, 10.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Axis");
    let mut tool = MirrorTool::default();
    // The line's midpoint, then its end: a temporary axis along the line, not the line.
    click(&mut ed, &mut tool, v(-0.9, 0.1));
    assert!(tool.is_busy());
    click(&mut ed, &mut tool, v(-1.1, 9.9));
    assert!(has_point(&ed, v(-4.0, 1.0)));
    assert_eq!(symmetric(&ed), 0);
}

#[test]
fn a_two_point_axis_snaps_to_a_circle_centre_and_ends_on_a_line() {
    let (mut ed, _) = fixture();
    ed.doc.begin();
    let c = ed.doc.add_point(v(0.0, -20.0));
    ed.doc.add_circle(c, 2.0).unwrap();
    let a = ed.doc.add_point(v(-1.0, 30.0));
    let b = ed.doc.add_point(v(1.0, 30.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Targets");
    let mut tool = MirrorTool::default();
    // Near the circle's centre: the axis starts there, snapped.
    click(&mut ed, &mut tool, v(0.2, -19.8));
    assert!(tool.is_busy());
    // The second point may land on a line: its midpoint snap ends the axis on x = 0.
    click(&mut ed, &mut tool, v(0.1, 30.1));
    assert!(!tool.is_busy());
    assert!(has_point(&ed, v(-2.0, 1.0)), "mirrored about x = 0");
    assert!(has_point(&ed, v(-5.0, 4.0)), "mirrored about x = 0");
}

#[test]
fn nothing_picked_is_explained_and_starts_no_axis() {
    let (mut ed, _) = fixture();
    ed.selection.clear();
    let mut tool = MirrorTool::default();
    click(&mut ed, &mut tool, v(0.0, -3.0));
    assert!(!tool.is_busy());
    assert!(ed.out.notice.take().is_some());
}

#[test]
fn the_preview_of_an_arc_matches_the_mirrored_arc() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let c = ed.doc.add_point(v(4.0, 2.0));
    let s = ed.doc.add_point(v(7.0, 2.0));
    let e = ed.doc.add_point(v(4.0, 5.0));
    let arc = ed.doc.add_arc(c, s, e).unwrap();
    ed.commit("Setup");
    ed.selection = [arc].into();
    let mut tool = MirrorTool::default();
    // A slanted axis through (0, 0) and (1, 3).
    click(&mut ed, &mut tool, v(0.0, 0.0));
    hover(&mut ed, &mut tool, v(1.0, 3.0));
    let previewed = tool
        .preview(None)
        .into_iter()
        .find_map(|i| match i.shape {
            s @ Shape::Arc { .. } => Some(s),
            _ => None,
        })
        .expect("an arc previewed");
    click(&mut ed, &mut tool, v(1.0, 3.0));
    let copy = ed
        .selection
        .iter()
        .copied()
        .find(|id| {
            matches!(
                ed.doc.get(*id).map(|e| e.kind),
                Some(EntityKind::Arc { .. })
            )
        })
        .expect("the mirrored arc");
    let made = ed.doc.shape(copy).unwrap();
    let (
        Shape::Arc {
            center: c0,
            radius: r0,
            start: s0,
            sweep: w0,
        },
        Shape::Arc {
            center: c1,
            radius: r1,
            start: s1,
            sweep: w1,
        },
    ) = (previewed, made)
    else {
        panic!("{previewed:?} vs {made:?}");
    };
    assert!((c0 - c1).length() < 1e-9);
    assert!((r0 - r1).abs() < 1e-9);
    assert!((w0 - w1).abs() < 1e-9, "{w0} vs {w1}");
    let at = |s: f64| DVec2::from_angle(s);
    assert!((at(s0) - at(s1)).length() < 1e-9, "{s0} vs {s1}");
}
