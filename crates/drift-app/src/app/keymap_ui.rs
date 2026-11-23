//! Preferences → Keyboard (#499): every command's keys, to search, change, add, remove and
//! reset, and the presets (Drift default, AutoCAD-style and the user's own).
//!
//! A key cap is clicked to change that key, + adds one; the next key pressed is taken (Esc
//! cancels, Backspace removes the key). A key someone else has is asked about on the row.

use eframe::egui;

use crate::DriftApp;
use crate::commands::{self, Category, Shortcut, Spec};
use crate::keymap::{self, Active, BuiltIn, Clash};
use crate::theme;

/// A question that needs a second click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Confirm {
    DeletePreset,
    ResetAll,
}

/// What the page is in the middle of, kept while Preferences is open.
#[derive(Clone, Debug, Default)]
struct KeysPage {
    search: String,
    changed_only: bool,
    /// The command waiting for a key press, and the key it replaces (`None`: one more).
    recording: Option<(&'static str, Option<Shortcut>)>,
    /// A key pressed that someone else has: the command, the key it replaces, the key.
    clash: Option<(&'static str, Option<Shortcut>, Shortcut)>,
    confirm: Option<Confirm>,
    /// Said once after a change made the user's own preset.
    note: Option<String>,
    /// The user's preset's name, while it is being renamed.
    renaming: Option<String>,
    /// The frame the page was last drawn: away longer, a key wait is dropped.
    pass: u64,
}

fn page_id() -> egui::Id {
    egui::Id::new("preferences_keys")
}

/// The first key pressed this frame (not a modifier alone: egui sends none), used up so
/// nothing else acts on it, with the character it types.
fn take_press(ctx: &egui::Context) -> Option<(egui::Key, egui::Modifiers)> {
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
    ctx.input_mut(|i| {
        i.consume_key(pressed.1, pressed.0);
        i.events.retain(|e| !matches!(e, egui::Event::Text(_)));
    });
    Some(pressed)
}

/// Whether the search `q` (lower case) finds `spec`: by its name, what it does, an
/// AutoCAD name's start, or its keys as either platform writes them.
fn found(spec: &Spec, q: &str) -> bool {
    q.is_empty()
        || spec.title.to_lowercase().contains(q)
        || spec.does().to_lowercase().contains(q)
        || spec.words.to_lowercase().contains(q)
        || spec.aliases.iter().any(|a| a.to_lowercase().starts_with(q))
        || spec.shortcuts().iter().any(|k| {
            k.text(false).to_lowercase().contains(q) || k.text(true).to_lowercase().contains(q)
        })
}

impl DriftApp {
    pub(crate) fn keyboard_prefs(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut page: KeysPage = ctx.data(|d| d.get_temp(page_id())).unwrap_or_default();
        let mac = cfg!(target_os = "macos");
        let pass = ctx.cumulative_pass_nr();
        if pass > page.pass + 2 {
            // Preferences was closed or another page shown: nothing waits for a key.
            page.recording = None;
            page.clash = None;
            page.confirm = None;
        }
        page.pass = pass;

        // The key pressed while a command waits for one.
        if let Some((id, replacing)) = page.recording {
            if ctx.input(|i| i.pointer.any_pressed()) {
                // A click anywhere else gives up waiting.
                page.recording = None;
            } else if let Some((key, modifiers)) = take_press(&ctx) {
                page.recording = None;
                if let Some(spec) = commands::spec_by_id(id) {
                    if key == egui::Key::Escape {
                        // Cancelled.
                    } else if key == egui::Key::Backspace && modifiers.is_none() {
                        if let Some(old) = replacing {
                            self.change_keys(&mut page, |m| m.remove(spec, old));
                        }
                    } else {
                        let key = keymap::shortcut_of(key, modifiers);
                        if keymap::with(|m| m.clash(spec, key)).is_some() {
                            page.clash = Some((id, replacing, key));
                        } else {
                            self.change_keys(&mut page, |m| m.assign(spec, key, replacing));
                        }
                    }
                }
            }
        }

        theme::section(ui, "Preset");
        self.preset_row(ui, &mut page);

        theme::section(ui, "Shortcuts");
        ui.horizontal(|ui| {
            let width = ui.available_width() - 130.0;
            theme::search_field(ui, &mut page.search, "Search commands or keys", width);
            theme::checkbox(ui, &mut page.changed_only, "Changed only")
                .on_hover_text("Only the commands whose keys differ from the preset's");
        });
        let q = page.search.trim().to_lowercase();
        let mut any = false;
        for category in Category::ALL {
            let specs: Vec<&'static Spec> = commands::registry()
                .iter()
                .filter(|s| s.category == category && !s.when.hidden())
                .filter(|s| found(s, &q))
                .filter(|s| !page.changed_only || keymap::with(|m| m.changed(s)))
                .collect();
            if specs.is_empty() {
                continue;
            }
            any = true;
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("{} · {}", category.label(), specs.len()))
                    .small()
                    .strong(),
            );
            ui.separator();
            for spec in specs {
                self.key_row(ui, &mut page, spec, mac);
            }
        }
        if !any {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("No command matches").weak());
        }
        ctx.data_mut(|d| d.insert_temp(page_id(), page));
    }

    /// Change the keymap; a change that made the user's own preset says so.
    fn change_keys(&mut self, page: &mut KeysPage, f: impl FnOnce(&mut keymap::Keymap)) {
        let before = keymap::with(|m| m.active);
        keymap::update(f);
        let after = keymap::current();
        if let (Active::BuiltIn(b), Active::User(_)) = (before, after.active) {
            page.note = Some(format!(
                "Made your own preset, “{}”, from {}. {} stays as it was.",
                after.active_name(),
                b.label(),
                b.label()
            ));
        }
    }

    /// One command: its name, its keys (click one to change it), + for another, and Reset.
    fn key_row(&mut self, ui: &mut egui::Ui, page: &mut KeysPage, spec: &'static Spec, mac: bool) {
        let keys = spec.shortcuts();
        let changed = keymap::with(|m| m.changed(spec));
        let dark = ui.visuals().dark_mode;
        ui.horizontal(|ui| {
            ui.set_min_height(26.0);
            // Changed from the preset: an accent mark, and what it was.
            let (mark, resp) = ui.allocate_exact_size(egui::vec2(8.0, 18.0), egui::Sense::hover());
            if changed {
                ui.painter()
                    .circle_filled(mark.center(), 3.0, theme::accent(dark));
                let was: Vec<String> = keymap::with(|m| m.preset_keys(spec))
                    .iter()
                    .map(|k| k.text(mac))
                    .collect();
                resp.on_hover_text(if was.is_empty() {
                    "Changed: the preset gives it no key".to_owned()
                } else {
                    format!("Changed: the preset's key is {}", was.join(" / "))
                });
            }
            ui.add_sized(
                [180.0, 20.0],
                egui::Label::new(spec.title.trim_end_matches('…')).truncate(),
            )
            .on_hover_text(spec.does());

            // Waiting for a key: the modifiers held so far, and how to get out.
            if let Some((id, _)) = page.recording
                && id == spec.id
            {
                let held = ui.input(|i| i.modifiers);
                let mut so_far = String::new();
                if held.command {
                    so_far.push_str(if mac { "⌘" } else { "Ctrl+" });
                }
                if held.shift {
                    so_far.push_str("Shift+");
                }
                if held.alt {
                    so_far.push_str("Alt+");
                }
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, theme::accent(dark)))
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.label(format!("{so_far}… press a key"));
                    });
                ui.label(
                    egui::RichText::new("Esc cancels · Backspace removes")
                        .small()
                        .weak(),
                );
                return;
            }

            // A key someone else has: asked here, on the row.
            if let Some((id, replacing, key)) = page.clash
                && id == spec.id
            {
                match keymap::with(|m| m.clash(spec, key)) {
                    Some(Clash::Command(other)) => {
                        let only = keymap::with(|m| m.keys_of(other)).len() <= 1;
                        ui.label(format!(
                            "{} is {}'s{}.",
                            key.text(mac),
                            other.title.trim_end_matches('…'),
                            if only { " only key" } else { "" }
                        ));
                        if ui
                            .small_button("Reassign")
                            .on_hover_text(format!(
                                "Give {} to {}",
                                key.text(mac),
                                spec.title.trim_end_matches('…')
                            ))
                            .clicked()
                        {
                            self.change_keys(page, |m| m.assign(spec, key, replacing));
                            page.clash = None;
                        }
                        if ui.small_button("Cancel").clicked() {
                            page.clash = None;
                        }
                    }
                    Some(Clash::Tools(what)) => {
                        ui.label(format!("{} is kept for {what}.", key.text(mac)));
                        if ui.small_button("Choose another").clicked() {
                            page.clash = None;
                            page.recording = Some((spec.id, replacing));
                        }
                        if ui.small_button("Cancel").clicked() {
                            page.clash = None;
                        }
                    }
                    None => page.clash = None,
                }
                return;
            }

            if !spec.assignable {
                for k in &keys {
                    theme::keycap(ui, &k.text(mac));
                }
                ui.label(egui::RichText::new("System clipboard key").small().weak());
                return;
            }
            for k in &keys {
                let cap = theme::keycap(ui, &k.text(mac));
                let cap = ui
                    .interact(cap.rect, cap.id.with("change"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Click, then press the new key");
                let name = format!(
                    "Change {} key {}",
                    spec.title.trim_end_matches('…'),
                    k.text(mac)
                );
                cap.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.clone())
                });
                if cap.hovered() {
                    ui.painter().rect_stroke(
                        cap.rect.expand(1.0),
                        4.0,
                        egui::Stroke::new(1.0, theme::accent(dark)),
                        egui::StrokeKind::Outside,
                    );
                }
                if cap.clicked() {
                    page.recording = Some((spec.id, Some(*k)));
                    page.clash = None;
                }
            }
            let add = if keys.is_empty() { "Set key" } else { "+" };
            if ui
                .small_button(add)
                .on_hover_text(if keys.is_empty() {
                    "Then press the key"
                } else {
                    "Add another key: then press it"
                })
                .clicked()
            {
                page.recording = Some((spec.id, None));
                page.clash = None;
            }
            if changed {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Reset")
                        .on_hover_text("Back to the preset's keys")
                        .clicked()
                    {
                        keymap::update(|m| m.reset(spec));
                    }
                });
            }
        });
    }

    /// The preset in use, to choose; the user's own renamed, reset or deleted.
    fn preset_row(&mut self, ui: &mut egui::Ui, page: &mut KeysPage) {
        let map = keymap::current();
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("keymap_preset")
                .icon(theme::combo_icon)
                .selected_text(map.active_name())
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.label(egui::RichText::new("Built in").small().weak());
                    for b in BuiltIn::ALL {
                        if ui
                            .selectable_label(map.active == Active::BuiltIn(b), b.label())
                            .clicked()
                        {
                            keymap::update(|m| m.active = Active::BuiltIn(b));
                            page.note = None;
                        }
                    }
                    if !map.user.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new("Yours").small().weak());
                    }
                    for (i, p) in map.user.iter().enumerate() {
                        if ui
                            .selectable_label(map.active == Active::User(i), &p.name)
                            .clicked()
                        {
                            keymap::update(|m| m.active = Active::User(i));
                            page.note = None;
                        }
                    }
                });
            if let Active::User(_) = map.active {
                if ui.small_button("Rename").clicked() {
                    page.renaming = Some(map.active_name());
                }
                if map.change_count() > 0 && ui.small_button("Reset all").clicked() {
                    page.confirm = Some(Confirm::ResetAll);
                }
                if ui.small_button("Delete").clicked() {
                    page.confirm = Some(Confirm::DeletePreset);
                }
            }
        });
        if let Some(name) = &mut page.renaming {
            let mut done = false;
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.small_button("Save").clicked() || enter) && !name.trim().is_empty() {
                    let name = name.trim().to_owned();
                    keymap::update(|m| {
                        if let Active::User(i) = m.active
                            && let Some(p) = m.user.get_mut(i)
                        {
                            p.name = name;
                        }
                    });
                    done = true;
                }
                if ui.small_button("Cancel").clicked() {
                    done = true;
                }
            });
            if done {
                page.renaming = None;
            }
        }
        if let Some(confirm) = page.confirm {
            let n = map.change_count();
            let changes = format!("{n} change{}", if n == 1 { "" } else { "s" });
            let base = match map.active {
                Active::User(i) => map.user.get(i).map_or("Drift default", |p| p.base.label()),
                Active::BuiltIn(b) => b.label(),
            };
            ui.horizontal(|ui| {
                let (text, verb) = match confirm {
                    Confirm::DeletePreset => (
                        format!("Delete “{}” and its {changes}?", map.active_name()),
                        "Delete",
                    ),
                    Confirm::ResetAll => {
                        (format!("Undo all {changes}, back to {base}?"), "Reset all")
                    }
                };
                ui.label(text);
                if ui.small_button(verb).clicked() {
                    keymap::update(|m| match confirm {
                        Confirm::DeletePreset => m.delete_active(),
                        Confirm::ResetAll => m.reset_all(),
                    });
                    page.confirm = None;
                    page.note = None;
                }
                if ui.small_button("Cancel").clicked() {
                    page.confirm = None;
                }
            });
        }
        let hint = match (&page.note, map.active) {
            (Some(note), Active::User(_)) => note.clone(),
            (_, Active::BuiltIn(b)) => format!(
                "Change a key and your own preset is made from {}; {} itself stays as it is.",
                b.label(),
                b.label()
            ),
            (_, Active::User(i)) => format!(
                "Your preset, made from {}. Changes are saved as you make them; a dot marks \
                 a key that differs.",
                map.user.get(i).map_or("Drift default", |p| p.base.label())
            ),
        };
        ui.label(egui::RichText::new(hint).small().weak());
    }
}
