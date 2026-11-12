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

/// What the tool draws: the mirrored curves and the axis.
fn shown(tool: &MirrorTool, ed: &Editor) -> Vec<Shape> {
    tool.editor_preview(ed)
        .into_iter()
        .chain(tool.preview(None))
        .map(|i| i.shape)
        .collect()
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
    let previewed: Vec<Shape> = shown(&tool, &ed);
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
    let previewed: Vec<Shape> = shown(&tool, &ed);
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
    assert!(shown(&tool, &ed).is_empty());
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
fn a_click_on_a_line_s_end_or_midpoint_still_uses_the_line() {
    let (mut ed, picked) = fixture();
    ed.prefs.auto_relations = true;
    ed.doc.begin();
    let a = ed.doc.add_point(v(-1.0, -10.0));
    let b = ed.doc.add_point(v(-1.0, 10.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Axis");
    let mut tool = MirrorTool::default();
    // Its midpoint: mirrored about the line at once, kept symmetric.
    click(&mut ed, &mut tool, v(-0.9, 0.1));
    assert!(!tool.is_busy());
    assert!(has_point(&ed, v(-4.0, 1.0)));
    assert_eq!(symmetric(&ed), 2);
    // Its end, after undoing: the line again.
    ed.undo();
    ed.selection = [picked].into();
    click(&mut ed, &mut tool, v(-1.1, 9.9));
    assert!(!tool.is_busy());
    assert_eq!(symmetric(&ed), 2);
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
    // Near the circle's centre (off any line): the axis starts there, snapped.
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
    let previewed = shown(&tool, &ed)
        .into_iter()
        .find_map(|s| match s {
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

#[test]
fn a_second_click_on_the_first_point_keeps_picking_with_a_notice() {
    let (mut ed, _) = fixture();
    let entities = ed.doc.iter().count();
    let mut tool = MirrorTool::default();
    click(&mut ed, &mut tool, v(10.0, 10.0));
    // Within the pick distance of the first point: no axis yet.
    click(&mut ed, &mut tool, v(10.3, 10.0));
    assert!(tool.is_busy(), "still picking the second point");
    assert!(ed.out.notice.take().is_some());
    assert_eq!(ed.doc.iter().count(), entities);
    click(&mut ed, &mut tool, v(10.0, 20.0));
    assert!(!tool.is_busy());
    assert!(has_point(&ed, v(18.0, 1.0)), "mirrored about x = 10");
}

#[test]
fn an_undo_between_the_clicks_shows_in_the_preview_at_once() {
    let (mut ed, line) = fixture();
    ed.doc.begin();
    let a = ed.doc.add_point(v(2.0, -5.0));
    let b = ed.doc.add_point(v(5.0, -5.0));
    let other = ed.doc.add_line(a, b).unwrap();
    ed.commit("Another");
    ed.selection = [line, other].into();
    let mut tool = MirrorTool::default();
    click(&mut ed, &mut tool, v(0.0, -3.0));
    hover(&mut ed, &mut tool, v(0.0, 7.0));
    let lines_shown = |tool: &MirrorTool, ed: &Editor| {
        shown(tool, ed)
            .iter()
            .filter(|s| matches!(s, Shape::Line { .. }))
            .count()
    };
    assert_eq!(lines_shown(&tool, &ed), 3, "two images and the axis");
    // Undo takes the second line away; with no new pointer move the preview follows.
    ed.undo();
    assert_eq!(lines_shown(&tool, &ed), 2, "one image and the axis");
}

#[test]
fn the_prompt_says_to_pick_first_only_when_nothing_is_picked() {
    let (mut ed, _) = fixture();
    let mut tool = MirrorTool::default();
    hover(&mut ed, &mut tool, v(10.0, 10.0));
    assert_eq!(
        tool.hint(),
        "Mirror: click a line, or two points off any line, to mirror about"
    );
    click(&mut ed, &mut tool, v(10.0, 10.0));
    assert!(tool.hint().contains("a line's end or midpoint is fine"));
    tool.cancel(&mut ed);
    ed.selection.clear();
    hover(&mut ed, &mut tool, v(10.0, 10.0));
    assert!(tool.hint().contains("select the geometry first"));
}

/// A 30 × 20 rectangle at (2, 1), fully defined: level and plumb sides, its width and height,
/// and its first corner fixed. Its sides.
fn dimensioned_rectangle(ed: &mut Editor) -> Vec<EntityId> {
    ed.doc.begin();
    let p = [v(2.0, 1.0), v(32.0, 1.0), v(32.0, 21.0), v(2.0, 21.0)].map(|at| ed.doc.add_point(at));
    let l: Vec<EntityId> = (0..4)
        .map(|i| ed.doc.add_line(p[i], p[(i + 1) % 4]).unwrap())
        .collect();
    for c in [
        Constraint::Horizontal { line: l[0] },
        Constraint::Horizontal { line: l[2] },
        Constraint::Vertical { line: l[1] },
        Constraint::Vertical { line: l[3] },
        Constraint::Distance {
            a: p[0],
            b: p[1],
            value: 30.0,
        },
        Constraint::Distance {
            a: p[1],
            b: p[2],
            value: 20.0,
        },
        Constraint::Fix {
            point: p[0],
            at: v(2.0, 1.0),
        },
    ] {
        ed.doc.add_constraint(c).unwrap();
    }
    ed.commit("Rectangle");
    l
}

fn length(ed: &Editor, line: EntityId) -> f64 {
    let Some(EntityKind::Line { a, b }) = ed.doc.get(line).map(|e| e.kind) else {
        panic!("a line");
    };
    (ed.doc.point(b).unwrap() - ed.doc.point(a).unwrap()).length()
}

fn copied_lines(ed: &Editor) -> Vec<EntityId> {
    ed.selection
        .iter()
        .copied()
        .filter(|id| {
            matches!(
                ed.doc.get(*id).map(|e| e.kind),
                Some(EntityKind::Line { .. })
            )
        })
        .collect()
}

#[test]
fn a_dimensioned_rectangle_mirrored_about_two_points_stays_fully_defined() {
    let mut ed = Editor::default();
    let sides = dimensioned_rectangle(&mut ed);
    ed.selection = sides.iter().copied().collect();
    ed.sync();
    assert_eq!(ed.analysis().dof, Some(0));
    let mut tool = MirrorTool::default();
    // A plumb axis at x = -5: the copy keeps its level and plumb sides and its dimensions,
    // and its first corner is fixed at its image.
    click(&mut ed, &mut tool, v(-5.0, -10.0));
    click(&mut ed, &mut tool, v(-5.0, 40.0));
    assert_eq!(ed.out.notice, None);
    let copy = copied_lines(&ed);
    assert_eq!(copy.len(), 4);
    ed.sync();
    assert_eq!(ed.analysis().dof, Some(0), "still fully defined");
    let mut lengths: Vec<f64> = copy.iter().map(|l| length(&ed, *l)).collect();
    lengths.sort_by(f64::total_cmp);
    for (got, want) in lengths.iter().zip([20.0, 20.0, 30.0, 30.0]) {
        assert!((got - want).abs() < 1e-9, "{lengths:?}");
    }
    for at in [v(-12.0, 1.0), v(-42.0, 1.0), v(-42.0, 21.0), v(-12.0, 21.0)] {
        assert!(has_point(&ed, at), "{at:?}");
    }
    assert_eq!(ed.history.undo_label(), Some("Mirror"));
    ed.undo();
    assert_eq!(ed.history.undo_label(), Some("Rectangle"));
}

#[test]
fn a_slanted_two_point_axis_keeps_lengths_and_angles_but_not_level_or_plumb() {
    let mut ed = Editor::default();
    ed.doc.begin();
    let o = ed.doc.add_point(v(0.0, 0.0));
    let a = ed.doc.add_point(v(10.0, 0.0));
    let b = ed.doc.add_point(v(5.0, 5.0 * 3f64.sqrt()));
    let la = ed.doc.add_line(o, a).unwrap();
    let lb = ed.doc.add_line(o, b).unwrap();
    for c in [
        Constraint::Distance {
            a: o,
            b: a,
            value: 10.0,
        },
        Constraint::Distance {
            a: o,
            b,
            value: 10.0,
        },
        Constraint::Angle {
            a: la,
            b: lb,
            degrees: 60.0,
        },
        Constraint::Horizontal { line: la },
    ] {
        ed.doc.add_constraint(c).unwrap();
    }
    ed.commit("Vee");
    ed.selection = [la, lb].into();
    let mut tool = MirrorTool::default();
    // An axis through (20, 0) and (21, 2).
    let reflect = |p: DVec2| crate::modify::reflect_about(v(20.0, 0.0), v(21.0, 2.0), p);
    click(&mut ed, &mut tool, v(20.0, 0.0));
    click(&mut ed, &mut tool, v(21.0, 2.0));
    assert_eq!(ed.out.notice, None);
    // The solve left the copy where the reflection put it: the angle ran the right way
    // round, and the lengths held.
    for p in [v(0.0, 0.0), v(10.0, 0.0), v(5.0, 5.0 * 3f64.sqrt())] {
        let at = reflect(p);
        assert!(
            ed.doc.iter().any(|(_, e)| match e.kind {
                EntityKind::Point(q) => (q - at).length() < 1e-6,
                _ => false,
            }),
            "{at:?}"
        );
    }
    let count =
        |pred: fn(&Constraint) -> bool| ed.doc.constraints().filter(|(_, c)| pred(c)).count();
    assert_eq!(count(|c| matches!(c, Constraint::Distance { .. })), 4);
    assert_eq!(count(|c| matches!(c, Constraint::Angle { .. })), 2);
    assert_eq!(
        count(|c| matches!(c, Constraint::Horizontal { .. })),
        1,
        "a slanted reflection isn't level"
    );
}
