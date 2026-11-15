//! A sheet's own annotations in the app (#432): the Note and Dimension tools on a sheet tab,
//! and picking, moving, editing and deleting what they make there.

use drift_core::{SheetAnnotation, SheetAnnotationId, SheetId};

use super::*;
use crate::app::sheet_editor::SheetPick;
use crate::sheet_notes::tests::{Plan, plan};

const V: fn(f64, f64) -> DVec2 = DVec2::new;

/// The app showing the plan's sheet: a 1000 mm line in a 1:10 view, the line from (150, 150)
/// to (250, 150) on the paper.
fn on_sheet() -> Option<(Harness<'static, DriftApp>, Plan)> {
    let mut h = harness()?;
    let p = plan();
    h.state_mut().drawing.editor = drift_edit::editor::Editor::new(p.ed.doc.clone());
    h.state_mut().show_sheet(Some(p.sheet));
    h.state_mut().show_panel(Panel::Inspector);
    h.run();
    h.run();
    assert!(h.state().drawing.view.sheet_preview);
    Some((h, p))
}

/// Where a paper point is on screen in the sheet editor.
fn on_paper(h: &Harness<'_, DriftApp>, p: DVec2) -> Pos2 {
    let app = h.state();
    let rect = app.drawing.view.canvas_rect;
    let d = (p - app.drawing.view.preview.center) * f64::from(app.drawing.view.preview.zoom);
    rect.center() + egui::vec2(d.x as f32, -d.y as f32)
}

fn click(h: &mut Harness<'_, DriftApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    button(h, at, true);
    h.run();
    button(h, at, false);
    h.run();
}

/// A click at a paper point of the sheet shown.
fn click_paper(h: &mut Harness<'_, DriftApp>, p: DVec2) {
    let at = on_paper(h, p);
    click(h, at);
}

fn drag(h: &mut Harness<'_, DriftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run();
    button(h, from, true);
    h.run();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run();
    }
    button(h, to, false);
    h.run();
}

fn annotations(
    h: &Harness<'_, DriftApp>,
    sheet: SheetId,
) -> Vec<(SheetAnnotationId, SheetAnnotation)> {
    h.state()
        .drawing
        .editor
        .doc
        .sheet(sheet)
        .expect("the sheet")
        .annotations
        .iter()
        .map(|(id, a)| (*id, a.clone()))
        .collect()
}

fn depth(h: &Harness<'_, DriftApp>) -> usize {
    h.state().drawing.editor.history.undo_depth()
}

/// Where annotation `id` is on screen: the middle of its text.
fn text_of(h: &Harness<'_, DriftApp>, sheet: SheetId, id: SheetAnnotationId) -> Pos2 {
    let app = h.state();
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        app.drawing.view.canvas_rect,
    );
    let doc = &app.drawing.editor.doc;
    let map = app.paper_map(app.drawing.view.canvas_rect);
    let layouts = crate::sheet_notes::layout_sheet_annotations(
        &painter,
        doc,
        doc.sheet(sheet).unwrap(),
        &map,
    );
    let l = layouts.iter().find(|l| l.id == id).expect("laid out");
    match &l.shape {
        crate::sheet_notes::SheetShape::Dimension(d) => d.text_rect.center(),
        crate::sheet_notes::SheetShape::Note(n) => n.text_rect().expect("text").center(),
    }
}

