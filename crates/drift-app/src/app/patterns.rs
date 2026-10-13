//! Linear and circular patterns of the selection, in the properties panel.

use crate::*;

/// Edit buffers for the pattern fields (text, so they take expressions and units).
pub(crate) struct PatternUi {
    linear_count: usize,
    dx: String,
    dy: String,
    circular_count: usize,
    angle: String,
    cx: String,
    cy: String,
}

impl Default for PatternUi {
    fn default() -> Self {
        Self {
            linear_count: 3,
            dx: "20".into(),
            dy: "0".into(),
            circular_count: 6,
            angle: "360".into(),
            cx: "0".into(),
            cy: "0".into(),
        }
    }
}

impl DriftApp {
    pub(crate) fn pattern_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let ids: Vec<drift_core::EntityId> = self
            .editor
            .selection
            .iter()
            .copied()
            .filter(|id| self.editor.doc.get(*id).is_some_and(|e| !e.kind.is_point()))
            .collect();
        if ids.is_empty() {
            return;
        }
        let mut result = None;
        crate::theme::fold(ui, "pattern", "Pattern", false, |ui| {
            let field = |ui: &mut egui::Ui, label: &str, text: &mut String| {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(text).desired_width(70.0));
            };
            let ed = &self.editor;
            let eval = |text: &str, kind| {
                ed.evaluate_text(text, kind)
                    .map(|e| e.value)
                    .map_err(|e| format!("\"{text}\": {e}"))
            };
            let p = &mut self.patterns;
            ui.label(egui::RichText::new("Linear").strong());
            egui::Grid::new("linear_pattern")
                .num_columns(4)
                .show(ui, |ui| {
                    ui.label("Count");
                    ui.add(egui::DragValue::new(&mut p.linear_count).range(2..=500));
                    ui.end_row();
                    field(ui, "Spacing X", &mut p.dx);
                    field(ui, "Y", &mut p.dy);
                    ui.end_row();
                });
            if ui.button("Make linear pattern").clicked() {
                result = Some(
                    eval(&p.dx, Kind::Length)
                        .and_then(|dx| Ok((dx, eval(&p.dy, Kind::Length)?)))
                        .map(|(dx, dy)| PatternRequest::Linear {
                            count: p.linear_count,
                            step: DVec2::new(dx, dy),
                        }),
                );
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Circular").strong());
            egui::Grid::new("circular_pattern")
                .num_columns(4)
                .show(ui, |ui| {
                    ui.label("Count");
                    ui.add(egui::DragValue::new(&mut p.circular_count).range(2..=500));
                    field(ui, "Angle", &mut p.angle);
                    ui.end_row();
                    field(ui, "Centre X", &mut p.cx);
                    field(ui, "Y", &mut p.cy);
                    ui.end_row();
                });
            if ui
                .button("Make circular pattern")
                .on_hover_text("360° spreads the copies evenly round the full circle")
                .clicked()
            {
                result = Some((|| {
                    Ok(PatternRequest::Circular {
                        count: p.circular_count,
                        center: DVec2::new(eval(&p.cx, Kind::Length)?, eval(&p.cy, Kind::Length)?),
                        degrees: eval(&p.angle, Kind::Angle)?,
                    })
                })());
            }
        });
        let outcome = match result {
            None => return,
            Some(Err(e)) => Err(e),
            Some(Ok(PatternRequest::Linear { count, step })) => self
                .editor
                .linear_pattern(&ids, count, step)
                .map_err(|e| e.to_string()),
            Some(Ok(PatternRequest::Circular {
                count,
                center,
                degrees,
            })) => self
                .editor
                .circular_pattern(&ids, count, center, degrees)
                .map_err(|e| e.to_string()),
        };
        match outcome {
            Ok(created) => {
                let n = created.len();
                self.set_status(format!("Pattern: added {n} curves"), false, now);
            }
            Err(e) => self.set_status(format!("Pattern: {e}"), true, now),
        }
    }
}

enum PatternRequest {
    Linear {
        count: usize,
        step: DVec2,
    },
    Circular {
        count: usize,
        center: DVec2,
        degrees: f64,
    },
}
