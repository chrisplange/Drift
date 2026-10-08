//! Shortcuts and typed values for tools.

use crate::*;

impl DriftApp {
    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |key| KeyboardShortcut::new(EguiModifiers::COMMAND, key);
        let cmd_shift =
            |key| KeyboardShortcut::new(EguiModifiers::COMMAND | EguiModifiers::SHIFT, key);
        let consume = |shortcut: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&shortcut));

        // Order matters: check the more specific (Shift) variants first.
        if consume(cmd_shift(Key::Z)) || consume(cmd(Key::Y)) {
            self.redo();
        }
        if consume(cmd(Key::Z)) {
            self.undo();
        }
        if consume(cmd_shift(Key::S)) {
            self.save_as();
        }
        if consume(cmd(Key::S)) {
            self.save();
        }
        if consume(cmd(Key::O)) {
            self.request(Pending::Open);
        }
        if consume(cmd(Key::N)) {
            self.request(Pending::New);
        }
        // Order matters: Shift first.
        if consume(cmd_shift(Key::G)) {
            self.editor.ungroup();
        }
        if consume(cmd(Key::G)) {
            self.editor.make_group();
        }
        if consume(cmd(Key::L)) {
            self.toggle_layers_hud();
        }
        if consume(cmd(Key::P)) {
            let painter = ctx.layer_painter(egui::LayerId::background());
            self.print(&painter);
        }
        // ⌘' / Ctrl+': show or hide the grid (as in Apple's apps).
        if consume(cmd(Key::Quote)) {
            self.show_grid = !self.show_grid;
        }

        if ctx.text_edit_focused() {
            return;
        }
        // The sheet preview takes no drawing keys: Esc (or F) returns to the model.
        if self.sheet_preview {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.sheet_preview = false;
            }
            if ctx.input(|i| i.key_pressed(Key::F)) {
                self.preview.zoom = 0.0;
            }
            return;
        }
        if consume(cmd(Key::A)) {
            self.set_tool(ToolKind::Select);
            self.editor.select_all();
        }
        let plain = ctx.input(|i| i.modifiers.is_none());
        let pressed = |key| plain && ctx.input(|i| i.key_pressed(key));
        // Tab selects a chain of connected curves (Revit-style). egui moves keyboard focus on
        // Tab, so take focus back from any button it landed on.
        if pressed(Key::Tab) && !self.tool.is_busy() {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            if self.tool.kind() != ToolKind::Select {
                self.set_tool(ToolKind::Select);
            }
            let n = self.editor.select_chain();
            if n > 0 {
                let now = ctx.input(|i| i.time);
                self.set_status(format!("Selected a chain of {n}"), false, now);
            }
        }

        // Modal keys go to the tool first: X / Y lock an axis, G grabs (moves) the selection,
        // B picks a move's base point.
        let mut claimed = |key, tool_key| {
            pressed(key)
                && self
                    .tool
                    .key(tool_key, self.pointer.as_ref(), &mut self.editor)
        };
        let x_claimed = claimed(Key::X, ToolKey::X);
        claimed(Key::Y, ToolKey::Y);
        claimed(Key::G, ToolKey::G);
        claimed(Key::B, ToolKey::B);
        // R rotates a selection (with the select tool); otherwise it's the rectangle tool.
        let r_claimed = claimed(Key::R, ToolKey::R);
        if ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::G)) {
            self.grid_snap = !self.grid_snap;
        }
        // Shift+C: constraint status colours, or each curve's own / its layer's colour.
        if ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::C)) {
            self.status_colors = !self.status_colors;
        }
        if self.tool.is_modal() {
            // Only Esc and Enter (and the modal keys above) act during a grab.
            if pressed(Key::Escape) {
                self.tool.cancel(&mut self.editor);
            }
            if pressed(Key::Enter) {
                self.tool.handle(Input::Finish, &mut self.editor);
            }
            return;
        }

        // Shift+D duplicates the selection in place and grabs the copy (as in Blender).
        if ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::D)) && !self.tool.is_busy()
        {
            if self.tool.kind() != ToolKind::Select {
                self.set_tool(ToolKind::Select);
            }
            if self.editor.duplicate_selection() > 0 {
                self.tool
                    .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
            }
            return;
        }
        if pressed(Key::Escape) {
            if self.selected_constraint.take().is_some()
                || self.editor.selected_annotation.take().is_some()
            {
                // First Esc just clears a picked relation, dimension or annotation.
            } else if !self.tool.cancel(&mut self.editor) {
                self.set_tool(ToolKind::Select);
            }
        }
        if pressed(Key::Enter) {
            self.tool.handle(Input::Finish, &mut self.editor);
        }
        if (pressed(Key::Delete) || pressed(Key::Backspace)) && !self.tool.is_busy() {
            match self.selected_constraint.take() {
                Some(id) => self.editor.remove_constraint(id),
                None => self.editor.delete_selection(),
            }
        }
        // X toggles construction unless a tool used it (an axis lock).
        if pressed(Key::X) && !x_claimed {
            self.editor.toggle_construction();
        }
        if pressed(Key::W) {
            self.thin_lines = !self.thin_lines;
        }
        if pressed(Key::F) {
            self.fit_target = Some(self.fit_bounds());
        }
        let requested = ToolKind::ALL
            .into_iter()
            .filter(|_| !r_claimed)
            .find(|kind| {
                kind.shortcut()
                    .and_then(|c| Key::from_name(&c.to_string()))
                    .is_some_and(pressed)
            });
        if let Some(mut kind) = requested {
            let current = self.tool.kind();
            if kind == ToolKind::Slot && current == ToolKind::Slot {
                // U again: the next kind of slot.
                self.editor.slot_mode = self.editor.slot_mode.next();
                self.tool.cancel(&mut self.editor);
                let now = ctx.input(|i| i.time);
                self.set_status(self.editor.slot_mode.label().to_owned(), false, now);
            }
            if kind == ToolKind::Arc {
                kind = if let Some(i) = ToolKind::ARCS.iter().position(|k| *k == current) {
                    // A again: the next arc type.
                    ToolKind::ARCS[(i + 1) % ToolKind::ARCS.len()]
                } else if current == ToolKind::Line && self.tool.chain_end().is_some() {
                    // Mid-polyline: continue with a tangent arc (SolidWorks-style).
                    ToolKind::TangentArc
                } else {
                    ToolKind::Arc
                };
            }
            self.switch_tool(kind);
        }
    }

    /// Change tool, continuing a polyline / tangent-arc chain across the switch.
    pub(crate) fn switch_tool(&mut self, kind: ToolKind) {
        let chain = self.tool.chain_end();
        self.set_tool(kind);
        if let Some(point) = chain
            && matches!(kind, ToolKind::Line | ToolKind::TangentArc)
        {
            self.tool.continue_from(point, &mut self.editor);
        }
    }

    /// Route number keys to the active tool's typed fields. Runs before other shortcuts so
    /// Enter, Esc, Tab and Backspace go to the typing when it is active.
    pub(crate) fn handle_typing(&mut self, ctx: &egui::Context) {
        let fields = self.tool.fields();
        // egui moves keyboard focus between widgets on Tab before we see the key. While a
        // tool takes typed values, Tab belongs to it, so release focus that landed on a
        // button (otherwise Enter would also click that button).
        if !fields.is_empty()
            && !ctx.text_edit_focused()
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        if fields.is_empty() || ctx.text_edit_focused() {
            self.typing = None;
            self.tool.set_typed([None; 2]);
            return;
        }
        if self.typing.as_ref().is_some_and(|t| t.fields != fields) {
            self.typing = None;
        }
        // Typing starts with a digit, `.`, `-`, `(`, or `=` (to start with a variable name).
        // Once it has started, any text goes into the field (units, `'`, `"`, variables), and
        // letter keys no longer act as shortcuts.
        let active = self.typing.is_some();
        let in_call = self
            .typing
            .as_ref()
            .is_some_and(|t| t.values[t.active].contains('('));
        let typed: String = ctx.input_mut(|i| {
            let mut typed = String::new();
            i.events.retain(|e| match e {
                egui::Event::Text(t)
                    if active || t.chars().all(|c| c.is_ascii_digit() || ".,-(=".contains(c)) =>
                {
                    // A decimal comma, except between function arguments.
                    let t = if in_call {
                        t.clone()
                    } else {
                        t.replace(',', ".")
                    };
                    typed.push_str(&t);
                    false
                }
                egui::Event::Key { key, modifiers, .. }
                    if active
                        && !modifiers.command
                        && !matches!(key, Key::Tab | Key::Enter | Key::Escape | Key::Backspace) =>
                {
                    false
                }
                _ => true,
            });
            typed
        });
        let starting = !active && typed.starts_with('=');
        let typed = if active {
            typed
        } else {
            typed.trim_start_matches('=').to_owned()
        };
        let consume = |key| ctx.input_mut(|i| i.consume_key(EguiModifiers::NONE, key));
        if !typed.is_empty() || starting {
            let t = self.typing.get_or_insert_with(|| Typing::new(fields));
            t.values[t.active].push_str(&typed);
        } else if self.typing.is_none() && fields.len() > 1 && consume(Key::Tab) {
            // Tab first: start with the second field (e.g. angle).
            let mut t = Typing::new(fields);
            t.active = 1;
            self.typing = Some(t);
        }
        if let Some(t) = &mut self.typing {
            if consume(Key::Tab) {
                t.active = (t.active + 1) % t.fields.len();
            }
            if consume(Key::Backspace) {
                t.values[t.active].pop();
            }
            if consume(Key::Escape) {
                self.typing = None;
            } else if consume(Key::Enter) {
                if let Some(error) = t.evaluate(&self.editor).into_iter().find_map(Result::err) {
                    // Keep the typing so it can be fixed.
                    self.editor.notice = Some(error);
                } else {
                    let values = t.parsed(&self.editor);
                    self.editor.typed_exprs = t.expressions(&self.editor);
                    self.typing = None;
                    let pointer = self.pointer.unwrap_or_else(|| {
                        let mut p =
                            Pointer::new(self.camera.center, PICK_RADIUS / self.camera.scale);
                        p.units = self.editor.doc.units();
                        p
                    });
                    self.tool.set_typed(values);
                    self.tool.commit_typed(&pointer, &mut self.editor);
                    self.editor.typed_exprs = [None, None];
                }
            }
        }
        let values = self
            .typing
            .as_ref()
            .map_or([None; 2], |t| t.parsed(&self.editor));
        self.tool.set_typed(values);
    }

    /// What zoom-to-fit frames: the geometry plus placed dimension labels (with a little room
    /// for the label text).
    pub(crate) fn fit_bounds(&self) -> Bounds {
        let mut b = self.editor.bounds();
        for (_, u) in self.editor.doc.underlays().filter(|(_, u)| u.visible) {
            let ub = u.bounds();
            if !ub.is_empty() {
                b.include(ub.min);
                b.include(ub.max);
            }
        }
        if b.is_empty() {
            return EMPTY_VIEW;
        }
        if self.show_dimensions {
            let doc = &self.editor.doc;
            let pad = 0.04 * (b.max - b.min).max_element();
            for (id, c) in doc.constraints() {
                if let (Some(anchor), Some(offset)) = (
                    crate::annotations::dimension_anchor(doc, c),
                    doc.placement(id),
                ) {
                    let at = anchor + offset;
                    b.include(at - DVec2::splat(pad));
                    b.include(at + DVec2::splat(pad));
                }
            }
            // Notes, callouts and ordinate labels too (the text extends a little further).
            for (_, a) in doc.annotations() {
                if let Some(at) = crate::notes::label_world(doc, a) {
                    b.include(at - DVec2::splat(pad * 2.0));
                    b.include(at + DVec2::splat(pad * 2.0));
                }
            }
        }
        b
    }
}