#[test]
fn the_dimension_tool_works_on_a_sheet_and_what_it_makes_is_picked_moved_and_deleted_there() {
    let Some((mut h, p)) = on_sheet() else { return };
    // The rail's Dimension tool stays on the sheet.
    h.get_by_label("Dimension").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Dimension);
    assert!(
        h.state().drawing.view.sheet_preview,
        "it stays on the sheet"
    );
    let before = depth(&h);
    // The line inside the view (off its middle), then under it for the label.
    click_paper(&mut h, V(170.0, 150.0));
    assert!(h.state().tool.is_busy());
    click_paper(&mut h, V(200.0, 135.0));
    let made = annotations(&h, p.sheet);
    assert_eq!(made.len(), 1, "{made:?}");
    let id = made[0].0;
    let SheetAnnotation::Dimension { label, .. } = &made[0].1 else {
        panic!("a dimension: {made:?}");
    };
    let label = label.expect("placed");
    assert!(
        (label.y + 15.0).abs() < 0.5,
        "15 mm under the line: {label}"
    );
    assert_eq!(depth(&h), before + 1, "one undo step");
    // Picked, in the Inspector; the model got nothing; no value editor opened.
    assert_eq!(h.state().picked_sheet_annotation(), Some((p.sheet, id)));
    assert_eq!(h.state().drawing.editor.doc.constraints().count(), 0);
    assert_eq!(h.state().drawing.editor.doc.annotations().count(), 0);
    assert!(h.state().drawing.editor.on_sheet.is_none());
    assert!(h.query_by_label("Sheet dimension").is_some());

    // Esc: back to Select, still on the sheet. Then it is picked by its text and dragged.
    h.key_press(Key::Escape);
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert!(h.state().drawing.view.sheet_preview);
    click_paper(&mut h, V(60.0, 40.0));
    assert_eq!(
        h.state().picked_sheet_annotation(),
        None,
        "the paper is picked"
    );
    let at = text_of(&h, p.sheet, id);
    click(&mut h, at);
    assert_eq!(h.state().picked_sheet_annotation(), Some((p.sheet, id)));
    assert_eq!(h.state().drawing.view.sheet_pick, SheetPick::Annotation);
    let before = depth(&h);
    let zoom = h.state().drawing.view.preview.zoom;
    drag(&mut h, at, at + egui::vec2(0.0, 10.0 * zoom));
    let SheetAnnotation::Dimension { label: moved, .. } = &annotations(&h, p.sheet)[0].1 else {
        panic!("a dimension");
    };
    let moved = moved.expect("placed");
    assert!(
        (moved.y - (label.y - 10.0)).abs() < 0.5,
        "10 mm further down: {moved}"
    );
    assert_eq!(depth(&h), before + 1, "a drag is one undo step");
    // The view it measures through did not move with it.
    let sheet = h.state().drawing.editor.doc.sheet(p.sheet).unwrap().clone();
    assert_eq!(sheet.viewports[0].at, V(200.0, 150.0));

    // Delete deletes it (not the view); undo brings it back; redo takes it again.
    let before = depth(&h);
    h.key_press(Key::Delete);
    h.run();
    assert!(annotations(&h, p.sheet).is_empty());
    assert_eq!(depth(&h), before + 1);
    let sheet = h.state().drawing.editor.doc.sheet(p.sheet).unwrap().clone();
    assert_eq!(sheet.viewports.len(), 1, "the view stays");
    h.state_mut().undo();
    h.run();
    assert_eq!(annotations(&h, p.sheet).len(), 1);
    h.state_mut().redo();
    h.run();
    assert!(annotations(&h, p.sheet).is_empty());
}

#[test]
fn the_note_tool_works_on_a_sheet_and_its_text_is_typed_and_restyled() {
    let Some((mut h, p)) = on_sheet() else { return };
    h.get_by_label("Note").click();
    h.run();
    assert_eq!(h.state().tool.kind(), ToolKind::Note);
    assert!(h.state().drawing.view.sheet_preview);
    // The line inside the view, for a leader; then the paper above it, for the text.
    click_paper(&mut h, V(170.0, 150.0));
    assert!(h.state().tool.is_busy());
    click_paper(&mut h, V(120.0, 230.0));
    let made = annotations(&h, p.sheet);
    assert_eq!(made.len(), 1, "{made:?}");
    let id = made[0].0;
    let SheetAnnotation::Note {
        at, leaders, text, ..
    } = &made[0].1
    else {
        panic!("a note: {made:?}");
    };
    assert!(at.distance(V(120.0, 230.0)) < 0.5, "{at}");
    assert!(text.is_empty());
    assert_eq!(leaders.len(), 1);
    assert_eq!(leaders[0].leader.to.entity(), Some(p.line));
    assert!(leaders[0].port.is_some(), "tied through the view");
    assert_eq!(h.state().drawing.editor.doc.annotations().count(), 0);
    // Its text field has the keyboard: typing goes to the note, as one undo step.
    h.run();
    let before = depth(&h);
    for word in ["SEE ", "DETAIL 3"] {
        h.event(Event::Text(word.into()));
        h.run();
    }
    let text_now = |h: &Harness<'_, DriftApp>| match &annotations(h, p.sheet)[0].1 {
        SheetAnnotation::Note { text, .. } => text.clone(),
        _ => unreachable!(),
    };
    assert_eq!(text_now(&h), "SEE DETAIL 3");
    assert_eq!(depth(&h), before + 1, "typing is one undo step");
    // Restyled in the Inspector: bold.
    h.get_by_label("Bold").click();
    h.run();
    assert!(matches!(
        &annotations(&h, p.sheet)[0].1,
        SheetAnnotation::Note { style, .. } if style.bold
    ));
    assert_eq!(depth(&h), before + 2, "a style change is its own step");
    // Undo: the style, then the text; the note itself is a step of its own before them.
    h.state_mut().undo();
    h.state_mut().undo();
    h.run();
    assert_eq!(text_now(&h), "");
    assert_eq!(annotations(&h, p.sheet)[0].0, id);
    h.state_mut().undo();
    h.run();
    assert!(annotations(&h, p.sheet).is_empty());
}

