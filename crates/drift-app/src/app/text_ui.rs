//! The inspector for sketch text.

use drift_core::{TextAlign, TextFont};

use crate::*;

/// The key that starts a new line while typing text (Return alone finishes).
fn new_line_key() -> egui::KeyboardShortcut {
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter)
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

    /// Typing a text in place: a field under it on the canvas while the outlines update
    /// live above. Return, Esc or a click elsewhere finishes; Shift+Return starts a new line.
    pub(crate) fn text_editor(&mut self, ctx: &egui::Context, map: &ScreenMap) {
        let Some(id) = self.editor.editing_text else {
            self.text_focused = None;
            return;
        };
        let Some(t) = self.editor.doc.text(id).cloned() else {
            self.editor.editing_text = None;
            return;
        };
        // Under the text's lowest line, so the field doesn't cover what is being typed.
        let low = self
            .editor
            .text_outline(id)
            .map_or(t.at.y, |o| o.bounds.min.y.min(t.at.y));
        let at = map.to_screen(DVec2::new(t.at.x, low));
        let field = egui::Id::new("text_edit_field");
        let mut done = false;
        let style = ctx.style_of(ctx.theme());
        egui::Area::new(egui::Id::new("text_edit"))
            .fixed_pos(at + egui::vec2(-6.0, 8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(&style)
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        let mut words = t.text.clone();
                        let rows =
                            words.lines().count().max(1) + usize::from(words.ends_with('\n'));
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut words)
                                .id(field)
                                .hint_text("Type the text, then Enter (Shift+Enter: new line)")
                                .char_limit(crate::texts::MAX_TEXT_CHARS)
                                .desired_rows(rows)
                                // Return finishes; a new line is Shift+Return.
                                .return_key(Some(new_line_key()))
                                .desired_width(260.0),
                        );
                        let enter =
                            ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                        if self.text_focused != Some(id) {
                            response.request_focus();
                            self.text_focused = Some(id);
                        } else if response.lost_focus() || (response.has_focus() && enter) {
                            response.surrender_focus();
                            done = true;
                        }
                        if response.changed() {
                            // Typing is one undo step per visit to the field.
                            let continuing = self.text_typing == Some(id);
                            self.editor.type_text(id, &words, continuing);
                            self.text_typing = Some(id);
                        }
                    });
            });
        if done {
            self.editor.editing_text = None;
            self.text_typing = None;
            self.text_focused = None;
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
