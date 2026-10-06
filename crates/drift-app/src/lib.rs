//! Drift application shell: UI chrome, canvas interaction and the GPU canvas callback.
//!
//! The same [`DriftApp`] runs natively and in the browser via eframe.

mod canvas;

use drift_core::{Bounds, DVec2, SketchItem, demo};
use drift_render::{Camera, grid_info};
use eframe::egui;

use crate::canvas::CanvasPainter;

/// Zoom factor per logical point of scroll.
const SCROLL_ZOOM_SPEED: f64 = 0.0015;
/// Margin left around geometry when zooming to fit, in logical points.
const FIT_MARGIN: f64 = 40.0;

pub struct DriftApp {
    items: Vec<SketchItem>,
    /// What "zoom to fit" frames. The precision-test feature far from the origin is excluded.
    home_bounds: Bounds,
    far_bounds: Bounds,
    camera: Camera,
    painter: CanvasPainter,
    /// While set, the view keeps fitting these bounds as the canvas resizes (e.g. while the
    /// browser lays out the page), until the user pans or zooms.
    fit_target: Option<Bounds>,
    /// Last pointer position of an in-progress pan drag.
    pan_anchor: Option<egui::Pos2>,
    cursor_world: Option<DVec2>,
    backend: String,
}

impl DriftApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let render_state = cc
            .wgpu_render_state
            .as_ref()
            .expect("Drift requires the wgpu renderer");
        let painter = CanvasPainter::install(render_state);
        let backend = format!("{:?}", render_state.adapter.get_info().backend);
        log::info!("Drift renderer backend: {backend}");

        let plate = demo::mounting_plate();
        let far = demo::far_feature();
        let home_bounds = plate.iter().map(|i| i.shape.bounds()).collect();
        let far_bounds = far.iter().map(|i| i.shape.bounds()).collect();
        let mut items = plate;
        items.extend(far);

        Self {
            items,
            home_bounds,
            far_bounds,
            camera: Camera::default(),
            painter,
            fit_target: Some(home_bounds),
            pan_anchor: None,
            cursor_world: None,
            backend,
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                ui.add_enabled(false, egui::Button::new("New"));
                ui.add_enabled(false, egui::Button::new("Open…"));
                ui.add_enabled(false, egui::Button::new("Save"));
                ui.label(egui::RichText::new("Files arrive in M1").weak().small());
            });
            ui.menu_button("View", |ui| {
                if ui
                    .add(egui::Button::new("Zoom to fit").shortcut_text("F"))
                    .clicked()
                {
                    self.fit_target = Some(self.home_bounds);
                }
                if ui
                    .button("Go to precision test (1 km from origin)")
                    .clicked()
                {
                    self.fit_target = Some(self.far_bounds);
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
            });
        });
    }

    fn status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match self.cursor_world {
                Some(p) => ui.monospace(format!("X {:>12.4}   Y {:>12.4}  mm", p.x, p.y)),
                None => ui.monospace(format!("{:>40}", "")),
            };
            ui.separator();
            let grid = grid_info(&self.camera);
            ui.monospace(format!("Grid {} mm", format_length(grid.minor)));
            ui.separator();
            ui.monospace(format!("{:.4} px/mm", self.camera.scale));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&self.backend).weak());
            });
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::drag());
        let view_size = DVec2::new(f64::from(rect.width()), f64::from(rect.height()));
        let to_view = |pos: egui::Pos2| {
            let v = pos - rect.center();
            DVec2::new(f64::from(v.x), f64::from(v.y))
        };

        if response.double_clicked_by(egui::PointerButton::Middle)
            || (ui.input(|i| i.key_pressed(egui::Key::F)) && !ui.ctx().egui_wants_keyboard_input())
        {
            self.fit_target = Some(self.home_bounds);
        }

        // Pan with any mouse button (tools take over the primary button in M1). Track positions
        // from the press origin rather than using per-frame deltas, which include the pointer
        // jump when a touch or pen press lands far from the last known position.
        if response.drag_started() {
            self.pan_anchor = ui.input(|i| i.pointer.press_origin());
        }
        if response.dragged()
            && let (Some(from), Some(to)) = (self.pan_anchor, response.interact_pointer_pos())
        {
            let delta = to - from;
            if delta != egui::Vec2::ZERO {
                self.camera
                    .pan(DVec2::new(f64::from(delta.x), f64::from(delta.y)));
                self.fit_target = None;
            }
            self.pan_anchor = Some(to);
        }
        if response.drag_stopped() {
            self.pan_anchor = None;
        }

        let hover = response.hover_pos();
        if let Some(pos) = hover {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (f64::from(scroll) * SCROLL_ZOOM_SPEED).exp() * f64::from(pinch);
            if factor != 1.0 {
                self.camera.zoom_at(to_view(pos), factor);
                self.fit_target = None;
            }
        }

        if let Some(bounds) = self.fit_target {
            self.camera.fit(bounds, view_size, FIT_MARGIN);
        }
        self.cursor_world = hover.map(|pos| self.camera.view_to_world(to_view(pos)));

        let ppp = f64::from(ui.ctx().pixels_per_point());
        let callback =
            self.painter
                .callback(rect, ppp, &self.items, &self.camera, ui.visuals().dark_mode);
        ui.painter().add(callback);
    }
}

impl eframe::App for DriftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
    }
}

/// Format a power-of-ten length compactly, e.g. `0.001`, `10`, `1e6`.
fn format_length(value: f64) -> String {
    if (1e-4..1e5).contains(&value) {
        let decimals = (-value.log10().floor()).max(0.0) as usize;
        format!("{value:.decimals$}")
    } else {
        format!("{value:e}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_length;

    #[test]
    fn formats_grid_lengths() {
        assert_eq!(format_length(0.001), "0.001");
        assert_eq!(format_length(1.0), "1");
        assert_eq!(format_length(100.0), "100");
        assert_eq!(format_length(1e6), "1e6");
    }
}
