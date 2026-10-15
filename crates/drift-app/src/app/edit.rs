//! Clipboard commands and nudging, as the app sees them: keys, menus and status messages.

use crate::*;

impl DriftApp {
    /// Alt+C: a drawing tool's line type (geometry or construction), or else the picked
    /// curves'.
    pub(crate) fn toggle_construction(&mut self, now: f64) {
        if self.tool.kind().draws_curves() {
            let on = !self.editor.draw_construction;
            self.editor.draw_construction = on;
            let text = if on {
                "Drawing construction geometry · Alt+C for geometry"
            } else {
                "Drawing geometry · Alt+C for construction"
            };
            self.set_status(text.into(), false, now);
        } else {
            self.editor.toggle_construction();
        }
    }

    /// Clear the relations (or, with `dimensions`, the dimensions) on the selection, as one
    /// undo step, and say what went.
    pub(crate) fn clear_selection_constraints(&mut self, dimensions: bool, now: f64) {
        let (relations, dims) = self.editor.selection_constraints();
        let ids = if dimensions { dims } else { relations };
        if ids.is_empty() {
            return;
        }
        let what = crate::constraint_summary(&self.editor.doc, &ids);
        let (label, kind) = if dimensions {
            ("Clear dimensions", "dimension")
        } else {
            ("Clear relations", "relation")
        };
        let n = self.editor.clear_constraints(&ids, label);
        if n > 0 {
            self.selected_constraint = None;
            let plural = if n == 1 { "" } else { "s" };
            self.set_status(
                format!("Removed {n} {kind}{plural} ({what}) · Ctrl+Z to undo"),
                false,
                now,
            );
        }
    }

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
                Ok(n) => {
                    self.set_status(format!("Pasted {n}"), false, now);
                    // Straight into a move, to put the copy where it belongs (as after
                    // duplicating); Esc leaves it where it landed.
                    if !in_place && self.pointer.is_some() {
                        self.tool
                            .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
                    }
                }
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

#[cfg(test)]
mod tests {
    use drift_core::{Constraint, DVec2};

    use crate::editor::Editor;

    #[test]
    fn typed_positions_and_angles_move_dimensioned_lines() {
        let mut ed = Editor::default();
        ed.doc.begin();
        let a = ed.doc.add_point(DVec2::new(-44.0, -3.0));
        let b = ed.doc.add_point(DVec2::new(-44.0, 77.0));
        let line = ed.doc.add_line(a, b).unwrap();
        ed.doc
            .add_constraint(Constraint::Distance { a, b, value: 80.0 })
            .unwrap();
        ed.commit("Line");
        // The start moves; the length holds, so the line follows.
        let moved = ed.move_point_to(a, DVec2::new(10.0, -3.0));
        assert!(moved);
        let (pa, pb) = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        assert!((pa - DVec2::new(10.0, -3.0)).length() < 1e-6);
        assert!((pa.distance(pb) - 80.0).abs() < 1e-6);
        // Turned to 0°: the end lies along +X from the start.
        assert!(ed.set_line_angle(line, 0.0));
        let (pa, pb) = (ed.doc.point(a).unwrap(), ed.doc.point(b).unwrap());
        assert!((pb - pa - DVec2::new(80.0, 0.0)).length() < 1e-5, "{pb:?}");
        // One undo step each.
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc.point(a).unwrap(), DVec2::new(-44.0, -3.0));
    }
}
