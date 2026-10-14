//! The inspector for sketch text.

use drift_core::{SketchText, TextAlign, TextFont};

use crate::*;

/// The key that starts a new line while typing text (Return alone finishes).
fn new_line_key() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter)
}

/// Text with capitals shorter than this on screen (in points) is typed in a field under it:
/// a caret in it would be too small to see or to aim at.
const LIVE_MIN_POINTS: f64 = 6.0;

/// A sketch text as the paragraph it is set as.
fn paragraph(t: &SketchText) -> drift_draft::outline::Paragraph<'_> {
    drift_draft::outline::Paragraph {
        text: &t.text,
        font: &t.font,
        height: t.height,
        at: t.at,
        angle: t.angle,
        align: t.align,
        spacing: t.spacing,
        wrap: t.wrap,
    }
}

/// The field a text on the canvas is typed through.
fn text_field(words: &mut String, id: egui::Id) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(words)
        .id(id)
        .char_limit(crate::texts::MAX_TEXT_CHARS)
        .desired_rows(1)
        // Return finishes; a new line is Shift+Return.
        .return_key(Some(new_line_key()))
        .desired_width(260.0)
}

/// What was chosen from a font menu.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FontPick {
    /// The standard look (only where the menu offers it).
    Standard,
    BuiltIn(TextFont),
    /// A family kept in the drawing or installed on this computer.
    Family(String),
    /// "Add a font file…".
    AddFile,
}

