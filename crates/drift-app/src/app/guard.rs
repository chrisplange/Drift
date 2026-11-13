//! The last-resort guard around the drawing's interactive steps (#440): what the tool in hand
//! does with the pointer and the keys, and a command (from the palette, a menu or a key).
//! A bug there (a panic) must not close the window and lose the drawing. The step is ended,
//! the drawing put back exactly as it was before it, the tool put down, and the status line
//! says what went wrong. The panic is in `crash.log`, as every caught one is.
//!
//! The guard is `drift_platform::crash::catch`, as for imports and the assistant's calls
//! (#216). In the browser a panic aborts, so there it guards nothing.

use drift_edit::editor::Editor;
use drift_edit::tools::{Input, Tool, ToolKey, ToolKind};

use crate::DriftApp;
use crate::commands::{self, Command};

impl DriftApp {
    /// Let the tool in hand do `f` with the drawing's editor, guarded. `None` if it failed.
    pub(crate) fn with_tool<T>(
        &mut self,
        f: impl FnOnce(&mut dyn Tool, &mut Editor) -> T,
    ) -> Option<T> {
        let what = self.tool.kind().label();
        self.guarded(what, |app| f(app.tool.as_mut(), &mut app.drawing.editor))
    }

    /// Hand `input` to the tool in hand, guarded.
    pub(crate) fn tool_input(&mut self, input: Input) {
        self.with_tool(|tool, ed| tool.handle(input, ed));
    }

    /// Abandon what the tool in hand is doing, guarded. False if there was nothing to cancel.
    /// (If cancelling failed, the tool is put down: there was something.)
    pub(crate) fn tool_cancel(&mut self) -> bool {
        self.with_tool(|tool, ed| tool.cancel(ed)).unwrap_or(true)
    }

    /// Hand a modal key to the tool in hand, guarded, with the pointer where it is now (if
    /// over the canvas). False if the tool doesn't use the key, or it failed.
    pub(crate) fn tool_modal_key(&mut self, key: ToolKey) -> bool {
        let pointer = self.drawing.view.pointer;
        self.tool_modal_key_at(key, pointer)
    }

    /// [`DriftApp::tool_modal_key`] with the pointer at `pointer`.
    pub(crate) fn tool_modal_key_at(
        &mut self,
        key: ToolKey,
        pointer: Option<drift_edit::editor::Pointer>,
    ) -> bool {
        self.with_tool(|tool, ed| tool.key(key, pointer.as_ref(), ed))
            .unwrap_or(false)
    }

    /// What the status line calls `command` when it fails.
    pub(crate) fn command_name(command: Command) -> String {
        match command {
            Command::Tool(kind) => kind.label().to_owned(),
            _ => commands::spec(command).map_or_else(
                || "that command".to_owned(),
                |s| s.title.trim_end_matches('…').to_owned(),
            ),
        }
    }

    /// Do `step`. If it panics: what it had begun is rolled back and the steps it had
    /// recorded taken off the undo history (unrecorded, so Redo can't bring them back), so the
    /// drawing and its history are as before it; the tool in hand is put down (the select
    /// tool takes over); and the status line says `what` went wrong. `None` then.
    pub(crate) fn guarded<T>(
        &mut self,
        what: &str,
        step: impl FnOnce(&mut Self) -> T,
    ) -> Option<T> {
        let recorded = self.drawing.editor.history.recorded();
        match drift_platform::crash::catch(|| step(self)) {
            Ok(out) => Some(out),
            Err(_) => {
                self.recover_from_panic(what, recorded);
                None
            }
        }
    }

    /// A step that began with `recorded` steps in the history panicked: undo all of it.
    fn recover_from_panic(&mut self, what: &str, recorded: u64) {
        let ed = &mut self.drawing.editor;
        if ed.doc.in_transaction() {
            ed.doc.rollback();
        }
        // Steps it had committed before the panic: undone and forgotten. (Steps squashed
        // together count as one, so this is how many it added.)
        let added = ed.history.recorded().saturating_sub(recorded);
        if added > 0 {
            ed.history.discard_last(added as usize, &mut ed.doc);
        }
        // What it had to say is moot now.
        ed.out.notice = None;
        ed.out.info = None;
        ed.out.fill_changes.clear();
        ed.highlight.clear();
        ed.preview_hidden.clear();
        ed.chain_preview = None;
        ed.region_motion = None;
        ed.hovered = None;
        ed.reach = None;
        // The tool is put down without asking it to cancel: it is in a state nobody
        // planned for, and the drawing is already back.
        self.tool = ToolKind::Select.create();
        self.drawing.typing = None;
        self.drawing.inference_points.clear();
        self.drawing.editor.sync();
        self.status_error_next = Some(format!(
            "Something went wrong in {what}, so it was stopped; your drawing is unchanged \
             (the details are in Drift's crash log)"
        ));
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: make this command panic part way through an edit, as a bug in it would.
    pub(crate) static PANIC_IN_COMMAND: std::cell::Cell<Option<Command>> =
        const { std::cell::Cell::new(None) };
}