#[test]
fn a_sheet_note_is_picked_dragged_and_deleted_and_an_empty_one_is_not_left_behind() {
    let Some((mut h, p)) = on_sheet() else { return };
    // A note on the paper alone, with text.
    let id = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let id = ed
            .doc
            .add_sheet_annotation(
                p.sheet,
                SheetAnnotation::note("ISSUED FOR PERMIT", V(100.0, 60.0)),
            )
            .unwrap();
        ed.commit("Note");
        id
    };
    h.run();
    h.run();
    let at = text_of(&h, p.sheet, id);
    click(&mut h, at);
    assert_eq!(h.state().picked_sheet_annotation(), Some((p.sheet, id)));
    assert!(h.query_by_label("Sheet note").is_some());
    let before = depth(&h);
    let zoom = h.state().drawing.view.preview.zoom;
    drag(&mut h, at, at + egui::vec2(20.0 * zoom, 0.0));
    assert!(matches!(
        &annotations(&h, p.sheet)[0].1,
        SheetAnnotation::Note { at, .. } if at.distance(V(120.0, 60.0)) < 0.5
    ));
    assert_eq!(depth(&h), before + 1);
    // The Inspector's Delete.
    h.get_by_label("Delete").click();
    h.run();
    assert!(annotations(&h, p.sheet).is_empty());
    assert_eq!(h.state().drawing.view.sheet_pick, SheetPick::Paper);
    h.state_mut().undo();
    h.run();
    assert_eq!(annotations(&h, p.sheet).len(), 1);

    // A note placed and left without text, then the pick moved off it: it goes.
    h.get_by_label("Note").click();
    h.run();
    click_paper(&mut h, V(60.0, 250.0));
    assert_eq!(annotations(&h, p.sheet).len(), 2);
    // Esc leaves its text field, Esc again puts the tool down, Esc again lets go of it.
    for _ in 0..3 {
        h.key_press(Key::Escape);
        h.run();
    }
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert_eq!(annotations(&h, p.sheet).len(), 1, "the empty note is gone");
    assert!(h.state().drawing.view.sheet_preview, "still on the sheet");
}

#[test]
fn in_the_model_the_tools_are_the_models_and_a_drawing_tool_leaves_the_sheet() {
    let Some((mut h, p)) = on_sheet() else { return };
    // A drawing tool goes back to the model, as it always did.
    h.state_mut().set_tool(ToolKind::Line);
    h.run();
    assert!(!h.state().drawing.view.sheet_preview);
    // The Note tool picked on the sheet, then the Model tab: it is the model's tool there.
    h.state_mut().show_sheet(Some(p.sheet));
    h.run();
    h.state_mut().set_tool(ToolKind::Note);
    h.run();
    assert!(h.state().drawing.view.sheet_preview);
    // A leader started on the sheet is let go when the model is shown.
    click_paper(&mut h, V(170.0, 150.0));
    assert!(h.state().tool.is_busy());
    h.state_mut().show_sheet(None);
    h.run();
    h.run();
    assert!(!h.state().tool.is_busy());
    assert!(h.state().drawing.editor.on_sheet.is_none());
    // A note clicked in the model is the model's: the sheet gets nothing.
    let model_notes = h.state().drawing.editor.doc.annotations().count();
    let rect = h.state().drawing.view.canvas_rect;
    let empty = rect.center() + egui::vec2(0.0, -rect.height() * 0.3);
    click(&mut h, empty);
    h.run();
    assert_eq!(
        h.state().drawing.editor.doc.annotations().count(),
        model_notes + 1
    );
    assert!(annotations(&h, p.sheet).is_empty());
}