impl DriftApp {
    /// A font menu: the built-in fonts (or, for the drawing's font, the standard look), the
    /// families kept in this drawing, the computer's families with a field to find one, and
    /// a way to add a font file. `current` marks what is chosen now.
    pub(crate) fn font_menu(
        &mut self,
        ui: &mut egui::Ui,
        id: impl std::hash::Hash + std::fmt::Debug,
        shown: &str,
        current: Option<&TextFont>,
    ) -> Option<FontPick> {
        let mut picked = None;
        let own: Vec<String> = self.editor.doc.fonts().embedded.keys().cloned().collect();
        let drawing = self.editor.doc.fonts().drawing.clone();
        // Sketch text chooses among the built-in fonts; the drawing's font, the standard look.
        let for_text = current.is_some();
        let is = |family: &str| match current {
            Some(TextFont::System(f)) => f == family,
            Some(_) => false,
            None => drawing.as_deref() == Some(family),
        };
        egui::ComboBox::from_id_salt(id)
            .selected_text(shown)
            .width(ui.available_width() - 8.0)
            .height(380.0)
            // Clicking into "Find a font" mustn't close it; a pick does.
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.set_min_width(220.0);
                if for_text {
                    for f in TextFont::BUILT_IN {
                        if ui
                            .selectable_label(current == Some(&f), f.label())
                            .clicked()
                        {
                            picked = Some(FontPick::BuiltIn(f));
                        }
                    }
                } else if ui
                    .selectable_label(drawing.is_none(), "Standard (Helvetica)")
                    .clicked()
                {
                    picked = Some(FontPick::Standard);
                }
                if !own.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new("In this drawing").small().weak());
                    for family in &own {
                        if ui.selectable_label(is(family), family).clicked() {
                            picked = Some(FontPick::Family(family.clone()));
                        }
                    }
                }
                ui.separator();
                if ui
                    .button("Add a font file…")
                    .on_hover_text(
                        "Keep a .ttf or .otf font in this drawing. It then works on any \
                         computer, and in the browser",
                    )
                    .clicked()
                {
                    picked = Some(FontPick::AddFile);
                }
                // The computer's fonts, found the first time a font menu is opened.
                let families = self
                    .font_families
                    .get_or_insert_with(drift_platform::fonts::families);
                if families.is_empty() {
                    if picked.is_some() {
                        ui.close();
                    }
                    return;
                }
                ui.separator();
                ui.label(egui::RichText::new("On this computer").small().weak());
                ui.add(
                    egui::TextEdit::singleline(&mut self.font_filter)
                        .hint_text("Find a font…")
                        .desired_width(f32::INFINITY),
                );
                let wanted = self.font_filter.trim().to_lowercase();
                for family in families
                    .iter()
                    .filter(|f| wanted.is_empty() || f.to_lowercase().contains(&wanted))
                    .filter(|f| !own.contains(f))
                {
                    if ui.selectable_label(is(family), family).clicked() {
                        picked = Some(FontPick::Family(family.clone()));
                    }
                }
                if picked.is_some() {
                    ui.close();
                }
            });
        picked
    }

    /// The drawing's own font, in the Drafting section: what dimensions, notes, view titles
    /// and title blocks are set in.
    pub(crate) fn drawing_font_row(&mut self, ui: &mut egui::Ui) {
        let drawing = self.editor.doc.fonts().drawing.clone();
        let shown = drawing
            .clone()
            .unwrap_or_else(|| "Standard (Helvetica)".to_owned());
        let mut picked = None;
        theme::row(ui, "Font", |ui| {
            picked = self.font_menu(ui, "drawing_font", &shown, None);
        });
        let now = ui.input(|i| i.time);
        match picked {
            Some(FontPick::Standard) if drawing.is_some() => {
                self.editor.set_drawing_font(None);
            }
            Some(FontPick::Family(family)) if drawing.as_deref() != Some(family.as_str()) => {
                if let Some(note) = self.editor.set_drawing_font(Some(family)) {
                    self.set_status(note, false, now);
                }
            }
            Some(FontPick::AddFile) => self.files.open(crate::FONT_FILE),
            _ => {}
        }
        if let Some(family) = self.editor.drawing_font_missing() {
            ui.label(
                egui::RichText::new(format!(
                    "{family} isn't in this drawing or on this computer: the standard font is \
                     shown. The drawing keeps the name, and uses it where it is found."
                ))
                .small()
                .color(egui::Color32::from_rgb(0xff, 0x9f, 0x0a)),
            );
        } else if let Some(family) = &drawing {
            let kept = self.editor.doc.fonts().embedded.contains_key(family);
            ui.label(
                egui::RichText::new(if kept {
                    "Dimensions, notes and sheets are set in it. It is kept in the drawing and \
                     embedded in PDFs."
                } else {
                    "Dimensions, notes and sheets are set in it. It is used from this computer \
                     only (it can't be kept in the drawing); PDFs draw its text as outlines."
                })
                .small()
                .weak(),
            );
        }
    }

    /// Every way a drawing's fonts add to its file, for the Drafting section.
    pub(crate) fn kept_fonts_note(&self, ui: &mut egui::Ui) {
        let fonts = self.editor.doc.fonts();
        let used = self.editor.doc.fonts_in_use();
        let kept: usize = fonts
            .embedded
            .iter()
            .filter(|(family, _)| used.contains(*family))
            .map(|(_, f)| f.regular.data.len() + f.bold.as_ref().map_or(0, |b| b.data.len()))
            .sum();
        if kept > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "Fonts in use add {:.1} MB to the file. One that nothing is set in any \
                     more is dropped when the drawing is saved.",
                    kept as f64 * 4.0 / 3.0 / (1024.0 * 1024.0)
                ))
                .small()
                .weak(),
            );
        }
    }

    /// Whether a text is typed straight into on the canvas (large enough on screen to see
    /// a caret in), or in a field under it.
    fn text_is_live(&self, t: &SketchText) -> bool {
        t.height * self.camera.scale >= LIVE_MIN_POINTS
    }

    /// Stop typing into the text on the canvas.
    fn finish_text(&mut self) {
        self.editor.editing_text = None;
        self.editor.text_caret_at = None;
        self.text_typing = None;
        self.text_focused = None;
        self.text_cursor = None;
        self.text_set_cursor = None;
        self.text_grip = false;
        self.text_touched = false;
    }

    /// The pointer in the text being typed: a press puts the caret there (Shift: selects up
    /// to there), a drag selects, a double click selects a word and a triple click
    /// everything. A press anywhere else on the canvas finishes the typing. True when the
    /// pointer was used here.
    pub(crate) fn text_pointer(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        map: &ScreenMap,
    ) -> bool {
        let Some(id) = self.editor.editing_text else {
            self.text_grip = false;
            return false;
        };
        let Some(t) = self.editor.doc.text(id).cloned() else {
            return false;
        };
        let (pressed, down, at, shift) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.interact_pos(),
                i.modifiers.shift,
            )
        });
        let para = paragraph(&t);
        let from = self.text_cursor.map(|c| c.1);
        if pressed && response.is_pointer_button_down_on() {
            let inside = self.text_is_live(&t)
                && at.is_some_and(|at| self.in_text(id, map.to_world(at), map));
            let Some(at) = at.filter(|_| inside) else {
                self.finish_text();
                return false;
            };
            let to = para.index_at(map.to_world(at));
            self.text_set_cursor = Some((to, from.filter(|_| shift).unwrap_or(to)));
            self.text_grip = true;
            self.text_touched = true;
            return true;
        }
        if !self.text_grip {
            return false;
        }
        self.text_touched = true;
        if down {
            if let Some(at) = at {
                let to = para.index_at(map.to_world(at));
                self.text_set_cursor = Some((to, from.unwrap_or(to)));
            }
        } else {
            self.text_grip = false;
            let caret = self.text_cursor.map_or(0, |c| c.0);
            if response.triple_clicked() {
                self.text_set_cursor = Some((t.text.chars().count(), 0));
            } else if response.double_clicked() {
                let (start, end) = drift_draft::outline::word_range(&t.text, caret);
                self.text_set_cursor = Some((end, start));
            }
        }
        true
    }

    /// Whether `p` is on the text being typed, with room to spare around it.
    fn in_text(&self, id: drift_core::TextId, p: DVec2, map: &ScreenMap) -> bool {
        let Some(bounds) = self.editor.text_outline(id).map(|o| o.bounds) else {
            return false;
        };
        let pad = 10.0 / map.camera.scale;
        p.x >= bounds.min.x - pad
            && p.x <= bounds.max.x + pad
            && p.y >= bounds.min.y - pad
            && p.y <= bounds.max.y + pad
    }

    /// Typing a text in place. The keyboard goes to a text field, for all a field knows
    /// (the clipboard, composed characters, the platform's keys): out of sight when the
    /// text is large enough to show a caret and a selection of its own on the canvas, and
    /// under the text when it is too small for that. Return, Esc or a click elsewhere
    /// finishes; Shift+Return starts a new line.
    pub(crate) fn text_editor(&mut self, ui: &mut egui::Ui, map: &ScreenMap, on: &egui::Painter) {
        let Some(id) = self.editor.editing_text else {
            self.text_focused = None;
            self.text_cursor = None;
            return;
        };
        let Some(t) = self.editor.doc.text(id).cloned() else {
            self.finish_text();
            return;
        };
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let live = self.text_is_live(&t);
        let field = egui::Id::new("text_edit_field");
        let starting = self.text_focused != Some(id);
        let count = t.text.chars().count();
        let para = paragraph(&t);

        // Where the typing starts: at the click that began it, or at the end.
        if let Some(p) = self.editor.text_caret_at.take() {
            let at = if live { para.index_at(p) } else { count };
            self.text_set_cursor = Some((at, at));
        } else if starting && self.text_set_cursor.is_none() {
            self.text_set_cursor = Some((count, count));
        }
        // Up, Down, Home and End go by the lines as they are set on the canvas, which the
        // field knows nothing of.
        if live
            && !starting
            && ctx.memory(|m| m.has_focus(field))
            && let Some((caret, from)) = self.text_cursor
        {
            for key in [
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
                egui::Key::Home,
                egui::Key::End,
            ] {
                let pressed = ctx.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::SHIFT, key) {
                        Some(true)
                    } else {
                        i.consume_key(egui::Modifiers::NONE, key).then_some(false)
                    }
                });
                if let Some(extend) = pressed {
                    let to = match key {
                        egui::Key::ArrowUp => para.line_step(caret, -1),
                        egui::Key::ArrowDown => para.line_step(caret, 1),
                        egui::Key::Home => para.line_ends(caret).0,
                        _ => para.line_ends(caret).1,
                    };
                    self.text_set_cursor = Some((to, if extend { from } else { to }));
                }
            }
        }
        if let Some((caret, from)) = self.text_set_cursor.take() {
            let mut state = egui::TextEdit::load_state(&ctx, field).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(from.min(count)),
                    egui::text::CCursor::new(caret.min(count)),
                )));
            state.store(&ctx, field);
            self.text_blink = now;
        }

        let mut words = t.text.clone();
        let response = if live {
            // Out of sight, by the caret (where a window for composing characters opens).
            let caret = self.text_cursor.map_or(count, |c| c.0);
            let at = map.to_screen(para.caret(caret)[0]);
            let mut hidden = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(at, egui::vec2(260.0, 20.0))),
            );
            hidden.set_clip_rect(egui::Rect::from_min_size(
                at - egui::vec2(16.0, 16.0),
                egui::Vec2::ZERO,
            ));
            hidden.add(text_field(&mut words, field))
        } else {
            // Under the text's lowest line, so the field doesn't cover what is being typed.
            let low = self
                .editor
                .text_outline(id)
                .map_or(t.at.y, |o| o.bounds.min.y.min(t.at.y));
            let at = map.to_screen(DVec2::new(t.at.x, low));
            let style = ctx.style_of(ctx.theme());
            egui::Area::new(egui::Id::new("text_edit"))
                .fixed_pos(at + egui::vec2(-6.0, 8.0))
                .order(egui::Order::Foreground)
                .show(&ctx, |ui| {
                    egui::Frame::popup(&style)
                        .inner_margin(egui::Margin::same(4))
                        .show(ui, |ui| {
                            let rows =
                                words.lines().count().max(1) + usize::from(words.ends_with('\n'));
                            ui.add(
                                text_field(&mut words, field)
                                    .hint_text("Type the text, then Enter (Shift+Enter: new line)")
                                    .desired_rows(rows),
                            )
                        })
                        .inner
                })
                .inner
        };
        if response.changed() {
            // Typing is one undo step per visit to the text.
            let continuing = self.text_typing == Some(id);
            self.editor.type_text(id, &words, continuing);
            self.text_typing = Some(id);
            self.text_blink = now;
        }
        let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        if starting {
            response.request_focus();
            self.text_focused = Some(id);
        } else if std::mem::take(&mut self.text_touched) {
            // A press or a click in the text took the keyboard from the field: give it back.
            response.request_focus();
        } else if !response.has_focus() || enter {
            response.surrender_focus();
            self.finish_text();
            return;
        }
        let cursor = egui::TextEdit::load_state(&ctx, field)
            .and_then(|state| state.cursor.char_range())
            .map(|r| (r.primary.index.0, r.secondary.index.0));
        if cursor != self.text_cursor {
            self.text_blink = now;
        }
        self.text_cursor = cursor;
        if !live {
            return;
        }

        // The caret and the selection, on the text as it now is.
        let Some(t) = self.editor.doc.text(id).cloned() else {
            return;
        };
        let para = paragraph(&t);
        let (caret, from) = cursor.unwrap_or((0, 0));
        let dark = ctx.style_of(ctx.theme()).visuals.dark_mode;
        let colors = OverlayColors::new(dark);
        for corners in para.selection(from, caret) {
            on.add(egui::Shape::convex_polygon(
                corners.iter().map(|c| map.to_screen(*c)).collect(),
                colors.window.gamma_multiply(0.35),
                egui::Stroke::NONE,
            ));
        }
        let [foot, head] = para.caret(caret).map(|p| map.to_screen(p));
        // Steady for half a second after it moves, then half a second off and on.
        let since = (now - self.text_blink).max(0.0);
        if since % 1.0 < 0.5 {
            let ink = if dark {
                egui::Color32::WHITE
            } else {
                egui::Color32::BLACK
            };
            on.line_segment([foot, head], egui::Stroke::new(1.5, ink));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(0.5 - since % 0.5));
        if t.text.is_empty() {
            on.text(
                foot + egui::vec2(8.0, 0.0),
                egui::Align2::LEFT_BOTTOM,
                "Type, then Enter",
                egui::FontId::proportional(12.0),
                colors.window.gamma_multiply(0.7),
            );
        }
        // Composed characters (accents, Chinese, Japanese…) open their window at the caret.
        ctx.output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                purpose: egui::IMEPurpose::Normal,
                rect: self.canvas_rect,
                cursor_rect: egui::Rect::from_two_pos(foot, head),
                should_interrupt_composition: false,
            });
        });
        // An I-beam over the text.
        if !self.text_grip
            && let Some(at) = ctx.input(|i| i.pointer.hover_pos())
            && self.canvas_rect.contains(at)
            && self.in_text(id, map.to_world(at), map)
            && ctx.layer_id_at(at) == Some(ui.layer_id())
        {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }

    /// Edit the picked text: its words, size, angle and font; convert it to curves.
    pub(crate) fn text_inspector(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(id) = self.editor.selected_text else {
            return false;
        };
        let Some(t) = self.editor.doc.text(id).cloned() else {
            self.editor.selected_text = None;
            return false;
        };
        let units = self.editor.doc.units();
        let layer = self.editor.doc.layer(t.layer).map_or(String::new(), |l| {
            format!("{} · on layer {}", t.font.label(), l.name)
        });
        app::panels::title(ui, Some(ToolKind::Text), "Text", &layer);
        ui.add_space(4.0);
        let mut words = t.text.clone();
        let rows = words.lines().count().clamp(1, 8);
        let edit = ui
            .add(
                egui::TextEdit::multiline(&mut words)
                    .hint_text("Type the text")
                    .char_limit(crate::texts::MAX_TEXT_CHARS)
                    .desired_rows(rows)
                    .return_key(Some(new_line_key()))
                    .desired_width(f32::INFINITY),
            )
            .on_hover_text("Shift+Enter starts a new line");
        if edit.changed() {
            // Typing is one undo step per visit to the field.
            let continuing = self.text_typing == Some(id);
            self.editor.type_text(id, &words, continuing);
            self.text_typing = Some(id);
        }
        if edit.lost_focus() {
            self.text_typing = None;
        }
        let unit = units.length.mm_per_unit();
        let mut edited = t.clone();
        theme::row(ui, "Height", |ui| {
            let mut shown = t.height / unit;
            if ui
                .add(
                    egui::DragValue::new(&mut shown)
                        .speed(0.1)
                        .range(0.001..=1e6)
                        .max_decimals(4)
                        .suffix(format!(" {}", units.length.symbol())),
                )
                .on_hover_text("Height of capital letters")
                .changed()
            {
                edited.height = shown * unit;
            }
        });
        theme::row(ui, "Angle", |ui| {
            let mut a = t.angle;
            if ui
                .add(egui::DragValue::new(&mut a).suffix("°").speed(0.5))
                .changed()
            {
                edited.angle = a.rem_euclid(360.0);
            }
        });
        // On its own row, so a narrow dock isn't pushed wider.
        theme::row(ui, "", |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for q in [0.0, 90.0, 180.0, 270.0] {
                if ui.small_button(format!("{q:.0}")).clicked() {
                    edited.angle = q;
                }
            }
        });
        theme::row(ui, "Align", |ui| {
            let options: Vec<(TextAlign, &str)> =
                TextAlign::ALL.iter().map(|a| (*a, a.label())).collect();
            if let Some(align) = theme::segmented(ui, t.align, &options) {
                // The text stays where it is: its anchor moves to the new side of the
                // first line.
                let width =
                    drift_draft::outline::paragraph_lines(&t.text, &t.font, t.height, t.wrap)
                        .first()
                        .map_or(0.0, |line| {
                            drift_draft::outline::line_width(line.trim_end(), &t.font, t.height)
                        });
                let span = t.wrap.unwrap_or(width);
                let along = DVec2::from_angle(t.angle.to_radians());
                edited.at = t.at + along * span * (align.factor() - t.align.factor());
                edited.align = align;
            }
        });
        theme::row(ui, "Spacing", |ui| {
            let mut spacing = t.spacing;
            if ui
                .add(
                    egui::DragValue::new(&mut spacing)
                        .speed(0.02)
                        .range(0.5..=10.0)
                        .max_decimals(2)
                        .prefix("× "),
                )
                .on_hover_text("From one line to the next, in letter heights")
                .changed()
            {
                edited.spacing = spacing;
            }
        });
        theme::row(ui, "Wrap", |ui| {
            let mut wraps = t.wrap.is_some();
            if ui
                .checkbox(&mut wraps, "")
                .on_hover_text("Break lines at spaces to keep within a width")
                .changed()
            {
                // Start at the width it has now, so nothing moves until it is narrowed.
                let widest =
                    drift_draft::outline::paragraph_lines(&t.text, &t.font, t.height, None)
                        .iter()
                        .map(|l| drift_draft::outline::line_width(l.trim_end(), &t.font, t.height))
                        .fold(t.height, f64::max);
                edited.wrap = wraps.then_some(widest);
            }
            if let Some(width) = t.wrap {
                let mut shown = width / unit;
                if ui
                    .add(
                        egui::DragValue::new(&mut shown)
                            .speed(0.5)
                            .range(0.001..=1e6)
                            .max_decimals(3)
                            .suffix(format!(" {}", units.length.symbol())),
                    )
                    .changed()
                {
                    edited.wrap = Some(shown * unit);
                }
            }
        });
        let mut picked = None;
        theme::row(ui, "Font", |ui| {
            picked = self.font_menu(ui, ("text_font", id.0), &t.font.label(), Some(&t.font));
        });
        match picked {
            Some(FontPick::BuiltIn(font)) => edited.font = font,
            Some(FontPick::Family(family)) => {
                // Kept in the drawing where its licence and size allow, so it travels.
                if let Err(why) = self.editor.embed_font(&family)
                    && drift_platform::fonts::load(&family).is_some()
                {
                    let now = ui.input(|i| i.time);
                    self.set_status(why, false, now);
                }
                edited.font = TextFont::System(family);
            }
            Some(FontPick::AddFile) => self.files.open(crate::FONT_FILE),
            Some(FontPick::Standard) | None => {}
        }
        if let TextFont::System(family) = &t.font
            && drift_draft::outline::font_known(family) == Some(false)
        {
            ui.label(
                egui::RichText::new(format!(
                    "{family} isn't in this drawing or on this computer: shown in Sans. The \
                     text keeps the font's name, and uses it where it is found."
                ))
                .small()
                .color(egui::Color32::from_rgb(0xff, 0x9f, 0x0a)),
            );
        }
        if edited != t {
            self.editor.text_height = edited.height;
            self.editor.update_text(id, |x| *x = edited);
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Convert to curves")
                .on_hover_text("Replace the text with its outlines as curves and lines (grouped)")
                .clicked()
            {
                let n = self.editor.convert_text(id);
                let now = ui.input(|i| i.time);
                self.set_status(format!("Converted the text into {n} curves"), false, now);
            }
            if ui.button("Delete").clicked() {
                self.editor.remove_text(id);
            }
        });
        true
    }
}
