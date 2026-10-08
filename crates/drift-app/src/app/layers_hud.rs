//! The Layers HUD: a compact layer list to keep at hand while drawing. It floats over the
//! canvas or docks to its left or right edge; the full layer editor stays in the sidebar.

use drift_core::LayerId;

use crate::icons::Glyph;
use crate::layers_ui::{icon_toggle, to_color32};
use crate::*;

/// Where the Layers HUD is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LayersDock {
    #[default]
    Hidden,
    Floating,
    Left,
    Right,
}

impl LayersDock {
    pub(crate) fn key(self) -> &'static str {
        match self {
            LayersDock::Hidden => "hidden",
            LayersDock::Floating => "floating",
            LayersDock::Left => "left",
            LayersDock::Right => "right",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        [
            LayersDock::Hidden,
            LayersDock::Floating,
            LayersDock::Left,
            LayersDock::Right,
        ]
        .into_iter()
        .find(|d| d.key() == s)
    }
}

/// Width of the floating HUD and the docked panel's default.
const HUD_WIDTH: f32 = 228.0;

impl DriftApp {
    /// Show or hide the HUD (Ctrl/⌘+L), returning to where it was last.
    pub(crate) fn toggle_layers_hud(&mut self) {
        if self.layers_dock == LayersDock::Hidden {
            self.layers_dock = if self.layers_last == LayersDock::Hidden {
                LayersDock::Floating
            } else {
                self.layers_last
            };
        } else {
            self.layers_last = self.layers_dock;
            self.layers_dock = LayersDock::Hidden;
        }
    }

    fn set_layers_dock(&mut self, dock: LayersDock) {
        if dock == LayersDock::Hidden {
            self.layers_last = self.layers_dock;
        }
        self.layers_dock = dock;
    }