/// The sheet's dimensions and the leaders of its notes, counted.
fn tied(h: &Harness<'_, DriftApp>, sheet: SheetId) -> (usize, usize) {
    let all = annotations(h, sheet);
    let dimensions = all
        .iter()
        .filter(|(_, a)| matches!(a, SheetAnnotation::Dimension { .. }))
        .count();
    let leaders = all
        .iter()
        .map(|(_, a)| match a {
            SheetAnnotation::Note { leaders, .. } => leaders.len(),
            SheetAnnotation::Dimension { .. } => 0,
        })
        .sum();
    (dimensions, leaders)
}

#[test]
fn removing_views_deleting_geometry_and_copying_sheets_leave_nothing_dangling() {
    let Some((mut h, mut p)) = on_sheet() else {
        return;
    };
    // A dimension of the line and a note with a leader to it, both through the view.
    p.dimension();
    p.note();
    h.state_mut().drawing.editor = drift_edit::editor::Editor::new(p.ed.doc.clone());
    h.run();
    h.run();
    assert_eq!(tied(&h, p.sheet), (1, 1));
    let sound = |h: &mut Harness<'_, DriftApp>| {
        // Drawn (the sheet is shown), and nothing on any sheet names what isn't there.
        h.run();
        h.run();
        let doc = &h.state().drawing.editor.doc;
        for (_, s) in doc.sheets() {
            for a in s.annotations.values() {
                assert!(
                    a.ports()
                        .iter()
                        .all(|port| doc.port_view(s, *port).is_some())
                );
                assert!(a.entities().iter().all(|e| doc.get(*e).is_some()));
            }
        }
    };
    let whole = h.state().drawing.editor.doc.clone();

    // The view taken off the sheet: the dimension goes, the note keeps its text and loses
    // its leader. One undo brings all of it back; redo takes it again.
    h.state_mut().drawing.view.sheet_pick = SheetPick::View;
    h.state_mut().remove_viewport(p.sheet);
    sound(&mut h);
    assert_eq!(tied(&h, p.sheet), (0, 0));
    assert_eq!(annotations(&h, p.sheet).len(), 1, "the note's text stays");
    h.state_mut().undo();
    sound(&mut h);
    assert_eq!(h.state().drawing.editor.doc, whole);
    h.state_mut().redo();
    sound(&mut h);
    assert_eq!(tied(&h, p.sheet), (0, 0));
    h.state_mut().undo();

    // The view deleted from the drawing: the same, in one step.
    h.state_mut().delete_view(p.view);
    sound(&mut h);
    assert_eq!(tied(&h, p.sheet), (0, 0));
    h.state_mut().undo();
    sound(&mut h);
    assert_eq!(h.state().drawing.editor.doc, whole);

    // The view moved on the sheet and rescaled: everything stays tied, nothing is lost.
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let mut s = ed.doc.sheet(p.sheet).unwrap().clone();
        s.viewports[0].at = V(260.0, 120.0);
        ed.doc.set_sheet(p.sheet, s);
        let mut v = ed.doc.view(p.view).unwrap().clone();
        v.scale = 25.0;
        ed.doc.set_view(p.view, v);
        ed.commit("Move and rescale");
    }
    sound(&mut h);
    assert_eq!(tied(&h, p.sheet), (1, 1));
    h.state_mut().undo();

    // The line deleted in the model: the dimension goes with it and the leader lets go;
    // undo brings them back; redo takes them again.
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        ed.doc.delete([p.line]);
        ed.commit("Delete");
    }
    sound(&mut h);
    assert_eq!(tied(&h, p.sheet), (0, 0));
    h.state_mut().undo();
    sound(&mut h);
    assert_eq!(h.state().drawing.editor.doc, whole);
    h.state_mut().redo();
    sound(&mut h);
    assert_eq!(tied(&h, p.sheet), (0, 0));
    h.state_mut().undo();

    // Duplicate sheet with its views: the copy has its own, tied to its own view.
    let sheets = |h: &Harness<'_, DriftApp>| -> Vec<SheetId> {
        h.state()
            .drawing
            .editor
            .doc
            .sheets()
            .map(|(id, _)| id)
            .collect()
    };
    let before = sheets(&h);
    h.state_mut().duplicate_sheet(p.sheet, true);
    sound(&mut h);
    let copy = h.state().current_sheet().expect("the copy is shown");
    assert_eq!(sheets(&h).len(), before.len() + 1);
    assert_eq!(tied(&h, copy), (1, 1));
    assert_eq!(tied(&h, p.sheet), (1, 1));
    let doc = &h.state().drawing.editor.doc;
    let (first, second) = (doc.sheet(p.sheet).unwrap(), doc.sheet(copy).unwrap());
    assert_ne!(first.viewports[0].view, second.viewports[0].view);
    assert_ne!(first.viewports[0].id, second.viewports[0].id);
    assert!(
        first
            .annotations
            .keys()
            .all(|a| !second.annotations.contains_key(a)),
        "ids of its own"
    );
    h.state_mut().undo();
    sound(&mut h);
    assert_eq!(sheets(&h), before);

    // Duplicate without views: nothing tied comes along; the note's text does.
    h.state_mut().show_sheet(Some(p.sheet));
    h.state_mut().duplicate_sheet(p.sheet, false);
    sound(&mut h);
    let copy = h.state().current_sheet().expect("the copy is shown");
    assert_eq!(tied(&h, copy), (0, 0));
    assert_eq!(annotations(&h, copy).len(), 1);
    assert_eq!(tied(&h, p.sheet), (1, 1));
}

