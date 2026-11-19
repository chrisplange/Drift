//! Notes typed live on the canvas, as in a design app (#431): the caret and the selection on
//! the note itself, Return for a new line, Esc or a click away to finish keeping the text.

use drift_core::{Annotation, AnnotationId, NoteStyle};

use super::*;

fn painter(h: &Harness<'_, DriftApp>) -> egui::Painter {
    egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::EVERYTHING,
    )
}

/// The note's text as it is laid out on the canvas now.
fn note_text(h: &Harness<'_, DriftApp>, id: AnnotationId) -> crate::notes::NoteText {
    let map = h.state().canvas_map();
    h.state()
        .layout_notes(&painter(h), &map)
        .into_iter()
        .find(|l| l.id == id)
        .and_then(|l| l.text)
        .expect("the note is laid out")
}

/// On the note's text, just after the boundary before character `k`.
fn at_char(h: &Harness<'_, DriftApp>, id: AnnotationId, k: usize) -> Pos2 {
    let [foot, head] = note_text(h, id).caret_map(&painter(h)).caret(k);
    foot.lerp(head, 0.5) + egui::vec2(1.0, 0.0)
}

fn notes(h: &Harness<'_, DriftApp>) -> Vec<(AnnotationId, String, NoteStyle)> {
    h.state()
        .drawing
        .editor
        .doc
        .annotations()
        .filter_map(|(id, a)| match a {
            Annotation::Note { text, style, .. } => Some((id, text.clone(), *style)),
            _ => None,
        })
        .collect()
}

fn words(h: &Harness<'_, DriftApp>, id: AnnotationId) -> String {
    notes(h)
        .into_iter()
        .find(|n| n.0 == id)
        .map(|n| n.1)
        .expect("the note")
}

fn cursor(h: &Harness<'_, DriftApp>) -> Option<(usize, usize)> {
    h.state()
        .drawing
        .note_edit
        .as_ref()
        .and_then(|e| e.cursor())
}

fn depth(h: &Harness<'_, DriftApp>) -> usize {
    h.state().drawing.editor.history.undo_depth()
}

/// `n` clicks in a row at `at`, quick enough to count as a double or triple click.
fn clicks(h: &mut Harness<'_, DriftApp>, at: Pos2, n: usize) {
    h.hover_at(at);
    h.run();
    // (The harness's frames are a quarter of a second apart: allow that between clicks,
    // and leave longer than that since any click before.)
    h.ctx
        .options_mut(|o| o.input_options.max_double_click_delay = 0.6);
    for _ in 0..4 {
        h.step();
    }
    for _ in 0..n {
        button(h, at, true);
        h.step();
        button(h, at, false);
        h.step();
    }
    h.run();
}

/// A note placed with the Note tool on empty canvas, typing at once.
fn place_note(h: &mut Harness<'_, DriftApp>) -> AnnotationId {
    h.state_mut().set_tool(ToolKind::Note);
    h.run();
    let had: Vec<AnnotationId> = notes(h).iter().map(|n| n.0).collect();
    let at = screen(h, DVec2::new(-40.0, 20.0));
    clicks(h, at, 1);
    let new: Vec<_> = notes(h)
        .into_iter()
        .filter(|n| !had.contains(&n.0))
        .collect();
    let [(id, text, _)] = new.as_slice() else {
        panic!("one note placed: {new:?}");
    };
    assert!(text.is_empty());
    assert!(
        h.state().drawing.note_edit.is_some(),
        "placing a note starts typing at once"
    );
    *id
}

