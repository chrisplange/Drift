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
