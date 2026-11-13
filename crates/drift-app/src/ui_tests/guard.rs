//! The last-resort guard (#440): a bug (a panic) in a tool's pointer handling, a command, or
//! the reader of a `.drift` file being opened stops that step and leaves the drawing, its
//! undo history and the window as they were.

use super::*;
use drift_edit::editor::Editor;
use drift_edit::tools::{Input, Tool};

/// A tool with a bug: a click makes one step, begins another and panics part way through.
struct PanickingTool;

impl Tool for PanickingTool {
    fn kind(&self) -> ToolKind {
        ToolKind::Line
    }

    fn hint(&self) -> String {
        String::new()
    }

    fn handle(&mut self, input: Input, ed: &mut Editor) {
        if let Input::Click(p, _) = input {
            ed.doc.begin();
            ed.doc.add_point(p.pos());
            ed.commit("Point");
            ed.doc.begin();
            ed.doc.add_point(p.pos() + DVec2::new(5.0, 5.0));
            panic!("a tool's bug (injected)");
        }
    }

    fn cancel(&mut self, _ed: &mut Editor) -> bool {
        false
    }

    fn is_busy(&self) -> bool {
        false
    }
}

/// A drawing with a step to undo: a line.
fn drawn(h: &mut Harness<'_, DriftApp>) {
    let ed = &mut h.state_mut().drawing.editor;
    ed.doc.begin();
    let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
    let b = ed.doc.add_point(DVec2::new(40.0, 10.0));
    ed.doc.add_line(a, b).unwrap();
    ed.commit("Line");
    h.run();
}

fn status(h: &Harness<'_, DriftApp>) -> (String, bool) {
    h.state()
        .status
        .as_ref()
        .map(|s| (s.text.clone(), s.error))
        .unwrap_or_default()
}

/// The drawing is as `before`, with the one step of [`drawn`] still to undo and none to
/// redo; undoing it empties the drawing.
fn unchanged(h: &mut Harness<'_, DriftApp>, before: &drift_core::Document, recorded: u64) {
    let ed = &h.state().drawing.editor;
    assert!(!ed.doc.in_transaction());
    assert!(ed.doc == *before, "the drawing is as it was");
    assert_eq!(ed.history.recorded(), recorded);
    assert_eq!(ed.history.undo_depth(), 1);
    assert_eq!(ed.history.undo_label(), Some("Line"));
    assert!(!ed.history.can_redo());
    h.key_press_modifiers(EguiModifiers::COMMAND, Key::Z);
    h.run_steps(2);
    assert!(h.state().drawing.editor.doc.is_empty());
}

#[test]
fn a_tool_that_panics_on_a_click_is_put_down_and_the_drawing_kept() {
    let Some(mut h) = harness() else { return };
    drawn(&mut h);
    let before = h.state().drawing.editor.doc.clone();
    let recorded = h.state().drawing.editor.history.recorded();
    h.state_mut().tool = Box::new(PanickingTool);

    click_world(&mut h, DVec2::new(20.0, 20.0));
    h.run_steps(2);

    assert_eq!(
        h.state().tool.kind(),
        ToolKind::Select,
        "the tool is put down"
    );
    let (text, error) = status(&h);
    assert!(
        error && text.starts_with("Something went wrong in Line"),
        "{text}"
    );
    assert!(text.contains("your drawing is unchanged"), "{text}");
    unchanged(&mut h, &before, recorded);
}

#[test]
fn a_command_that_panics_is_stopped_and_the_drawing_kept() {
    let Some(mut h) = harness() else { return };
    drawn(&mut h);
    let before = h.state().drawing.editor.doc.clone();
    let recorded = h.state().drawing.editor.history.recorded();
    let command = crate::commands::Command::SelectAll;
    crate::app::guard::PANIC_IN_COMMAND.set(Some(command));

    let ctx = h.ctx.clone();
    h.state_mut().run_command(&ctx, command);
    crate::app::guard::PANIC_IN_COMMAND.set(None);
    h.run_steps(2);

    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    let (text, error) = status(&h);
    assert!(
        error && text.starts_with("Something went wrong in Select all"),
        "{text}"
    );
    unchanged(&mut h, &before, recorded);
}

