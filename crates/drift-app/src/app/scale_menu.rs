//! The one scale menu (drift-releases#33): the drawing scale, view scales and underlay scales
//! all pick from the same list, the groups for the drawing's units first, and take a typed
//! custom scale (`1:37`, `5" = 1'-0"`, `2:1`).

use drift_core::sheets::{parse_scale, scale_label, scale_list};
use eframe::egui;

/// Whether two scales (1 : n) are the same one.
pub(crate) fn same_scale(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1e-9)
}

/// A scale menu for `scale` (n in 1 : n): the standard scales in groups, the drawing's units'
/// first (`imperial`: architectural and engineering, else metric), then a Custom field that
/// takes any scale typed as `1:n`, `n:1`, `n` or paper `=` model lengths. Returns true if the
/// scale changed.
pub(crate) fn scale_menu(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    scale: &mut f64,
    imperial: bool,
    width: f32,
) -> bool {
    let before = *scale;
    let id = egui::Id::new(("scale_menu", id));
    let text_id = id.with("custom");
    let error_id = id.with("error");
    egui::ComboBox::from_id_salt(id)
        .selected_text(scale_label(*scale, imperial))
        .width(width)
        .height(460.0)
        // Not the default close-on-any-click: a click into the Custom field to type would
        // close the menu. A pick from the list closes it below.
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(170.0);
            for (k, group) in scale_list(imperial).into_iter().enumerate() {
                if k > 0 {
                    ui.separator();
                }
                ui.label(egui::RichText::new(group.name).small().weak());
                for n in group.scales {
                    if ui
                        .selectable_label(same_scale(*scale, n), scale_label(n, imperial))
                        .clicked()
                    {
                        *scale = n;
                        ui.close();
                    }
                }
            }
            ui.separator();
            ui.label(egui::RichText::new("Custom").small().weak());
            let mut text: String = ui
                .data(|d| d.get_temp::<String>(text_id))
                .unwrap_or_default();
            let edit = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text("1:37 or 5\" = 1'-0\"")
                    .desired_width(150.0),
            );
            let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if entered {
                match parse_scale(&text) {
                    Ok(n) => {
                        *scale = n;
                        text.clear();
                        ui.data_mut(|d| d.remove::<String>(error_id));
                        ui.close();
                    }
                    Err(e) => {
                        let e = e.to_string();
                        ui.data_mut(|d| d.insert_temp(error_id, e));
                    }
                }
            }
            ui.data_mut(|d| d.insert_temp(text_id, text));
            if let Some(error) = ui.data(|d| d.get_temp::<String>(error_id)) {
                ui.label(
                    egui::RichText::new(error)
                        .small()
                        .color(egui::Color32::from_rgb(0xff, 0x5d, 0x5d)),
                );
            }
        })
        .response
        .on_hover_text("Pick a scale, or type one under Custom and press Enter");
    !same_scale(*scale, before)
}
