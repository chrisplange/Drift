//! Clipboard commands and nudging, as the app sees them: keys, menus and status messages.

use crate::*;

impl DriftApp {
    /// Copy the selection to the clipboard (and remember it, for Edit → Paste).
    pub(crate) fn copy(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match self.editor.copy_selection() {
            Some(text) => {
                ctx.copy_text(text.clone());
                self.clipboard = Some(text);
                self.set_status("Copied".into(), false, now);
            }
            None => self.set_status("Select something to copy first".into(), false, now),
        }
    }

    pub(crate) fn cut(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match self.editor.cut_selection() {
            Some(text) => {
                ctx.copy_text(text.clone());
                self.clipboard = Some(text);
                self.set_status("Cut".into(), false, now);
            }
            None => self.set_status("Select something to cut first".into(), false, now),
        }
    }

    /// Where a paste lands: under the pointer when it's over the canvas, else mid-view.
    fn paste_point(&self) -> DVec2 {
        self.pointer.map_or(self.camera.center, |p| p.raw)
    }

    /// Paste clipboard text: copied geometry (under the pointer, or where it came from for
    /// `in_place`), or any other text as a line of sketch text.
    pub(crate) fn paste(&mut self, text: &str, in_place: bool, now: f64) {
        if text.trim().is_empty() {
            return;
        }
        self.abort_operation();
        if self.tool.kind() != ToolKind::Select {
            self.set_tool(ToolKind::Select);
        }
        let at = self.paste_point();
        if clipboard::is_fragment(text) {
            match self.editor.paste(text, (!in_place).then_some(at)) {
                Ok(n) => self.set_status(format!("Pasted {n}"), false, now),
                Err(e) => self.set_status(format!("Can't paste that: {e}"), true, now),
            }
            return;
        }
        // Plain text becomes sketch text: its first line, at the usual height.
        let line: String = text
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .chars()
            .take(texts::MAX_TEXT_CHARS)
            .collect();
        if line.is_empty() {
            return;
        }
        let height = if self.editor.text_height > 0.0 {
            self.editor.text_height
        } else {
            10.0
        };
        self.editor.selection.clear();
        self.editor
            .add_text(drift_core::SketchText::new(line, at, height));
        self.set_status("Pasted as text".into(), false, now);
    }

    /// Edit → Paste: what was last copied here (menus can't read the system clipboard).
    pub(crate) fn paste_copied(&mut self, in_place: bool, now: f64) {
        match self.clipboard.clone() {
            Some(text) => self.paste(&text, in_place, now),
            None => self.set_status(
                format!(
                    "Nothing copied yet ({} pastes from other windows)",
                    app::panels::shortcut("V", false)
                ),
                false,
                now,
            ),
        }
    }

    /// Clipboard keys and arrow-key nudging (call when no text field has the keyboard).
    pub(crate) fn edit_keys(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let (copy, cut, paste, shift) = ctx.input(|i| {
            let mut paste = None;
            let (mut copy, mut cut) = (false, false);
            for e in &i.events {
                match e {
                    egui::Event::Copy => copy = true,
                    egui::Event::Cut => cut = true,
                    egui::Event::Paste(text) => paste = Some(text.clone()),
                    _ => {}
                }
            }
            (copy, cut, paste, i.modifiers.shift)
        });
        if copy {
            self.copy(ctx);
        }
        if cut {
            self.cut(ctx);
        }
        if let Some(text) = paste {
            self.paste(&text, shift, now);
        }
        // Arrows nudge the selection by a grid step (ten with Shift), with the select tool.
        if self.tool.kind() != ToolKind::Select || self.tool.is_busy() || self.sheet_preview {
            return;
        }
        let step = grid_info(&self.camera, self.imperial()).minor;
        let mut delta = DVec2::ZERO;
        ctx.input(|i| {
            if i.modifiers.command || i.modifiers.alt {
                return;
            }
            for (key, dir) in [
                (Key::ArrowLeft, DVec2::NEG_X),
                (Key::ArrowRight, DVec2::X),
                (Key::ArrowUp, DVec2::Y),
                (Key::ArrowDown, DVec2::NEG_Y),
            ] {
                delta += dir * i.num_presses(key) as f64;
            }
            if i.modifiers.shift {
                delta *= 10.0;
            }
        });
        if delta != DVec2::ZERO && self.editor.nudge(delta * step) == Some(false) {
            self.set_status(
                "That can't move (it's held by its relations)".into(),
                false,
                now,
            );
        }
    }
}
