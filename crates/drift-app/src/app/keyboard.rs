//! Shortcuts and typed values for tools.
//!
//! Plain commands' keys come from the command registry ([`crate::commands`]) and are
//! dispatched here in three phases: the Ctrl shortcuts first (even with a text field
//! focused), then those that act during a grab, then the rest. Keys that mean something
//! inside a tool stay here and in the tools: Tab, X / Y, G, B and R's modal claims, Esc,
//! Enter (and Space, to repeat the last command), the sheet preview's keys and Q's pie.

use crate::commands::{self, Command, Guard, Shortcut, Spec};
use crate::*;

/// Which of the registry's keys a dispatch looks at.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Ctrl shortcuts: before anything else, even with a text field focused.
    Global,
    /// Keys that act during a grab or a turn too.
    Early,
    /// The rest (with `skip_tools`: R was taken by a turn, so no tool key acts).
    Late { skip_tools: bool },
}

impl DriftApp {
    pub(crate) fn shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl/⌘ shortcuts (the command palette stops the rest).
        if self.key_commands(ctx, Phase::Global) {
            return;
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
        // ? (the shortcuts), Ctrl+A and F2 (rename, which stops the rest).
        if self.key_commands(ctx, Phase::Early) {
            return;
        }
        let plain = ctx.input(|i| i.modifiers.is_none());
        let pressed = |key| plain && ctx.input(|i| i.key_pressed(key));
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
        // Drawing walls: Space flips to the other face, Backspace takes a segment back.
        claimed(Key::Space, ToolKey::Space);
        claimed(Key::Backspace, ToolKey::Backspace);
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
        // Enter or Space with nothing in hand does the last command again (AutoCAD-style);
        // otherwise Enter finishes what the tool is doing. (Typed values took Enter already.)
        let idle = self.tool.kind() == ToolKind::Select && !self.tool.is_busy();
        let again = idle && (pressed(Key::Enter) || pressed(Key::Space));
        if !(again && self.repeat_last_command(ctx)) && pressed(Key::Enter) {
            self.tool.handle(Input::Finish, &mut self.editor);
        }
        self.key_commands(
            ctx,
            Phase::Late {
                skip_tools: r_claimed,
            },
        );
    }

    /// Run the registry's commands whose keys were pressed, in `phase`. True if the rest of
    /// the keys should wait (the palette opened, a rename started, or a late command ran).
    fn key_commands(&mut self, ctx: &egui::Context, phase: Phase) -> bool {
        for &(key, spec) in commands::dispatched() {
            let ours = match phase {
                Phase::Global => spec.global,
                Phase::Early => !spec.global && spec.in_grab,
                Phase::Late { skip_tools } => {
                    !spec.global
                        && !spec.in_grab
                        && !(skip_tools && matches!(spec.command, Command::Tool(_)))
                }
            };
            if !ours || !Self::key_down(ctx, key, spec) {
                continue;
            }
            let busy = self.tool.is_busy();
            let blocked = match spec.guard {
                Guard::Any => false,
                Guard::Idle => busy,
                Guard::IdleUnfocused => busy || ctx.egui_wants_keyboard_input(),
            };
            if blocked {
                continue;
            }
            match spec.command {
                Command::Tool(kind) => self.tool_key(ctx, kind),
                command => self.run_command(ctx, command),
            }
            let stop = match phase {
                Phase::Global => spec.command == Command::Palette,
                Phase::Early => spec.command == Command::Rename,
                Phase::Late { .. } => true,
            };
            if stop {
                return true;
            }
        }
        false
    }

    /// Whether a command's key was pressed this frame. Ctrl shortcuts are used up (egui
    /// matches them ignoring extra Shift, so the registry tries the longer ones first);
    /// others must have exactly their modifiers.
    fn key_down(ctx: &egui::Context, key: Shortcut, spec: &Spec) -> bool {
        if key.typed {
            let text = key.text(false);
            return ctx.input(|i| {
                i.events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Text(t) if *t == text))
            });
        }
        if key.command {
            // Print and the like: once per press, not once per key repeat while held.
            let fresh = ctx.input(|i| {
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Key {
                            key: k,
                            pressed: true,
                            repeat: false,
                            ..
                        } if *k == key.key
                    )
                })
            });
            let mut mods = EguiModifiers::COMMAND;
            if key.shift {
                mods |= EguiModifiers::SHIFT;
            }
            if key.alt {
                mods |= EguiModifiers::ALT;
            }
            let shortcut = KeyboardShortcut::new(mods, key.key);
            let down = ctx.input_mut(|i| i.consume_shortcut(&shortcut));
            return down && (fresh || !spec.once);
        }
        ctx.input(|i| {
            let m = i.modifiers;
            m.shift == key.shift
                && m.alt == key.alt
                && !m.command
                && !m.ctrl
                && !m.mac_cmd
                && i.key_pressed(key.key)
        })
    }

    /// A tool's key: picks the tool, or steps through its cycle (C: circle, ellipse, partial
    /// ellipse; A: the arcs; F: fillet, chamfer; U: the kinds of slot).
    fn tool_key(&mut self, ctx: &egui::Context, mut kind: ToolKind) {
        let current = self.tool.kind();
        if current == ToolKind::Wall {
            // W again: straight or arc walls; A / L mid-run: an arc or a straight segment
            // next. The run goes on.
            let key = match kind {
                ToolKind::Wall => Some(ToolKey::Again),
                ToolKind::ThreePointArc if self.tool.is_busy() => Some(ToolKey::ArcNext),
                ToolKind::Line if self.tool.is_busy() => Some(ToolKey::LineNext),
                _ => None,
            };
            if let Some(key) = key {
                self.tool.key(key, self.pointer.as_ref(), &mut self.editor);
                let now = ctx.input(|i| i.time);
                let what = if self.editor.wall_arc {
                    "Arc walls: click the end, then a point the wall curves through"
                } else {
                    "Straight walls"
                };
                self.set_status(what.to_owned(), false, now);
                return;
            }
        }
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
        if kind == ToolKind::ThreePointArc {
            kind = if let Some(i) = ToolKind::ARCS.iter().position(|k| *k == current) {
                // A again: the next arc type.
                ToolKind::ARCS[(i + 1) % ToolKind::ARCS.len()]
            } else if current == ToolKind::Line && self.tool.chain_end().is_some() {
                // Mid-polyline: continue with a tangent arc (SolidWorks-style).
                ToolKind::TangentArc
            } else {
                ToolKind::ThreePointArc
            };
        }
        self.switch_tool(kind);
    }

    /// Delete what is picked: geometry, text, annotations and dimensions together; or a
    /// picked leader on its own, or a picked view's crop (asking first if it's on a sheet).
    pub(crate) fn delete_picked(&mut self) {
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
            self.delete_picked_view(view);
            return;
        }
        // Everything picked goes together: geometry, text, annotations and dimensions.
        let picked: Vec<_> = self.selected_constraint.take().into_iter().collect();
        self.editor.delete_selection_with(&picked);
    }

    /// A picked view goes, from the model and its sheets. One on sheets asks first, in the
    /// Sheets tab.
    pub(crate) fn delete_picked_view(&mut self, view: drift_core::ViewId) {
        if self.editor.doc.sheets_showing(view).is_empty() {
            self.delete_view(view);
        } else {
            self.confirm_delete_view = Some(view);
            self.show_panel(crate::dock::Panel::Sheets);
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
        // The last tool picked (however it was picked) is what Enter repeats.
        if commands::spec(Command::Tool(kind)).is_some_and(|s| s.repeatable) {
            self.last_command = Some(Command::Tool(kind));
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
