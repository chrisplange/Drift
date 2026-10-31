//! The Inspector's report on the last Walls from lines, while the drawing is as it left it:
//! what was made, what was skipped and why (with the lines to pick), and what was done with
//! the lines used (changing it does the command again that way).

use crate::*;
use drift_edit::walls_from_lines::{FromLinesReport, UsedLines};

impl DriftApp {
    /// The report section, at the top of the Inspector.
    pub(crate) fn walls_from_lines_report(&mut self, ui: &mut egui::Ui) {
        let Some(report) = self.drawing.editor.from_lines_now().cloned() else {
            return;
        };
        theme::section(ui, "Walls from lines");
        let n = report.walls.len();
        ui.label(format!(
            "Made {} from {} lines.",
            if n == 1 {
                "1 wall".to_owned()
            } else {
                format!("{n} walls")
            },
            report.used
        ));
        if report.openings > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{} left open between walls in line (doors and windows): the walls end \
                     at their jambs.",
                    if report.openings == 1 {
                        "1 opening".to_owned()
                    } else {
                        format!("{} openings", report.openings)
                    }
                ))
                .small()
                .weak(),
            );
        }
        let mut pick = None;
        for (why, count, lines) in &report.skipped {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} skipped: {}.",
                    FromLinesReport::what(*why, *count),
                    why.text()
                ));
                if ui
                    .small_button("Pick")
                    .on_hover_text("Pick these lines to look at them")
                    .clicked()
                {
                    pick = Some(lines.clone());
                }
            });
        }
        if !report.partly.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} kept: the walls cover them only in part.",
                    if report.partly.len() == 1 {
                        "1 line".to_owned()
                    } else {
                        format!("{} lines", report.partly.len())
                    }
                ));
                if ui.small_button("Pick").clicked() {
                    pick = Some(report.partly.clone());
                }
            });
        }
        let now = ui.input(|i| i.time);
        theme::row(ui, "Lines used", |ui| {
            egui::ComboBox::from_id_salt("walls_from_lines_used")
                .selected_text(report.lines.label())
                .show_ui(ui, |ui| {
                    for u in UsedLines::ALL {
                        if ui.selectable_label(u == report.lines, u.label()).clicked()
                            && u != report.lines
                        {
                            match self.drawing.editor.redo_walls_from_lines(u) {
                                Ok(r) => self.set_status(r.summary(), false, now),
                                Err(why) => self.drawing.editor.out.notice = Some(why),
                            }
                        }
                    }
                })
                .response
                .on_hover_text("What Walls from lines does with the lines it makes into walls");
        });
        if let Some(lines) = pick {
            self.drawing.editor.selection = lines
                .into_iter()
                .filter(|id| self.drawing.editor.doc.contains(*id))
                .collect();
        }
    }
}