/// A second sheet for the plan: an unrelated view "Elsewhere" at 1:2, placed where the
/// first sheet's view is.
fn sheet_elsewhere(h: &mut Harness<'_, DriftApp>, other: SheetId) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let view = ed.doc.add_view(drift_core::View::new(
        "Elsewhere",
        drift_core::Bounds {
            min: V(5000.0, 5000.0),
            max: V(5400.0, 5300.0),
        },
        2.0,
    ));
    let mut s = ed.doc.sheet(other).unwrap().clone();
    s.viewports
        .push(drift_core::Viewport::new(view, V(200.0, 150.0)));
    ed.doc.set_sheet(other, s);
    ed.commit("Elsewhere");
}

fn all_annotations(h: &Harness<'_, DriftApp>) -> usize {
    let doc = &h.state().drawing.editor.doc;
    doc.sheets().map(|(_, s)| s.annotations.len()).sum()
}

#[test]
fn a_pick_made_on_one_sheet_is_not_finished_on_another() {
    let Some((mut h, p)) = on_sheet() else { return };
    sheet_elsewhere(&mut h, p.other);
    h.run();
    // The Dimension tool: the line picked in Plan on the first sheet, then the other
    // sheet's tab, then a click there.
    h.state_mut().set_tool(ToolKind::Dimension);
    h.run();
    click_paper(&mut h, V(170.0, 150.0));
    assert!(h.state().tool.is_busy());
    h.state_mut().show_sheet(Some(p.other));
    h.run();
    h.run();
    assert!(
        !h.state().tool.is_busy(),
        "the pick is let go with the sheet"
    );
    click_paper(&mut h, V(200.0, 135.0));
    assert_eq!(all_annotations(&h), 0, "nothing is made on either sheet");

    // The Note tool: a leader started on the first sheet, then the other sheet's tab. The
    // click there places a note on the paper, with no leader carried over.
    h.state_mut().show_sheet(Some(p.sheet));
    h.state_mut().set_tool(ToolKind::Note);
    h.run();
    h.run();
    click_paper(&mut h, V(170.0, 150.0));
    assert!(h.state().tool.is_busy());
    h.state_mut().show_sheet(Some(p.other));
    h.run();
    h.run();
    assert!(!h.state().tool.is_busy());
    click_paper(&mut h, V(60.0, 60.0));
    assert!(annotations(&h, p.sheet).is_empty());
    let made = annotations(&h, p.other);
    assert!(
        matches!(&made[..], [(_, SheetAnnotation::Note { leaders, .. })] if leaders.is_empty()),
        "{made:?}"
    );
}