    /// The docked panel, if docked on `side` (call between the side panels and the canvas).
    pub(crate) fn layers_dock_panel(&mut self, ui: &mut egui::Ui, left: bool) {
        let wanted = if left {
            LayersDock::Left
        } else {
            LayersDock::Right
        };
        if self.layers_dock != wanted || self.sheet_preview {
            return;
        }
        let style = ui.style().clone();
        let frame =
            egui::Frame::side_top_panel(&style).inner_margin(egui::Margin::symmetric(10, 8));
        let panel = if left {
            egui::Panel::left("layers_dock")
        } else {
            egui::Panel::right("layers_dock")
        };
        panel
            .resizable(true)
            .frame(frame)
            .default_size(HUD_WIDTH - 20.0)
            .min_size(170.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.layers_hud(ui));
            });
    }

    /// The floating HUD over the canvas (call after the canvas).
    pub(crate) fn layers_float(&mut self, ctx: &egui::Context) {
        if self.layers_dock != LayersDock::Floating || self.sheet_preview {
            return;
        }
        let canvas = self.canvas_rect;
        if !canvas.is_positive() {
            return;
        }
        let dark = ctx.theme() == egui::Theme::Dark;
        let fill = if dark {
            egui::Color32::from_rgba_unmultiplied(0x26, 0x28, 0x2d, 236)
        } else {
            egui::Color32::from_rgba_unmultiplied(0xfb, 0xfb, 0xfc, 240)
        };
        let frame = egui::Frame::new()
            .fill(fill)
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(
                1.0,
                if dark {
                    egui::Color32::from_white_alpha(18)
                } else {
                    egui::Color32::from_black_alpha(22)
                },
            ))
            .shadow(egui::epaint::Shadow {
                offset: [0, 6],
                blur: 22,
                spread: 0,
                color: egui::Color32::from_black_alpha(if dark { 90 } else { 40 }),
            })
            .inner_margin(egui::Margin::same(10));
        egui::Window::new("Layers HUD")
            .id(egui::Id::new("layers_hud"))
            .title_bar(false)
            .resizable(false)
            .movable(true)
            .frame(frame)
            .fixed_size([HUD_WIDTH - 20.0, 0.0])
            .pivot(egui::Align2::RIGHT_TOP)
            .default_pos(canvas.right_top() + egui::vec2(-12.0, 12.0))
            .constrain_to(canvas)
            .show(ctx, |ui| self.layers_hud(ui));
    }

    /// The HUD's contents: header with docking, one row per layer, and quick actions.
    pub(crate) fn layers_hud(&mut self, ui: &mut egui::Ui) {
        let dark = ui.visuals().dark_mode;
        let accent = theme::accent(dark);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Layers")
                    .family(theme::bold())
                    .size(14.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(egui::RichText::new("×").size(15.0)).frame(false))
                    .on_hover_text(format!("Hide ({})", app::panels::shortcut("L", false)))
                    .clicked()
                {
                    self.set_layers_dock(LayersDock::Hidden);
                }
                ui.menu_button(egui::RichText::new("•••").size(11.0), |ui| {
                    for (dock, label) in [
                        (LayersDock::Floating, "Float over the canvas"),
                        (LayersDock::Left, "Dock left"),
                        (LayersDock::Right, "Dock right"),
                    ] {
                        if ui
                            .add(egui::Button::selectable(self.layers_dock == dock, label))
                            .clicked()
                        {
                            self.set_layers_dock(dock);
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Edit layers…").clicked() {
                        self.show_properties = Some(true);
                        self.params.tab = parameters::RightTab::Layers;
                        ui.close();
                    }
                })
                .response
                .on_hover_text("Float or dock this panel");
            });
        });
        ui.add_space(4.0);
        let layers: Vec<(LayerId, drift_core::Layer)> = self
            .editor
            .doc
            .layers()
            .map(|(id, l)| (id, l.clone()))
            .collect();
        let current = self.editor.doc.current_layer();
        // Curves per layer, and the layers the selection is on.
        let mut counts = std::collections::HashMap::<LayerId, usize>::new();
        for (_, e) in self.editor.doc.iter() {
            if !e.kind.is_point() {
                *counts.entry(e.layer).or_default() += 1;
            }
        }
        let selected: std::collections::BTreeSet<LayerId> = self
            .editor
            .selection
            .iter()
            .filter_map(|id| self.editor.doc.get(*id).map(|e| e.layer))
            .collect();
        for (id, layer) in layers {
            let is_current = id == current;
            let mut edited = layer.clone();
            let row = egui::Frame::new()
                .fill(if is_current {
                    accent.gamma_multiply(0.18)
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(6, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // The current layer's marker, and the selection's.
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(8.0, 18.0), egui::Sense::hover());
                        if is_current {
                            ui.painter().circle_filled(dot.center(), 3.5, accent);
                        } else if selected.contains(&id) {
                            ui.painter().circle_stroke(
                                dot.center(),
                                3.0,
                                egui::Stroke::new(1.2, accent),
                            );
                        }
                        let (swatch, _) =
                            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                        ui.painter()
                            .rect_filled(swatch, 3.0, to_color32(layer.color, dark));
                        let name = egui::RichText::new(&layer.name);
                        let name = if is_current {
                            name.family(theme::bold())
                        } else if layer.visible {
                            name
                        } else {
                            name.weak()
                        };
                        let response = ui
                            .add(
                                egui::Label::new(name)
                                    .sense(egui::Sense::click())
                                    .truncate(),
                            )
                            .on_hover_text(if is_current {
                                "The current layer: new geometry goes here"
                            } else {
                                "Click to make this the current layer"
                            });
                        if response.clicked() {
                            self.editor.set_current_layer(id);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut locked = edited.locked;
                            let glyph = if locked { Glyph::Lock } else { Glyph::Unlock };
                            icon_toggle(ui, &mut locked, glyph, false, "Lock");
                            edited.locked = locked;
                            icon_toggle(ui, &mut edited.visible, Glyph::Eye, true, "Show or hide");
                            let n = counts.get(&id).copied().unwrap_or(0);
                            ui.label(egui::RichText::new(n.to_string()).small().weak())
                                .on_hover_text("Curves on this layer");
                        });
                    });
                });
            row.response.context_menu(|ui| {
                if ui.button("Make current").clicked() {
                    self.editor.set_current_layer(id);
                    ui.close();
                }
                if ui.button("Select all on this layer").clicked() {
                    self.editor.select_layer(id);
                    ui.close();
                }
                if ui
                    .add_enabled(
                        !self.editor.selection.is_empty(),
                        egui::Button::new("Move selection here"),
                    )
                    .clicked()
                {
                    self.editor.move_selection_to_layer(id);
                    ui.close();
                }
            });
            if edited != layer {
                self.editor.update_layer(id, edited);
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button("+ New")
                .on_hover_text("A new layer, made current")
                .clicked()
            {
                let n = self.editor.doc.layers().count();
                let id = self.editor.add_layer(&format!("Layer {n}"));
                self.editor.set_current_layer(id);
            }
            if !self.editor.selection.is_empty()
                && ui
                    .button("Move here")
                    .on_hover_text("Move the selection to the current layer")
                    .clicked()
            {
                self.editor.move_selection_to_layer(current);
            }
            if ui
                .button("Edit…")
                .on_hover_text("Colours, weights, line types and names: the Layers tab")
                .clicked()
            {
                self.show_properties = Some(true);
                self.params.tab = parameters::RightTab::Layers;
            }
        });
    }
}
