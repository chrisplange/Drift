//! Preferences → Keyboard (#499): every command's keys, to search, change, clear and reset,
//! and the presets (Drift default, AutoCAD-style and the user's own).

use eframe::egui;

use crate::DriftApp;
use crate::commands::{self, Category, Shortcut};
use crate::keymap::{self, Active, BuiltIn, Clash};
use crate::theme;

/// What the page is in the middle of, kept while Preferences is open.
#[derive(Clone, Debug, Default)]
pub(crate) struct KeysPage {
    pub search: String,
    /// The command waiting for a key press.
    pub recording: Option<&'static str>,
    /// A key pressed for a command that someone else has: the user chooses.
    pub clash: Option<(&'static str, Shortcut)>,
}

fn page_id() -> egui::Id {
    egui::Id::new("preferences_keys")
}

/// The key pressed this frame, used up so nothing else acts on it. Esc cancels (`Some(None)`).
fn take_press(ctx: &egui::Context) -> Option<Option<Shortcut>> {
    let pressed = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => Some((*key, *modifiers)),
            _ => None,
        })
    })?;
    let (key, modifiers) = pressed;
    ctx.input_mut(|i| {
        i.consume_key(modifiers, key);
        // The character it types, if any, goes too.
        i.events.retain(|e| !matches!(e, egui::Event::Text(_)));
    });
    Some(keymap::shortcut_of(key, modifiers))
}

impl DriftApp {
    pub(crate) fn keyboard_prefs(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut page: KeysPage = ctx.data(|d| d.get_temp(page_id())).unwrap_or_default();
        let mac = cfg!(target_os = "macos");

        // A key pressed while a command waits for one.
        if let Some(id) = page.recording
            && let Some(press) = take_press(&ctx)
        {
            page.recording = None;
            if let (Some(key), Some(spec)) = (press, commands::spec_by_id(id)) {
                match keymap::with(|m| m.clash(spec, key)) {
                    Some(_) => page.clash = Some((id, key)),
                    None => keymap::update(|m| m.assign(spec, key)),
                }
            }
        }

        theme::section(ui, "Preset");
        self.preset_row(ui);

        // The question a clash asks.
        if let Some((id, key)) = page.clash
            && let Some(spec) = commands::spec_by_id(id)
        {
            let clash = keymap::with(|m| m.clash(spec, key));
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match &clash {
                    Some(Clash::Command(other)) => {
                        ui.label(format!(
                            "{} is already {}. Give it to {} instead?",
                            key.text(mac),
                            other.title.trim_end_matches('…'),
                            spec.title.trim_end_matches('…'),
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Reassign").clicked() {
                                keymap::update(|m| m.assign(spec, key));
                                page.clash = None;
                            }
                            if ui.button("Cancel").clicked() {
                                page.clash = None;
                            }
                        });
                    }
                    Some(Clash::Tools(what)) => {
                        ui.label(format!(
                            "{} is kept for {what}, so it can't run a command. Choose \
                             another key.",
                            key.text(mac)
                        ));
                        if ui.button("OK").clicked() {
                            page.clash = None;
                        }
                    }
                    None => page.clash = None,
                }
            });
        }

        theme::section(ui, "Shortcuts");
        theme::search_field(
            ui,
            &mut page.search,
            "Search commands or keys",
            ui.available_width(),
        );
        ui.add_space(4.0);
        let q = page.search.trim().to_lowercase();
        let changed_any = keymap::with(|m| matches!(m.active, Active::User(_)));
        for category in Category::ALL {
            let specs: Vec<_> = commands::registry()
                .iter()
                .filter(|s| s.category == category && !s.when.hidden())
                .filter(|s| {
                    q.is_empty()
                        || s.title.to_lowercase().contains(&q)
                        || s.all_keys(mac).to_lowercase().contains(&q)
                        || s.aliases.iter().any(|a| a.to_lowercase() == q)
                })
                .collect();
            if specs.is_empty() {
                continue;
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new(category.label()).strong());
            for spec in specs {
                let keys = spec.shortcuts();
                let changed = keymap::with(|m| m.changed(spec));
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    let title = spec.title.trim_end_matches('…');
                    let label = if changed {
                        egui::RichText::new(format!("{title} •"))
                    } else {
                        egui::RichText::new(title)
                    };
                    ui.add_sized([200.0, 20.0], egui::Label::new(label).truncate())
                        .on_hover_text(spec.description);
                    if page.recording == Some(spec.id) {
                        ui.label(
                            egui::RichText::new("Press a key… (Esc cancels)")
                                .color(ui.visuals().selection.stroke.color),
                        );
                        return;
                    }
                    if keys.is_empty() {
                        ui.label(egui::RichText::new("No key").weak());
                    }
                    for k in &keys {
                        theme::keycap(ui, &k.text(mac));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !spec.assignable {
                            ui.label(egui::RichText::new("System key").small().weak())
                                .on_hover_text(
                                    "The system's clipboard keys: they can't be changed",
                                );
                            return;
                        }
                        if changed
                            && ui
                                .small_button("Reset")
                                .on_hover_text("Back to the preset's keys")
                                .clicked()
                        {
                            keymap::update(|m| m.reset(spec));
                        }
                        if !keys.is_empty()
                            && ui
                                .small_button("Clear")
                                .on_hover_text("No key for this command")
                                .clicked()
                        {
                            keymap::update(|m| m.clear(spec));
                        }
                        if ui
                            .small_button(if keys.is_empty() { "Add key" } else { "Change" })
                            .on_hover_text("Then press the new key")
                            .clicked()
                        {
                            page.recording = Some(spec.id);
                            page.clash = None;
                        }
                    });
                });
            }
        }
        if changed_any {
            ui.add_space(8.0);
            if ui
                .button("Reset all to the preset")
                .on_hover_text(
                    "Every command back to the keys of the preset this one was made from",
                )
                .clicked()
            {
                keymap::update(|m| m.reset_all());
            }
        }
        ctx.data_mut(|d| d.insert_temp(page_id(), page));
    }

    /// The preset in use, to choose; deleting the user's own.
    fn preset_row(&mut self, ui: &mut egui::Ui) {
        let map = keymap::current();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("keymap_preset")
                .icon(theme::combo_icon)
                .selected_text(map.active_name())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for b in BuiltIn::ALL {
                        if ui
                            .selectable_label(map.active == Active::BuiltIn(b), b.label())
                            .clicked()
                        {
                            keymap::update(|m| m.active = Active::BuiltIn(b));
                        }
                    }
                    for (i, p) in map.user.iter().enumerate() {
                        if ui
                            .selectable_label(map.active == Active::User(i), &p.name)
                            .clicked()
                        {
                            keymap::update(|m| m.active = Active::User(i));
                        }
                    }
                });
            if let Active::User(_) = map.active
                && ui
                    .button("Delete preset")
                    .on_hover_text("Remove your preset and go back to the one it was made from")
                    .clicked()
            {
                keymap::update(|m| m.delete_active());
            }
        });
        let hint = match map.active {
            Active::BuiltIn(_) => {
                "Changing a key makes your own preset from this one; this preset stays as it is."
                    .to_owned()
            }
            Active::User(i) => format!(
                "Your preset, made from {}. Changes are saved as you make them.",
                map.user.get(i).map_or("Drift default", |p| p.base.label())
            ),
        };
        ui.label(egui::RichText::new(hint).small().weak());
    }
}