#[test]
fn an_undo_between_a_leaders_click_and_its_text_lets_the_leader_go() {
    let Some((mut h, p)) = on_sheet() else { return };
    // A second view put on the sheet, as one undo step; the leader is picked through it.
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let mut s = ed.doc.sheet(p.sheet).unwrap().clone();
        s.viewports
            .insert(0, drift_core::Viewport::new(p.view, V(200.0, 60.0)));
        ed.doc.set_sheet(p.sheet, s);
        ed.commit("Place view on sheet");
    }
    h.run();
    h.state_mut().set_tool(ToolKind::Note);
    h.run();
    // The line in the new viewport (the first in the list): it is at y = 60 there.
    click_paper(&mut h, V(170.0, 60.0));
    assert!(h.state().tool.is_busy());
    // Undo takes that viewport off again: the one left is now first in the list.
    h.state_mut().undo();
    h.run();
    h.run();
    assert_eq!(
        h.state()
            .drawing
            .editor
            .doc
            .sheet(p.sheet)
            .unwrap()
            .viewports
            .len(),
        1
    );
    assert!(!h.state().tool.is_busy(), "the leader is let go");
    // The text click places a plain note: nothing is tied through the other viewport.
    click_paper(&mut h, V(60.0, 250.0));
    let made = annotations(&h, p.sheet);
    assert!(
        matches!(&made[..], [(_, SheetAnnotation::Note { leaders, .. })] if leaders.is_empty()),
        "{made:?}"
    );

    // A Dimension tool's pick whose viewport is moved (by a change that is not an undo)
    // before the label is placed: let go the same way, and nothing is made.
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().set_tool(ToolKind::Dimension);
    h.run();
    click_paper(&mut h, V(170.0, 150.0));
    assert!(h.state().tool.is_busy());
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let mut s = ed.doc.sheet(p.sheet).unwrap().clone();
        s.viewports[0].at = V(260.0, 150.0);
        ed.doc.set_sheet(p.sheet, s);
        ed.commit("Move view on sheet");
    }
    click_paper(&mut h, V(200.0, 135.0));
    assert!(!h.state().tool.is_busy());
    // (The note left empty above went with the first click; no dimension was made.)
    assert_eq!(all_annotations(&h), 0);
}

#[test]
fn undo_during_a_drag_puts_the_note_back_and_keeps_the_history() {
    let Some((mut h, p)) = on_sheet() else { return };
    // One step in the history: a note added.
    let id = {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let id = ed
            .doc
            .add_sheet_annotation(
                p.sheet,
                SheetAnnotation::note("ISSUED FOR PERMIT", V(100.0, 60.0)),
            )
            .unwrap();
        ed.commit("Note");
        id
    };
    h.run();
    h.run();
    let before = depth(&h);
    let at = text_of(&h, p.sheet, id);
    // Press on it, move 30 points, and undo with the button still down.
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    for k in 1..=3 {
        h.hover_at(at + egui::vec2(10.0 * k as f32, 0.0));
        h.run();
    }
    assert!(h.state().drawing.view.sheet_anno_drag.is_some());
    h.state_mut().undo();
    assert!(
        h.state().drawing.view.sheet_anno_drag.is_none(),
        "the drag is given up"
    );
    assert!(!h.state().drawing.editor.doc.in_transaction());
    // The undo took the step before the drag (the note), whole; the drag left no step.
    assert_eq!(depth(&h), before - 1);
    assert!(annotations(&h, p.sheet).is_empty());
    // Moving on and letting go changes nothing more.
    h.hover_at(at + egui::vec2(60.0, 0.0));
    h.run();
    button(&h, at + egui::vec2(60.0, 0.0), false);
    h.run();
    assert_eq!(depth(&h), before - 1);
    assert!(annotations(&h, p.sheet).is_empty());
    // Redo is still there, and brings the note back where it was.
    h.state_mut().redo();
    h.run();
    assert!(matches!(
        &annotations(&h, p.sheet)[..],
        [(_, SheetAnnotation::Note { at, .. })] if *at == V(100.0, 60.0)
    ));
    assert_eq!(depth(&h), before);

    // Redo during a drag: the same. (Undo, then drag the note... there is none to drag, so
    // drag it now and redo with nothing to redo: the drag is given up, nothing changes.)
    let at = text_of(&h, p.sheet, id);
    h.hover_at(at);
    h.run();
    button(&h, at, true);
    h.run();
    h.hover_at(at + egui::vec2(25.0, 0.0));
    h.run();
    h.hover_at(at + egui::vec2(40.0, 0.0));
    h.run();
    assert!(h.state().drawing.view.sheet_anno_drag.is_some());
    h.state_mut().redo();
    assert!(h.state().drawing.view.sheet_anno_drag.is_none());
    assert!(!h.state().drawing.editor.doc.in_transaction());
    button(&h, at + egui::vec2(40.0, 0.0), false);
    h.run();
    assert_eq!(depth(&h), before);
    assert!(matches!(
        &annotations(&h, p.sheet)[..],
        [(_, SheetAnnotation::Note { at, .. })] if *at == V(100.0, 60.0)
    ));
}