#[test]
fn a_drift_file_whose_reader_panics_says_so_and_keeps_the_drawing() {
    let Some(mut h) = harness() else { return };
    drawn(&mut h);
    let before = h.state().drawing.editor.doc.clone();
    let recorded = h.state().drawing.editor.history.recorded();
    let name = h.state().drawing.file.name.clone();
    let other = h.state().new_document();
    let text = drift_io::save(&other, &drift_io::Metadata::default());
    crate::app::document::PANIC_IN_LOADER.set(true);

    h.state().files.deliver(drift_platform::FileEvent::Opened {
        name: "Other.drift".into(),
        path: None,
        contents: text.into_bytes(),
    });
    h.run_steps(2);
    crate::app::document::PANIC_IN_LOADER.set(false);

    assert_eq!(h.state().drawing.file.name, name);
    assert!(h.state().pending.is_none());
    let (text, error) = status(&h);
    assert!(
        error && text.starts_with("Could not open Other.drift: reading it hit a bug"),
        "{text}"
    );
    unchanged(&mut h, &before, recorded);
}

/// The pointer handling that isn't a click goes through the guard too: a tool that panics
/// on hover is put down as soon as the pointer is over the canvas.
#[test]
fn a_tool_that_panics_on_hover_is_put_down() {
    struct HoverBug;
    impl Tool for HoverBug {
        fn kind(&self) -> ToolKind {
            ToolKind::Circle
        }
        fn hint(&self) -> String {
            String::new()
        }
        fn handle(&mut self, input: Input, _ed: &mut Editor) {
            if matches!(input, Input::Hover(_)) {
                panic!("a hover bug (injected)");
            }
        }
        fn cancel(&mut self, _ed: &mut Editor) -> bool {
            false
        }
        fn is_busy(&self) -> bool {
            false
        }
    }
    let Some(mut h) = harness() else { return };
    h.state_mut().tool = Box::new(HoverBug);
    let pos = screen(&h, DVec2::new(5.0, 5.0));
    h.hover_at(pos);
    h.run_steps(2);
    assert_eq!(h.state().tool.kind(), ToolKind::Select);
    assert!(status(&h).0.starts_with("Something went wrong in Circle"));
}

/// A bug while a dimension's label is dragged on the canvas: the drag stops, and the
/// dimension, the drawing and its undo history are as before it.
#[test]
fn a_dimension_drag_that_panics_is_stopped_and_the_drawing_kept() {
    let Some(mut h) = harness() else { return };
    h.state_mut().grid_snap = false;
    {
        let ed = &mut h.state_mut().drawing.editor;
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(0.0, 0.0));
        let b = ed.doc.add_point(DVec2::new(40.0, 0.0));
        ed.doc.add_line(a, b).unwrap();
        let id = ed
            .doc
            .add_constraint(drift_core::Constraint::Distance { a, b, value: 40.0 })
            .unwrap();
        ed.doc.set_placement(id, DVec2::new(0.0, 8.0)).unwrap();
        ed.commit("Dimension");
    }
    h.state_mut().drawing.view.fit_target = Some(drift_core::Bounds {
        min: DVec2::new(-10.0, -20.0),
        max: DVec2::new(50.0, 30.0),
    });
    h.run();
    h.state_mut().drawing.view.fit_target = None;
    h.run();
    let before = h.state().drawing.editor.doc.clone();
    let recorded = h.state().drawing.editor.history.recorded();
    let from = screen_map(&h).to_world(first_dimension(&h).handle);
    crate::app::guard::PANIC_IN_DIMENSION_DRAG.set(true);

    drag_world(&mut h, from, from + DVec2::new(0.0, 6.0));
    crate::app::guard::PANIC_IN_DIMENSION_DRAG.set(false);
    h.run_steps(2);

    let app = h.state();
    assert!(app.drawing.label_drag.is_none(), "the drag is cleared");
    assert!(app.drawing.dim_guide.is_none());
    let ed = &app.drawing.editor;
    assert!(!ed.doc.in_transaction());
    assert!(ed.doc == before, "the drawing is as it was");
    assert_eq!(ed.history.recorded(), recorded);
    assert_eq!(ed.history.undo_label(), Some("Dimension"));
    let (text, error) = status(&h);
    assert!(
        error && text.starts_with("Something went wrong in moving the dimension"),
        "{text}"
    );
    assert!(text.contains("your drawing is unchanged"), "{text}");
}