#[test]
fn a_note_is_typed_straight_onto_the_canvas() {
    let Some(mut h) = harness() else { return };
    let before = depth(&h);
    let id = place_note(&mut h);
    let width = note_text(&h, id).rect.width();

    // Typing goes into the note as it is typed: its frame grows with it.
    h.event(Event::Text("SEE DETAIL".into()));
    h.run();
    assert_eq!(words(&h, id), "SEE DETAIL", "straight into the note");
    assert!(
        note_text(&h, id).rect.width() > width + 20.0,
        "the note's frame follows the text"
    );
    assert_eq!(cursor(&h), Some((10, 10)));
    // Return adds a line.
    h.key_press(Key::Enter);
    h.run();
    h.event(Event::Text("A-101".into()));
    h.run();
    assert_eq!(words(&h, id), "SEE DETAIL\nA-101");
    assert!(
        h.state().drawing.note_edit.is_some(),
        "Return doesn't finish"
    );
    assert_eq!(
        depth(&h),
        before + 1,
        "placing and typing are one undo step"
    );

    // A click on the text puts the caret at the character clicked.
    {
        let at = at_char(&h, id, 4);
        clicks(&mut h, at, 1);
    }
    assert_eq!(cursor(&h), Some((4, 4)), "the caret where the click was");
    assert_eq!(
        words(&h, id),
        "SEE DETAIL\nA-101",
        "the click typed nothing"
    );
    // Down, End, Home and Up go by the lines as set.
    h.key_press(Key::ArrowDown);
    h.run();
    let (down, _) = cursor(&h).unwrap();
    assert!((11..=16).contains(&down), "on the second line: {down}");
    h.key_press(Key::End);
    h.run();
    assert_eq!(cursor(&h), Some((16, 16)));
    h.key_press(Key::Home);
    h.run();
    assert_eq!(cursor(&h), Some((11, 11)));
    h.key_press(Key::ArrowUp);
    h.run();
    let (up, _) = cursor(&h).unwrap();
    assert!(up <= 10, "on the first line: {up}");
    save_png("drift_ui_note_caret.png", &mut h);

    // A double click selects a word, a triple click a line, Ctrl+A everything.
    {
        let at = at_char(&h, id, 6);
        clicks(&mut h, at, 2);
    }
    assert_eq!(cursor(&h), Some((10, 4)), "DETAIL");
    {
        let at = at_char(&h, id, 13);
        clicks(&mut h, at, 3);
    }
    assert_eq!(cursor(&h), Some((16, 11)), "the second line");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::A);
    h.run();
    let (a, b) = cursor(&h).unwrap();
    assert_eq!((a.min(b), a.max(b)), (0, 16), "all of it");
    save_png("drift_ui_note_selection.png", &mut h);

    // Ctrl+B and Ctrl+I switch the whole note's bold and italic, a step each.
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::B);
    h.run();
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::I);
    h.run();
    let style = notes(&h)[0].2;
    assert!(style.bold && style.italic, "{style:?}");
    assert_eq!(depth(&h), before + 3);
    assert_eq!(words(&h, id), "SEE DETAIL\nA-101", "the keys typed nothing");
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::B);
    h.run();
    assert!(!notes(&h)[0].2.bold, "Ctrl+B again: not bold");

    // Esc finishes and keeps the text.
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert!(h.state().drawing.note_edit.is_none());
    assert_eq!(words(&h, id), "SEE DETAIL\nA-101", "Esc keeps the text");

    // Ctrl+Z undoes it: the styles, then the note and its words together.
    for _ in 0..3 {
        h.state_mut().undo();
    }
    h.run();
    assert_eq!(notes(&h)[0].2, NoteStyle::default());
    assert_eq!(words(&h, id), "SEE DETAIL\nA-101");
    h.state_mut().undo();
    h.run();
    assert!(notes(&h).is_empty());
    assert_eq!(depth(&h), before);
}

#[test]
fn a_note_is_typed_into_again_from_a_double_click_and_a_click_away_finishes() {
    let Some(mut h) = harness() else { return };
    let id = place_note(&mut h);
    h.event(Event::Text("TYP.".into()));
    h.run();
    h.key_press(Key::Escape);
    h.run();
    h.state_mut().set_tool(ToolKind::Select);
    h.run();
    assert!(h.state().drawing.note_edit.is_none());
    let before = depth(&h);

    // A double click on the note types into it, the caret where it was clicked.
    {
        let at = at_char(&h, id, 3);
        clicks(&mut h, at, 2);
    }
    assert!(
        h.state().drawing.note_edit.is_some(),
        "typing into the note"
    );
    assert_eq!(cursor(&h), Some((3, 3)));
    h.event(Event::Text(" 4".into()));
    h.run();
    assert_eq!(words(&h, id), "TYP 4.");
    // A click away finishes, keeping the text. (On empty canvas, clear of the note: with the
    // slim rail the canvas's middle is further left, and the point this used fell on the
    // status bar's controls.)
    {
        let at = screen(&h, DVec2::new(60.0, -60.0));
        clicks(&mut h, at, 1);
    }
    assert!(h.state().drawing.note_edit.is_none());
    assert_eq!(words(&h, id), "TYP 4.");
    assert_eq!(depth(&h), before + 1, "the typing is one undo step");
    h.state_mut().undo();
    h.run();
    assert_eq!(words(&h, id), "TYP.");
}

#[test]
fn a_new_note_left_empty_goes_with_the_step_that_placed_it() {
    let Some(mut h) = harness() else { return };
    let before = depth(&h);
    place_note(&mut h);
    assert_eq!(depth(&h), before + 1);
    h.key_press(Key::Escape);
    h.run();
    h.run();
    assert!(h.state().drawing.note_edit.is_none());
    assert!(notes(&h).is_empty(), "the empty note is gone");
    assert_eq!(depth(&h), before, "and so is the step that placed it");

    // Typed into and emptied again: the same.
    place_note(&mut h);
    h.event(Event::Text("X".into()));
    h.run();
    h.key_press(Key::Backspace);
    h.run();
    h.key_press(Key::Escape);
    h.run();
    assert!(notes(&h).is_empty());
    assert_eq!(depth(&h), before);
}