#[test]
fn an_empty_note_is_not_brought_back_by_undo() {
    let Some((mut h, p)) = on_sheet() else { return };
    h.get_by_label("Note").click();
    h.run();
    click_paper(&mut h, V(60.0, 250.0));
    assert_eq!(annotations(&h, p.sheet).len(), 1);
    // Nothing typed; Esc out of the field, the tool and the pick.
    for _ in 0..3 {
        h.key_press(Key::Escape);
        h.run();
    }
    assert!(annotations(&h, p.sheet).is_empty());
    // Undo, as often as there is anything to undo, never shows an empty note again; nor
    // does redo.
    for _ in 0..3 {
        h.state_mut().undo();
        h.run();
        assert!(annotations(&h, p.sheet).is_empty());
    }
    for _ in 0..3 {
        h.state_mut().redo();
        h.run();
        assert!(annotations(&h, p.sheet).is_empty());
    }
    // And none is saved.
    let text = drift_io::save(
        &h.state().drawing.editor.doc,
        &drift_io::Metadata::default(),
    );
    assert!(!text.contains("\"annotations\""), "{text}");
}

#[test]
fn two_notes_placed_without_typing_leave_no_empty_note_for_undo() {
    let Some((mut h, p)) = on_sheet() else { return };
    let empty = |h: &Harness<'_, DriftApp>| {
        annotations(h, p.sheet)
            .iter()
            .filter(|(_, a)| matches!(a, SheetAnnotation::Note { text, .. } if text.is_empty()))
            .count()
    };
    h.state_mut().set_tool(ToolKind::Note);
    h.run();
    // Note A, nothing typed; then note B: the pick moves, and A goes.
    click_paper(&mut h, V(60.0, 250.0));
    let a = annotations(&h, p.sheet)[0].0;
    click_paper(&mut h, V(60.0, 60.0));
    h.run();
    let now = annotations(&h, p.sheet);
    assert_eq!(now.len(), 1, "{now:?}");
    assert_ne!(now[0].0, a, "the one left is B");
    // Undo takes B back and does not bring A back; further undos and redos never show A.
    h.state_mut().undo();
    h.run();
    assert!(
        annotations(&h, p.sheet).is_empty(),
        "{:?}",
        annotations(&h, p.sheet)
    );
    for _ in 0..3 {
        h.state_mut().undo();
        h.run();
        assert!(annotations(&h, p.sheet).iter().all(|(id, _)| *id != a));
    }
    for _ in 0..4 {
        h.state_mut().redo();
        h.run();
        assert!(annotations(&h, p.sheet).iter().all(|(id, _)| *id != a));
    }
    // A note with text placed before an empty one is not touched by the empty one going.
    for _ in 0..4 {
        h.state_mut().undo();
    }
    h.run();
    click_paper(&mut h, V(60.0, 250.0));
    h.run();
    h.event(Event::Text("KEEP".into()));
    h.run();
    click_paper(&mut h, V(60.0, 60.0));
    click_paper(&mut h, V(120.0, 60.0));
    h.run();
    let texts: Vec<String> = annotations(&h, p.sheet)
        .iter()
        .map(|(_, a)| match a {
            SheetAnnotation::Note { text, .. } => text.clone(),
            SheetAnnotation::Dimension { .. } => String::new(),
        })
        .collect();
    assert_eq!(
        texts.iter().filter(|t| *t == "KEEP").count(),
        1,
        "{texts:?}"
    );
    assert_eq!(
        empty(&h),
        1,
        "only the note just placed is empty: {texts:?}"
    );
    h.state_mut().undo();
    h.run();
    assert_eq!(empty(&h), 0, "undo brings no empty note back");
    assert_eq!(annotations(&h, p.sheet).len(), 1);
}
