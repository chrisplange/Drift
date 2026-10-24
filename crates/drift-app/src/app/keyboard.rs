//! Shortcuts and typed values for tools.

use crate::*;

impl DriftApp {
    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |key| KeyboardShortcut::new(EguiModifiers::COMMAND, key);
        let cmd_shift =
            |key| KeyboardShortcut::new(EguiModifiers::COMMAND | EguiModifiers::SHIFT, key);
        let consume = |shortcut: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&shortcut));

        // The command palette: ⌘K / Ctrl+K, or Ctrl+Shift+P as in code editors.
        if consume(cmd(Key::K)) || consume(cmd_shift(Key::P)) {
            self.open_palette();
            return;
        }
        if consume(cmd(Key::Comma)) {
            self.show_settings = !self.show_settings;
        }
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
        // Arrange: Ctrl+] and Ctrl+[ one step, with Shift all the way (Shift first).
        for (keys, how) in [
            (cmd_shift(Key::CloseBracket), crate::stack::Arrange::ToFront),
            (cmd_shift(Key::OpenBracket), crate::stack::Arrange::ToBack),
            (cmd(Key::CloseBracket), crate::stack::Arrange::Forward),
            (cmd(Key::OpenBracket), crate::stack::Arrange::Backward),
        ] {
            if consume(keys) {
                let now = ctx.input(|i| i.time);
                self.arrange(how, now);
            }
        }
        if consume(cmd(Key::L)) {
            self.toggle_panel(crate::dock::Panel::Layers);
        }
        // Ctrl/⌘+\: hide every panel for a clear view, and bring them back.
        if consume(cmd(Key::Backslash)) {
            self.dock_view.hide_all = !self.dock_view.hide_all;
        }
        // Print opens the system's PDF viewer: once per press, not once per key repeat while
        // the keys are held.
        let fresh = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: Key::P,
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            })
        });
        if consume(cmd(Key::P)) && fresh {
            let painter = ctx.layer_painter(egui::LayerId::background());
            self.print(&painter);
        }
        // ⌘' / Ctrl+': show or hide the grid (as in Apple's apps).
        if consume(cmd(Key::Quote)) {
            self.show_grid = !self.show_grid;
        }

        // Keys belong to a text field while it has the keyboard, and for the frame in which
        // it gives it up: egui drops a field's focus on Esc before this runs, and that Esc
        // must not also clear the selection.
        if ctx.text_edit_focused() || self.text_was_focused {
            return;
        }
        // Add view from a sheet: Esc (before the crop is started) goes back to the sheet.
        if let Some(sheet) = self.add_view_on
            && !self.tool.is_busy()
            && ctx.input(|i| i.key_pressed(Key::Escape))
        {
            self.set_tool(ToolKind::Select);
            self.show_sheet(Some(sheet));
            return;
        }
        // The sheet preview takes no drawing keys: Esc (or F) returns to the model.
        if self.sheet_preview {
            // V adds a view, by its region in the model.
            if ctx.input(|i| i.modifiers.is_none() && i.key_pressed(Key::V))
                && let Some(id) = self.current_sheet()
            {
                self.start_add_view(id);
                return;
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.sheet_preview = false;
            }
            if ctx.input(|i| i.key_pressed(Key::F) && i.modifiers.shift_only()) {
                self.preview.zoom = 0.0;
            }
            // Delete takes the picked view off the sheet (it stays in the model).
            if ctx.input(|i| {
                i.modifiers.is_none()
                    && (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace))
            }) && let Some(id) = self.current_sheet()
            {
                self.remove_viewport(id);
            }
            return;
        }
        // Q: the relations pie, which keeps the keys while it is open.
        if self.pie_keys(ctx) {
            return;
        }
        self.edit_keys(ctx);
        // ? lists the shortcuts.
        if ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Text(t) if t == "?"))
        }) {
            self.show_shortcuts = !self.show_shortcuts;
        }
        if consume(cmd(Key::A)) {
            self.set_tool(ToolKind::Select);
            self.editor.select_all();
        }
        let plain = ctx.input(|i| i.modifiers.is_none());
        let pressed = |key| plain && ctx.input(|i| i.key_pressed(key));
        // F2 renames the picked group or block, as in most editors.
        if pressed(Key::F2) && !self.tool.is_busy() && !ctx.egui_wants_keyboard_input() {
            self.start_rename();
            return;
        }
        // Tab selects a chain of connected curves (Revit-style). egui moves keyboard focus on
        // Tab, so take focus back from any button it landed on.
        if pressed(Key::Tab) && !self.tool.is_busy() {
            if let Some(id) = ctx.memory(|m| m.focused()) {
                ctx.memory_mut(|m| m.surrender_focus(id));
            }
            // Nor move it to the next button (the File menu) as Tab otherwise does.
            ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
            if self.tool.kind() != ToolKind::Select {
                self.set_tool(ToolKind::Select);
            }
            let now = ctx.input(|i| i.time);
            match self.editor.preview_chain() {
                // Over a curve: its chain is highlighted, for a click to pick.
                Some(0) => {}
                Some(n) => self.set_status(
                    format!("A chain of {n}: click to pick it · Tab again for the one curve"),
                    false,
                    now,
                ),
                // Over nothing: the selection widens to whole chains.
                None => {
                    let n = self.editor.select_chain();
                    if n > 0 {
                        self.set_status(format!("Selected a chain of {n}"), false, now);
                    }
                }
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
        claimed(Key::X, ToolKey::X);
        claimed(Key::Y, ToolKey::Y);
        let g_claimed = claimed(Key::G, ToolKey::G);
        claimed(Key::B, ToolKey::B);
        // R rotates a selection (with the select tool); otherwise it's the rectangle tool.
        let r_claimed = claimed(Key::R, ToolKey::R);
        // G with another tool in hand moves what is picked: the select tool takes over.
        let picked = !self.editor.selection.is_empty()
            || !self.editor.selected_instances.is_empty()
            || self.editor.picks_annotations();
        if pressed(Key::G)
            && !g_claimed
            && picked
            && self.tool.kind() != ToolKind::Select
            && !self.tool.is_busy()
        {
            self.set_tool(ToolKind::Select);
            self.tool
                .key(ToolKey::G, self.pointer.as_ref(), &mut self.editor);
        }
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
            if self.adding_leader.take().is_some()
                || self.align_edge.take().is_some()
                || self.selected_leader.take().is_some()
                || self.selected_constraint.take().is_some()
                || self.editor.selected_annotation.take().is_some()
                || self.picked_view.take().is_some()
            {
                // First Esc just clears a picked relation, dimension, annotation or view.
            } else if !self.tool.cancel(&mut self.editor) {
                self.set_tool(ToolKind::Select);
            }
        }
        if pressed(Key::Enter) {
            self.tool.handle(Input::Finish, &mut self.editor);
        }
        if (pressed(Key::Delete) || pressed(Key::Backspace)) && !self.tool.is_busy() {
            if let Some((id, k)) = self.selected_leader.take()
                && self.editor.selection.is_empty()
                && self.editor.selected_texts.is_empty()
                && self.editor.annotation_picks.is_empty()
            {
                // A picked leader goes on its own; its note stays.
                self.remove_leader(id, k);
                return;
            }
            if self.editor.selection.is_empty()
                && self.selected_constraint.is_none()
                && let Some(view) = self.picked_view
            {
                // A picked crop: its view goes. One on sheets asks first, in the Sheets tab.
                if self.editor.doc.sheets_showing(view).is_empty() {
                    self.delete_view(view);
                } else {
                    self.confirm_delete_view = Some(view);
                    self.show_panel(crate::dock::Panel::Sheets);
                }
                return;
            }
            // Everything picked goes together: geometry, text, annotations and dimensions.
            let picked: Vec<_> = self.selected_constraint.take().into_iter().collect();
            self.editor.delete_selection_with(&picked);
        }
        if pressed(Key::W) {
            self.thin_lines = !self.thin_lines;
        }
        // Shift+F: zoom to fit (F is the fillet tool).
        if ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::F)) {
            self.fit_target = Some(self.fit_bounds());
        }
        // Alt+D / Alt+R: show or hide dimensions / relations.
        let alt_only = |key| {
            ctx.input(|i| {
                i.modifiers.alt && !i.modifiers.shift && !i.modifiers.command && i.key_pressed(key)
            })
        };
        if alt_only(Key::D) {
            self.show_dimensions = !self.show_dimensions;
        }
        // Shift+Alt+R / Shift+Alt+V: automatic relations, and end points always shown.
        let shift_alt = |key| {
            ctx.input(|i| {
                i.modifiers.alt && i.modifiers.shift && !i.modifiers.command && i.key_pressed(key)
            })
        };
        if shift_alt(Key::R) {
            self.auto_relations = !self.auto_relations;
            let now = ctx.input(|i| i.time);
            let text = if self.auto_relations {
                "Auto relations on"
            } else {
                "Auto relations off: plain geometry, nothing added for you"
            };
            self.set_status(text.into(), false, now);
        }
        if shift_alt(Key::V) {
            self.always_endpoints = !self.always_endpoints;
        }
        // Shift+Alt+B: views' crops and names (their boundaries) on the canvas.
        if shift_alt(Key::B) {
            self.show_view_crops = !self.show_view_crops;
        }
        // Alt+C: with a drawing tool, draw construction geometry (or not); else make the
        // picked curves construction (or not).
        if alt_only(Key::C) {
            let now = ctx.input(|i| i.time);
            self.toggle_construction(now);
        }
        // Alt+H: show (faded) or hide again the dimensions and annotations hidden on the canvas.
        if alt_only(Key::H) {
            self.show_hidden_annotations = !self.show_hidden_annotations;
        }
        // Shift+Z: fit the view to the selection.
        if ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::Z)) {
            self.zoom_to_selection();
        }
        // Shift+T: the text tool (T alone trims).
        if ctx.input(|i| i.modifiers.shift_only() && i.key_pressed(Key::T)) {
            self.switch_tool(ToolKind::Text);
            return;
        }
        if alt_only(Key::R) {
            self.show_relations = !self.show_relations;
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
            if kind == ToolKind::Fillet && current == ToolKind::Fillet {
                // F again: chamfer (and again, fillet).
                kind = ToolKind::Chamfer;
            }
            if kind == ToolKind::Circle
                && let Some(i) = ToolKind::CIRCLES.iter().position(|k| *k == current)
            {
                // C again: the next of circle, ellipse, partial ellipse.
                kind = ToolKind::CIRCLES[(i + 1) % ToolKind::CIRCLES.len()];
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
        if fields.is_empty() || ctx.text_edit_focused() || self.text_was_focused {
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
